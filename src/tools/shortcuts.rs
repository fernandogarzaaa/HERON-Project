//! F5: run the Shortcuts the owner ticked, through `/usr/bin/shortcuts`.
//!
//! `shortcuts` is run outside the sandbox, because a shortcut's actions run inside the Shortcuts
//! service, not in this process. So the call is narrowed in every other way: only a name the owner
//! ticked (`[features] shortcuts_allowed`) is run, it is refused before the decision function
//! otherwise, every run is approved once at a time ([`crate::permissions::once_only`]), and the child
//! gets the environment allowlist, a private folder that is removed afterwards, a timeout below the
//! agent's own, and a process-group kill on timeout and on drop.

use super::{opt_str, schema, Tool, ToolCtx};
use crate::child::{child_env_base, kill_group, own_var, private_temp_dir};
use crate::config::Config;
use crate::permissions::Effect;
use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

/// The core key (see [`crate::features`]).
pub const KEY: &str = "shortcuts";
/// The tool's name.
pub const TOOL: &str = "run_shortcut";
/// The binary on every Mac.
pub const REAL_BIN: &str = "/usr/bin/shortcuts";
/// What a call to a shortcut the owner has not ticked gets, before the decision function runs.
pub const NOT_ALLOWED: &str = "That shortcut isn't one you've allowed Heron to run.";
/// The longest `input`, in characters. Longer input is refused, not cut, because the approval shows it.
pub const MAX_INPUT_CHARS: usize = 20_000;
/// The longest result, in characters.
pub const MAX_OUTPUT_CHARS: usize = 20_000;
/// How much of stderr a failure shows, in characters.
const MAX_STDERR_CHARS: usize = 500;
/// `shortcuts list` is stopped after this long.
pub const LIST_TIMEOUT: Duration = Duration::from_secs(10);
/// `shortcuts run` is stopped after at most this many seconds…
const RUN_TIMEOUT_MAX_SECS: u64 = 110;
/// …and at least this many.
const RUN_TIMEOUT_MIN_SECS: u64 = 5;
/// The most of a child's stdout or stderr kept in memory. The rest is read and dropped, so a child
/// that writes a lot never blocks on a full pipe.
const PIPE_CAP: usize = 1024 * 1024;

/// Reads a variable of Heron's own environment, for [`child_env_base`]: [`own_var`] outside tests.
pub type EnvReader<'a> = &'a (dyn Fn(&str) -> Option<String> + Sync);

/// The `shortcuts` binary: `/usr/bin/shortcuts`, or in a debug build `HERON_SHORTCUTS_BIN` when it is
/// set (tests and feature tasks put a fake there). Read on every call, never cached.
pub fn bin() -> PathBuf {
    #[cfg(test)]
    if let Some(path) = test_bin::get() {
        return path;
    }
    bin_from(&own_var)
}

/// [`bin`] from an environment reader: a debug build takes a non-empty `HERON_SHORTCUTS_BIN`; a
/// release build never reads it.
fn bin_from(get: EnvReader<'_>) -> PathBuf {
    if cfg!(debug_assertions) {
        if let Some(path) = get("HERON_SHORTCUTS_BIN").filter(|p| !p.is_empty()) {
            return PathBuf::from(path);
        }
    }
    PathBuf::from(REAL_BIN)
}

/// How long `shortcuts run` may take: 110 seconds, or 5 seconds less than the agent's tool timeout when
/// that is smaller, but never less than 5 seconds. So the run's own timeout, which reports what
/// happened, fires before the agent's.
pub fn run_timeout(tool_timeout_secs: u64) -> Duration {
    Duration::from_secs(RUN_TIMEOUT_MAX_SECS.min(tool_timeout_secs.saturating_sub(5)).max(RUN_TIMEOUT_MIN_SECS))
}

/// Whether the owner ticked `name` in Settings › Features › Shortcuts. The name must match exactly.
pub fn allowed(cfg: &Config, name: &str) -> bool {
    cfg.features.shortcuts_allowed.iter().any(|n| n == name)
}

