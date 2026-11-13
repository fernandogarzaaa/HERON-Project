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
    let sandbox = crate::sandbox::wrap(&ctx.cfg, &ctx.workspace);
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
        cmd.env_remove("HERON_API_KEY").env_remove(&ctx.cfg.llm.api_key_env);
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
            text.push_str("\n");
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
    async fn endless_output_is_cut_off_instead_of_filling_memory() {
        let ws = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.agent.sandbox = "off".into();
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
        let mut cfg = Config::default();
        cfg.home = dunce::canonicalize(home.path()).unwrap();
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

        // With the sandbox switched off the same write goes through (so the test really tested the sandbox).
        let mut open = cfg.clone();
        open.agent.sandbox = "off".into();
        let ctx_off = ToolCtx { cfg: Arc::new(open), ..ctx.clone() };
        run(&json!({"command": format!("echo fine > '{}/y.txt'", outside.display())}), &ctx_off).await.unwrap();
        assert!(outside.join("y.txt").exists());
        std::fs::remove_dir_all(&outside).unwrap();
    }
}
