//! Shell tool: runs a command in the workspace with a timeout. The agent's own API key
//! is removed from the child environment so a command can never read or leak it.

use super::{arg_str, opt_i64, schema, FnTool, ToolCtx};
use crate::permissions::{classify_shell, Effect};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::process::Stdio;
use std::time::Duration;

fn summary(args: &Value) -> String {
    args.get("command").and_then(|c| c.as_str()).unwrap_or("").to_string()
}

fn risk(args: &Value, _ctx: &ToolCtx) -> Effect {
    classify_shell(&summary(args))
}

async fn run(args: &Value, ctx: &ToolCtx) -> Result<String> {
    let command = arg_str(args, "command")?;
    let timeout = opt_i64(args, "timeout_secs").unwrap_or(ctx.cfg.agent.tool_timeout_secs as i64).clamp(1, 1800) as u64;
    // Windows: PowerShell, falling back to cmd.exe on systems without it. Elsewhere: bash, falling back to sh.
    #[cfg(windows)]
    let candidates: [(&str, &[&str]); 2] = [("powershell.exe", &["-NoProfile", "-NonInteractive", "-Command"]), ("cmd.exe", &["/C"])];
    #[cfg(not(windows))]
    let candidates: [(&str, &[&str]); 2] = [("/bin/bash", &["-c"]), ("sh", &["-c"])];
    let chosen = std::env::var("HERON_SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| ctx.cfg.agent.shell.clone());
    // Decide by the program's own name, so a full path (C:\Windows\System32\cmd.exe, /usr/bin/pwsh) works too.
    let chosen_name = chosen.rsplit(['/', '\\']).next().unwrap_or("").to_lowercase();
    let chosen_flags: &[&str] = match chosen_name.trim_end_matches(".exe") {
        "cmd" => &["/C"],
        "powershell" | "pwsh" => &["-NoProfile", "-NonInteractive", "-Command"],
        _ => &["-c"],
    };
    let candidates: Vec<(&str, &[&str])> = if chosen.is_empty() { candidates.to_vec() } else { vec![(chosen.as_str(), chosen_flags)] };
    let grants = crate::sandbox::shell_grants(&ctx.cfg, &ctx.store, &ctx.session_id);
    let sandbox = crate::sandbox::wrap(&ctx.cfg, &ctx.workspace, &crate::sandbox::Profile::Shell { grants });
    // The classifier cannot see what a command reads at run time, so it cannot stand in for the
    // sandbox: on macOS, no sandbox means no shell.
    if sandbox.is_none() && crate::sandbox::required() {
        anyhow::bail!("the sandbox is off, so Heron won't run shell commands");
    }
    let mut spawned = None;
    let mut last_err = None;
    for (shell, flags) in candidates {
        let mut cmd = match &sandbox {
            Some((program, prefix)) => {
                let mut c = tokio::process::Command::new(program);
                c.args(prefix).arg(shell);
                c
            }
            None => tokio::process::Command::new(shell),
        };
        cmd.args(flags).arg(command).current_dir(&ctx.workspace).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        // Start from nothing and hand over only what is on the list: a command should not be able to
        // read (or send away) every token and key that happens to be in the user's environment.
        cmd.env_clear();
        for (name, value) in std::env::vars_os() {
            if env_allowed(&name.to_string_lossy(), &value.to_string_lossy(), &ctx.cfg.agent.shell_env, &ctx.cfg.llm.api_key_env) {
                cmd.env(name, value);
            }
        }
        #[cfg(unix)]
        cmd.process_group(0); // so a timeout kills the whole process tree, not just the shell
        match cmd.spawn() {
            Ok(child) => {
                spawned = Some(child);
                break;
            }
            Err(e) => last_err = Some(e),
        }
    }
    let mut child = match spawned {
        Some(c) => c,
        None => return Err(last_err.map(anyhow::Error::from).unwrap_or_else(|| anyhow::anyhow!("no shell available"))).context("could not start a shell"),
    };
    let pid = child.id();
    // Keep only as much output as could ever be shown, and throw the rest away as it arrives: a command
    // that prints without end (`cat /dev/zero`) must not be able to fill the computer's memory.
    let keep = (ctx.cfg.agent.max_tool_output_chars * 4).clamp(64 * 1024, 4 * 1024 * 1024);
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let collect = async {
        let (out, err, status) = tokio::join!(read_capped(stdout_pipe, keep), read_capped(stderr_pipe, keep), child.wait());
        status.map(|status| (out, err, status))
    };
    let waited = tokio::time::timeout(Duration::from_secs(timeout), collect).await;
    let ((stdout, stdout_cut), (stderr, stderr_cut), status) = match waited {
        Ok(r) => r?,
        Err(_) => {
            // Kill the whole tree while the shell is still there to be found. (On Windows the tree is
            // looked up through the shell's process id, so this has to happen before the shell is gone.)
            if let Some(pid) = pid {
                #[cfg(unix)]
                let _ = std::process::Command::new("kill").args(["-9", &format!("-{pid}")]).status();
                #[cfg(windows)]
                let _ = std::process::Command::new("taskkill").args(["/PID", &pid.to_string(), "/T", "/F"]).status();
            }
            let _ = child.kill().await;
            return Ok(format!("[timed out after {timeout}s and was killed. For long-running work, raise timeout_secs or run it in the background.]"));
        }
    };
    let cut_note = |cut: bool| if cut { "\n[more output than can be kept; the rest was discarded]" } else { "" };
    let stdout = format!("{}{}", String::from_utf8_lossy(&stdout), cut_note(stdout_cut));
    let stderr = format!("{}{}", String::from_utf8_lossy(&stderr), cut_note(stderr_cut));
    let mut text = String::new();
    if !stdout.trim().is_empty() {
        text.push_str(stdout.trim_end());
    }
    if !stderr.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str("[stderr]\n");
        text.push_str(stderr.trim_end());
    }
    let code = status.code().unwrap_or(-1);
    if code != 0 {
        text.push_str(&format!("\n[exit code {code}]"));
        if sandbox.is_some() && crate::sandbox::looks_denied(&text) {
            text.push('\n');
            text.push_str(crate::sandbox::DENIED_HINT);
        }
    } else if text.is_empty() {
        text.push_str("(no output, exit code 0)");
    }
    Ok(text)
}