/// The owner's shortcuts, from `shortcuts list`, one name per line, in its order. `None` when the
/// binary is not there.
pub async fn list(bin: &Path, get: EnvReader<'_>) -> Result<Option<Vec<String>>> {
    let dir = private_temp_dir()?;
    let cmd = command(bin, &["list".into()], &dir.dir, true, get);
    let (status, stdout, stderr) = match run_child(cmd, LIST_TIMEOUT).await {
        Outcome::NotStarted(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Outcome::NotStarted(e) => bail!("could not start {}: {e}", bin.display()),
        Outcome::TimedOut => bail!("`shortcuts list` took longer than {} seconds and was stopped.", LIST_TIMEOUT.as_secs()),
        Outcome::Exited { status, stdout, stderr } => (status, stdout, stderr),
    };
    if !status.success() {
        bail!("`shortcuts list` failed (exit {}): {}", exit_code(status), first_chars(&String::from_utf8_lossy(&stderr), MAX_STDERR_CHARS));
    }
    let mut names: Vec<String> = Vec::new();
    for line in String::from_utf8_lossy(&stdout).lines() {
        let name = line.trim();
        if !name.is_empty() && !names.iter().any(|n| n == name) {
            names.push(name.to_string());
        }
    }
    Ok(Some(names))
}

/// Run the shortcut `name` with `bin`: `run <name> [--input-path <file>] --output-path <file>`, both
/// files in a private folder that is removed afterwards, stopped after `limit`.
///
/// The result is the output file, read as UTF-8 with bad bytes replaced and cut to
/// [`MAX_OUTPUT_CHARS`]; stdout is ignored. A non-zero exit is a result too (it carries the
/// shortcut's stderr, which is outside content, so the agent wraps and marks it like any output). A
/// run that takes too long is an error.
pub async fn run(bin: &Path, name: &str, input: Option<&str>, limit: Duration, get: EnvReader<'_>) -> Result<String> {
    if let Some(input) = input {
        let n = input.chars().count();
        if n > MAX_INPUT_CHARS {
            bail!("The input for shortcut {name} is {n} characters long; the most a shortcut can be given is {MAX_INPUT_CHARS}.");
        }
    }
    let dir = private_temp_dir()?;
    let mut args: Vec<OsString> = vec!["run".into(), name.into()];
    if let Some(input) = input {
        let input_path = dir.dir.join("input.txt");
        std::fs::write(&input_path, input)?;
        args.push("--input-path".into());
        args.push(input_path.into());
    }
    let output_path = dir.dir.join("output.txt");
    args.push("--output-path".into());
    args.push(output_path.clone().into());
    let cmd = command(bin, &args, &dir.dir, false, get);
    match run_child(cmd, limit).await {
        Outcome::NotStarted(e) => Err(anyhow!("could not start {}: {e}", bin.display())),
        Outcome::TimedOut => Err(anyhow!("Shortcut {name} took longer than {} seconds and was stopped.", limit.as_secs())),
        Outcome::Exited { status, stderr, .. } if !status.success() => {
            Ok(format!("Shortcut {name} failed (exit {}): {}", exit_code(status), first_chars(&String::from_utf8_lossy(&stderr), MAX_STDERR_CHARS)))
        }
        Outcome::Exited { .. } => {
            let bytes = match std::fs::read(&output_path) {
                Ok(b) => b,
                // A shortcut with no output writes no file.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                Err(e) => return Err(anyhow!("could not read the output of shortcut {name}: {e}")),
            };
            Ok(first_chars(&String::from_utf8_lossy(&bytes), MAX_OUTPUT_CHARS))
        }
    }
}

/// The child: `bin args`, in `dir`, from an empty environment plus the allowlist with `TMPDIR` at
/// `dir`, leading its own process group, killed if dropped. Stdout is kept only when `keep_stdout`.
fn command(bin: &Path, args: &[OsString], dir: &Path, keep_stdout: bool, get: &dyn Fn(&str) -> Option<String>) -> Command {
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .current_dir(dir)
        .env_clear()
        .envs(child_env_base(dir, get))
        .stdin(Stdio::null())
        .stdout(if keep_stdout { Stdio::piped() } else { Stdio::null() })
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);
    cmd
}

