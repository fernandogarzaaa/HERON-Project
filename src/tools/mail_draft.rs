//! `draft_email` (F8): a filled-in compose window in the user's default mail app. Heron never sends;
//! the owner reads the draft and presses Send.
//!
//! The draft goes out as one `mailto:` URL handed to `/usr/bin/open`. The model chooses the
//! addresses, the subject and the body, so the URL is built here and never taken from it: each
//! address must be a plain one (no `?`, `&` or `=` that could start hidden parameters), there are at
//! most [`MAX_RECIPIENTS`], and the only parameters ever set are `subject` and `body`, encoded through
//! the `url` crate. So `cc`, `bcc` and attachment parameters can't be slipped in. Arguments that
//! break these rules are refused before the permission decision ([`Tool::refusal`]) and again when
//! the call runs, so a call approved later is checked the same way.
//!
//! The effect is `ExternalWrite` and the tool is approved one call at a time
//! ([`crate::permissions::once_only`]): the content leaves Heron for another app, one click away from
//! being sent.

use super::{schema, Tool, ToolCtx};
use crate::permissions::Effect;
use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

/// The tool's name.
pub const NAME: &str = "draft_email";
/// The core feature key the tool belongs to.
pub const FEATURE: &str = "email_drafts";
/// At most this many addresses in one draft.
pub const MAX_RECIPIENTS: usize = 10;
/// The body is capped at this many characters, because mail apps cut long `mailto:` URLs.
pub const MAX_BODY_CHARS: usize = 2_000;
/// What a model gets for an address that isn't a plain one.
pub const NOT_PLAIN: &str = "That isn't a plain email address.";
/// The only program the tool runs. It hands the URL to Launch Services.
const OPEN: &str = "/usr/bin/open";
/// How long `open` may take before it is stopped.
const TIMEOUT: Duration = Duration::from_secs(10);

/// A draft whose arguments passed every check. The approval text and the URL are both built from
/// this one value, so the owner approves exactly what is opened.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Draft {
    to: Vec<String>,
    subject: String,
    body: String,
}

fn plain_address(s: &str) -> bool {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"^[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}$").unwrap()).is_match(s)
}

/// The checked draft, or the reason the call is refused.
fn parse(args: &Value) -> Result<Draft, String> {
    let Some(Value::Array(list)) = args.get("to") else {
        return Err(NOT_PLAIN.into());
    };
    if list.len() > MAX_RECIPIENTS {
        return Err(format!("{NOT_PLAIN} A draft can have at most {MAX_RECIPIENTS} addresses."));
    }
    let mut to = Vec::with_capacity(list.len());
    for entry in list {
        match entry.as_str() {
            Some(a) if plain_address(a) => to.push(a.to_string()),
            _ => return Err(NOT_PLAIN.into()),
        }
    }
    let text = |key: &str| args.get(key).and_then(Value::as_str).map(str::to_string).ok_or_else(|| format!("missing required string argument '{key}'"));
    let (subject, body) = (text("subject")?, text("body")?);
    if body.chars().count() > MAX_BODY_CHARS {
        return Err(format!("The body is longer than {MAX_BODY_CHARS} characters, which mail apps cut off. Shorten it and try again."));
    }
    Ok(Draft { to, subject, body })
}

/// The `mailto:` URL for a checked draft: the addresses joined by `,`, then exactly two query
/// parameters, `subject` and `body`.
///
/// The parameters are encoded with the `url` crate's form encoder, with its `+` for a space turned
/// into `%20` (a real `+` is already `%2B`), because mail apps show a `+` in a `mailto:` URL as it
/// is. The address check allows `%`, which a mail app would decode, so `a%2Cb@x.com` could become two
/// recipients: each `%` is written as `%25`, so an address reaches the mail app exactly as checked.
fn mailto(d: &Draft) -> url::Url {
    let encode = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>().replace('+', "%20");
    let to = d.to.iter().map(|a| a.replace('%', "%25")).collect::<Vec<_>>().join(",");
    let mut url = url::Url::parse(&format!("mailto:{to}")).expect("checked addresses always make a valid mailto URL");
    url.set_query(Some(&format!("subject={}&body={}", encode(&d.subject), encode(&d.body))));
    url
}

