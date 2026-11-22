//! `heron chat` as a person (or a script) uses it: lines on stdin, answers on stdout. This holds the
//! terminal conversation loop to its commands; `repl` itself needs a real process because it reads
//! the terminal's own input.

use heron::config::Config;
use heron::store::Store;
use std::io::Write;
use std::process::{Command, Stdio};

/// Run `heron chat` in a fresh home with the demo model and the risk notice already accepted, feed
/// it `input` on stdin, and return (exit success, stdout, the history file's text).
fn chat(input: &str) -> (bool, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let workspace = dir.path().join("ws");
    std::fs::create_dir_all(&workspace).unwrap();
    let cfg = Config { home: home.clone(), ..Default::default() };
    cfg.ensure_home().unwrap();
    {
        let store = Store::open(&cfg.db_path()).unwrap();
        heron::risk_notice::accept_current(&store).unwrap();
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_heron"))
        .arg("chat")
        .current_dir(&workspace)
        .env("HERON_HOME", &home)
        .env("HERON_BASE_URL", "demo")
        .env_remove("HERON_MODEL")
        .env_remove("HERON_PROVIDER")
        .env_remove("HERON_CLI_COMMAND")
        .env_remove("HERON_BROWSER")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    let history = std::fs::read_to_string(home.join("history.txt")).unwrap_or_default();
    (out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned(), history)
}

#[test]
fn chat_runs_the_commands_it_is_given_and_exits_cleanly() {
    let (ok, stdout, history) = chat("/mode readonly\n/help\n/bogus\n/exit\n/mode auto\n");
    assert!(ok, "heron chat exits 0 after /exit: {stdout}");
    assert!(stdout.contains("mode: readonly"), "/mode answered: {stdout}");
    assert!(stdout.contains("Commands:") && stdout.contains("/new"), "/help answered: {stdout}");
    assert!(stdout.contains("unknown command /bogus"), "{stdout}");
    assert!(!stdout.contains("mode: auto"), "nothing after /exit is run: {stdout}");
    // On the way out the loop saves what was typed.
    assert!(history.contains("/mode readonly") && history.contains("/exit"), "{history:?}");
}

#[test]
fn chat_ends_when_its_input_ends() {
    let (ok, stdout, history) = chat("/tokens\n");
    assert!(ok, "end of input is a normal exit: {stdout}");
    assert!(stdout.contains("in /") && stdout.contains("context"), "/tokens answered before the input ended: {stdout}");
    assert!(history.contains("/tokens"), "{history:?}");
}