/// Variables every command may see. Nothing here is a credential: locations, language, time zone,
/// display, and where toolchains and certificate bundles live.
const ENV_ALLOWED: &[&str] = &[
    "PATH", "HOME", "USER", "LOGNAME", "SHELL", "TERM", "COLORTERM", "LANG", "LANGUAGE", "TZ", "PWD", "TMPDIR", "TMP", "TEMP", "EDITOR", "PAGER", "DISPLAY", "WAYLAND_DISPLAY",
    "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_RUNTIME_DIR",
    // toolchains
    "CARGO_HOME", "RUSTUP_HOME", "RUSTUP_TOOLCHAIN", "GOPATH", "GOROOT", "GOCACHE", "JAVA_HOME", "NVM_DIR", "NODE_PATH", "PYENV_ROOT", "VIRTUAL_ENV", "CONDA_PREFIX", "PYTHONPATH",
    // certificate bundles (needed to reach the network through a company proxy; they are not secrets)
    "SSL_CERT_FILE", "SSL_CERT_DIR", "NODE_EXTRA_CA_CERTS", "REQUESTS_CA_BUNDLE", "CURL_CA_BUNDLE", "GIT_SSL_CAINFO", "CARGO_HTTP_CAINFO",
    // Windows
    "SYSTEMROOT", "SYSTEMDRIVE", "WINDIR", "COMSPEC", "PATHEXT", "USERPROFILE", "USERNAME", "APPDATA", "LOCALAPPDATA", "PROGRAMDATA", "PROGRAMFILES", "PROGRAMFILES(X86)", "COMMONPROGRAMFILES",
    "HOMEDRIVE", "HOMEPATH", "OS", "NUMBER_OF_PROCESSORS", "PROCESSOR_ARCHITECTURE", "PSMODULEPATH",
];
/// Proxy settings are passed on only when they carry no password.
const ENV_PROXY: &[&str] = &["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY"];

/// May a command see this variable? Names are compared without regard to case (Windows does not
/// distinguish it). The model's own key is never passed, whatever the user's list says: neither
/// Heron's own variables nor the variable the settings name as holding the key (`model_key`).
fn env_allowed(name: &str, value: &str, extra: &[String], model_key: &str) -> bool {
    let upper = name.to_uppercase();
    if upper.starts_with("HERON_") || (!model_key.is_empty() && name.eq_ignore_ascii_case(model_key)) {
        return false;
    }
    if ENV_ALLOWED.contains(&upper.as_str()) || upper.starts_with("LC_") {
        return true;
    }
    if ENV_PROXY.contains(&upper.as_str()) {
        return !value.contains('@');
    }
    extra.iter().any(|e| e.eq_ignore_ascii_case(name))
}