/// The program to run: `/usr/bin/open`, or in a debug build the one `HERON_OPEN_BIN` names, so tests
/// and feature tasks can use a fake. A release build never reads the variable.
fn open_bin_from(get: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    #[cfg(debug_assertions)]
    if let Some(fake) = get("HERON_OPEN_BIN").filter(|p| !p.is_empty()) {
        return PathBuf::from(fake);
    }
    #[cfg(not(debug_assertions))]
    let _ = get;
    PathBuf::from(OPEN)
}

/// Run `bin` with the URL as its only argument, with the environment allowlist, a private folder
/// as `TMPDIR` and working directory, and a time limit. The child is killed if the call is dropped
/// or takes too long.
async fn open(bin: &Path, url: &str, limit: Duration) -> Result<()> {
    let tmp = crate::child::private_temp_dir()?;
    let mut cmd = tokio::process::Command::new(bin);
    cmd.arg(url)
        .env_clear()
        .envs(crate::child::child_env_base(&tmp.dir, &crate::child::own_var))
        .current_dir(&tmp.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = cmd.spawn().map_err(|e| anyhow!("couldn't start {}: {e}", bin.display()))?;
    let out = tokio::time::timeout(limit, child.wait_with_output()).await.map_err(|_| anyhow!("the mail app didn't open within {} seconds, so it was stopped", limit.as_secs()))??;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!("the mail app couldn't be opened ({}): {}", out.status, crate::util::truncate_middle(err.trim(), 500));
    }
    Ok(())
}

/// The `draft_email` tool.
pub struct DraftEmail {
    bin: PathBuf,
    timeout: Duration,
}

impl DraftEmail {
    /// The tool as Heron runs it (see [`open_bin_from`]).
    pub fn from_env() -> Self {
        Self { bin: open_bin_from(&crate::child::own_var), timeout: TIMEOUT }
    }

    /// The tool, running `bin` in place of `open`. For tests.
    #[cfg(test)]
    pub(crate) fn with_bin(bin: impl Into<PathBuf>) -> Self {
        Self { bin: bin.into(), timeout: TIMEOUT }
    }
}

#[async_trait]
impl Tool for DraftEmail {
    fn name(&self) -> &str {
        NAME
    }

    fn description(&self) -> &str {
        "Open a filled-in compose window in the user's default mail app. It never sends: the user reads the draft and presses Send. `to` holds up to 10 plain email addresses (name@example.com, nothing else). The body may be at most 2,000 characters, because mail apps cut longer drafts. The user approves every draft. Returns \"opened\"."
    }

    fn parameters(&self) -> Value {
        schema(
            json!({
                "to": {"type": "array", "items": {"type": "string"}, "maxItems": MAX_RECIPIENTS, "description": "Plain email addresses, such as name@example.com."},
                "subject": {"type": "string"},
                "body": {"type": "string", "maxLength": MAX_BODY_CHARS, "description": "Plain text, at most 2,000 characters."}
            }),
            &["to", "subject", "body"],
        )
    }

    fn summary(&self, args: &Value) -> String {
        let to: Vec<&str> = args.get("to").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        let subject = args.get("subject").and_then(Value::as_str).unwrap_or("");
        format!("to {}: {subject}", to.join(", "))
    }

    fn risk(&self, _args: &Value, _ctx: &ToolCtx) -> Effect {
        Effect::ExternalWrite
    }

    /// The recipients, the subject and the full body.
    fn detail(&self, args: &Value, _ctx: &ToolCtx) -> String {
        match parse(args) {
            Ok(d) => format!("Open an email draft in your mail app (Heron won't send it)\nTo: {}\nSubject: {}\n\n{}", d.to.join(", "), d.subject, d.body),
            Err(reason) => format!("Refused: {reason}"),
        }
    }