enum Outcome {
    NotStarted(std::io::Error),
    TimedOut,
    Exited { status: ExitStatus, stdout: Vec<u8>, stderr: Vec<u8> },
}

/// Kills the child's whole process group when dropped: when the run returns, times out, or its future
/// is dropped by the agent's own timeout or a cancel. `kill_on_drop` alone reaches only the direct
/// child, not what it started.
struct KillGroupOnDrop(Option<u32>);

impl Drop for KillGroupOnDrop {
    fn drop(&mut self) {
        if let Some(pgid) = self.0 {
            kill_group(pgid);
        }
    }
}

/// Start `cmd` and wait for it, reading stdout and stderr while it runs, for at most `limit`.
async fn run_child(mut cmd: Command, limit: Duration) -> Outcome {
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => return Outcome::NotStarted(e),
    };
    // Saved now: once the child is reaped, `child.id()` is `None`.
    let pgid = child.id();
    let _group = KillGroupOnDrop(pgid);
    let (stdout, stderr) = (child.stdout.take(), child.stderr.take());
    let finished = tokio::time::timeout(limit, async { tokio::join!(read_capped(stdout), read_capped(stderr), child.wait()) }).await;
    match finished {
        Ok((stdout, stderr, Ok(status))) => Outcome::Exited { status, stdout, stderr },
        Ok((_, _, Err(e))) => Outcome::NotStarted(e),
        Err(_) => {
            if let Some(pgid) = pgid {
                kill_group(pgid);
            }
            let _ = child.kill().await;
            Outcome::TimedOut
        }
    }
}

/// Everything `pipe` gives until it closes, keeping at most [`PIPE_CAP`] bytes.
async fn read_capped<R: AsyncRead + Unpin>(pipe: Option<R>) -> Vec<u8> {
    let Some(mut pipe) = pipe else { return Vec::new() };
    let (mut kept, mut buf) = (Vec::new(), [0u8; 8192]);
    loop {
        match pipe.read(&mut buf).await {
            Ok(0) | Err(_) => return kept,
            Ok(n) => {
                let room = PIPE_CAP.saturating_sub(kept.len());
                kept.extend_from_slice(&buf[..n.min(room)]);
            }
        }
    }
}

fn exit_code(status: ExitStatus) -> String {
    match status.code() {
        Some(code) => code.to_string(),
        None => "killed by a signal".to_string(),
    }
}

/// The first `max` characters of `text`.
fn first_chars(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((cut, _)) => text[..cut].to_string(),
        None => text.to_string(),
    }
}

/// `run_shortcut {name, input?}`.
pub struct RunShortcut;

#[async_trait]
impl Tool for RunShortcut {
    fn name(&self) -> &str {
        TOOL
    }
    fn description(&self) -> &str {
        "Run one of the user's Shortcuts (the macOS Shortcuts app), which can reach Reminders, Calendar, Notes, Home and other apps. Only shortcuts the user has ticked in Settings › Features › Shortcuts can run, by their exact name; if you are unsure of the name, ask the user. Optional text input is passed to the shortcut. Returns what the shortcut outputs."
    }
    fn parameters(&self) -> Value {
        schema(
            json!({
                "name": {"type": "string", "description": "The shortcut's exact name"},
                "input": {"type": "string", "description": "Text passed to the shortcut as its input, at most 20000 characters. Leave out for none."}
            }),
            &["name"],
        )
    }
    fn summary(&self, args: &Value) -> String {
        args.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string()
    }
    /// A shortcut can send or change anything.
    fn risk(&self, _args: &Value, _ctx: &ToolCtx) -> Effect {
        Effect::ExternalWrite
    }
    /// The approval shows the shortcut's name and the whole input.
    fn detail(&self, args: &Value, _ctx: &ToolCtx) -> String {
        let name = self.summary(args);
        match opt_str(args, "input") {
            Some(input) => format!("Run the shortcut \"{name}\" with this input:\n{input}"),
            None => format!("Run the shortcut \"{name}\", with no input."),
        }
    }
    /// A shortcut can fetch web pages or read other people's text.
    fn untrusted_output(&self) -> bool {
        true
    }
    fn feature(&self) -> Option<&'static str> {
        Some(KEY)
    }
    fn refusal(&self, args: &Value, ctx: &ToolCtx) -> Option<String> {
        let name = args.get("name").and_then(|n| n.as_str()).unwrap_or("");
        (!allowed(&ctx.cfg, name)).then(|| NOT_ALLOWED.to_string())
    }
    async fn call(&self, args: &Value, ctx: &ToolCtx) -> Result<String> {
        // Checked again here: an approval parked while away is carried out without the agent.
        if let Some(reason) = self.refusal(args, ctx) {
            bail!(reason);
        }
        let name = args.get("name").and_then(|n| n.as_str()).unwrap_or("");
        run(&bin(), name, opt_str(args, "input"), run_timeout(ctx.cfg.agent.tool_timeout_secs), &own_var).await
    }
}