/// Read a pipe to its end, keeping at most `keep` bytes. Reading continues past that point (the bytes
/// are dropped) so the command is never left blocked on a full pipe. Returns what was kept and whether
/// anything was dropped.
async fn read_capped<R: tokio::io::AsyncRead + Unpin>(pipe: Option<R>, keep: usize) -> (Vec<u8>, bool) {
    use tokio::io::AsyncReadExt;
    let mut kept = Vec::new();
    let mut dropped = false;
    let Some(mut pipe) = pipe else { return (kept, dropped) };
    let mut chunk = vec![0u8; 16 * 1024];
    loop {
        match pipe.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let room = keep.saturating_sub(kept.len());
                kept.extend_from_slice(&chunk[..n.min(room)]);
                dropped |= n > room;
            }
        }
    }
    (kept, dropped)
}

pub fn tools() -> Vec<FnTool> {
    vec![FnTool {
        name: "shell",
        description: if cfg!(windows) { "Run a PowerShell command in the workspace directory and return its output and exit code. Use for builds, tests, git, scripts, and system tasks. Commands are non-interactive (no stdin)." } else { "Run a bash command in the workspace directory and return its output and exit code. Use for builds, tests, git, scripts, and system tasks. Commands are non-interactive (no stdin)." },
        parameters: schema(json!({"command": {"type": "string"}, "timeout_secs": {"type": "integer", "description": "Default 120, max 1800"}}), &["command"]),
        summary,
        risk,
        detail: None,
        untrusted: false,
        target: None,
        run: |a, c| Box::pin(run(a, c)),
    }]
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::memory::Memory;
    use crate::store::Store;
    use std::sync::Arc;

    #[tokio::test]
    async fn commands_see_a_short_list_of_variables_not_the_whole_environment() {
        let ws = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.llm.api_key_env = "TEST_MODEL_KEY_OF_HERON".into();
        // What a real environment looks like: the model's key, a cloud key, a token, a proxy with a password.
        for (k, v) in [
            ("TEST_MODEL_KEY_OF_HERON", "sk-model-secret"), ("AWS_SECRET_ACCESS_KEY", "aws-secret"), ("GITHUB_TOKEN", "ghp_secret"), ("HERON_TEST_WANTED", "needed-by-the-build"),
            ("HERON_TEST_PROXY_PROBE", "x"),
        ] {
            std::env::set_var(k, v);
        }
        let store = Store::in_memory().unwrap();
        let ctx = ToolCtx { workspace: dunce::canonicalize(ws.path()).unwrap(), memory: Memory::new(store.clone(), None), store, session_id: "t".into(), cfg: Arc::new(cfg.clone()), tainted: Default::default(), seen_urls: Default::default() };
        let seen = run(&json!({"command": "env"}), &ctx).await.unwrap();
        for secret in ["sk-model-secret", "aws-secret", "ghp_secret", "needed-by-the-build"] {
            assert!(!seen.contains(secret), "a command could read {secret}");
        }
        assert!(seen.contains("PATH=") && seen.contains("HOME="), "ordinary commands still work: {seen}");
        assert!(run(&json!({"command": "ls / > /dev/null && echo fine"}), &ctx).await.unwrap().contains("fine"));

        // The user can hand over a variable by name; the model's own key stays out regardless.
        cfg.agent.shell_env = vec!["GITHUB_TOKEN".into(), "HERON_API_KEY".into(), "test_model_key_of_heron".into()];
        std::env::set_var("HERON_API_KEY", "sk-heron-secret");
        let seen = run(&json!({"command": "env"}), &ToolCtx { cfg: Arc::new(cfg), ..ctx.clone() }).await.unwrap();
        std::env::remove_var("HERON_API_KEY");
        assert!(seen.contains("ghp_secret"), "{seen}");
        assert!(!seen.contains("sk-heron-secret") && !seen.contains("aws-secret"));
        assert!(!seen.contains("sk-model-secret"), "listing the variable that holds the model's key does not hand it over");

        // The rule itself, including a proxy address that carries a password.
        assert!(env_allowed("Path", "x", &[], "") && env_allowed("LC_ALL", "C", &[], "") && env_allowed("SSL_CERT_FILE", "/x", &[], ""));
        assert!(env_allowed("HTTPS_PROXY", "http://proxy.example:8080", &[], ""));
        assert!(!env_allowed("HTTPS_PROXY", "http://user:pw@proxy.example:8080", &[], ""));
        let listed = ["OPENROUTER_API_KEY".to_string()];
        assert!(env_allowed("OPENROUTER_API_KEY", "x", &listed, "SOME_OTHER_KEY"), "a listed variable that is not the model's key is passed");
        assert!(!env_allowed("OPENROUTER_API_KEY", "x", &listed, "OPENROUTER_API_KEY"), "the model's key is not, even when listed");
        for hidden in ["OPENAI_API_KEY", "OPENROUTER_API_KEY", "AWS_SESSION_TOKEN", "SSH_AUTH_SOCK", "NPM_TOKEN", "DATABASE_URL", "HERON_HOME"] {
            assert!(!env_allowed(hidden, "x", &[], ""), "{hidden}");
        }
    }

    #[tokio::test]
    async fn endless_output_is_cut_off_instead_of_filling_memory() {
        let ws = tempfile::tempdir().unwrap();
        let cfg = Config::default();
        let store = Store::in_memory().unwrap();
        let ctx = ToolCtx { workspace: dunce::canonicalize(ws.path()).unwrap(), memory: Memory::new(store.clone(), None), store, session_id: "t".into(), cfg: Arc::new(cfg), tainted: Default::default(), seen_urls: Default::default() };
        // About 50 MB of output; only a bounded part is kept, and the command still finishes normally.
        let out = run(&json!({"command": "head -c 50000000 /dev/zero | tr '\\0' 'x'; echo; echo finished"}), &ctx).await.unwrap();
        assert!(out.len() < 5 * 1024 * 1024, "kept {} bytes", out.len());
        assert!(out.contains("the rest was discarded"), "{}", &out[out.len().saturating_sub(200)..]);
        // A full path to the shell picks the right way to pass the command.
        let mut named = (*ctx.cfg).clone();
        named.agent.shell = "/bin/sh".into();
        let out = run(&json!({"command": "echo by-path"}), &ToolCtx { cfg: Arc::new(named), ..ctx.clone() }).await.unwrap();
        assert!(out.contains("by-path"), "{out}");
    }

    #[tokio::test]
    async fn sandboxed_commands_cannot_write_outside_the_folder_or_read_secrets() {
        if crate::sandbox::available() == crate::sandbox::Kind::None {
            eprintln!("no sandbox on this machine; skipping");
            return;
        }
        let ws = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let cfg = Config { home: dunce::canonicalize(home.path()).unwrap(), ..Default::default() };
        std::fs::write(cfg.home.join("secrets.json"), "TOPSECRET-KEY").unwrap();
        // A place outside the working folder that is not a temporary folder: the user's home.
        let outside = dirs::home_dir().unwrap().join(format!(".heron-sandbox-test-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&outside).unwrap();
        let store = Store::in_memory().unwrap();
        let workspace = dunce::canonicalize(ws.path()).unwrap();
        let ctx = ToolCtx { workspace: workspace.clone(), memory: Memory::new(store.clone(), None), store, session_id: "t".into(), cfg: Arc::new(cfg.clone()), tainted: Default::default(), seen_urls: Default::default() };
        let sh = |command: String| {
            let ctx = ctx.clone();
            async move { run(&json!({"command": command}), &ctx).await.unwrap() }
        };

        // Normal work in the folder is unaffected, including temporary files and pipes.
        let out = sh("echo built > result.txt && cat result.txt && echo scratch > \"${TMPDIR:-/tmp}/heron-t.txt\" && echo ok | tr a-z A-Z".into()).await;
        assert!(out.contains("built") && out.contains("OK"), "{out}");
        assert_eq!(std::fs::read_to_string(workspace.join("result.txt")).unwrap().trim(), "built");

        // Writing elsewhere fails and leaves nothing behind; the model is told why.
        let out = sh(format!("echo pwned > '{}/x.txt'", outside.display())).await;
        assert!(!outside.join("x.txt").exists(), "a sandboxed command wrote outside the working folder: {out}");
        assert!(out.contains("exit code") && out.contains("Heron's sandbox"), "{out}");

        // Heron's own data (and with it the saved keys) cannot be read, whatever the command line looks like.
        let out = sh(format!("cat '{}/secrets.json' || true; ls '{}' || true", cfg.home.display(), cfg.home.display())).await;
        assert!(!out.contains("TOPSECRET"), "{out}");

        // With the sandbox switched off: on macOS the shell is refused outright; elsewhere the same
        // write goes through (so the test really tested the sandbox).
        let mut open = cfg.clone();
        open.agent.sandbox = "off".into();
        let ctx_off = ToolCtx { cfg: Arc::new(open), ..ctx.clone() };
        let result = run(&json!({"command": format!("echo fine > '{}/y.txt'", outside.display())}), &ctx_off).await;
        if crate::sandbox::required() {
            assert!(result.unwrap_err().to_string().contains("the sandbox is off, so Heron won't run shell commands"));
            assert!(!outside.join("y.txt").exists());
        } else {
            result.unwrap();
            assert!(outside.join("y.txt").exists());
        }
        std::fs::remove_dir_all(&outside).unwrap();
    }

    #[tokio::test]
    async fn shell_refused_when_sandbox_off() {
        let ws = tempfile::tempdir().unwrap();
        let workspace = dunce::canonicalize(ws.path()).unwrap();
        let mut cfg = Config::default();
        cfg.agent.sandbox = "off".into();
        let ctx = test_ctx(&cfg, &workspace, "t");
        let result = run(&json!({"command": "echo ran > marker.txt && echo done"}), &ctx).await;
        if cfg!(target_os = "macos") {
            let err = result.expect_err("on macOS the shell is refused with the sandbox off");
            assert_eq!(err.to_string(), "the sandbox is off, so Heron won't run shell commands");
            assert!(!workspace.join("marker.txt").exists(), "nothing ran");
        } else {
            assert!(result.unwrap().contains("done"), "elsewhere commands run unsandboxed, as before");
            assert!(workspace.join("marker.txt").exists());
        }
    }

    // ---------- the deny-default sandbox (Seatbelt) ----------

    fn test_ctx(cfg: &Config, workspace: &std::path::Path, session: &str) -> ToolCtx {
        let store = Store::in_memory().unwrap();
        ToolCtx { workspace: workspace.to_path_buf(), memory: Memory::new(store.clone(), None), store, session_id: session.into(), cfg: Arc::new(cfg.clone()), tainted: Default::default(), seen_urls: Default::default() }
    }

    /// A temporary folder inside the home folder (the user roots are what the profile denies), removed
    /// when dropped.
    struct HomeTemp(std::path::PathBuf);
    impl HomeTemp {
        fn new() -> HomeTemp {
            let home = dirs::home_dir().unwrap();
            let dir = home.join(format!(".heron-sandbox-test-{}", uuid::Uuid::new_v4().simple()));
            std::fs::create_dir_all(&dir).unwrap();
            HomeTemp(dunce::canonicalize(&dir).unwrap())
        }
        fn put(&self, rel: &str, text: &str) -> std::path::PathBuf {
            let p = self.0.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, text).unwrap();
            p
        }
    }
    impl Drop for HomeTemp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The Seatbelt tests run only where Seatbelt is in effect, and say so elsewhere.
    fn seatbelt(cfg: &Config) -> bool {
        if crate::sandbox::active(cfg) == crate::sandbox::Kind::Seatbelt {
            return true;
        }
        eprintln!("Seatbelt is not in effect here ({}); skipping", crate::sandbox::active(cfg).name());
        false
    }

    async fn sh(ctx: &ToolCtx, command: &str) -> String {
        run(&json!({"command": command}), ctx).await.unwrap()
    }

    fn q(p: &std::path::Path) -> String {
        format!("'{}'", p.display().to_string().replace('\'', "'\\''"))
    }

    #[tokio::test]
    async fn sandboxed_shell_cannot_read_ask_tier_paths() {
        let mut cfg = Config::default();
        cfg.permissions.mode = "auto".into();
        if !seatbelt(&cfg) {
            return;
        }
        let tmp = HomeTemp::new();
        let ws = tmp.0.join("project");
        std::fs::create_dir_all(&ws).unwrap();
        let private = tmp.put("elsewhere/diary.txt", "ASK-TIER-CONTENT");
        tmp.put("project/readme.txt", "WORKSPACE-CONTENT");
        let ctx = test_ctx(&cfg, &ws, "t");
        // The working folder reads as usual.
        assert!(sh(&ctx, "cat readme.txt").await.contains("WORKSPACE-CONTENT"));
        // The rest of the home folder does not, however it is asked for.
        for cmd in [format!("cat {}", q(&private)), format!("python3 -c \"print(open('{}').read())\"", private.display()), format!("cd {} && cat ../elsewhere/diary.txt", q(&ws))] {
            let out = sh(&ctx, &cmd).await;
            assert!(!out.contains("ASK-TIER-CONTENT"), "{cmd}: {out}");
            assert!(out.contains("[exit code"), "{cmd}: {out}");
        }
        let out = sh(&ctx, &format!("cat {}", q(&private))).await;
        assert!(out.contains("read_file"), "the hint points at read_file: {out}");
    }

    /// `/System/Volumes/Data<$HOME>/...` names the same file as `$HOME/...`, and `realpath` leaves it
    /// as written: the profile denies the alias as it denies the path.
    #[tokio::test]
    async fn firmlink_alias_is_denied_in_the_sandbox() {
        let cfg = Config::default();
        if !seatbelt(&cfg) {
            return;
        }
        let tmp = HomeTemp::new();
        let cfg = Config { home: tmp.0.join("heron-home"), ..Default::default() };
        let ws = tmp.0.join("project");
        std::fs::create_dir_all(&ws).unwrap();
        let private = tmp.put("elsewhere/diary.txt", "ASK-TIER-CONTENT");
        let refused = tmp.put("heron-home/config.toml", "REFUSED-CONTENT");
        let alias = |p: &std::path::Path| std::path::PathBuf::from(format!("{}{}", crate::permissions::DATA_VOLUME, p.display()));
        if std::fs::read_to_string(alias(&private)).ok().as_deref() != Some("ASK-TIER-CONTENT") {
            eprintln!("{} does not name the same file here; skipping", alias(&private).display());
            return;
        }
        let ctx = test_ctx(&cfg, &ws, "t");
        for file in [alias(&private), alias(&refused)] {
            for cmd in [format!("cat {}", q(&file)), format!("python3 -c \"print(open('{}').read())\"", file.display())] {
                let out = sh(&ctx, &cmd).await;
                assert!(!out.contains("-CONTENT"), "{cmd}: {out}");
                assert!(out.contains("[exit code"), "{cmd}: {out}");
            }
        }
    }

    #[tokio::test]
    async fn sandbox_cannot_read_cargo_credentials_via_python() {
        let cfg = Config::default();
        if !seatbelt(&cfg) {
            return;
        }
        let tmp = HomeTemp::new();
        let ws = tmp.0.join("project");
        tmp.put("project/.cargo/config.toml", "[build]\njobs = 1\n");
        tmp.put("project/.cargo/credentials.toml", "[registry]\ntoken = \"CARGO-TOKEN-SECRET\"\n");
        tmp.put("granted/.cargo/credentials.toml", "[registry]\ntoken = \"GRANTED-TOKEN-SECRET\"\n");
        let grants = format!("[\"read:{}/**\"]", tmp.0.join("granted").display());
        let ctx = test_ctx(&cfg, &ws, "t");
        ctx.store.kv_set("grants:t", &grants).unwrap();
        // Python runs, and the folder is readable: the config next to the credentials reads.
        assert!(sh(&ctx, "python3 -c \"print(open('.cargo/config.toml').read())\"").await.contains("jobs = 1"));
        for path in [ws.join(".cargo/credentials.toml"), tmp.0.join("granted/.cargo/credentials.toml")] {
            let out = sh(&ctx, &format!("python3 -c \"print(open('{}').read())\"", path.display())).await;
            assert!(!out.contains("TOKEN-SECRET"), "{out}");
            assert!(out.contains("[exit code"), "{out}");
        }
        // The real one, if there is one, is covered by the same rule (only the outcome is printed).
        let real = dirs::home_dir().unwrap().join(".cargo/credentials.toml");
        let out = sh(&ctx, &format!("python3 -c \"open('{}').read()\" >/dev/null 2>&1 && echo READABLE || echo DENIED", real.display())).await;
        assert!(out.contains("DENIED"), "{out}");
    }

    #[tokio::test]
    async fn granted_folder_is_readable_in_the_sandbox() {
        let mut cfg = Config::default();
        if !seatbelt(&cfg) {
            return;
        }
        let tmp = HomeTemp::new();
        let ws = tmp.0.join("project");
        std::fs::create_dir_all(&ws).unwrap();
        let talk = tmp.put("talk/notes.txt", "GRANTED-FOR-THIS-CONVERSATION");
        let standing = tmp.put("standing/notes.txt", "GRANTED-STANDING");
        let other = tmp.put("other/notes.txt", "NOT-GRANTED");
        cfg.permissions.allow = vec![format!("read:{}/**", tmp.0.join("standing").display())];
        let ctx = test_ctx(&cfg, &ws, "s1");
        ctx.store.kv_set("grants:s1", &serde_json::to_string(&[format!("read:{}/**", tmp.0.join("talk").display())]).unwrap()).unwrap();
        assert!(sh(&ctx, &format!("cat {}", q(&talk))).await.contains("GRANTED-FOR-THIS-CONVERSATION"));
        assert!(sh(&ctx, &format!("python3 -c \"print(open('{}').read())\"", standing.display())).await.contains("GRANTED-STANDING"));
        assert!(!sh(&ctx, &format!("cat {}", q(&other))).await.contains("NOT-GRANTED"));
        // Another conversation's grant does not open the folder here.
        let elsewhere = ToolCtx { session_id: "s2".into(), ..ctx.clone() };
        assert!(!sh(&elsewhere, &format!("cat {}", q(&talk))).await.contains("GRANTED-FOR-THIS-CONVERSATION"));
    }

    #[tokio::test]
    async fn build_and_git_still_work_under_deny_default_reads() {
        let cfg = Config::default();
        if !seatbelt(&cfg) {
            return;
        }
        let tmp = HomeTemp::new();
        let ws = tmp.0.join("fixture");
        tmp.put("fixture/Cargo.toml", "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n");
        tmp.put("fixture/src/main.rs", "fn main() {\n    println!(\"built\");\n}\n");
        tmp.put("fixture/package.json", "{\"name\": \"fixture\", \"version\": \"1.0.0\", \"private\": true, \"scripts\": {\"test\": \"node test.js\"}}\n");
        tmp.put("fixture/test.js", "require('assert').strictEqual(1 + 1, 2);\nconsole.log('npm-test-passed');\n");
        tmp.put("fixture/test_fixture.py", "import unittest\n\nclass T(unittest.TestCase):\n    def test_adds(self):\n        self.assertEqual(1 + 1, 2)\n");
        let ctx = test_ctx(&cfg, &ws, "t");
        let mut tool_timeout = (*ctx.cfg).clone();
        tool_timeout.agent.tool_timeout_secs = 600;
        let ctx = ToolCtx { cfg: Arc::new(tool_timeout), ..ctx };
        for (cmd, expect) in [
            ("cargo build --offline && ./target/debug/fixture", "built"),
            ("npm test", "npm-test-passed"),
            ("python3 -m unittest -v", "OK"),
            ("git init -q && git status", "No commits yet"),
        ] {
            let out = sh(&ctx, cmd).await;
            assert!(!out.contains("[exit code"), "`{cmd}` failed under the sandbox:\n{out}");
            assert!(out.contains(expect), "`{cmd}`:\n{out}");
        }
    }

    #[tokio::test]
    async fn every_registry_config_dir_is_refused_in_the_sandbox() {
        let mut cfg = Config::default();
        if !seatbelt(&cfg) {
            return;
        }
        let tmp = HomeTemp::new();
        let ws = tmp.0.join("project");
        // A configured account folder inside the working folder: Refused still wins there.
        let account = tmp.put("project/acct/notes.txt", "ACCOUNT-FOLDER-CONTENT");
        cfg.llm.cli_config_dir = tmp.0.join("project/acct").display().to_string();
        let home = dirs::home_dir().unwrap();
        let mut grants = vec![];
        let mut dirs: Vec<std::path::PathBuf> = vec![];
        for spec in crate::cli_llm::providers() {
            for d in spec.config_dirs {
                let path = home.join(d.trim_start_matches("~/"));
                grants.push(format!("read:{}/**", path.display()));
                dirs.push(path);
            }
        }
        let ctx = test_ctx(&cfg, &ws, "t");
        // Even a grant on them opens none of them.
        ctx.store.kv_set("grants:t", &serde_json::to_string(&grants).unwrap()).unwrap();
        // A file next to it in the working folder reads; the one in the account folder does not.
        tmp.put("project/plain.txt", "WORKSPACE-CONTENT");
        assert!(sh(&ctx, "cat plain.txt").await.contains("WORKSPACE-CONTENT"));
        let out = sh(&ctx, &format!("cat {}", q(&account))).await;
        assert!(!out.contains("ACCOUNT-FOLDER-CONTENT"), "{out}");
        // The real folders: only whether they could be read is printed, never what is in them.
        for dir in dirs.iter().filter(|d| d.exists()) {
            let probe = if dir.is_dir() { format!("ls -A {} >/dev/null 2>&1", q(dir)) } else { format!("head -c1 {} >/dev/null 2>&1", q(dir)) };
            let out = sh(&ctx, &format!("{probe} && echo READABLE || echo DENIED")).await;
            assert!(out.contains("DENIED"), "{} is readable in the sandbox", dir.display());
        }
        // And the profile names every one of them, present or not.
        let profile = crate::sandbox::seatbelt_profile(&cfg, &home, &ws, &crate::sandbox::Profile::Shell { grants: vec![] });
        let refused = &profile[profile.find("(deny file-read* file-write*").unwrap()..];
        for dir in &dirs {
            assert!(refused.contains(&format!("(subpath \"{}\")", dir.display())), "{} missing from the Refused block", dir.display());
        }
    }

    #[tokio::test]
    async fn m4_sandbox_denies_globs_quotes_and_unlisted_secrets() {
        let cfg = Config::default();
        if !seatbelt(&cfg) {
            return;
        }
        let tmp = HomeTemp::new();
        let ws = tmp.0.join("project");
        tmp.put("project/notes.txt", "ORDINARY-NOTES");
        tmp.put("project/.env", "DOTENV-SECRET");
        tmp.put("project/.config/gh/hosts.yml", "GH-TOKEN-SECRET");
        tmp.put("project/.cargo/credentials.toml", "CARGO-TOKEN-SECRET");
        tmp.put("project/.zsh_history", "HISTORY-SECRET");
        tmp.put("project/Default/Login Data", "BROWSER-LOGIN-SECRET");
        let ctx = test_ctx(&cfg, &ws, "t");
        assert!(sh(&ctx, "cat notes.txt").await.contains("ORDINARY-NOTES"));
        for cmd in [
            "cat .en?",
            "cat \".env\"",
            "cat '.e''nv'",
            "cat \\.env",
            "f=.env; cat \"$f\"",
            "python3 -c \"print(open('.env').read())\"",
            "cat .config/gh/hosts.yml",
            "cat .cargo/credentials.toml",
            "cat .zsh_history",
            "cat Default/Login\\ Data",
        ] {
            let out = sh(&ctx, cmd).await;
            assert!(!out.contains("-SECRET"), "{cmd}: {out}");
            assert!(out.contains("[exit code"), "{cmd}: {out}");
        }
    }

    /// Creates a throwaway keychain in `dir` holding one item, outside the sandbox. The owner's login
    /// Keychain is never touched; the keychain is deleted again when dropped.
    struct TestKeychain(std::path::PathBuf);
    impl TestKeychain {
        fn new(dir: &std::path::Path) -> Option<TestKeychain> {
            let path = dir.join("test.keychain-db");
            let ok = |args: &[&str]| std::process::Command::new("security").args(args).output().map(|o| o.status.success()).unwrap_or(false);
            let p = path.to_string_lossy().to_string();
            if !ok(&["create-keychain", "-p", "heron-test-pw", &p]) {
                return None;
            }
            let kc = TestKeychain(path);
            (ok(&["unlock-keychain", "-p", "heron-test-pw", &p]) && ok(&["add-generic-password", "-s", "heron-test", "-a", "heron", "-w", "KEYCHAIN-ITEM-SECRET", &p])).then_some(kc)
        }
        fn lookup(&self) -> Vec<String> {
            ["find-generic-password", "-s", "heron-test", "-w", &self.0.to_string_lossy()].iter().map(|s| s.to_string()).collect()
        }
    }
    impl Drop for TestKeychain {
        fn drop(&mut self) {
            let _ = std::process::Command::new("security").args(["delete-keychain", &self.0.to_string_lossy()]).output();
        }
    }

    /// Run `security` with these arguments under a sandbox prefix; returns whether it succeeded and its output.
    fn under(prefix: &(String, Vec<String>), args: &[String]) -> (bool, String) {
        let out = std::process::Command::new(&prefix.0).args(&prefix.1).arg("/usr/bin/security").args(args).output().unwrap();
        (out.status.success(), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
    }

    // The throwaway keychain is made with `security create-keychain` in a temporary folder and
    // deleted again at the end; the owner's login Keychain is never read or written.
    #[test]
    fn shell_profile_cannot_reach_keychain() {
        let cfg = Config::default();
        if !seatbelt(&cfg) {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let dir = dunce::canonicalize(tmp.path()).unwrap();
        let kc = TestKeychain::new(&dir).expect("could not make a test keychain");
        let plain = std::process::Command::new("security").args(kc.lookup()).output().unwrap();
        assert!(String::from_utf8_lossy(&plain.stdout).contains("KEYCHAIN-ITEM-SECRET"), "the lookup works outside the sandbox");
        let prefix = crate::sandbox::wrap(&cfg, &dir, &crate::sandbox::Profile::Shell { grants: vec![] }).unwrap();
        let (ok, out) = under(&prefix, &kc.lookup());
        assert!(!ok && !out.contains("KEYCHAIN-ITEM-SECRET"), "{out}");
    }

    #[test]
    fn cli_profile_reaches_keychain_only_for_keychain_clis() {
        let cfg = Config::default();
        if !seatbelt(&cfg) {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let dir = dunce::canonicalize(tmp.path()).unwrap();
        let kc = TestKeychain::new(&dir).expect("could not make a test keychain");
        for (store, reaches) in [(crate::cli_llm::CredentialStore::MacosKeychain, true), (crate::cli_llm::CredentialStore::HomeFolder, false)] {
            let spec = crate::cli_llm::CliSpec { credential_store: store, ..crate::cli_llm::spec_for("codex-cli").unwrap() };
            let prefix = crate::sandbox::wrap(&cfg, &dir, &crate::sandbox::Profile::Cli { spec: &spec, binary_dir: "/usr/bin".into(), extra_read: vec![] }).unwrap();
            let (ok, out) = under(&prefix, &kc.lookup());
            assert_eq!(ok && out.contains("KEYCHAIN-ITEM-SECRET"), reaches, "{store:?}: {out}");
        }
    }
}