    /// It returns only "opened".
    fn untrusted_output(&self) -> bool {
        false
    }

    fn feature(&self) -> Option<&'static str> {
        Some(FEATURE)
    }

    fn refusal(&self, args: &Value, _ctx: &ToolCtx) -> Option<String> {
        parse(args).err()
    }

    async fn call(&self, args: &Value, _ctx: &ToolCtx) -> Result<String> {
        let draft = parse(args).map_err(|reason| anyhow!(reason))?;
        open(&self.bin, mailto(&draft).as_str(), self.timeout).await?;
        Ok("opened".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{null_sink, Agent};
    use crate::llm::{Message, ScriptedLlm};
    use crate::permissions::{decide_for, test_ctx, Approval, ApprovalRequest, Approver, Mode, Policy, Verdict};
    use crate::tools::ToolRegistry;
    use std::sync::{Arc, Mutex};

    /// A fake `open` in its own folder: it writes its argument count and each argument next to itself.
    struct FakeOpen {
        dir: tempfile::TempDir,
    }

    impl FakeOpen {
        fn new() -> Self {
            Self::with_script(
                "printf '%s\\n' \"$#\" >> \"$d/argc\"\nfor a in \"$@\"; do printf '%s\\n' \"$a\" >> \"$d/args\"; done\nprintf '%s\\n' \"${CARGO_MANIFEST_DIR-unset}\" \"${HOME-unset}\" \"${TMPDIR-unset}\" \"$PWD\" > \"$d/env\"\n",
            )
        }

        fn with_script(body: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let bin = dir.path().join("open");
            std::fs::write(&bin, format!("#!/bin/sh\nd=\"${{0%/*}}\"\n{body}")).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            Self { dir }
        }

        fn tool(&self) -> DraftEmail {
            DraftEmail::with_bin(self.dir.path().join("open"))
        }

        /// One line per argument, over every run.
        fn args(&self) -> Vec<String> {
            std::fs::read_to_string(self.dir.path().join("args")).map(|s| s.lines().map(str::to_string).collect()).unwrap_or_default()
        }

        /// The argument count of each run.
        fn runs(&self) -> Vec<String> {
            std::fs::read_to_string(self.dir.path().join("argc")).map(|s| s.lines().map(str::to_string).collect()).unwrap_or_default()
        }
    }

    fn args(to: &[&str], subject: &str, body: &str) -> Value {
        json!({"to": to, "subject": subject, "body": body})
    }

    /// The query of a built URL, decoded, in order.
    fn query(url: &url::Url) -> Vec<(String, String)> {
        url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect()
    }

    /// Records each approval request and gives the same answer every time.
    struct Recorder(Approval, Mutex<Vec<ApprovalRequest>>);

    #[async_trait]
    impl Approver for Recorder {
        async fn approve(&self, req: &ApprovalRequest) -> Approval {
            self.1.lock().unwrap().push(req.clone());
            self.0.clone()
        }
    }

    /// An agent over an in-memory store whose tools are the built-in ones, with `draft_email`
    /// running `fake` and its feature key at its default.
    fn agent(fake: &FakeOpen, llm: ScriptedLlm, mode: Mode, approver: Arc<dyn Approver>, ws: &Path) -> Agent {
        let store = crate::store::Store::in_memory().unwrap();
        crate::risk_notice::accept_current(&store).unwrap();
        let mut cfg = crate::config::Config { home: ws.join(".heron-home"), ..Default::default() };
        cfg.agent.auto_memory = false;
        let mut tools = ToolRegistry::builtin();
        tools.register(Arc::new(fake.tool()));
        Agent {
            cfg: Arc::new(cfg),
            llm: Arc::new(llm),
            tools: Arc::new(tools),
            memory: crate::memory::Memory::new(store.clone(), None),
            store,
            policy: Arc::new(Policy::new(mode, &[], &[])),
            approver,
            workspace: dunce::canonicalize(ws).unwrap(),
            usage: Default::default(),
            depth: 0,
            token_limit: None,
        }
    }

    fn twice(call: Value) -> ScriptedLlm {
        ScriptedLlm::new(vec![ScriptedLlm::tool_call(NAME, call.clone()), ScriptedLlm::tool_call(NAME, call), Message::assistant("done")])
    }

    #[tokio::test]
    async fn draft_email_builds_an_escaped_mailto() {
        let d = parse(&args(&["ann@example.com", "bo.b+news@mail.example.org"], "Lunch & plans? 100% sure", "Hi Ann,\nSee you at 1 + 2 = 3.\n#done ü")).unwrap();
        let url = mailto(&d);
        let s = url.as_str();
        assert_eq!(url.scheme(), "mailto");
        assert!(s.starts_with("mailto:ann@example.com,bo.b+news@mail.example.org?"), "{s}");
        assert_eq!(url.path(), "ann@example.com,bo.b+news@mail.example.org");
        // A space is %20, never `+`, which mail apps show as it is; a real `+` is %2B.
        assert!(!url.query().unwrap().contains('+'), "{s}");
        assert!(s.contains("subject=Lunch%20%26%20plans%3F%20100%25%20sure"), "{s}");
        assert!(s.contains("1%20%2B%202%20%3D%203"), "{s}");
        assert!(s.contains("%0A") && s.contains("%23done%20%C3%BC"), "{s}");
        assert!(!s.contains('#'), "nothing in the body starts a fragment: {s}");
        // Decoded, the subject and body come back exactly as written.
        assert_eq!(query(&url), vec![("subject".into(), d.subject.clone()), ("body".into(), d.body.clone())]);

        // A `%` the address check allows is written as %25, so a mail app can't decode `%2C` into a
        // second recipient or `%3F` into the start of parameters.
        let d = parse(&args(&["victim%40x.com%2Cevil@attacker.example"], "s", "b")).unwrap();
        let url = mailto(&d);
        assert_eq!(url.path(), "victim%2540x.com%252Cevil@attacker.example");
        assert!(!url.path().contains(','), "still one recipient: {url}");

        // The URL the fake `open` receives is the same one, whole.
        let fake = FakeOpen::new();
        let call = args(&["ann@example.com"], "Lunch & plans?", "Hi Ann,\nsee you.");
        assert_eq!(fake.tool().call(&call, &test_ctx(false)).await.unwrap(), "opened");
        assert_eq!(fake.args(), vec![mailto(&parse(&call).unwrap()).to_string()]);
    }

    #[test]
    fn draft_email_sets_only_subject_and_body() {
        // Text that tries to add parameters stays inside the subject and the body.
        let subject = "hi&bcc=evil@attacker.example&attach=/etc/passwd";
        let body = "x&cc=evil@attacker.example\n?to=more@attacker.example&body=other";
        let url = mailto(&parse(&args(&["ann@example.com"], subject, body)).unwrap());
        assert_eq!(query(&url), vec![("subject".to_string(), subject.to_string()), ("body".to_string(), body.to_string())]);
        assert_eq!(url.path(), "ann@example.com");
        assert_eq!(url.query().unwrap().matches('&').count(), 1, "one separator, between subject and body: {url}");
        assert_eq!(url.query().unwrap().matches('=').count(), 2, "{url}");

        // An empty subject and body are still the only two parameters.
        let url = mailto(&parse(&args(&["ann@example.com"], "", "")).unwrap());
        assert_eq!(url.as_str(), "mailto:ann@example.com?subject=&body=");

        // Extra arguments the model adds are ignored, never turned into parameters.
        let mut extra = args(&["ann@example.com"], "s", "b");
        extra["cc"] = json!(["evil@attacker.example"]);
        extra["bcc"] = json!("evil@attacker.example");
        extra["attach"] = json!("/etc/passwd");
        let url = mailto(&parse(&extra).unwrap());
        assert_eq!(url.as_str(), "mailto:ann@example.com?subject=s&body=b");
    }

    #[tokio::test]
    async fn draft_email_rejects_addresses_with_query_characters() {
        let tool = DraftEmail::with_bin("/nonexistent/open");
        let ctx = test_ctx(false);
        for bad in [
            "ann@example.com?bcc=evil@attacker.example",
            "ann@example.com&cc=evil@attacker.example",
            "ann=x@example.com",
            "ann@example.com,evil@attacker.example",
            "ann@example.com#x",
            "Ann <ann@example.com>",
            "ann @example.com",
            "ann@example.com\nbcc: evil@attacker.example",
            "ann@example",
            "ann@example.c",
            "@example.com",
            "ann",
            "",
        ] {
            assert_eq!(parse(&args(&["ok@example.com", bad], "s", "b")), Err(NOT_PLAIN.to_string()), "{bad:?}");
            assert_eq!(tool.refusal(&args(&[bad], "s", "b"), &ctx).as_deref(), Some(NOT_PLAIN), "{bad:?}");
        }
        for bad in [json!("ann@example.com"), json!([7]), json!(null), json!([["ann@example.com"]])] {
            assert_eq!(tool.refusal(&json!({"to": bad, "subject": "s", "body": "b"}), &ctx).as_deref(), Some(NOT_PLAIN), "{bad}");
        }
        assert_eq!(tool.refusal(&json!({"subject": "s", "body": "b"}), &ctx).as_deref(), Some(NOT_PLAIN), "no `to` at all");
        // At most 10.
        let ten: Vec<String> = (0..10).map(|i| format!("p{i}@example.com")).collect();
        let ten: Vec<&str> = ten.iter().map(String::as_str).collect();
        assert_eq!(tool.refusal(&args(&ten, "s", "b"), &ctx), None);
        let mut eleven = ten.clone();
        eleven.push("p10@example.com");
        assert_eq!(tool.refusal(&args(&eleven, "s", "b"), &ctx).as_deref(), Some("That isn't a plain email address. A draft can have at most 10 addresses."));
        // Plain addresses pass, `+`, `%`, `.`, `_` and `-` included.
        assert_eq!(tool.refusal(&args(&["a.b_c-d+e%f@sub.example-mail.co.uk", "X@Y.IO"], "s", "b"), &ctx), None);

        // Refused before the decision: in Ask mode nothing is asked, in Yolo mode nothing runs, and
        // the model is told why.
        for mode in [Mode::Ask, Mode::Yolo] {
            let fake = FakeOpen::new();
            let ws = tempfile::tempdir().unwrap();
            let approver = Arc::new(Recorder(Approval::Once, Mutex::new(vec![])));
            let a = agent(&fake, twice(args(&["ann@example.com?bcc=evil@attacker.example"], "s", "b")), mode, approver.clone(), ws.path());
            let mut s = crate::agent::Session::create(&a.store, "").unwrap();
            a.run_turn(&mut s, "go", &[], &null_sink()).await.unwrap();
            assert!(approver.1.lock().unwrap().is_empty(), "{mode:?}: nothing is asked");
            assert!(fake.runs().is_empty(), "{mode:?}: open never ran");
            let results: Vec<String> = s.messages.iter().filter(|m| m.role == "tool").map(|m| m.content.clone().unwrap_or_default()).collect();
            assert_eq!(results.len(), 2, "{mode:?}: {results:?}");
            assert!(results.iter().all(|r| r.contains(NOT_PLAIN)), "{mode:?}: {results:?}");
            let audit = a.store.audit_log(5).unwrap();
            assert!(audit.iter().all(|row| row.decision == "denied-by-policy"), "{mode:?}: {:?}", audit.iter().map(|r| r.decision.clone()).collect::<Vec<_>>());
        }

        // Checked again when the call runs (a call approved later skips the agent): open never runs.
        let fake = FakeOpen::new();
        let err = fake.tool().call(&args(&["ann@example.com&cc=evil@attacker.example"], "s", "b"), &test_ctx(false)).await.unwrap_err().to_string();
        assert_eq!(err, NOT_PLAIN);
        assert!(fake.runs().is_empty());
    }

    #[tokio::test]
    async fn draft_email_never_sends() {
        let fake = FakeOpen::new();
        let tool = fake.tool();
        let out = tool.call(&args(&["ann@example.com", "bob@example.com"], "Hello", "Body"), &test_ctx(false)).await.unwrap();
        // The model learns only that a window opened: nothing about sending, nothing from the mail app.
        assert_eq!(out, "opened");
        // `open` gets the URL alone: no app, no flag, nothing that could script a send.
        assert_eq!(fake.runs(), vec!["1"]);
        assert_eq!(fake.args(), vec!["mailto:ann@example.com,bob@example.com?subject=Hello&body=Body"]);
        assert!(tool.description().contains("never sends"));
        assert!(tool.detail(&args(&["ann@example.com"], "Hello", "Body"), &test_ctx(false)).contains("won't send"));
        // `open` gets the environment allowlist only, with a private TMPDIR as its working folder.
        assert!(std::env::var("CARGO_MANIFEST_DIR").is_ok(), "the precondition: Heron's own environment has a variable off the list");
        let env: Vec<String> = std::fs::read_to_string(fake.dir.path().join("env")).unwrap().lines().map(str::to_string).collect();
        assert_eq!(env[0], "unset", "a variable off the allowlist is not passed: {env:?}");
        assert_eq!(env[1], std::env::var("HOME").unwrap_or_else(|_| "unset".into()), "HOME is on the list");
        let tmp = Path::new(&env[2]);
        assert!(tmp.file_name().unwrap().to_string_lossy().starts_with("heron-cli-"), "{env:?}");
        assert_eq!(env[3], env[2], "it runs in the private folder: {env:?}");
        assert!(!tmp.exists(), "the private folder is removed afterwards");
        // The real program is `open`, which only hands the URL to Launch Services; only a debug build
        // reads the override.
        assert_eq!(open_bin_from(&|_| None), PathBuf::from("/usr/bin/open"));
        assert_eq!(open_bin_from(&|_| Some(String::new())), PathBuf::from("/usr/bin/open"));
        let fake_path = fake.dir.path().join("open").to_string_lossy().into_owned();
        let overridden = open_bin_from(&|k| (k == "HERON_OPEN_BIN").then(|| fake_path.clone()));
        if cfg!(debug_assertions) {
            assert_eq!(overridden, PathBuf::from(&fake_path));
        } else {
            assert_eq!(overridden, PathBuf::from("/usr/bin/open"));
        }
        // A failing `open` is an error, never "opened".
        let failing = FakeOpen::with_script("echo 'no mail app' >&2\nexit 3\n");
        let err = failing.tool().call(&args(&["ann@example.com"], "s", "b"), &test_ctx(false)).await.unwrap_err().to_string();
        assert!(err.contains("no mail app") && err.contains('3'), "{err}");
        // One that hangs is stopped at the time limit, and the process is killed, not left running.
        let hanging = FakeOpen::with_script("echo $$ > \"$d/pid\"\nexec sleep 30\n");
        let tool = DraftEmail { timeout: Duration::from_millis(500), ..hanging.tool() };
        let started = std::time::Instant::now();
        let err = tool.call(&args(&["ann@example.com"], "s", "b"), &test_ctx(false)).await.unwrap_err().to_string();
        assert!(err.contains("didn't open"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(10), "{:?}", started.elapsed());
        let pid: libc::pid_t = std::fs::read_to_string(hanging.dir.path().join("pid")).unwrap().trim().parse().unwrap();
        // Gone means reaped here (waitpid returns it) or already reaped by tokio (ECHILD); 0 is still running.
        let gone = || {
            let mut status = 0;
            let r = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
            r == pid || (r == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD))
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !gone() {
            assert!(std::time::Instant::now() < deadline, "open (pid {pid}) is still running after the timeout");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(TIMEOUT, Duration::from_secs(10));
    }

    #[tokio::test]
    async fn draft_email_body_is_capped() {
        let tool = FakeOpen::new();
        let at_cap = "é".repeat(MAX_BODY_CHARS);
        assert_eq!(parse(&args(&["ann@example.com"], "s", &at_cap)).map(|d| d.body), Ok(at_cap.clone()), "2,000 characters (not bytes) are accepted");
        let over = format!("{at_cap}x");
        let err = parse(&args(&["ann@example.com"], "s", &over)).unwrap_err();
        assert!(err.contains("2000") && err.contains("Shorten"), "{err}");
        let fake = tool.tool();
        assert_eq!(fake.refusal(&args(&["ann@example.com"], "s", &over), &test_ctx(false)), Some(err.clone()));
        assert_eq!(fake.call(&args(&["ann@example.com"], "s", &over), &test_ctx(false)).await.unwrap_err().to_string(), err);
        assert!(tool.runs().is_empty(), "an over-long body never reaches open");
        // The model is told the cap.
        assert!(fake.description().contains("2,000 characters"));
        assert_eq!(fake.parameters()["properties"]["body"]["maxLength"], MAX_BODY_CHARS);
        assert_eq!(fake.parameters()["properties"]["to"]["maxItems"], MAX_RECIPIENTS);
    }

    #[test]
    fn draft_email_is_external_write() {
        let tool = DraftEmail::from_env();
        let ctx = test_ctx(false);
        for a in [args(&["ann@example.com"], "s", "b"), args(&["bad?x"], "s", "b"), json!({})] {
            assert_eq!(tool.risk(&a, &ctx), Effect::ExternalWrite, "{a}");
        }
        assert!(!tool.untrusted_output(), "it returns only \"opened\"");
        assert_eq!(tool.feature(), Some("email_drafts"));
        assert!(crate::permissions::once_only(NAME));
        // The key: on by default, titled for Settings.
        let key = crate::features::find("email_drafts").expect("the email_drafts key");
        assert_eq!((key.title, key.default), ("Email drafts", true));
        // Built in, offered while the key is on, gone while it is off, and never served over MCP.
        let registry = ToolRegistry::builtin();
        assert_eq!(registry.get(NAME).map(|t| t.risk(&json!({}), &ctx)), Some(Effect::ExternalWrite));
        let mut cfg = crate::config::Config::default();
        assert!(registry.names_for(&cfg).contains(&NAME.to_string()));
        cfg.features.flags.insert("email_drafts".into(), false);
        assert!(!registry.names_for(&cfg).contains(&NAME.to_string()));
        assert!(registry.get_for(&cfg, NAME).is_none());
        assert!(!crate::mcp::SERVED_TOOLS.contains(&NAME));
    }

    #[tokio::test]
    async fn draft_email_offers_once_only() {
        let call = args(&["ann@example.com", "bob@example.com"], "Plans", "Line one\nLine two, the whole body.");
        // "For this conversation" is answered twice: each draft is still asked about, and no grant is kept.
        let fake = FakeOpen::new();
        let ws = tempfile::tempdir().unwrap();
        let approver = Arc::new(Recorder(Approval::Session, Mutex::new(vec![])));
        let a = agent(&fake, twice(call.clone()), Mode::Ask, approver.clone(), ws.path());
        let mut s = crate::agent::Session::create(&a.store, "").unwrap();
        a.run_turn(&mut s, "go", &[], &null_sink()).await.unwrap();
        let asked = approver.1.lock().unwrap().clone();
        assert_eq!(asked.len(), 2, "each draft is asked about");
        assert_eq!(fake.runs().len(), 2);
        let grants = a.store.kv_get(&format!("grants:{}", s.id)).unwrap_or_default();
        assert!(!grants.contains(NAME), "{grants}");
        for req in &asked {
            assert_eq!(req.risk, Effect::ExternalWrite);
            assert!(!req.can_always(), "\"always\" is never offered");
            // The approval shows the recipients, the subject and the full body.
            assert!(req.detail.contains("ann@example.com, bob@example.com") && req.detail.contains("Plans") && req.detail.contains("Line one\nLine two, the whole body."), "{}", req.detail);
        }
        assert!(!crate::agent::may_grant_for_conversation(NAME, Effect::ExternalWrite, false));

        // "Always" is taken as once: the call runs, and no standing rule is left behind.
        let fake = FakeOpen::new();
        let ws = tempfile::tempdir().unwrap();
        let approver = Arc::new(Recorder(Approval::Always, Mutex::new(vec![])));
        let a = agent(&fake, twice(call.clone()), Mode::Ask, approver.clone(), ws.path());
        let mut s = crate::agent::Session::create(&a.store, "").unwrap();
        a.run_turn(&mut s, "go", &[], &null_sink()).await.unwrap();
        assert_eq!(approver.1.lock().unwrap().len(), 2, "the second draft is asked about too");
        assert!(a.policy.allow_rules().is_empty(), "{:?}", a.policy.allow_rules());
        assert!(a.store.audit_log(5).unwrap().iter().all(|r| r.decision == "approved"));

        // Nor can Settings set it to "allow".
        assert!(Policy::check_tool_setting(NAME, "allow").is_err());
    }

    #[test]
    fn decision_table_draft_email() {
        let tool = DraftEmail::from_env();
        let call = args(&["ann@example.com"], "s", "b");
        let effect = tool.risk(&call, &test_ctx(false));
        let summary = tool.summary(&call);
        let deny = || Verdict::Deny(String::new());
        let same = |got: &Verdict, want: &Verdict| matches!((got, want), (Verdict::Allow, Verdict::Allow) | (Verdict::Ask, Verdict::Ask) | (Verdict::Deny(_), Verdict::Deny(_)));
        // [readonly, ask, auto, yolo]: asked in ask and auto mode, and whenever outside content is present.
        let clean = [deny(), Verdict::Ask, Verdict::Ask, Verdict::Allow];
        let tainted = [deny(), Verdict::Ask, Verdict::Ask, Verdict::Ask];
        let modes = [Mode::ReadOnly, Mode::Ask, Mode::Auto, Mode::Yolo];
        let star = vec!["*".to_string()];
        let rules: [&[String]; 2] = [&[], &star];
        for allow in rules {
            for (i, mode) in modes.iter().enumerate() {
                let p = Policy::new(*mode, allow, &[]);
                let got = decide_for(&p, NAME, &summary, effect, false);
                assert!(same(&got, &clean[i]), "{mode:?}, rules {allow:?}, clean: got {got:?}");
                let got = decide_for(&p, NAME, &summary, effect, true);
                assert!(same(&got, &tainted[i]), "{mode:?}, rules {allow:?}, tainted: got {got:?}");
            }
        }
        // The control: under `*`, an ordinary tool with the same effect is allowed on a clean
        // conversation in ask and auto mode.
        for mode in [Mode::Ask, Mode::Auto] {
            assert_eq!(decide_for(&Policy::new(mode, &star, &[]), "browser_type", "x", Effect::ExternalWrite, false), Verdict::Allow, "{mode:?}");
        }
        // The user's "never" still refuses it in every mode.
        for mode in modes {
            assert!(matches!(decide_for(&Policy::new(mode, &star, &["draft_email:*".to_string()]), NAME, &summary, effect, false), Verdict::Deny(_)), "{mode:?}");
        }
    }
}