/// A test-only replacement for the binary that [`bin`] returns, for tests that go through the agent or
/// the routes, so no test changes the process environment. Tests that use it run one at a time.
#[cfg(test)]
pub(crate) mod test_bin {
    use std::path::PathBuf;

    static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    static BIN: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

    /// While this lives, [`super::bin`] is the fake.
    pub(crate) struct Fake {
        _turn: tokio::sync::MutexGuard<'static, ()>,
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            *BIN.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
    }

    pub(crate) async fn fake(path: PathBuf) -> Fake {
        let turn = ONE_AT_A_TIME.lock().await;
        *BIN.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
        Fake { _turn: turn }
    }

    pub(super) fn get() -> Option<PathBuf> {
        BIN.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// Test helper: write `body` as a `/bin/sh` script called `shortcuts` in `dir`, executable.
#[cfg(test)]
pub(crate) fn fake_bin(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("shortcuts");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}

/// Test helper: a fake in `dir` that records its arguments one per line in `<rec>/argv`, copies the
/// input file to `<rec>/input`, and writes "done" to the output file.
#[cfg(test)]
pub(crate) fn recording_bin(dir: &Path, rec: &Path) -> PathBuf {
    let rec = rec.display();
    fake_bin(
        dir,
        &format!(
            r#"prev=""
for a in "$@"; do
  printf '%s\n' "$a" >> "{rec}/argv"
  if [ "$prev" = "--input-path" ]; then cat "$a" > "{rec}/input"; fi
  if [ "$prev" = "--output-path" ]; then printf 'done' > "$a"; fi
  prev="$a"
done"#
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::may_grant_for_conversation;
    use crate::child::CHILD_ENV_ALLOWED;
    use crate::permissions::{decide_for, once_only, ApprovalRequest, Mode, Policy, Verdict};
    use std::collections::{BTreeMap, BTreeSet};
    use std::time::Instant;

    /// The environment the tests hand the child: enough for `/bin/sh` to find its tools.
    fn env(key: &str) -> Option<String> {
        match key {
            "PATH" => Some("/usr/bin:/bin".into()),
            "HOME" => Some("/Users/someone".into()),
            _ => None,
        }
    }

    fn lines(path: &Path) -> Vec<String> {
        std::fs::read_to_string(path).unwrap().lines().map(str::to_string).collect()
    }

    #[tokio::test]
    async fn shortcut_without_input_omits_the_flag() {
        let (bins, rec) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let bin = recording_bin(bins.path(), rec.path());
        let limit = Duration::from_secs(20);

        // No input: exactly `run <name> --output-path <file>`.
        assert_eq!(run(&bin, "Get Notes", None, limit, &env).await.unwrap(), "done");
        let argv = lines(&rec.path().join("argv"));
        assert_eq!(argv.len(), 4, "{argv:?}");
        assert_eq!(argv[..3], ["run", "Get Notes", "--output-path"]);
        let output = PathBuf::from(&argv[3]);
        assert!(!argv.iter().any(|a| a == "--input-path"), "{argv:?}");
        assert!(!rec.path().join("input").exists(), "no input file was handed over");
        let folder = output.parent().unwrap().to_path_buf();
        assert!(folder.file_name().unwrap().to_string_lossy().starts_with("heron-cli-"), "a private folder: {folder:?}");
        assert!(!folder.exists(), "the private folder is removed afterwards");

        // With input: `run <name> --input-path <file> --output-path <file>`, both in one private folder.
        std::fs::remove_file(rec.path().join("argv")).unwrap();
        let input = "Buy milk\ntomorrow at 9, \"quoted\" and $HOME";
        assert_eq!(run(&bin, "Add Reminder", Some(input), limit, &env).await.unwrap(), "done");
        let argv = lines(&rec.path().join("argv"));
        assert_eq!(argv.len(), 6, "{argv:?}");
        assert_eq!([&argv[0], &argv[1], &argv[2], &argv[4]], ["run", "Add Reminder", "--input-path", "--output-path"]);
        let (input_path, output_path) = (PathBuf::from(&argv[3]), PathBuf::from(&argv[5]));
        assert_eq!(input_path.parent(), output_path.parent());
        assert_ne!(input_path, output_path);
        assert!(input_path.parent().unwrap() != folder, "each run gets its own folder");
        assert_eq!(std::fs::read_to_string(rec.path().join("input")).unwrap(), input, "the input reaches the shortcut as it is");
        assert!(!input_path.parent().unwrap().exists(), "removed afterwards");

        // Input at the limit is passed; one character more is refused, and the shortcut never runs.
        std::fs::remove_file(rec.path().join("argv")).unwrap();
        let longest = "é".repeat(MAX_INPUT_CHARS);
        assert_eq!(run(&bin, "Add Reminder", Some(&longest), limit, &env).await.unwrap(), "done");
        assert_eq!(std::fs::read_to_string(rec.path().join("input")).unwrap(), longest);
        std::fs::remove_file(rec.path().join("argv")).unwrap();
        let err = run(&bin, "Add Reminder", Some(&format!("{longest}x")), limit, &env).await.unwrap_err().to_string();
        assert!(err.contains("20001") && err.contains("20000"), "{err}");
        assert!(!rec.path().join("argv").exists(), "a refused input runs nothing");

        // Through the tool, an empty input is no input.
        let mut ctx = crate::permissions::test_ctx(false);
        let mut cfg = Config::default();
        cfg.features.shortcuts_allowed = vec!["Get Notes".into()];
        ctx.cfg = std::sync::Arc::new(cfg);
        assert_eq!(RunShortcut.detail(&json!({"name": "Get Notes", "input": ""}), &ctx), "Run the shortcut \"Get Notes\", with no input.");
        assert_eq!(RunShortcut.detail(&json!({"name": "Get Notes", "input": "milk"}), &ctx), "Run the shortcut \"Get Notes\" with this input:\nmilk");
    }

    #[tokio::test]
    async fn shortcut_output_is_read_from_the_output_file_and_capped() {
        let (bins, _rec) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let limit = Duration::from_secs(20);
        let to_output = r#"for a in "$@"; do out="$a"; done"#;

        // Stdout is ignored; a bad byte is replaced; the result is cut to 20,000 characters, counted
        // as characters, not bytes.
        let bin = fake_bin(bins.path(), &format!("{to_output}\necho STDOUT-NOISE\nprintf '\\377' > \"$out\"\nyes é | head -n 25000 | tr -d '\\n' >> \"$out\""));
        let got = run(&bin, "Get Notes", None, limit, &env).await.unwrap();
        assert_eq!(got.chars().count(), MAX_OUTPUT_CHARS);
        assert!(got.starts_with('\u{FFFD}'), "the bad byte is replaced: {:?}", &got[..8]);
        assert!(got.chars().skip(1).all(|c| c == 'é'), "nothing else is added");
        assert!(!got.contains("STDOUT-NOISE"));

        // Short output comes back whole, as it is.
        let bin = fake_bin(bins.path(), &format!("{to_output}\necho STDOUT-NOISE\nprintf 'Milk, eggs\\nBread' > \"$out\""));
        assert_eq!(run(&bin, "Get Notes", None, limit, &env).await.unwrap(), "Milk, eggs\nBread");

        // A shortcut that outputs nothing writes no file: an empty result, not an error.
        let bin = fake_bin(bins.path(), "echo STDOUT-NOISE");
        assert_eq!(run(&bin, "Get Notes", None, limit, &env).await.unwrap(), "");
    }

    #[tokio::test]
    async fn shortcut_failure_reports_exit_and_stderr() {
        let bins = tempfile::tempdir().unwrap();
        let bin = fake_bin(
            bins.path(),
            r#"for a in "$@"; do out="$a"; done
printf 'partial' > "$out"
echo ignored-stdout
printf 'BEGIN' >&2
head -c 700 /dev/zero | tr '\0' E >&2
exit 3"#,
        );
        let got = run(&bin, "Get Notes", None, Duration::from_secs(20), &env).await.unwrap();
        assert_eq!(got, format!("Shortcut Get Notes failed (exit 3): BEGIN{}", "E".repeat(495)));

        // A short stderr is shown whole; an empty one leaves the reason empty.
        let bin = fake_bin(bins.path(), "echo 'The shortcut was not found.' >&2\nexit 1");
        assert_eq!(run(&bin, "Nope", None, Duration::from_secs(20), &env).await.unwrap(), "Shortcut Nope failed (exit 1): The shortcut was not found.\n");
        let bin = fake_bin(bins.path(), "exit 7");
        assert_eq!(run(&bin, "Nope", None, Duration::from_secs(20), &env).await.unwrap(), "Shortcut Nope failed (exit 7): ");

        // A binary that is not there is an error, not a result.
        let err = run(&bins.path().join("missing"), "Get Notes", None, Duration::from_secs(20), &env).await.unwrap_err().to_string();
        assert!(err.contains("could not start"), "{err}");
    }

    /// Writes `started`, then starts a descendant; each writes its marker two seconds later unless killed.
    fn slow_bin(dir: &Path, rec: &Path) -> PathBuf {
        let rec = rec.display();
        fake_bin(dir, &format!("pwd > \"{rec}/started\"\n(sleep 2; echo late > \"{rec}/descendant\") &\nsleep 2\necho late > \"{rec}/child\""))
    }

    #[tokio::test]
    async fn shortcut_timeout_is_below_the_tool_timeout() {
        // 110 seconds, or 5 less than the agent's tool timeout when that is smaller, never under 5.
        for (tool_timeout, want) in [(120, 110), (115, 110), (600, 110), (114, 109), (60, 55), (11, 6), (10, 5), (9, 5), (5, 5), (1, 5), (0, 5)] {
            assert_eq!(run_timeout(tool_timeout), Duration::from_secs(want), "tool timeout {tool_timeout}");
        }
        assert!(run_timeout(Config::default().agent.tool_timeout_secs) < Duration::from_secs(Config::default().agent.tool_timeout_secs));

        // On timeout the message says so, and the whole process group is stopped.
        let (bins, rec) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let bin = slow_bin(bins.path(), rec.path());
        let start = Instant::now();
        let err = run(&bin, "Get Notes", None, Duration::from_secs(1), &env).await.unwrap_err().to_string();
        assert!(start.elapsed() < Duration::from_millis(1900), "stopped at the limit: {:?}", start.elapsed());
        assert_eq!(err, "Shortcut Get Notes took longer than 1 seconds and was stopped.");
        assert!(rec.path().join("started").exists(), "it did start");
        tokio::time::sleep(Duration::from_millis(2500)).await;
        assert!(!rec.path().join("child").exists(), "the shortcut was killed");
        assert!(!rec.path().join("descendant").exists(), "and what it started");
    }

    #[tokio::test]
    async fn shortcut_process_is_killed_on_drop() {
        let (bins, rec) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let bin = slow_bin(bins.path(), rec.path());
        // The agent's own tool timeout drops the call's future: its run limit is still far away.
        let started = rec.path().join("started");
        let mut call = Box::pin(run(&bin, "Get Notes", None, Duration::from_secs(60), &env));
        let deadline = Instant::now() + Duration::from_secs(10);
        while !std::fs::read_to_string(&started).is_ok_and(|s| s.ends_with('\n')) {
            assert!(Instant::now() < deadline, "the shortcut never started");
            tokio::select! {
                ended = &mut call => panic!("the run ended before it was dropped: {ended:?}"),
                _ = tokio::time::sleep(Duration::from_millis(20)) => {}
            }
        }
        drop(call);
        let folder = PathBuf::from(std::fs::read_to_string(&started).unwrap().trim());
        tokio::time::sleep(Duration::from_millis(2500)).await;
        assert!(!rec.path().join("child").exists(), "the shortcut was killed");
        assert!(!rec.path().join("descendant").exists(), "and what it started");
        assert!(!folder.exists(), "its private folder is removed: {folder:?}");
    }

    #[tokio::test]
    async fn shortcuts_gets_only_the_allowlisted_env() {
        let (bins, rec) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let r = rec.path().display();
        let bin = fake_bin(bins.path(), &format!("env > \"{r}/env-$1\"\npwd > \"{r}/pwd-$1\"\n[ \"$1\" = list ] && echo 'Get Notes'\nexit 0"));
        // Heron's environment: every allowed variable, plus secrets and its own TMPDIR.
        let mut parent: BTreeMap<String, String> = CHILD_ENV_ALLOWED.iter().map(|k| (k.to_string(), format!("value-of-{k}"))).collect();
        parent.insert("PATH".into(), "/usr/bin:/bin".into());
        for (k, v) in [("OPENAI_API_KEY", "sk-secret"), ("HERON_TOKEN", "tok"), ("TELEGRAM_BOT_TOKEN", "123:abc"), ("TMPDIR", "/tmp/heron-own"), ("HERON_SHORTCUTS_BIN", "/x")] {
            parent.insert(k.into(), v.into());
        }
        let get = |k: &str| parent.get(k).cloned();
        run(&bin, "Get Notes", None, Duration::from_secs(20), &get).await.unwrap();
        assert_eq!(list(&bin, &get).await.unwrap(), Some(vec!["Get Notes".to_string()]));

        for verb in ["run", "list"] {
            let got: BTreeMap<String, String> = lines(&rec.path().join(format!("env-{verb}"))).iter().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.to_string(), v.to_string())).collect();
            // `/bin/sh` itself exports these to what it starts; Heron passes none of them.
            let shell_own = ["PWD", "OLDPWD", "SHLVL", "_"];
            let keys: BTreeSet<&str> = got.keys().map(String::as_str).filter(|k| !shell_own.contains(k)).collect();
            let want: BTreeSet<&str> = CHILD_ENV_ALLOWED.iter().copied().chain(["TMPDIR"]).collect();
            assert_eq!(keys, want, "{verb}: exactly the allowlist and TMPDIR: {got:?}");
            for k in CHILD_ENV_ALLOWED {
                assert_eq!(got.get(*k), parent.get(*k), "{verb}: {k} is Heron's own value");
            }
            let folder = std::fs::read_to_string(rec.path().join(format!("pwd-{verb}"))).unwrap().trim().to_string();
            assert_eq!(got["TMPDIR"], folder, "{verb}: TMPDIR is the private folder it runs in, not Heron's");
            assert!(folder.contains("heron-cli-") && !Path::new(&folder).exists(), "{verb}: private and removed: {folder}");
        }
    }

    #[tokio::test]
    async fn run_shortcut_is_external_write_with_untrusted_output() {
        let registry = crate::tools::ToolRegistry::builtin();
        let tool = registry.get(TOOL).expect("run_shortcut is a built-in tool");
        let ctx = crate::permissions::test_ctx(false);
        for args in [json!({"name": "Get Notes"}), json!({"name": "Add Reminder", "input": "milk"}), json!({})] {
            assert_eq!(tool.risk(&args, &ctx), Effect::ExternalWrite, "{args}");
        }
        assert!(tool.untrusted_output());
        assert_eq!(tool.feature(), Some(KEY));
        assert!(once_only(TOOL));
        assert_eq!(tool.parameters()["required"], json!(["name"]));
        assert_eq!(tool.summary(&json!({"name": "Get Notes", "input": "x"})), "Get Notes");
        // A feature-keyed tool is not served over MCP, which has no feature filter.
        assert!(!crate::mcp::SERVED_TOOLS.contains(&TOOL));

        // The key: "Run your Shortcuts", on by default, and the tool follows it.
        let key = crate::features::find(KEY).expect("the shortcuts key is registered");
        assert_eq!((key.title, key.default), ("Run your Shortcuts", true));
        let mut cfg = Config::default();
        assert!(registry.names_for(&cfg).contains(&TOOL.to_string()));
        assert!(cfg.features.shortcuts_allowed.is_empty(), "nothing is ticked at first, so nothing runs");
        cfg.features.flags.insert(KEY.into(), false);
        assert!(!registry.names_for(&cfg).contains(&TOOL.to_string()));
        assert!(registry.get_for(&cfg, TOOL).is_none());

        // The binary: `/usr/bin/shortcuts`, or in a debug build (tests are one) a non-empty
        // `HERON_SHORTCUTS_BIN`, read on each call.
        assert_eq!(bin_from(&|k| (k == "HERON_SHORTCUTS_BIN").then(|| "/tmp/fake/shortcuts".to_string())), PathBuf::from("/tmp/fake/shortcuts"));
        assert_eq!(bin_from(&|k| (k == "HERON_SHORTCUTS_BIN").then(String::new)), PathBuf::from(REAL_BIN));
        assert_eq!(bin_from(&|_| None), PathBuf::from(REAL_BIN));
        assert_eq!(bin_from(&|k| (k == "OTHER").then(|| "/x".to_string())), PathBuf::from(REAL_BIN));
    }

    #[test]
    fn decision_table_run_shortcut() {
        let tool = RunShortcut;
        let effect = tool.risk(&json!({"name": "Add Reminder", "input": "milk"}), &crate::permissions::test_ctx(false));
        let d = || Verdict::Deny(String::new());
        let same = |got: &Verdict, want: &Verdict| matches!((got, want), (Verdict::Allow, Verdict::Allow) | (Verdict::Ask, Verdict::Ask) | (Verdict::Deny(_), Verdict::Deny(_)));
        let modes = [Mode::ReadOnly, Mode::Ask, Mode::Auto, Mode::Yolo];
        // [read-only, ask, auto, yolo]: it asks in ask and auto, and always once outside content is
        // present; it runs unasked only in yolo on a clean conversation.
        let clean = [d(), Verdict::Ask, Verdict::Ask, Verdict::Allow];
        let tainted = [d(), Verdict::Ask, Verdict::Ask, Verdict::Ask];
        let rule_sets: [&[&str]; 5] = [&[], &["*"], &["*:*"], &["run_shortcut:*"], &["run_shortcut:Add Reminder"]];
        for rules in rule_sets {
            let rules: Vec<String> = rules.iter().map(|r| r.to_string()).collect();
            for (i, mode) in modes.iter().enumerate() {
                let p = Policy::new(*mode, &rules, &[]);
                let got = decide_for(&p, TOOL, "Add Reminder", effect, false);
                assert!(same(&got, &clean[i]), "{mode:?} rules={rules:?}, clean: got {got:?}, want {:?}", clean[i]);
                let got = decide_for(&p, TOOL, "Add Reminder", effect, true);
                assert!(same(&got, &tainted[i]), "{mode:?} rules={rules:?}, tainted: got {got:?}, want {:?}", tainted[i]);
                // The control: the same rule does allow an ordinary tool with the same effect in ask
                // mode, so the rule is live and only once-only keeps it from the shortcut.
                if *mode == Mode::Ask && rules.iter().any(|r| r == "*" || r == "*:*") {
                    assert_eq!(decide_for(&p, "browser_click", "Add Reminder", effect, false), Verdict::Allow, "{rules:?}");
                }
            }
        }
        // A "never" refuses it in every mode.
        for mode in modes {
            let p = Policy::new(mode, &["*".to_string()], &["run_shortcut:*".to_string()]);
            assert!(matches!(decide_for(&p, TOOL, "Add Reminder", effect, false), Verdict::Deny(_)), "{mode:?}");
        }
        // No conversation grant, and "always" is never offered, clean or not.
        for tainted in [false, true] {
            assert!(!may_grant_for_conversation(TOOL, effect, tainted));
            let req = ApprovalRequest { tool: TOOL.into(), summary: "Add Reminder".into(), risk: effect, detail: String::new(), tainted, args: json!({}), session_id: "s".into(), always_rule: Some("run_shortcut:Add Reminder".into()) };
            assert!(!req.can_always());
        }
    }
}
