//! Model providers that run on the user's own subscriptions through the official
//! agent CLIs, instead of an API key.
//!
//! How this works, and where the design comes from:
//!
//! * Hermes (`NousResearch/hermes-plugin-claude-subscription-directsdk`): the
//!   unmodified official `claude` binary is driven as a request-scoped model
//!   client. Hermes keeps its own agent loop, tools, approvals and compaction.
//!   The plugin holds no credentials of its own, never opens, copies, refreshes
//!   or prints the CLI's credential files, and rejects inherited API-key or
//!   custom-endpoint overrides before spawning. Each request runs in a private
//!   temporary directory, native tools are disabled, and cancellation kills the
//!   whole process tree. This module copies that shape for Claude, in the
//!   simpler `claude -p --output-format stream-json` form: one turn, tools off,
//!   no session persistence, Heron's own loop on top. It does not copy Hermes'
//!   loopback admission relay, which exists to keep the CLI to exactly one
//!   upstream request and to keep prompt-cache prefixes stable. If a CLI
//!   version ever retries a turn on its own, the extra turn still runs in the
//!   same throwaway directory with no tools, so the blast radius stays small.
//! * T3 Code (`pingdotgg/t3code`): a control surface over the CLIs you already
//!   pay for. It never reads a harness credential at all: it resolves the
//!   binary, points the CLI at a config directory with an environment variable,
//!   and asks the CLI who it is logged in as (`claude --version` plus an SDK
//!   probe, `codex app-server` `account/read`, `opencode --version` plus the
//!   server's provider inventory). Provider drivers are per-instance, so two
//!   accounts using the same driver do not share state, and setup never happens
//!   as a health-check side effect. This module copies the provider list and
//!   the same three rules: resolve, do not read credentials, ask the CLI.
//!
//! Safety invariants, enforced here and in the tests below:
//!
//! 1. Heron never reads, copies, or prints a CLI token. Login happens in the
//!    CLI itself (`claude auth login`, `codex login`, ...), on the user's own
//!    machine, before Heron ever spawns it.
//! 2. The CLI runs in a fresh, empty, private temporary directory, never in
//!    the workspace. Even a CLI that ignores the no-tools instruction can only
//!    touch throwaway files there.
//! 3. No provider is ever spawned with an auto-approve flag (`--yolo`,
//!    `--force`, `--dangerously-skip-permissions`, `--allow-all-tools`, ...).
//!    The CLI is a text model for Heron, not a second agent. Heron owns the
//!    tool loop, permissions, sandbox, audit log and taint tracking, exactly
//!    as it does for the HTTP path.
//! 4. Subscription billing is never silently flipped to metered API billing:
//!    for subscription providers the child environment has the API-key
//!    variables removed, so a stray `OPENAI_API_KEY` cannot turn a ChatGPT
//!    subscription turn into a metered API call.
//! 5. A provider whose headless mode cannot disable its own tools (Cline
//!    one-shot defaults to yolo, Goose runs its own loop) refuses to run
//!    unless the user sets `cli_allow_agent_tools = true` explicitly in the
//!    config. That flag is a deliberate, visible choice, not a default.
//!
//! The model answers with a JSON envelope in its text, which Heron parses into
//! a reply plus tool calls:
//!
//! ```json
//! {"text": "I found two files.", "tool_calls": [{"name": "list_dir", "arguments": {"path": "."}}]}
//! ```
//!
//! Plain text with no envelope is a reply with no tool calls. Tool names not
//! in the request are dropped, so a CLI that invents a tool cannot reach one.

use crate::config::Config;
use crate::llm::{ChatRequest, ChatResponse, DeltaFn, FunctionCall, Llm, Message, ToolCall, Usage};
use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

/// How the prompt reaches the CLI.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PromptInput {
    /// Prompt on stdin (`claude -p`, `codex exec`).
    Stdin,
    /// Prompt as a positional argument (`opencode run "<prompt>"`, `cline "<prompt>"`).
    Arg,
    /// Prompt as the value of `-p` / `--prompt` (`copilot -p "<prompt>"`).
    Flag,
    /// Prompt as the value of a `-p` flag that takes a string: the prompt
    /// must sit directly after the flag (`gemini -p "<prompt>" --model …`).
    /// Appending it at the end lets `-p` swallow the next option
    /// (`--model`) as its value instead.
    FlagValue,
}

/// How the CLI's answer is shaped.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputKind {
    /// Claude `stream-json`: one JSON event per line.
    ClaudeStreamJson,
    /// One JSON event per line, text in various shapes (Codex, OpenCode, Cline, Copilot).
    JsonLines,
    /// One JSON object on stdout (Cursor, Gemini, Qwen, Antigravity).
    JsonSingle,
    /// Try a single object first, then fall back to lines (Qwen, Gemini).
    JsonAuto,
    /// Plain text (generic / custom CLIs).
    Text,
}

/// One provider in the registry. Data, not code: adding a provider is a new
/// row here plus a row in the docs, not a new code path.
#[derive(Debug, Clone)]
pub struct CliSpec {
    /// Config value for `[llm] provider`, e.g. `claude-cli`.
    pub id: &'static str,
    pub display_name: &'static str,
    /// Binary to resolve on PATH (or in the usual install dirs).
    pub binary: &'static str,
    /// How the user logs in, shown in errors and `heron providers`.
    pub login_command: &'static str,
    /// Where to send the user when the binary is missing.
    pub install_hint: &'static str,
    /// Subscription, API key via the CLI's own config, free local model, or router.
    pub auth_kind: &'static str,
    pub cost_note: &'static str,
    /// Flags are unverified against a live binary; check `binary --help` first.
    pub experimental: bool,
    /// Headless mode cannot turn its own tools off; needs `cli_allow_agent_tools`.
    pub needs_allow_agent_tools: bool,
    /// Env var that points the CLI at a config directory (T3 Code pattern).
    pub config_dir_env: Option<&'static str>,
    /// Env var for the per-provider login probe, if any.
    pub input: PromptInput,
    pub output: OutputKind,
}

impl CliSpec {
    /// Fixed argv for the provider, without the prompt and without `--model`.
    /// Never contains an auto-approve flag; the test below asserts that.
    fn base_args(&self, model: Option<&str>) -> Vec<String> {
        let mut args: Vec<String> = Vec::new();
        let model_arg = |flag: &str, args: &mut Vec<String>| {
            if let Some(m) = model.filter(|m| !m.is_empty()) {
                args.push(flag.to_string());
                args.push(m.to_string());
            }
        };
        match self.id {
            // Hermes argv, print-mode form: --tools '' turns native tools off,
            // --max-turns 1 is one turn, dontAsk never waits on a human, and
            // --no-session-persistence leaves no parked native session.
            "claude-cli" => {
                args.push("-p".into());
                model_arg("--model", &mut args);
                args.extend([
                    "--output-format".into(),
                    "stream-json".into(),
                    "--verbose".into(),
                    "--include-partial-messages".into(),
                    "--tools".into(),
                    String::new(),
                    "--max-turns".into(),
                    "1".into(),
                    "--permission-mode".into(),
                    "dontAsk".into(),
                    "--no-session-persistence".into(),
                    "--setting-sources".into(),
                    String::new(),
                    "--disable-slash-commands".into(),
                ]);
            }
            // Codex: --sandbox read-only plus approval never. Note the sharp
            // edge T3 Code and the R-project helper both hit: Codex 0.164 keeps
            // its shell tool even under read-only, so the temp-dir cwd (never
            // the workspace) is what actually bounds it here. `-a` is a
            // top-level option: `codex exec -a never` fails with
            // "unexpected argument '-a' found" (verified on 0.164.0).
            "codex-cli" => {
                args.extend([
                    "-a".into(),
                    "never".into(),
                    "exec".into(),
                    "--json".into(),
                    "--sandbox".into(),
                    "read-only".into(),
                    "--skip-git-repo-check".into(),
                ]);
                model_arg("--model", &mut args);
            }
            // Cursor: --mode ask is the read-only mode; --trust is required
            // headless so it does not stop on a trust prompt. No --force.
            "cursor-cli" => {
                args.push("-p".into());
                model_arg("--model", &mut args);
                args.extend([
                    "--output-format".into(),
                    "json".into(),
                    "--mode".into(),
                    "ask".into(),
                    "--trust".into(),
                ]);
            }
            // Grok Build, via the `grok` binary (T3 Code drives it over ACP;
            // the print flags here are best effort and marked experimental).
            "grok-cli" => {
                args.push("-p".into());
                model_arg("--model", &mut args);
                args.extend(["--output-format".into(), "json".into()]);
            }
            // OpenCode: the plan agent is the read-only one. OpenCode routes
            // to whatever the user configured in opencode.json: a Claude Pro
            // OAuth login, OpenRouter free models, or a local Ollama model.
            "opencode-cli" => {
                args.extend([
                    "run".into(),
                    "--format".into(),
                    "json".into(),
                    "--agent".into(),
                    "plan".into(),
                ]);
                model_arg("--model", &mut args);
            }
            // Antigravity is the successor to Gemini CLI for Google logins
            // (Gemini CLI stopped serving free and Pro/Ultra users 2026-06-18).
            "antigravity-cli" => {
                args.push("-p".into());
                model_arg("--model", &mut args);
                args.extend(["--output-format".into(), "json".into()]);
            }
            // Cline one-shot defaults to yolo, so this provider is gated by
            // cli_allow_agent_tools and runs in a throwaway directory.
            "cline-cli" => {
                args.push("--json".into());
                model_arg("--model", &mut args);
            }
            // Copilot: -s prints only the answer; --no-ask-user suppresses
            // clarifying questions. No --allow-all-tools.
            "copilot-cli" => {
                args.push("-p".into());
                model_arg("--model", &mut args);
                args.extend([
                    "--output-format".into(),
                    "json".into(),
                    "-s".into(),
                    "--no-ask-user".into(),
                ]);
            }
            // Qwen Code: free OAuth tier ended 2026-04-15; now a paid key or
            // a local Ollama/vLLM model. The CLI itself is free and open.
            "qwen-cli" => {
                args.push("-p".into());
                model_arg("--model", &mut args);
                args.extend(["--output-format".into(), "json".into()]);
            }
            // Gemini CLI: Google OAuth free tier is gone (see Antigravity
            // above); the API-key path still has a free tier via GEMINI_API_KEY.
            "gemini-cli" => {
                args.push("-p".into());
                model_arg("--model", &mut args);
                args.extend(["--output-format".into(), "json".into()]);
            }
            // ACP agents (Pi, fx, Kiro, Muse): T3 Code drives these over the
            // Agent Client Protocol, not print mode. The flags here are best
            // effort and marked experimental until verified against the binary.
            "pi-cli" | "fx-cli" | "kiro-cli" | "muse-cli" => {
                args.push("-p".into());
                model_arg("--model", &mut args);
                args.extend(["--output-format".into(), "json".into()]);
            }
            // Goose runs its own tool loop, so it is gated like Cline.
            "goose-cli" => {
                args.extend([
                    "run".into(),
                    "--output-format".into(),
                    "json".into(),
                    "--max-turns".into(),
                    "1".into(),
                ]);
                model_arg("--model", &mut args);
            }
            _ => {}
        }
        args
    }
}

/// The provider registry. T3 Code's list (Claude, Codex, Cursor, Grok,
/// OpenCode, Antigravity, Kiro, Pi, Muse, fx, ACP) plus the free-model
/// routers the user asked for (Cline, Copilot, Qwen, Gemini, Goose) and a
/// generic escape hatch for anything else (Crush, Amp, Kimi, Droid, `llm`).
pub fn providers() -> Vec<CliSpec> {
    vec![
        CliSpec { id: "claude-cli", display_name: "Claude Code (subscription)", binary: "claude", login_command: "claude auth login", install_hint: "npm install -g @anthropic-ai/claude-code", auth_kind: "Claude Pro/Max subscription", cost_note: "Draws on the plan's usage window; Agent SDK use drains it faster than the CLI directly.", experimental: false, needs_allow_agent_tools: false, config_dir_env: Some("CLAUDE_CONFIG_DIR"), input: PromptInput::Stdin, output: OutputKind::ClaudeStreamJson },
        CliSpec { id: "codex-cli", display_name: "Codex (ChatGPT subscription)", binary: "codex", login_command: "codex login", install_hint: "npm install -g @openai/codex", auth_kind: "ChatGPT Plus/Pro/Team subscription", cost_note: "Draws on the ChatGPT plan. A stray OPENAI_API_KEY would flip billing to metered API; Heron removes it from the child env.", experimental: false, needs_allow_agent_tools: false, config_dir_env: Some("CODEX_HOME"), input: PromptInput::Stdin, output: OutputKind::JsonLines },
        CliSpec { id: "cursor-cli", display_name: "Cursor (subscription)", binary: "cursor-agent", login_command: "agent login", install_hint: "curl https://cursor.com/install -fsS | bash", auth_kind: "Cursor subscription", cost_note: "Draws on the Cursor plan's usage pool.", experimental: false, needs_allow_agent_tools: false, config_dir_env: None, input: PromptInput::Flag, output: OutputKind::JsonSingle },
        CliSpec { id: "grok-cli", display_name: "Grok Build (subscription)", binary: "grok", login_command: "grok login", install_hint: "brew install --cask grok-build (macOS) or the Grok Build installer", auth_kind: "Grok Build subscription", cost_note: "Draws on the Grok plan.", experimental: true, needs_allow_agent_tools: false, config_dir_env: None, input: PromptInput::Flag, output: OutputKind::JsonSingle },
        CliSpec { id: "opencode-cli", display_name: "OpenCode (router)", binary: "opencode", login_command: "opencode auth login (or configure opencode.json)", install_hint: "npm install -g @opencode/cli (2.x) or opencode (1.x)", auth_kind: "Whatever opencode.json routes to: Claude Pro OAuth, OpenRouter free models, Ollama", cost_note: "Free when routed to a free model or a local model; otherwise the upstream's price.", experimental: false, needs_allow_agent_tools: false, config_dir_env: None, input: PromptInput::Arg, output: OutputKind::JsonLines },
        CliSpec { id: "antigravity-cli", display_name: "Antigravity (Google account)", binary: "agy", login_command: "agy (then sign in with Google)", install_hint: "Install Antigravity CLI from https://antigravity.google/docs/getting-started", auth_kind: "Google account (successor to Gemini CLI for logins)", cost_note: "Google's pricing for the selected method.", experimental: false, needs_allow_agent_tools: false, config_dir_env: None, input: PromptInput::FlagValue, output: OutputKind::JsonSingle },
        CliSpec { id: "cline-cli", display_name: "Cline (router, free models)", binary: "cline", login_command: "cline auth <provider>", install_hint: "npm install -g cline", auth_kind: "Whatever Cline is configured with: cline / openai-codex / oca OAuth, API keys, Ollama", cost_note: "Free when routed to a free or local model. One-shot defaults to yolo, so this provider needs cli_allow_agent_tools = true.", experimental: false, needs_allow_agent_tools: true, config_dir_env: None, input: PromptInput::Arg, output: OutputKind::JsonLines },
        CliSpec { id: "copilot-cli", display_name: "GitHub Copilot (subscription)", binary: "copilot", login_command: "copilot (then /login)", install_hint: "npm install -g @github/copilot", auth_kind: "GitHub Copilot subscription", cost_note: "Draws on the Copilot plan's credit pool.", experimental: false, needs_allow_agent_tools: false, config_dir_env: None, input: PromptInput::FlagValue, output: OutputKind::JsonLines },
        CliSpec { id: "qwen-cli", display_name: "Qwen Code (paid key or local)", binary: "qwen", login_command: "qwen (then /auth)", install_hint: "npm install -g @qwen-code/qwen-code", auth_kind: "Alibaba Cloud plan, OpenRouter, Fireworks, or local Ollama/vLLM. Free Qwen OAuth ended 2026-04-15.", cost_note: "Free with a local model; otherwise the upstream's price.", experimental: false, needs_allow_agent_tools: false, config_dir_env: None, input: PromptInput::FlagValue, output: OutputKind::JsonAuto },
        CliSpec { id: "gemini-cli", display_name: "Gemini CLI (API key)", binary: "gemini", login_command: "gemini (then pick Gemini API Key)", install_hint: "npm install -g @google/gemini-cli", auth_kind: "Gemini API key (free tier) or Vertex AI. Google OAuth free tier ended 2026-06-18; use Antigravity for Google login.", cost_note: "Free tier on the API key; otherwise Google's pricing.", experimental: false, needs_allow_agent_tools: false, config_dir_env: None, input: PromptInput::FlagValue, output: OutputKind::JsonAuto },
        CliSpec { id: "pi-cli", display_name: "Pi (ACP agent)", binary: "pi", login_command: "pi (then sign in)", install_hint: "Install Pi from https://github.com/badlogic/pi", auth_kind: "Pi's own providers", cost_note: "Whatever Pi is configured with.", experimental: true, needs_allow_agent_tools: false, config_dir_env: None, input: PromptInput::Flag, output: OutputKind::JsonSingle },
        CliSpec { id: "fx-cli", display_name: "fx (ACP agent)", binary: "fx", login_command: "fx login", install_hint: "Install fx from its releases", auth_kind: "fx's own providers", cost_note: "Whatever fx is configured with.", experimental: true, needs_allow_agent_tools: false, config_dir_env: None, input: PromptInput::Flag, output: OutputKind::JsonSingle },
        CliSpec { id: "kiro-cli", display_name: "Kiro (ACP agent)", binary: "kiro-cli", login_command: "kiro-cli (then sign in)", install_hint: "Install Kiro CLI", auth_kind: "Kiro's own providers", cost_note: "Whatever Kiro is configured with.", experimental: true, needs_allow_agent_tools: false, config_dir_env: None, input: PromptInput::Flag, output: OutputKind::JsonSingle },
        CliSpec { id: "muse-cli", display_name: "Muse Code (ACP agent)", binary: "muse", login_command: "muse (then sign in)", install_hint: "Install Muse Code CLI", auth_kind: "Muse's own providers", cost_note: "Whatever Muse is configured with.", experimental: true, needs_allow_agent_tools: false, config_dir_env: None, input: PromptInput::Flag, output: OutputKind::JsonSingle },
        CliSpec { id: "goose-cli", display_name: "Goose (open source, local/BYOK)", binary: "goose", login_command: "goose configure", install_hint: "Install Goose from https://github.com/block/goose", auth_kind: "Local model or any provider Goose is configured with", cost_note: "Free with a local model. Runs its own tool loop, so this provider needs cli_allow_agent_tools = true.", experimental: true, needs_allow_agent_tools: true, config_dir_env: None, input: PromptInput::Arg, output: OutputKind::JsonLines },
        CliSpec { id: "custom-cli", display_name: "Custom CLI", binary: "", login_command: "See the CLI's own docs", install_hint: "Set [llm] cli_command to the binary", auth_kind: "Whatever the custom CLI uses", cost_note: "Whatever the custom CLI uses. Use for Crush, Amp, Kimi, Droid, llm, or anything else not listed.", experimental: true, needs_allow_agent_tools: false, config_dir_env: None, input: PromptInput::Arg, output: OutputKind::Text },
    ]
}

pub fn spec_for(id: &str) -> Option<CliSpec> {
    providers().into_iter().find(|s| s.id == id)
}

pub fn is_cli_provider(id: &str) -> bool {
    spec_for(id).is_some()
}

/// Claude model catalog, copied from the Hermes plugin's `model_catalog.py`.
/// Aliases move when Anthropic moves them; the `[1m]` suffix selects the 1M
/// window explicitly, because the CLI defaults to 200K otherwise.
pub fn claude_native_model(model: &str) -> String {
    let base = model
        .trim()
        .strip_prefix("anthropic/")
        .unwrap_or(model.trim());
    let canonical = match base {
        "sonnet" => "claude-sonnet-5-5",
        "haiku" => "claude-haiku-5-5",
        "opus" => "claude-opus-5-5",
        "fable" => "claude-fable-5-1",
        "claude-haiku-4-5" => "claude-haiku-4-5-20251001",
        other => other,
    };
    // The 1M window check runs on a dash-normalized id, so the dot form
    // users actually type (`claude-sonnet-5.5`, or `anthropic/claude-sonnet-5.5`)
    // matches the dash-form catalog. The returned name keeps the user's
    // spelling and just gains the `[1m]` suffix.
    let dashed = canonical.replace('.', "-");
    match dashed.as_str() {
        "claude-sonnet-5-5" | "claude-sonnet-5" | "claude-haiku-5-5" | "claude-opus-5-5"
        | "claude-opus-5" | "claude-opus-4-8" | "claude-fable-5-1" => format!("{canonical}[1m]"),
        _ => canonical.to_string(),
    }
}

/// Resolve a CLI binary the Hermes way: PATH first, then the usual install
/// directories, so a backend started with a minimal PATH still finds it. An
/// explicit command is the user's choice and is never second-guessed.
pub fn resolve_binary(binary: &str, command_override: &str) -> Option<PathBuf> {
    if !command_override.is_empty() {
        let p = PathBuf::from(command_override);
        if p.is_absolute() {
            return if is_executable(&p) { Some(p) } else { None };
        }
        return find_on_path(command_override);
    }
    if binary.is_empty() {
        return None;
    }
    if let Some(p) = find_on_path(binary) {
        return Some(p);
    }
    let home = dirs::home_dir()?;
    for prefix in [
        ".local/bin",
        ".claude/local",
        "bin",
        ".npm-global/bin",
        ".bun/bin",
        ".volta/bin",
    ] {
        let p = home.join(prefix).join(binary);
        if is_executable(&p) {
            return Some(p);
        }
    }
    for prefix in ["/opt/homebrew/bin", "/usr/local/bin"] {
        let p = PathBuf::from(prefix).join(binary);
        if is_executable(&p) {
            return Some(p);
        }
    }
    None
}

fn is_executable(p: &PathBuf) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        p.is_file()
            && p.metadata()
                .map(|m| m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var("PATH").unwrap_or_default();
    // std::env::split_paths uses the platform separator (`:` on Unix,
    // `;` on Windows). Splitting on `:` by hand breaks Windows paths
    // like `C:\Users\user\bin;C:\Windows`.
    for dir in std::env::split_paths(&path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let p = dir.join(name);
        if is_executable(&p) {
            return Some(make_absolute(p));
        }
        #[cfg(windows)]
        {
            for ext in ["exe", "cmd", "bat"] {
                let p = dir.join(format!("{name}.{ext}"));
                if p.is_file() {
                    return Some(make_absolute(p));
                }
            }
        }
    }
    None
}

/// A configured account directory must resolve the same way for the probe
/// and for the request: the probe runs in Heron's working directory, the
/// request in a fresh temp dir, so a relative path would point at two
/// different accounts. Expand `~` (no shell runs here) and make a relative
/// path absolute, so both sides inspect the same account.
fn normalize_config_dir(dir: &str) -> String {
    if dir.is_empty() {
        return String::new();
    }
    let expanded = if dir == "~" {
        dirs::home_dir()
            .map(|h| h.to_string_lossy().into_owned())
            .unwrap_or_else(|| dir.to_string())
    } else if let Some(rest) = dir.strip_prefix("~/") {
        match dirs::home_dir() {
            Some(home) => home.join(rest).to_string_lossy().into_owned(),
            None => dir.to_string(),
        }
    } else {
        dir.to_string()
    };
    let p = PathBuf::from(&expanded);
    if p.is_absolute() {
        expanded
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(p).to_string_lossy().into_owned())
            .unwrap_or(expanded)
    }
}

/// A resolved executable must be absolute. chat() spawns with the working
/// directory set to the per-request temp dir, so a relative path (from a
/// relative PATH entry, or a relative command override) would resolve
/// against the temp dir and fail to start, while the probes, which never
/// change directory, still report the provider as installed.
fn make_absolute(p: PathBuf) -> PathBuf {
    if p.is_absolute() {
        p
    } else {
        std::env::current_dir().map(|cwd| cwd.join(&p)).unwrap_or(p)
    }
}

pub struct ProviderStatus {
    pub id: String,
    pub display_name: String,
    pub installed: bool,
    pub version: Option<String>,
    /// None = no cheap login probe exists for this provider.
    pub logged_in: Option<bool>,
    pub plan: Option<String>,
    pub hint: String,
}

/// Ask the CLI who it is logged in as, the T3 Code way. The probe never
/// sends a model request and never starts a session: `claude auth status`
/// reads the local credential store, `codex login status` reads the cached
/// login token. Anything unexpected returns unknown, never a failure.
/// Whole-word match on lowercased output, so "pro" does not match
/// "profile" and "max" does not match "maximum".
fn words_contain(lower: &str, word: &str) -> bool {
    lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|w| w == word)
}

/// Parse a CLI's login-status output into (logged_in, plan). Pure, so the
/// negation edge cases are unit-tested without spawning a real CLI.
fn parse_login_output(spec_id: &str, out: &str) -> (Option<bool>, Option<String>) {
    let lower = out.to_lowercase();
    match spec_id {
        "claude-cli" => {
            // `claude auth status` prints JSON by default; the probe asks
            // for --text, but a version that ignores the flag still has to
            // report the login correctly, so parse the JSON first.
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(out) {
                if let Some(logged) = v.get("loggedIn").and_then(|l| l.as_bool()) {
                    return (Some(logged), None);
                }
            }
            // "Not logged in" contains "logged in", so the negation check
            // must come first.
            let logged = (lower.contains("logged in")
                || lower.contains("active account")
                || lower.contains("authenticated"))
                && !lower.contains("not logged in")
                && !lower.contains("not authenticated")
                && !lower.contains("no active account");
            let plan = logged
                .then(|| {
                    ["max", "pro", "team", "enterprise"]
                        .iter()
                        .find(|p| words_contain(&lower, p))
                        .map(|p| format!("Claude {p}"))
                })
                .flatten();
            (Some(logged), plan)
        }
        "codex-cli" => {
            let logged = (lower.contains("logged in using chatgpt") || lower.contains("logged in"))
                && !lower.contains("not logged in");
            let plan = logged
                .then(|| {
                    ["plus", "pro", "team", "enterprise"]
                        .iter()
                        .find(|p| words_contain(&lower, p))
                        .map(|p| format!("ChatGPT {p}"))
                })
                .flatten();
            (Some(logged), plan)
        }
        _ => (None, None),
    }
}

pub async fn probe_status(
    spec: &CliSpec,
    command_override: &str,
    config_dir: &str,
) -> ProviderStatus {
    let hint = if spec.experimental {
        format!(
            "Experimental: verify the flags with `{}` --help before relying on it. {}",
            spec.binary, spec.install_hint
        )
    } else {
        spec.install_hint.to_string()
    };
    let Some(path) = resolve_binary(spec.binary, command_override) else {
        let hint = if spec.binary.is_empty() {
            format!(
                "No binary configured. {} Then run `{}`.",
                spec.install_hint, spec.login_command
            )
        } else {
            format!(
                "{} is not installed. {} Then run `{}`.",
                spec.binary, spec.install_hint, spec.login_command
            )
        };
        return ProviderStatus {
            id: spec.id.to_string(),
            display_name: spec.display_name.to_string(),
            installed: false,
            version: None,
            logged_in: None,
            plan: None,
            hint,
        };
    };
    // The same account-directory override the request path applies in
    // child_env: without it the probe inspects the default account
    // while requests use the configured one. Normalized first, so a
    // relative path means the same directory here as in a request.
    let config_dir = normalize_config_dir(config_dir);
    let config_env: Option<(String, String)> = if !config_dir.is_empty() {
        spec.config_dir_env
            .map(|var| (var.to_string(), config_dir.to_string()))
    } else {
        None
    };
    let version = run_capture(&path, &["--version"], 10, config_env.as_ref())
        .await
        .ok()
        .map(|o| o.trim().to_string())
        .filter(|s| !s.is_empty());
    let (logged_in, plan) = match spec.id {
        "claude-cli" => {
            match run_capture(
                &path,
                &["auth", "status", "--text"],
                20,
                config_env.as_ref(),
            )
            .await
            {
                Ok(out) => parse_login_output("claude-cli", &out),
                Err(_) => (None, None),
            }
        }
        "codex-cli" => {
            match run_capture(&path, &["login", "status"], 20, config_env.as_ref()).await {
                Ok(out) => parse_login_output("codex-cli", &out),
                Err(_) => (None, None),
            }
        }
        _ => (None, None),
    };
    ProviderStatus {
        id: spec.id.to_string(),
        display_name: spec.display_name.to_string(),
        installed: true,
        version,
        logged_in,
        plan,
        hint: if logged_in == Some(false) {
            format!("Installed but not logged in. Run `{}`.", spec.login_command)
        } else {
            hint
        },
    }
}

async fn run_capture(
    path: &PathBuf,
    args: &[&str],
    timeout_secs: u64,
    config_env: Option<&(String, String)>,
) -> Result<String> {
    let mut cmd = Command::new(path);
    cmd.args(args);
    if let Some((k, v)) = config_env {
        cmd.env_remove(k);
        cmd.env(k, v);
    }
    let out = tokio::time::timeout(Duration::from_secs(timeout_secs), cmd.output())
        .await
        .map_err(|_| anyhow!("timed out"))??;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        Ok(String::from_utf8_lossy(&out.stderr).to_string())
    }
}

/// Render a chat request as one prompt. The CLI gets the full conversation
/// and the tool inventory, and is told to answer with the JSON envelope and
/// to use no tools of its own: Heron is the agent, the CLI is the model.
pub fn render_prompt(req: &ChatRequest) -> String {
    let mut out = String::new();
    for m in &req.messages {
        match m.role.as_str() {
            "system" => out.push_str(&format!("System instructions:\n{}\n\n", m.text())),
            "user" => out.push_str(&format!("User:\n{}\n\n", m.text())),
            "assistant" => {
                out.push_str(&format!("Assistant:\n{}\n\n", m.text()));
                for c in &m.tool_calls {
                    out.push_str(&format!(
                        "Assistant called {} with {}\n\n",
                        c.function.name, c.function.arguments
                    ));
                }
            }
            "tool" => out.push_str(&format!(
                "Tool result for {}:\n{}\n\n",
                m.tool_call_id.as_deref().unwrap_or(""),
                m.text()
            )),
            _ => out.push_str(&format!("{}:\n{}\n\n", m.role, m.text())),
        }
    }
    if !req.tools.is_empty() {
        out.push_str("Tools you may call, as JSON schemas. Call them only by listing them in your reply envelope; do not run anything yourself:\n");
        for t in &req.tools {
            out.push_str(&format!("{}\n", t));
        }
        out.push('\n');
    }
    out.push_str("Reply with ONLY a JSON object, no other text, no markdown fence:\n{\"text\": \"your reply\", \"tool_calls\": [{\"name\": \"<one of the tools above>\", \"arguments\": { ... }}]}\nUse an empty tool_calls list when no tool is needed. Do not run commands, edit files, or use any of your own tools. You are the language model; the host program runs the tools.");
    out
}

/// Parse the JSON envelope out of model text. Accepts a bare object, a
/// fenced ```json block, or an object embedded in surrounding text. Tool
/// names not in the request are dropped; malformed arguments drop that call.
pub fn extract_envelope(text: &str, valid_tools: &[String]) -> (String, Vec<ToolCall>) {
    let mut candidates: Vec<&str> = vec![text.trim()];
    if let Some(start) = text.find("```json") {
        if let Some(end) = text[start + 7..].find("```") {
            candidates.push(text[start + 7..start + 7 + end].trim());
        }
    } else if let Some(start) = text.find("```") {
        if let Some(end) = text[start + 3..].find("```") {
            candidates.push(text[start + 3..start + 3 + end].trim());
        }
    }
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            if end > start {
                candidates.push(&text[start..=end]);
            }
        }
    }
    for cand in candidates {
        if let Ok(v) = serde_json::from_str::<Value>(cand) {
            if v.get("text").and_then(|t| t.as_str()).is_some() || v.get("tool_calls").is_some() {
                let reply = v
                    .get("text")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();
                let mut calls = vec![];
                if let Some(list) = v.get("tool_calls").and_then(|t| t.as_array()) {
                    for (i, c) in list.iter().enumerate() {
                        let Some(name) = c.get("name").and_then(|n| n.as_str()) else {
                            continue;
                        };
                        if !valid_tools.is_empty() && !valid_tools.contains(&name.to_string()) {
                            continue;
                        }
                        let args = match c.get("arguments") {
                            Some(Value::Object(_)) | Some(Value::Array(_)) => {
                                c["arguments"].to_string()
                            }
                            Some(Value::String(s)) => {
                                if serde_json::from_str::<Value>(s).is_ok() {
                                    s.clone()
                                } else {
                                    continue;
                                }
                            }
                            _ => continue,
                        };
                        calls.push(ToolCall {
                            id: format!("cli_{}", uuid::Uuid::new_v4().simple()),
                            kind: "function".into(),
                            function: FunctionCall {
                                name: name.to_string(),
                                arguments: args,
                            },
                        });
                        let _ = i;
                    }
                }
                return (reply, calls);
            }
        }
    }
    (text.trim().to_string(), vec![])
}

/// Decoded prefix of the `"text"` value inside a JSON envelope that is
/// still arriving, plus whether the string closed. Returns None when the
/// `"text"` key has not arrived yet (or the envelope carries none). This
/// is what keeps the envelope framing out of the streamed output: only
/// the text value itself reaches `on_delta`, never `{"text": "` or the
/// tool-call arguments that follow it.
///
/// Only the envelope's top-level `text` counts. The scan walks the
/// top-level object's members with nesting tracked, so a `"text"` key
/// nested inside a tool argument is never streamed as the reply.
fn envelope_text_prefix(text: &str) -> Option<(String, bool)> {
    let bytes = text.as_bytes();
    let mut i = text.find('{')? + 1;
    loop {
        while i < bytes.len() && ((bytes[i] as char).is_whitespace() || bytes[i] == b',') {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        if bytes[i] == b'}' {
            return None;
        }
        if bytes[i] != b'"' {
            i = skip_json_value(bytes, text, i)?;
            continue;
        }
        let (key, next) = parse_json_string(bytes, text, i)?;
        i = next;
        while i < bytes.len() && (bytes[i] as char).is_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b':' {
            return None;
        }
        i += 1;
        while i < bytes.len() && (bytes[i] as char).is_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        if key == "text" {
            if bytes[i] != b'"' {
                return None;
            }
            let (decoded, _, terminated) = decode_json_string(bytes, text, i)?;
            return Some((decoded, terminated));
        }
        i = skip_json_value(bytes, text, i)?;
    }
}

/// Decode the JSON string starting at the `"` at `start`. Returns the
/// decoded prefix, the index just past what was consumed, and whether
/// the closing quote was found. `None` only when the string is so
/// malformed that nothing can be decoded (invalid UTF-8 or hex).
fn decode_json_string(bytes: &[u8], text: &str, start: usize) -> Option<(String, usize, bool)> {
    let mut out = String::new();
    let mut k = start + 1;
    while k < bytes.len() {
        match bytes[k] {
            b'"' => return Some((out, k + 1, true)),
            b'\\' => {
                if k + 1 >= bytes.len() {
                    return Some((out, k, false));
                }
                match bytes[k + 1] {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{8}'),
                    b'f' => out.push('\u{c}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        if k + 6 > bytes.len() {
                            return Some((out, k, false));
                        }
                        let hex = std::str::from_utf8(&bytes[k + 2..k + 6]).ok()?;
                        let code = u32::from_str_radix(hex, 16).ok()?;
                        if (0xD800..0xDC00).contains(&code) {
                            // High surrogate: combine it with the
                            // following low surrogate into one
                            // character. If the pair is split across
                            // fragments, wait for the rest instead of
                            // emitting a replacement character for half
                            // a pair.
                            if k + 12 > bytes.len() {
                                return Some((out, k, false));
                            }
                            let mut paired = false;
                            if bytes[k + 6] == b'\\' && bytes[k + 7] == b'u' {
                                if let Ok(hex2) = std::str::from_utf8(&bytes[k + 8..k + 12]) {
                                    if let Ok(low) = u32::from_str_radix(hex2, 16) {
                                        if (0xDC00..0xE000).contains(&low) {
                                            let c =
                                                0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                                            out.push(char::from_u32(c).unwrap_or('\u{fffd}'));
                                            paired = true;
                                            k += 6;
                                        }
                                    }
                                }
                            }
                            if !paired {
                                // Lone high surrogate.
                                out.push('\u{fffd}');
                            }
                        } else if (0xDC00..0xE000).contains(&code) {
                            // Lone low surrogate.
                            out.push('\u{fffd}');
                        } else {
                            out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                        }
                        k += 4;
                    }
                    other => out.push(other as char),
                }
                k += 2;
            }
            _ => {
                let ch = text[k..].chars().next()?;
                out.push(ch);
                k += ch.len_utf8();
            }
        }
    }
    Some((out, k, false))
}

/// Parse a complete JSON string starting at the `"` at `start`.
/// Returns the decoded string and the index just past its closing
/// quote, or None when the string has not arrived in full yet.
fn parse_json_string(bytes: &[u8], text: &str, start: usize) -> Option<(String, usize)> {
    let (decoded, next, terminated) = decode_json_string(bytes, text, start)?;
    if terminated {
        Some((decoded, next))
    } else {
        None
    }
}

/// Skip one JSON value starting at `i` (object, array, string, number,
/// or literal), returning the index just past it. Returns None when the
/// value has not arrived in full yet, so the caller waits for more
/// input instead of guessing.
fn skip_json_value(bytes: &[u8], text: &str, mut i: usize) -> Option<usize> {
    while i < bytes.len() && (bytes[i] as char).is_whitespace() {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    match bytes[i] {
        b'"' => {
            let (_, next) = parse_json_string(bytes, text, i)?;
            Some(next)
        }
        b'{' | b'[' => {
            let mut stack = vec![if bytes[i] == b'{' { b'}' } else { b']' }];
            i += 1;
            while i < bytes.len() {
                match bytes[i] {
                    b'"' => {
                        let (_, next) = parse_json_string(bytes, text, i)?;
                        i = next;
                    }
                    b'{' => {
                        stack.push(b'}');
                        i += 1;
                    }
                    b'[' => {
                        stack.push(b']');
                        i += 1;
                    }
                    b'}' | b']' => {
                        if stack.last() == Some(&bytes[i]) {
                            stack.pop();
                            i += 1;
                            if stack.is_empty() {
                                return Some(i);
                            }
                        } else {
                            return None;
                        }
                    }
                    _ => i += 1,
                }
            }
            None
        }
        _ => {
            while i < bytes.len()
                && !matches!(bytes[i], b',' | b'}' | b']')
                && !(bytes[i] as char).is_whitespace()
            {
                i += 1;
            }
            Some(i)
        }
    }
}

/// Truncate to at most `max` bytes without splitting a UTF-8 character.
/// Slicing `&s[..max]` panics when byte `max` falls inside a multibyte
/// character, which a localized diagnostic can easily do.
fn truncate_str(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn valid_tool_names(req: &ChatRequest) -> Vec<String> {
    req.tools
        .iter()
        .filter_map(|t| {
            t.get("function")
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str())
                .map(|s| s.to_string())
        })
        .collect()
}

/// Pull a text fragment out of one provider JSON event, trying the shapes
/// the CLIs in the registry actually emit.
fn event_text(v: &Value) -> Option<String> {
    if let Some(t) = v.get("text").and_then(|t| t.as_str()) {
        return Some(t.to_string());
    }
    if let Some(t) = v.get("result").and_then(|t| t.as_str()) {
        return Some(t.to_string());
    }
    if let Some(t) = v.get("response").and_then(|t| t.as_str()) {
        return Some(t.to_string());
    }
    if let Some(t) = v.get("output").and_then(|t| t.as_str()) {
        return Some(t.to_string());
    }
    if let Some(t) = v
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
    {
        return Some(t.to_string());
    }
    if let Some(t) = v
        .get("part")
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
    {
        return Some(t.to_string());
    }
    if v.get("type").and_then(|t| t.as_str()) == Some("agent_message") {
        if let Some(t) = v.get("text").and_then(|t| t.as_str()) {
            return Some(t.to_string());
        }
    }
    if let Some(item) = v.get("item") {
        if item.get("type").and_then(|t| t.as_str()) == Some("agent_message") {
            if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                return Some(t.to_string());
            }
        }
        if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
            return Some(t.to_string());
        }
    }
    if let Some(delta) = v.get("delta") {
        if let Some(t) = delta.as_str() {
            return Some(t.to_string());
        }
        if let Some(t) = delta.get("text").and_then(|t| t.as_str()) {
            return Some(t.to_string());
        }
    }
    if let Some(ev) = v.get("event") {
        return event_text(ev);
    }
    None
}

fn event_usage(v: &Value) -> Usage {
    let mut usage = Usage::default();
    for key in ["usage", "tokens", "stats"] {
        if let Some(u) = v.get(key) {
            usage.prompt_tokens = u
                .get("input_tokens")
                .and_then(|n| n.as_u64())
                .or_else(|| u.get("prompt_tokens").and_then(|n| n.as_u64()))
                .or_else(|| u.get("input").and_then(|n| n.as_u64()))
                .unwrap_or(usage.prompt_tokens);
            usage.completion_tokens = u
                .get("output_tokens")
                .and_then(|n| n.as_u64())
                .or_else(|| u.get("completion_tokens").and_then(|n| n.as_u64()))
                .or_else(|| u.get("output").and_then(|n| n.as_u64()))
                .unwrap_or(usage.completion_tokens);
        }
    }
    if usage.prompt_tokens == 0 {
        usage.prompt_tokens = v.get("input_tokens").and_then(|n| n.as_u64()).unwrap_or(0);
    }
    if usage.completion_tokens == 0 {
        usage.completion_tokens = v.get("output_tokens").and_then(|n| n.as_u64()).unwrap_or(0);
    }
    usage
}

fn merge_usage(a: &mut Usage, b: Usage) {
    a.prompt_tokens = a.prompt_tokens.max(b.prompt_tokens);
    a.completion_tokens = a.completion_tokens.max(b.completion_tokens);
}

/// One spawned CLI, treated as a text model. Every request gets a fresh
/// process in a fresh empty directory, and the process is killed on timeout.
#[derive(Debug)]
pub struct CliLlm {
    pub spec: CliSpec,
    pub command_override: String,
    pub model: String,
    pub config_dir: String,
    pub allow_agent_tools: bool,
    pub allow_api_key: bool,
    pub custom_args: Vec<String>,
    pub timeout_secs: u64,
    pub home: PathBuf,
}

impl CliLlm {
    pub fn from_config(cfg: &Config) -> Result<Self> {
        let id = cfg.llm.provider.trim();
        if id.is_empty() || id == "http" {
            bail!("no CLI provider selected: set [llm] provider to one of the ids listed by `heron providers`");
        }
        let Some(spec) = spec_for(id) else {
            bail!("unknown CLI provider `{id}`: run `heron providers` for the list");
        };
        if spec.needs_allow_agent_tools && !cfg.llm.cli_allow_agent_tools {
            bail!("{0} runs its own tool loop in headless mode and cannot be used as a plain text model. Set [llm] cli_allow_agent_tools = true to accept that, knowing the CLI may act on its own inside a throwaway directory, or pick another provider.", spec.display_name);
        }
        if id == "custom-cli" && cfg.llm.cli_command.trim().is_empty() {
            bail!("custom-cli needs [llm] cli_command set to the binary to run");
        }
        Ok(CliLlm {
            spec,
            command_override: cfg.llm.cli_command.clone(),
            model: cfg.llm.cli_model.clone(),
            config_dir: normalize_config_dir(&cfg.llm.cli_config_dir),
            allow_agent_tools: cfg.llm.cli_allow_agent_tools,
            allow_api_key: cfg.llm.cli_allow_api_key,
            custom_args: cfg.llm.cli_args.clone(),
            timeout_secs: cfg.llm.request_timeout_secs,
            home: cfg.home.clone(),
        })
    }

    fn binary_path(&self) -> Result<PathBuf> {
        let binary = if self.spec.id == "custom-cli" {
            self.command_override.as_str()
        } else {
            self.spec.binary
        };
        resolve_binary(binary, &self.command_override).ok_or_else(|| {
            anyhow!(
                "{} is not installed (no `{}` on PATH or in the usual install directories). {} Then run `{}`.",
                self.spec.display_name, binary, self.spec.install_hint, self.spec.login_command
            )
        })
    }

    /// Child environment: pass the parent env through (T3 Code pattern, so
    /// the CLI does its own routing), except the API-key variables that
    /// would flip a subscription turn into metered API billing, and except
    /// a per-provider config directory override.
    fn child_env(&self) -> Vec<(String, String)> {
        let mut env: Vec<(String, String)> = std::env::vars().collect();
        if !self.allow_api_key {
            let scrub: &[&str] = match self.spec.id {
                "claude-cli" => &[
                    "ANTHROPIC_API_KEY",
                    "ANTHROPIC_AUTH_TOKEN",
                    "ANTHROPIC_BASE_URL",
                    "ANTHROPIC_CUSTOM_HEADERS",
                ],
                "codex-cli" => &["OPENAI_API_KEY", "CODEX_API_KEY"],
                "gemini-cli" => &["GEMINI_API_KEY"],
                _ => &[],
            };
            env.retain(|(k, _)| !scrub.contains(&k.as_str()));
        }
        if !self.config_dir.is_empty() {
            if let Some(var) = self.spec.config_dir_env {
                env.retain(|(k, _)| k != var);
                env.push((var.to_string(), self.config_dir.clone()));
            }
        }
        if self.spec.id == "claude-cli" {
            // Never let the CLI update itself or upload feedback mid-session.
            env.push(("DISABLE_AUTOUPDATER".into(), "1".into()));
            env.push(("DISABLE_FEEDBACK_COMMAND".into(), "1".into()));
        }
        env
    }

    fn argv(&self, prompt: &str, model: &str) -> Result<(PathBuf, Vec<String>)> {
        // The size cap needs no binary: check it first, so a prompt over
        // the cap is refused even when the CLI is not installed.
        if matches!(
            self.spec.input,
            PromptInput::Arg | PromptInput::Flag | PromptInput::FlagValue
        ) {
            let limit = if cfg!(windows) { 30_000 } else { 120_000 };
            if prompt.len() > limit {
                bail!(
                    "the prompt for {} is {} bytes, over the {} byte limit for providers that take the prompt as one command-line argument. Shorten the conversation, or switch to a provider that takes the prompt on stdin (for example claude-cli or codex-cli).",
                    self.spec.display_name,
                    prompt.len(),
                    limit
                );
            }
        }
        let path = self.binary_path()?;
        let model = if self.spec.id == "claude-cli" {
            let base = if !model.is_empty() {
                model.to_string()
            } else {
                "sonnet".to_string()
            };
            Some(claude_native_model(&base))
        } else if model.is_empty() {
            None
        } else {
            Some(model.to_string())
        };
        let template_has_prompt =
            self.spec.id == "custom-cli" && self.custom_args.iter().any(|a| a.contains("{prompt}"));
        let mut args = if self.spec.id == "custom-cli" {
            self.custom_args
                .iter()
                .map(|a| {
                    a.replace("{model}", model.as_deref().unwrap_or(""))
                        .replace("{prompt}", prompt)
                })
                .collect::<Vec<_>>()
        } else {
            self.spec.base_args(model.as_deref())
        };
        match self.spec.input {
            PromptInput::Arg | PromptInput::Flag if !template_has_prompt => {
                args.push(prompt.to_string())
            }
            PromptInput::FlagValue if !template_has_prompt => {
                // Gemini-lineage CLIs (and Copilot) take the prompt as the
                // value of -p / --prompt: it must sit directly after the
                // flag, or the flag swallows the next option (--model) as
                // its value and the invocation fails before reaching it.
                match args.iter().position(|a| a == "-p") {
                    Some(i) => args.insert(i + 1, prompt.to_string()),
                    None => args.push(prompt.to_string()),
                }
            }
            _ => {}
        }
        Ok((path, args))
    }
}

/// Whether streamed output goes straight to the caller or is held back
/// until the JSON envelope's `text` value can be read.
#[derive(Clone, Copy, PartialEq)]
enum StreamMode {
    /// Nothing seen yet.
    Unknown,
    /// Plain-text reply: stream fragments as they arrive.
    Plain,
    /// JSON envelope reply: stream only the decoded `text` value.
    Envelope,
}

/// How to treat the opening of a reply while the stream mode is still
/// undecided.
enum Opening {
    /// Too little has arrived to tell; emit nothing yet.
    Wait,
    /// The reply is a JSON envelope; stream only its `text` value.
    Envelope,
    /// The reply is plain text; stream everything buffered so far.
    Plain,
}

/// Classify the opening of a reply. A fenced envelope (```json\n{...}
/// or ```\n{...}) arrives backtick-first, so deciding Plain on the
/// backticks would stream the fence and the JSON framing to the terminal
/// while `finish()` quietly saves the clean reply.
fn classify_opening(rest: &str) -> Opening {
    // A fence opener still arriving is ambiguous: it could be the
    // start of ```json or just backticks.
    if "```json".starts_with(rest) {
        return Opening::Wait;
    }
    if !rest.starts_with("```") {
        return Opening::Plain;
    }
    let mut after = rest[3..].trim_start();
    if after.is_empty() {
        return Opening::Wait;
    }
    // A partial "json" tag is still ambiguous.
    let lower = after.to_ascii_lowercase();
    if lower.len() < 4 && "json".starts_with(&lower) {
        return Opening::Wait;
    }
    if lower.starts_with("json") {
        // The first four bytes are ASCII "json", so byte 4 is a char
        // boundary.
        after = after[4..].trim_start();
        if after.is_empty() {
            return Opening::Wait;
        }
    }
    if after.starts_with('{') {
        Opening::Envelope
    } else {
        Opening::Plain
    }
}

struct Accumulator {
    text: String,
    usage: Usage,
    tool_calls: Vec<ToolCall>,
    streamed: bool,
    /// True once a stream event delivered text into `text`, whether or
    /// not any of it reached the user as a delta. A tool-only envelope
    /// delivers text but emits nothing, and the assistant snapshot must
    /// not be appended a second time.
    received: bool,
    stream_mode: StreamMode,
    /// Bytes of the decoded envelope `text` value already streamed.
    emitted: usize,
    /// Lines of a JsonSingle/JsonAuto reply that have not parsed yet.
    /// A pretty-printed single JSON document arrives over many lines;
    /// it is only decoded once the whole document is there.
    json_buf: String,
}

impl Accumulator {
    fn new() -> Self {
        Accumulator {
            text: String::new(),
            usage: Usage::default(),
            tool_calls: vec![],
            streamed: false,
            received: false,
            stream_mode: StreamMode::Unknown,
            emitted: 0,
            json_buf: String::new(),
        }
    }

    /// Move any buffered JsonSingle/JsonAuto text into `text`. Called
    /// before the empty-response check so a plain-text reply is not
    /// rejected as empty; `finish` does the same move, harmlessly.
    fn flush_json_buf(&mut self) {
        if !self.json_buf.is_empty() {
            self.text.push_str(&self.json_buf);
            self.json_buf.clear();
        }
    }

    /// Append a fragment of the model's text and stream it, without ever
    /// showing the JSON envelope framing. Plain replies stream as they
    /// arrive; envelope replies stream only the decoded `text` value, so
    /// the user never sees `{"text": "` or the tool-call arguments.
    fn push_text(&mut self, frag: &str, on_delta: DeltaFn<'_>) {
        self.text.push_str(frag);
        match self.stream_mode {
            StreamMode::Unknown => {
                let rest = self.text.trim_start();
                if rest.is_empty() {
                    // Only whitespace so far; wait for real content
                    // before deciding whether this is an envelope or
                    // plain text. Deciding on whitespace would
                    // misclassify an envelope that starts with a
                    // newline or a space.
                } else if rest.starts_with('{') {
                    self.stream_mode = StreamMode::Envelope;
                    self.emit_envelope_prefix(on_delta);
                } else {
                    match classify_opening(rest) {
                        Opening::Wait => {}
                        Opening::Envelope => {
                            self.stream_mode = StreamMode::Envelope;
                            self.emit_envelope_prefix(on_delta);
                        }
                        Opening::Plain => {
                            self.stream_mode = StreamMode::Plain;
                            // Everything so far, including any leading
                            // whitespace, is the start of the reply.
                            let all = self.text.clone();
                            on_delta(&all);
                            self.streamed = true;
                        }
                    }
                }
            }
            StreamMode::Plain => {
                on_delta(frag);
                self.streamed = true;
            }
            StreamMode::Envelope => self.emit_envelope_prefix(on_delta),
        }
    }

    fn emit_envelope_prefix(&mut self, on_delta: DeltaFn<'_>) {
        if let Some((decoded, _)) = envelope_text_prefix(&self.text) {
            if decoded.len() > self.emitted {
                // `emitted` is always the length of a previously emitted
                // prefix of `decoded`, so this slice is a char boundary.
                let new = &decoded[self.emitted..];
                on_delta(new);
                self.streamed = true;
                self.emitted = decoded.len();
            }
        }
    }

    /// One line of the CLI's stdout. Returns text to stream to the caller.
    fn line(&mut self, line: &str, spec: &CliSpec, on_delta: DeltaFn<'_>) -> Option<String> {
        if spec.output == OutputKind::Text {
            // Plain-text replies keep their lines intact: no trimming,
            // and the newline is part of the streamed output. Trimming
            // here would strip indentation and blank lines and glue
            // lines together in the stream, corrupting code blocks.
            let with_nl = format!("{line}\n");
            self.push_text(&with_nl, on_delta);
            return Some(with_nl);
        }
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        match spec.output {
            OutputKind::ClaudeStreamJson => self.claude_line(line, on_delta),
            OutputKind::JsonLines => self.jsonl_line(line, on_delta),
            OutputKind::JsonSingle | OutputKind::JsonAuto => self.json_auto_line(line, on_delta),
            OutputKind::Text => unreachable!("handled above"),
        }
    }

    fn claude_line(&mut self, line: &str, on_delta: DeltaFn<'_>) -> Option<String> {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            self.text.push_str(line);
            return None;
        };
        merge_usage(&mut self.usage, event_usage(&v));
        match v.get("type").and_then(|t| t.as_str()) {
            Some("stream_event") => {
                if let Some(ev) = v.get("event") {
                    if ev.get("type").and_then(|t| t.as_str()) == Some("content_block_delta") {
                        if let Some(t) = ev
                            .get("delta")
                            .and_then(|d| d.get("text"))
                            .and_then(|t| t.as_str())
                        {
                            if !t.is_empty() {
                                self.received = true;
                            }
                            self.push_text(t, on_delta);
                            return Some(t.to_string());
                        }
                    }
                }
                None
            }
            Some("assistant") => {
                if let Some(content) = v
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_array())
                {
                    for block in content {
                        match block.get("type").and_then(|t| t.as_str()) {
                            Some("text") => {
                                if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                                    // The assistant snapshot repeats the
                                    // text blocks stream events already
                                    // delivered. `streamed` only records
                                    // what reached the user, so a tool-only
                                    // envelope (empty text, nothing
                                    // emitted) would be appended a second
                                    // time and corrupt the buffer. Skip it
                                    // when any text was received.
                                    if !self.received {
                                        self.push_text(t, on_delta);
                                    }
                                }
                            }
                            Some("tool_use") => {
                                // Defensive: --tools '' should make this
                                // impossible, but map it if a CLI version
                                // emits one anyway.
                                if let (Some(name), Some(input)) = (
                                    block.get("name").and_then(|n| n.as_str()),
                                    block.get("input"),
                                ) {
                                    let name = name.strip_prefix("mcp__heron__").unwrap_or(name);
                                    self.tool_calls.push(ToolCall {
                                        id: block
                                            .get("id")
                                            .and_then(|i| i.as_str())
                                            .unwrap_or("")
                                            .to_string(),
                                        kind: "function".into(),
                                        function: FunctionCall {
                                            name: name.to_string(),
                                            arguments: input.to_string(),
                                        },
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                }
                None
            }
            Some("result") => {
                if let Some(t) = v.get("result").and_then(|t| t.as_str()) {
                    if !self.streamed {
                        self.text = t.to_string();
                    } else if self.text.is_empty() {
                        self.text = t.to_string();
                    }
                }
                if let Some(err) = v.get("is_error").and_then(|e| e.as_bool()) {
                    if err && self.text.is_empty() {
                        self.text = v
                            .get("result")
                            .and_then(|t| t.as_str())
                            .unwrap_or("the CLI reported an error")
                            .to_string();
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn jsonl_line(&mut self, line: &str, on_delta: DeltaFn<'_>) -> Option<String> {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            self.text.push_str(line);
            self.text.push('\n');
            return None;
        };
        merge_usage(&mut self.usage, event_usage(&v));
        // Started/updated items are partials of the completed item that
        // follows; counting them would duplicate the message.
        if let Some(t) = v.get("type").and_then(|t| t.as_str()) {
            if t == "item.started" || t == "item.updated" || t == "reasoning" {
                return None;
            }
            if let Some(item) = v.get("item") {
                if item.get("type").and_then(|t| t.as_str()) == Some("reasoning") {
                    return None;
                }
            }
        }
        // Codex completes one agent message per item; OpenCode/Cline/Copilot
        // emit text events. Completed items are the unit, not starts, so a
        // message is not counted twice.
        let is_complete = v
            .get("type")
            .and_then(|t| t.as_str())
            .map(|t| t == "item.completed" || t == "text" || t == "agent_message" || t == "message")
            .unwrap_or(false);
        let is_turn_end = v.get("type").and_then(|t| t.as_str()) == Some("turn.completed");
        if is_turn_end {
            return None;
        }
        if let Some(t) = event_text(&v) {
            if is_complete || v.get("type").and_then(|t| t.as_str()) == Some("text") {
                self.push_text(&t, on_delta);
                return Some(t);
            }
            // A text fragment inside a larger event: keep it, stream it once.
            if !t.is_empty() && !self.text.ends_with(&t) {
                self.push_text(&t, on_delta);
                return Some(t);
            }
        }
        None
    }

    fn json_auto_line(&mut self, line: &str, on_delta: DeltaFn<'_>) -> Option<String> {
        // A single line that parses on its own is a complete JSON-lines
        // event; handle it directly, flushing any orphaned buffered lines
        // as plain text first. Otherwise the line may be one line of a
        // pretty-printed single document, so buffer it until the whole
        // document parses, then decode it (response/result first).
        // Decoding line by line would treat each pretty-printed line as
        // plain text and lose the envelope inside the wrapper.
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            if !self.json_buf.is_empty() {
                self.text.push_str(&self.json_buf);
                self.json_buf.clear();
            }
            merge_usage(&mut self.usage, event_usage(&v));
            if let Some(t) = event_text(&v) {
                self.push_text(&t, on_delta);
                return Some(t);
            }
            return None;
        }
        self.json_buf.push_str(line);
        self.json_buf.push('\n');
        if let Ok(v) = serde_json::from_str::<Value>(self.json_buf.trim()) {
            self.json_buf.clear();
            merge_usage(&mut self.usage, event_usage(&v));
            if let Some(t) = event_text(&v) {
                self.push_text(&t, on_delta);
                return Some(t);
            }
            return None;
        }
        None
    }

    fn finish(mut self, req: &ChatRequest, on_delta: DeltaFn<'_>) -> ChatResponse {
        // A JsonSingle/JsonAuto buffer that never parsed is plain text,
        // not a document; it is the reply.
        self.flush_json_buf();
        let text = self.text.trim().to_string();
        let valid = valid_tool_names(req);
        let (reply, mut calls) = extract_envelope(&text, &valid);
        // Native tool_use blocks (Claude only) win over envelope parsing
        // when the envelope carried none; otherwise the envelope is the
        // contract and native calls are dropped as duplicates.
        if calls.is_empty() && !self.tool_calls.is_empty() {
            calls = self
                .tool_calls
                .iter()
                .filter(|c| valid.is_empty() || valid.contains(&c.function.name))
                .cloned()
                .collect();
        }
        if !self.streamed && !reply.is_empty() {
            on_delta(&reply);
        }
        let message = Message {
            role: "assistant".into(),
            content: Some(reply),
            tool_calls: calls,
            tool_call_id: None,
        };
        ChatResponse {
            message,
            usage: self.usage,
            model: String::new(),
        }
    }
}

/// Kill a process group by id (Unix only). Best effort: the group may
/// already be gone.
#[cfg(unix)]
fn kill_group(pgid: u32) {
    unsafe {
        libc::killpg(pgid as libc::pid_t, libc::SIGKILL);
    }
}

#[cfg(not(unix))]
fn kill_group(_pgid: u32) {}

/// Kill the provider's whole process tree: the group first, then the
/// direct child. Used on timeout and on read errors, where the child is
/// still running.
async fn kill_process_tree(child: &mut tokio::process::Child) {
    if let Some(pid) = child.id() {
        kill_group(pid);
    }
    let _ = child.kill().await;
}

/// Kills the provider's process group when the chat future is dropped
/// (for example by /cancel aborting the task). `kill_on_drop` only
/// kills the direct child; without this, a wrapper's descendants keep
/// running and consuming usage after the user cancelled.
#[cfg(unix)]
struct KillTreeOnDrop {
    pgid: u32,
}

#[cfg(unix)]
impl Drop for KillTreeOnDrop {
    fn drop(&mut self) {
        kill_group(self.pgid);
    }
}

/// Removes the request's private temp directory when dropped, so
/// cancellation (which skips the explicit cleanup calls) and early
/// errors cannot leave request artifacts behind.
struct RemoveTempDirOnDrop {
    dir: PathBuf,
}

impl Drop for RemoveTempDirOnDrop {
    fn drop(&mut self) {
        let _ = remove_temp_dir(&self.dir);
    }
}

/// The command override applies only to the selected provider. Passing
/// it to every probe would report all providers as installed with that
/// one binary's version.
pub fn override_for_provider<'a>(spec_id: &str, selected: &str, cli_command: &'a str) -> &'a str {
    if spec_id == selected {
        cli_command
    } else {
        ""
    }
}

#[async_trait]
impl Llm for CliLlm {
    async fn chat(&self, req: ChatRequest, on_delta: DeltaFn<'_>) -> Result<ChatResponse> {
        let prompt = render_prompt(&req);
        // The agent routes simple and background requests to the fast model
        // through ChatRequest.model; that override wins over cli_model, so
        // the CLI actually runs the model the UI announces.
        let model = req
            .model
            .as_deref()
            .filter(|m| !m.trim().is_empty())
            .unwrap_or(self.model.as_str());
        let (path, args) = self.argv(&prompt, model)?;
        let run_dir = make_private_temp_dir(&self.home)?;
        // Removes the dir when chat() returns or the future is dropped
        // (cancel). This is the only cleanup: explicit calls cannot run
        // on cancellation or on the spawn-error return below.
        let _dir_guard = RemoveTempDirOnDrop {
            dir: run_dir.clone(),
        };
        let mut cmd = Command::new(&path);
        cmd.args(&args)
            .current_dir(&run_dir)
            .env_clear()
            .envs(self.child_env());
        // On Unix the child leads its own process group, so a kill can
        // reach the whole provider tree, not just the direct child.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        match self.spec.input {
            PromptInput::Stdin => {
                cmd.stdin(std::process::Stdio::piped());
            }
            _ => {
                cmd.stdin(std::process::Stdio::null());
            }
        }
        cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        // A binary that was just written can still be busy on some
        // filesystems (overlayfs on CI returns ETXTBSY even after the
        // writer closed it). The process never started, so retrying the
        // spawn is side-effect free. The same race can hit a real CLI
        // right after its own updater rewrote it.
        let mut child = {
            let mut attempt = 0;
            loop {
                match cmd.spawn() {
                    Ok(child) => break child,
                    Err(e) if e.raw_os_error() == Some(26) && attempt < 5 => {
                        attempt += 1;
                        tokio::time::sleep(Duration::from_millis(20 * attempt)).await;
                    }
                    Err(e) => {
                        return Err(anyhow!(
                            "could not start `{}` ({}): {e}. {}",
                            path.display(),
                            self.spec.display_name,
                            self.spec.install_hint
                        ));
                    }
                }
            }
        };
        // The process-group id, saved now: after wait() Tokio's
        // child.id() returns None, so the group can only be killed
        // from this saved value.
        let pgid = child.id();
        // Kills the group if this future is dropped (for example by
        // /cancel aborting the task). Lives until chat() returns.
        #[cfg(unix)]
        let _tree_guard = pgid.map(|pid| KillTreeOnDrop { pgid: pid });
        // Write the prompt on a separate task so a long prompt cannot
        // deadlock against the child's stdout pipe.
        if self.spec.input == PromptInput::Stdin {
            if let Some(mut stdin) = child.stdin.take() {
                let prompt = prompt.clone();
                tokio::spawn(async move {
                    let _ = stdin.write_all(prompt.as_bytes()).await;
                    let _ = stdin.shutdown().await;
                });
            }
        }
        // Drain stderr on its own task from the start. A chatty provider
        // can fill the stderr pipe and block forever if stderr is only
        // read after the child exits.
        let stderr_buf = std::sync::Arc::new(tokio::sync::Mutex::new(String::new()));
        let stderr_task = child.stderr.take().map(|mut err| {
            let buf = stderr_buf.clone();
            tokio::spawn(async move {
                use tokio::io::AsyncReadExt;
                let mut s = String::new();
                let _ = err.read_to_string(&mut s).await;
                *buf.lock().await = s;
            })
        });
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("could not read {} stdout", self.spec.binary))?;
        let mut lines = BufReader::new(stdout).lines();
        let timeout = Duration::from_secs(self.timeout_secs.max(10));
        let spec = self.spec.clone();
        let mut acc = Accumulator::new();
        let read = async {
            while let Some(line) = lines.next_line().await? {
                acc.line(&line, &spec, on_delta);
            }
            Result::<_, anyhow::Error>::Ok(())
        };
        match tokio::time::timeout(timeout, read).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                kill_process_tree(&mut child).await;
                return Err(e);
            }
            Err(_) => {
                kill_process_tree(&mut child).await;
                let _ = child.wait().await;
                bail!("{} took longer than {}s and was killed. The prompt may be too long for this CLI, or the CLI may be waiting on a login or approval. Run `{}` to check.", self.spec.display_name, self.timeout_secs, self.spec.login_command);
            }
        }
        let status = child.wait().await?;
        // A descendant that outlived the child still holds the stderr
        // pipe open; kill the group before awaiting the drain task, or
        // that await never returns. The id must come from the value
        // saved at spawn: after wait() child.id() is None.
        if let Some(pid) = pgid {
            kill_group(pid);
        }
        if let Some(handle) = stderr_task {
            let _ = handle.await;
        }
        let stderr = stderr_buf.lock().await.clone();
        // Flush before measuring: a plain-text JsonSingle/JsonAuto reply
        // sits in json_buf until finish(), and must not be rejected as
        // "returned no text".
        acc.flush_json_buf();
        let text_len = acc.text.trim().len();
        if !status.success() {
            let err = stderr.trim();
            let preview = if err.len() > 600 {
                format!("{}...", truncate_str(err, 600))
            } else {
                err.to_string()
            };
            // Hermes accepts error_max_turns with a complete batch; the same
            // applies here: a finished answer survives a non-zero exit.
            let recoverable = text_len > 0
                && (preview.contains("max_turns")
                    || preview.contains("max turns")
                    || status.code() == Some(1))
                && !preview.to_lowercase().contains("auth")
                && !preview.to_lowercase().contains("login");
            if !recoverable {
                // A failed exit is an error even when some text arrived:
                // a partial reply must not be saved as a successful answer.
                let lower = preview.to_lowercase();
                if lower.contains("auth")
                    || lower.contains("not logged in")
                    || lower.contains("login")
                    || lower.contains("unauthorized")
                {
                    bail!("{} is not logged in. Run `{}` on this machine, then try again. (CLI said: {})", self.spec.display_name, self.spec.login_command, preview);
                }
                if lower.contains("rate limit")
                    || lower.contains("limit") && lower.contains("usage")
                {
                    bail!("{} refused the request: {}. The subscription's usage window may be exhausted; wait or check the plan.", self.spec.display_name, preview);
                }
                if text_len == 0 {
                    bail!("{} failed (exit {}): {}. Check `{}` and run `heron providers` to re-check the install.", self.spec.display_name, status.code().unwrap_or(-1), preview, self.spec.login_command);
                }
                bail!("{} failed (exit {}): {}. The reply was incomplete, so it was not saved as a successful answer. Check `{}` and run `heron providers` to re-check the install.", self.spec.display_name, status.code().unwrap_or(-1), preview, self.spec.login_command);
            }
            // Recoverable: fall through and return the answer we have.
        }
        if text_len == 0 {
            let preview = stderr.trim();
            bail!(
                "{} returned no text. {}",
                self.spec.display_name,
                if preview.is_empty() {
                    "It may need a login; run the login command and try again.".to_string()
                } else {
                    format!("CLI said: {}", truncate_str(preview, 300))
                }
            );
        }
        let mut resp = acc.finish(&req, on_delta);
        resp.model = if model.is_empty() {
            self.spec.id.to_string()
        } else {
            model.to_string()
        };
        Ok(resp)
    }
}

/// A fresh empty directory under the Heron home's temp area, mode 0700 on
/// Unix, so the CLI's working directory holds nothing it can read or change.
fn make_private_temp_dir(home: &PathBuf) -> Result<PathBuf> {
    let base = home.join("tmp");
    std::fs::create_dir_all(&base)?;
    let dir = base.join(format!("cli-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    Ok(dir)
}

fn remove_temp_dir(dir: &PathBuf) -> Result<()> {
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Fake provider binary: a POSIX shell script. Tests that use it are
    /// gated off Windows (`#[cfg(not(windows))]`), because Windows cannot
    /// execute a `#!/bin/sh` script and fails with "not a valid Win32
    /// application". The parsing logic they cover is platform-independent.
    fn fake_bin(dir: &std::path::Path, name: &str, script: &str) -> PathBuf {
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(script.as_bytes()).unwrap();
        // Flush before returning: exec of a still-open file fails with
        // "Text file busy" on some filesystems.
        f.sync_all().unwrap();
        drop(f);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
        }
        path
    }

    fn test_cfg(home: &std::path::Path, provider: &str) -> Config {
        let mut cfg = Config::default();
        cfg.home = home.to_path_buf();
        cfg.llm.provider = provider.to_string();
        cfg.llm.request_timeout_secs = 30;
        cfg
    }

    #[test]
    fn registry_has_no_auto_approve_flags() {
        let mut ids = vec![];
        for spec in providers() {
            assert!(!spec.id.is_empty());
            assert!(!ids.contains(&spec.id), "duplicate provider id {}", spec.id);
            ids.push(spec.id);
            if spec.id != "custom-cli" {
                assert!(!spec.binary.is_empty(), "{} has no binary", spec.id);
                assert!(
                    !spec.login_command.is_empty(),
                    "{} has no login command",
                    spec.id
                );
            }
            for arg in spec.base_args(Some("test-model")) {
                let lower = arg.to_lowercase();
                for banned in [
                    "--yolo",
                    "--force",
                    "--dangerously-skip-permissions",
                    "--allow-all-tools",
                    "--allow-all",
                    "auto-approve",
                    "--auto-approve",
                ] {
                    assert!(
                        !lower.contains(banned),
                        "{} passes banned flag {arg}",
                        spec.id
                    );
                }
            }
        }
        assert!(
            ids.contains(&"claude-cli")
                && ids.contains(&"codex-cli")
                && ids.contains(&"opencode-cli")
                && ids.contains(&"cline-cli")
                && ids.contains(&"cursor-cli")
                && ids.contains(&"grok-cli")
                && ids.contains(&"antigravity-cli")
                && ids.contains(&"copilot-cli")
                && ids.contains(&"qwen-cli")
                && ids.contains(&"gemini-cli")
                && ids.contains(&"pi-cli")
                && ids.contains(&"custom-cli")
        );
    }

    #[test]
    fn claude_model_catalog_maps_aliases_and_1m() {
        assert_eq!(claude_native_model("sonnet"), "claude-sonnet-5-5[1m]");
        assert_eq!(claude_native_model("haiku"), "claude-haiku-5-5[1m]");
        assert_eq!(claude_native_model("opus"), "claude-opus-5-5[1m]");
        assert_eq!(claude_native_model("fable"), "claude-fable-5-1[1m]");
        assert_eq!(
            claude_native_model("anthropic/claude-sonnet-5.5"),
            "claude-sonnet-5.5[1m]"
        );
        assert_eq!(
            claude_native_model("claude-haiku-4-5"),
            "claude-haiku-4-5-20251001"
        );
        assert_eq!(
            claude_native_model("some-future-model"),
            "some-future-model"
        );
    }

    #[test]
    fn login_status_parsing_rejects_negations() {
        // Caught live: `codex login status` prints "Not logged in", which
        // contains "logged in" and was reported as logged in (ChatGPT pro).
        assert_eq!(
            parse_login_output("codex-cli", "Not logged in"),
            (Some(false), None)
        );
        assert_eq!(
            parse_login_output("codex-cli", "Logged in using ChatGPT\nPlan: Pro"),
            (Some(true), Some("ChatGPT pro".to_string()))
        );
        assert_eq!(
            parse_login_output("claude-cli", "Not logged in"),
            (Some(false), None)
        );
        assert_eq!(
            parse_login_output("claude-cli", "Logged in as fernando@example.com\nPlan: Max"),
            (Some(true), Some("Claude max".to_string()))
        );
        // "profile" must not read as the Pro plan.
        assert_eq!(
            parse_login_output("codex-cli", "Logged in using ChatGPT\nProfile: default"),
            (Some(true), None)
        );
    }

    #[test]
    fn envelope_parsing_accepts_bare_fenced_and_embedded() {
        let tools = vec!["list_dir".to_string(), "read_file".to_string()];
        let (text, calls) = extract_envelope(
            r#"{"text": "two files", "tool_calls": [{"name": "list_dir", "arguments": {"path": "."}}]}"#,
            &tools,
        );
        assert_eq!(text, "two files");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "list_dir");
        assert_eq!(calls[0].function.arguments, "{\"path\":\".\"}");
        let (text, calls) = extract_envelope(
            "here is the answer\n```json\n{\"text\": \"done\", \"tool_calls\": []}\n```\n",
            &tools,
        );
        assert_eq!(text, "done");
        assert!(calls.is_empty());
        let (text, calls) = extract_envelope(
            "prefix {\"text\": \"ok\", \"tool_calls\": []} suffix",
            &tools,
        );
        assert_eq!(text, "ok");
        assert!(calls.is_empty());
    }

    #[test]
    fn envelope_drops_unknown_tools_and_bad_arguments() {
        let tools = vec!["list_dir".to_string()];
        let (_, calls) = extract_envelope(
            r#"{"text": "x", "tool_calls": [{"name": "shell", "arguments": {"cmd": "rm -rf /"}}, {"name": "list_dir", "arguments": "not json"}]}"#,
            &tools,
        );
        assert!(
            calls.is_empty(),
            "unknown tool and malformed arguments must not reach the agent: {calls:?}"
        );
    }

    #[test]
    fn envelope_plain_text_is_a_reply_with_no_calls() {
        let (text, calls) =
            extract_envelope("just words, no json at all", &["list_dir".to_string()]);
        assert_eq!(text, "just words, no json at all");
        assert!(calls.is_empty());
    }

    #[test]
    fn prompt_names_the_envelope_and_forbids_cli_tools() {
        let req = ChatRequest {
            messages: vec![Message::user("hi")],
            tools: vec![
                serde_json::json!({"type": "function", "function": {"name": "list_dir", "parameters": {}}}),
            ],
            ..Default::default()
        };
        let prompt = render_prompt(&req);
        assert!(prompt.contains("list_dir"));
        assert!(prompt.contains("tool_calls"));
        assert!(prompt.contains("Do not run commands"));
    }

    #[test]
    fn cline_and_goose_refuse_without_an_explicit_allow_flag() {
        let dir = tempfile::tempdir().unwrap();
        for id in ["cline-cli", "goose-cli"] {
            let cfg = test_cfg(dir.path(), id);
            let err = CliLlm::from_config(&cfg).unwrap_err();
            assert!(
                err.to_string().contains("cli_allow_agent_tools"),
                "{id}: {err}"
            );
        }
        let mut cfg = test_cfg(dir.path(), "cline-cli");
        cfg.llm.cli_allow_agent_tools = true;
        assert!(CliLlm::from_config(&cfg).is_ok());
    }

    #[test]
    fn unknown_provider_is_an_error_naming_providers() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = test_cfg(dir.path(), "nope-cli");
        let err = CliLlm::from_config(&cfg).unwrap_err();
        assert!(err.to_string().contains("heron providers"), "{err}");
    }

    #[tokio::test]
    #[cfg(not(windows))]
    async fn claude_fake_binary_parses_stream_json_and_tool_call() {
        let dir = tempfile::tempdir().unwrap();
        let envelope = r#"{\"text\": \"two files\", \"tool_calls\": [{\"name\": \"list_dir\", \"arguments\": {\"path\": \".\"}}]}"#;
        let script = format!(
            "#!/bin/sh\ncat > /dev/null\nprintf '%s\\n' '{{\"type\":\"system\",\"subtype\":\"init\"}}' '{{\"type\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"content\":[{{\"type\":\"text\",\"text\":\"{envelope}\"}}]}}}}' '{{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"{envelope}\",\"usage\":{{\"input_tokens\":11,\"output_tokens\":4}}}}'\n"
        );
        let bin = fake_bin(dir.path(), "claude", &script);
        let mut cfg = test_cfg(dir.path(), "claude-cli");
        cfg.llm.cli_command = bin.to_string_lossy().to_string();
        let llm = CliLlm::from_config(&cfg).unwrap();
        let seen = std::sync::Mutex::new(String::new());
        let req = ChatRequest {
            messages: vec![Message::user("hi")],
            tools: vec![
                serde_json::json!({"type": "function", "function": {"name": "list_dir", "parameters": {}}}),
            ],
            ..Default::default()
        };
        let r = llm
            .chat(req, &|t| seen.lock().unwrap().push_str(t))
            .await
            .unwrap();
        assert_eq!(r.message.text(), "two files");
        assert_eq!(r.message.tool_calls.len(), 1);
        assert_eq!(
            r.message.tool_calls[0].function.arguments,
            "{\"path\":\".\"}"
        );
        assert_eq!(r.usage.prompt_tokens, 11);
        assert_eq!(r.usage.completion_tokens, 4);
        assert_eq!(
            seen.lock().unwrap().as_str(),
            "two files",
            "streamed output must be the reply text, not the JSON envelope"
        );
    }

    #[tokio::test]
    #[cfg(not(windows))]
    async fn claude_tool_only_envelope_is_not_duplicated_by_the_assistant_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let part1 = r#"{\"text\": \"\", \"tool_calls\": ["#;
        let part2 = r#"{\"name\": \"list_dir\", \"arguments\": {\"path\": \".\"}}]}"#;
        let envelope = r#"{\"text\": \"\", \"tool_calls\": [{\"name\": \"list_dir\", \"arguments\": {\"path\": \".\"}}]}"#;
        let script = format!(
            "#!/bin/sh\ncat > /dev/null\nprintf '%s\\n' '{{\"type\":\"stream_event\",\"event\":{{\"type\":\"content_block_delta\",\"delta\":{{\"text\":\"{part1}\"}}}}}}' '{{\"type\":\"stream_event\",\"event\":{{\"type\":\"content_block_delta\",\"delta\":{{\"text\":\"{part2}\"}}}}}}' '{{\"type\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"content\":[{{\"type\":\"text\",\"text\":\"{envelope}\"}}]}}}}'\necho 'max turns reached' >&2\nexit 1\n"
        );
        let bin = fake_bin(dir.path(), "claude", &script);
        let mut cfg = test_cfg(dir.path(), "claude-cli");
        cfg.llm.cli_command = bin.to_string_lossy().to_string();
        let llm = CliLlm::from_config(&cfg).unwrap();
        let seen = std::sync::Mutex::new(String::new());
        let req = ChatRequest {
            messages: vec![Message::user("hi")],
            tools: vec![
                serde_json::json!({"type": "function", "function": {"name": "list_dir", "parameters": {}}}),
            ],
            ..Default::default()
        };
        let r = llm
            .chat(req, &|t| seen.lock().unwrap().push_str(t))
            .await
            .unwrap();
        assert_eq!(r.message.text(), "");
        assert_eq!(r.message.tool_calls.len(), 1);
        assert_eq!(
            r.message.tool_calls[0].function.arguments,
            "{\"path\":\".\"}",
            "a tool-only envelope must survive the assistant snapshot, not come back doubled and unparseable"
        );
        assert_eq!(seen.lock().unwrap().as_str(), "");
    }

    #[tokio::test]
    #[cfg(not(windows))]
    async fn codex_fake_binary_parses_jsonl_and_scrubs_api_key() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("env-check.txt");
        let script = format!(
            "#!/bin/sh\nif [ -z \"$OPENAI_API_KEY\" ]; then echo scrubbed > '{}'; else echo leaked > '{}'; fi\ncat > /dev/null\nprintf '%s\\n' '{{\"type\":\"thread.started\"}}' '{{\"type\":\"item.completed\",\"item\":{{\"type\":\"agent_message\",\"text\":\"{{\\\"text\\\":\\\"hi there\\\",\\\"tool_calls\\\":[]}}\"}}}}' '{{\"type\":\"turn.completed\",\"usage\":{{\"input_tokens\":3,\"output_tokens\":2}}}}'\n",
            out.display(), out.display()
        );
        let bin = fake_bin(dir.path(), "codex", &script);
        let mut cfg = test_cfg(dir.path(), "codex-cli");
        cfg.llm.cli_command = bin.to_string_lossy().to_string();
        std::env::set_var("OPENAI_API_KEY", "sk-test-should-be-scrubbed");
        let llm = CliLlm::from_config(&cfg).unwrap();
        let seen = std::sync::Mutex::new(String::new());
        let r = llm
            .chat(
                ChatRequest {
                    messages: vec![Message::user("hi")],
                    ..Default::default()
                },
                &|t| seen.lock().unwrap().push_str(t),
            )
            .await
            .unwrap();
        std::env::remove_var("OPENAI_API_KEY");
        assert_eq!(r.message.text(), "hi there");
        assert_eq!(
            seen.lock().unwrap().as_str(),
            "hi there",
            "streamed output must be the reply text, not the JSON envelope"
        );
        assert_eq!(std::fs::read_to_string(&out).unwrap().trim(), "scrubbed");
    }

    #[tokio::test]
    #[cfg(not(windows))]
    async fn opencode_fake_binary_parses_text_events_and_runs_in_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("cwd.txt");
        let script = format!(
            "#!/bin/sh\npwd > '{}'\nprintf '%s\\n' '{{\"type\":\"text\",\"part\":{{\"text\":\"{{\\\"text\\\":\\\"listed\\\",\\\"tool_calls\\\":[]}}\"}}}}' '{{\"type\":\"step_finish\",\"tokens\":{{\"input\":5,\"output\":6}}}}'\n",
            out.display()
        );
        let bin = fake_bin(dir.path(), "opencode", &script);
        let mut cfg = test_cfg(dir.path(), "opencode-cli");
        cfg.llm.cli_command = bin.to_string_lossy().to_string();
        let llm = CliLlm::from_config(&cfg).unwrap();
        let seen = std::sync::Mutex::new(String::new());
        let r = llm
            .chat(
                ChatRequest {
                    messages: vec![Message::user("hi")],
                    ..Default::default()
                },
                &|t| seen.lock().unwrap().push_str(t),
            )
            .await
            .unwrap();
        assert_eq!(r.message.text(), "listed");
        assert_eq!(
            seen.lock().unwrap().as_str(),
            "listed",
            "streamed output must be the reply text, not the JSON envelope"
        );
        let cwd = std::fs::read_to_string(&out).unwrap();
        let cwd = cwd.trim();
        assert!(
            cwd.contains("cli-"),
            "CLI must run in a temp dir, ran in {cwd}"
        );
        // Canonicalize both sides: on macOS `tempfile` returns
        // `/var/folders/...` but the real path is `/private/var/folders/...`
        // (`/var` is a symlink), and the child's `pwd` reports the real one.
        let tmp_root = std::fs::canonicalize(dir.path().join("tmp")).unwrap();
        assert!(
            cwd.starts_with(tmp_root.to_str().unwrap()),
            "CLI must run under the private temp dir, ran in {cwd}"
        );
        let here = std::fs::canonicalize(std::env::current_dir().unwrap()).unwrap();
        assert!(
            !cwd.starts_with(here.to_str().unwrap()),
            "CLI must not run in the test workspace, ran in {cwd}"
        );
    }

    #[tokio::test]
    async fn missing_binary_errors_with_install_hint() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = test_cfg(dir.path(), "claude-cli");
        cfg.llm.cli_command = "/definitely/not/a/real/claude".to_string();
        let llm = CliLlm::from_config(&cfg).unwrap();
        let err = llm
            .chat(
                ChatRequest {
                    messages: vec![Message::user("hi")],
                    ..Default::default()
                },
                &|_| {},
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("claude auth login"), "{err}");
    }

    #[tokio::test]
    #[cfg(not(windows))]
    async fn not_logged_in_error_names_the_login_command() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake_bin(
            dir.path(),
            "codex",
            "#!/bin/sh\necho 'not logged in, run codex login' >&2\nexit 1\n",
        );
        let mut cfg = test_cfg(dir.path(), "codex-cli");
        cfg.llm.cli_command = bin.to_string_lossy().to_string();
        let llm = CliLlm::from_config(&cfg).unwrap();
        let err = llm
            .chat(
                ChatRequest {
                    messages: vec![Message::user("hi")],
                    ..Default::default()
                },
                &|_| {},
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("codex login"), "{err}");
    }

    #[test]
    fn envelope_text_prefix_decodes_incrementally() {
        assert_eq!(envelope_text_prefix(""), None);
        assert_eq!(envelope_text_prefix("{\"te"), None);
        assert_eq!(
            envelope_text_prefix("{\"text\": \"two fi"),
            Some(("two fi".to_string(), false))
        );
        assert_eq!(
            envelope_text_prefix("{\"text\": \"ab\\"),
            Some(("ab".to_string(), false))
        );
        assert_eq!(
            envelope_text_prefix("{\"text\": \"a\\\"b\\n c\", \"tool_calls\": []}"),
            Some(("a\"b\n c".to_string(), true))
        );
        assert_eq!(envelope_text_prefix("{\"tool_calls\": []}"), None);
        // A surrogate pair decodes to one character, not two
        // replacement characters; a pair split across fragments waits
        // for its second half instead of emitting half a pair.
        assert_eq!(
            envelope_text_prefix("{\"text\": \"a\\ud83d\\ude00b\"}"),
            Some(("a\u{1F600}b".to_string(), true))
        );
        assert_eq!(
            envelope_text_prefix("{\"text\": \"a\\ud83d"),
            Some(("a".to_string(), false))
        );
        assert_eq!(
            envelope_text_prefix("{\"text\": \"a\\udc00b\"}"),
            Some(("a\u{fffd}b".to_string(), true))
        );
        // A "text" nested inside a tool argument is not the reply.
        assert_eq!(
            envelope_text_prefix("{\"tool_calls\": [{\"name\": \"browser_type\", \"arguments\": {\"id\": 1, \"text\": \"input value\"}}], \"text\": \"Done\"}"),
            Some(("Done".to_string(), true))
        );
        assert_eq!(
            envelope_text_prefix("{\"tool_calls\": [{\"name\": \"x\", \"arguments\": {\"text\": \"input value\"}}], \"text\": \"Do"),
            Some(("Do".to_string(), false))
        );
    }

    #[test]
    fn fenced_envelope_streams_only_its_text() {
        // A fenced envelope arrives backtick-first: the fence must never
        // reach the terminal, only the decoded text value.
        let mut acc = Accumulator::new();
        let seen = std::sync::Mutex::new(String::new());
        acc.push_text("```json\n", &|t| seen.lock().unwrap().push_str(t));
        assert!(seen.lock().unwrap().is_empty(), "the fence must not stream");
        acc.push_text("{\"text\": \"Done\", \"tool_calls\": []}\n```", &|t| {
            seen.lock().unwrap().push_str(t)
        });
        assert_eq!(seen.lock().unwrap().as_str(), "Done");
        let req = ChatRequest {
            messages: vec![],
            ..Default::default()
        };
        let resp = acc.finish(&req, &|t| seen.lock().unwrap().push_str(t));
        assert_eq!(resp.message.content.as_deref(), Some("Done"));
        assert_eq!(seen.lock().unwrap().as_str(), "Done");
    }

    #[test]
    fn bare_fenced_envelope_and_slow_backticks_stream_only_its_text() {
        let mut acc = Accumulator::new();
        let seen = std::sync::Mutex::new(String::new());
        acc.push_text("```\n", &|t| seen.lock().unwrap().push_str(t));
        assert!(seen.lock().unwrap().is_empty());
        acc.push_text("{\"text\": \"Done\", \"tool_calls\": []}\n```", &|t| {
            seen.lock().unwrap().push_str(t)
        });
        assert_eq!(seen.lock().unwrap().as_str(), "Done");

        // Backticks arriving one at a time must wait, not commit to Plain.
        let mut acc = Accumulator::new();
        let seen = std::sync::Mutex::new(String::new());
        for frag in ["`", "`", "`json\n"] {
            acc.push_text(frag, &|t| seen.lock().unwrap().push_str(t));
            assert!(seen.lock().unwrap().is_empty(), "after {frag:?}");
        }
        acc.push_text("{\"text\": \"Done\", \"tool_calls\": []}", &|t| {
            seen.lock().unwrap().push_str(t)
        });
        assert_eq!(seen.lock().unwrap().as_str(), "Done");
    }

    #[test]
    fn plain_code_block_still_streams_as_plain_text() {
        // A fenced code block is not an envelope: it must stream at once,
        // fence and all, not wait for the end of the reply.
        let mut acc = Accumulator::new();
        let seen = std::sync::Mutex::new(String::new());
        acc.push_text("```rust\n", &|t| seen.lock().unwrap().push_str(t));
        assert_eq!(seen.lock().unwrap().as_str(), "```rust\n");
        acc.push_text("let x = 1;\n```", &|t| seen.lock().unwrap().push_str(t));
        assert_eq!(seen.lock().unwrap().as_str(), "```rust\nlet x = 1;\n```");
    }

    #[tokio::test]
    #[cfg(not(windows))]
    async fn stderr_is_drained_while_the_cli_runs() {
        let dir = tempfile::tempdir().unwrap();
        // 200KB on stderr would fill the pipe and block the child if
        // stderr were only read after the child exits.
        let script = "#!/bin/sh\nhead -c 200000 /dev/zero | tr '\\0' 'x' >&2\ncat > /dev/null\nprintf '%s\\n' '{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"{\\\"text\\\":\\\"loud\\\",\\\"tool_calls\\\":[]}\"}}'\n";
        let bin = fake_bin(dir.path(), "codex", script);
        let mut cfg = test_cfg(dir.path(), "codex-cli");
        cfg.llm.cli_command = bin.to_string_lossy().to_string();
        cfg.llm.request_timeout_secs = 5;
        let llm = CliLlm::from_config(&cfg).unwrap();
        let r = llm
            .chat(
                ChatRequest {
                    messages: vec![Message::user("hi")],
                    ..Default::default()
                },
                &|_| {},
            )
            .await
            .unwrap();
        assert_eq!(r.message.text(), "loud");
    }

    #[test]
    fn truncate_str_never_splits_a_character() {
        let s = "é".repeat(400);
        assert_eq!(truncate_str(&s, 600).chars().count(), 300);
        let s = format!("{}éx", "a".repeat(599));
        assert_eq!(truncate_str(&s, 600), "a".repeat(599));
        assert_eq!(truncate_str("short", 600), "short");
    }

    #[tokio::test]
    #[cfg(not(windows))]
    async fn qwen_fake_binary_decodes_pretty_printed_json_document() {
        let dir = tempfile::tempdir().unwrap();
        // A JsonSingle/JsonAuto provider may pretty-print one JSON
        // document across lines; the envelope inside its `response`
        // field must still be decoded, tool calls included.
        let script = "#!/bin/sh\ncat > /dev/null\nprintf '%s\\n' '{' '  \"response\": \"{\\\"text\\\": \\\"pretty\\\", \\\"tool_calls\\\": [{\\\"name\\\": \\\"list_dir\\\", \\\"arguments\\\": {\\\"path\\\": \\\".\\\"}}]}\"' '}'\n";
        let bin = fake_bin(dir.path(), "qwen", script);
        let mut cfg = test_cfg(dir.path(), "qwen-cli");
        cfg.llm.cli_command = bin.to_string_lossy().to_string();
        let llm = CliLlm::from_config(&cfg).unwrap();
        let seen = std::sync::Mutex::new(String::new());
        let req = ChatRequest {
            messages: vec![Message::user("hi")],
            tools: vec![
                serde_json::json!({"type": "function", "function": {"name": "list_dir", "parameters": {}}}),
            ],
            ..Default::default()
        };
        let r = llm
            .chat(req, &|t| seen.lock().unwrap().push_str(t))
            .await
            .unwrap();
        assert_eq!(r.message.text(), "pretty");
        assert_eq!(r.message.tool_calls.len(), 1);
        assert_eq!(
            r.message.tool_calls[0].function.arguments,
            "{\"path\":\".\"}"
        );
        assert_eq!(
            seen.lock().unwrap().as_str(),
            "pretty",
            "streamed output must be the reply text, not the JSON document"
        );
    }

    #[tokio::test]
    #[cfg(not(windows))]
    async fn qwen_fake_binary_plain_text_reply_is_not_rejected_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        let script = "#!/bin/sh\ncat > /dev/null\nprintf 'Hello\\n'\n";
        let bin = fake_bin(dir.path(), "qwen", script);
        let mut cfg = test_cfg(dir.path(), "qwen-cli");
        cfg.llm.cli_command = bin.to_string_lossy().to_string();
        let llm = CliLlm::from_config(&cfg).unwrap();
        let seen = std::sync::Mutex::new(String::new());
        let r = llm
            .chat(
                ChatRequest {
                    messages: vec![Message::user("hi")],
                    ..Default::default()
                },
                &|t| seen.lock().unwrap().push_str(t),
            )
            .await
            .unwrap();
        assert_eq!(r.message.text(), "Hello");
        assert_eq!(
            seen.lock().unwrap().as_str(),
            "Hello",
            "a plain-text JsonAuto reply must reach the user, not be rejected as empty"
        );
    }

    #[tokio::test]
    #[cfg(not(windows))]
    async fn partial_output_with_failed_exit_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let script = "#!/bin/sh\ncat > /dev/null\nprintf '%s\\n' '{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"{\\\"text\\\":\\\"partial\\\",\\\"tool_calls\\\":[]}\"}}'\necho 'boom' >&2\nexit 2\n";
        let bin = fake_bin(dir.path(), "codex", script);
        let mut cfg = test_cfg(dir.path(), "codex-cli");
        cfg.llm.cli_command = bin.to_string_lossy().to_string();
        let llm = CliLlm::from_config(&cfg).unwrap();
        let err = llm
            .chat(
                ChatRequest {
                    messages: vec![Message::user("hi")],
                    ..Default::default()
                },
                &|_| {},
            )
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("incomplete"),
            "partial output plus a failed exit must not read as success: {err}"
        );
        assert!(err.to_string().contains("exit 2"), "{err}");
    }

    #[test]
    #[cfg(not(windows))]
    fn arg_prompt_over_the_size_cap_is_refused_early() {
        let dir = tempfile::tempdir().unwrap();
        // A fake binary, not a hardcoded system path: /bin/true does
        // not resolve on the macOS runner.
        let bin = fake_bin(dir.path(), "opencode", "#!/bin/sh\n");
        let mut cfg = test_cfg(dir.path(), "opencode-cli");
        cfg.llm.cli_command = bin.to_string_lossy().to_string();
        let llm = CliLlm::from_config(&cfg).unwrap();
        let err = llm.argv(&"x".repeat(200_000), "").unwrap_err();
        assert!(
            err.to_string().contains("over the"),
            "a prompt over the argv cap must fail early, not at spawn: {err}"
        );
        assert!(llm.argv("short", "").is_ok());
    }

    #[tokio::test]
    #[cfg(not(windows))]
    async fn custom_cli_plain_text_keeps_lines_and_indentation() {
        let dir = tempfile::tempdir().unwrap();
        let script = "#!/bin/sh\nprintf '%s\\n' 'first' '  indented' '' 'second'\n";
        let bin = fake_bin(dir.path(), "mycmd", script);
        let mut cfg = test_cfg(dir.path(), "custom-cli");
        cfg.llm.cli_command = bin.to_string_lossy().to_string();
        let llm = CliLlm::from_config(&cfg).unwrap();
        let seen = std::sync::Mutex::new(String::new());
        let r = llm
            .chat(
                ChatRequest {
                    messages: vec![Message::user("hi")],
                    ..Default::default()
                },
                &|t| seen.lock().unwrap().push_str(t),
            )
            .await
            .unwrap();
        assert_eq!(r.message.text(), "first\n  indented\n\nsecond");
        assert_eq!(
            seen.lock().unwrap().as_str(),
            "first\n  indented\n\nsecond\n",
            "streamed plain text must keep its newlines, indentation, and blank lines"
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn kill_process_tree_kills_descendants() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("grandchild.pid");
        let script = format!(
            "#!/bin/sh\nsleep 30 & echo $! > '{}'\nwait\n",
            pid_file.display()
        );
        let bin = fake_bin(dir.path(), "wrapper", &script);
        let mut cmd = tokio::process::Command::new(&bin);
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        // The fake binary was just written: on overlayfs the spawn can hit
        // ETXTBSY ("Text file busy") even after the writer closed it, the
        // same race the production spawn retries around. Retry a few times.
        let mut child = {
            let mut attempt = 0;
            loop {
                match cmd.spawn() {
                    Ok(child) => break child,
                    Err(e) if e.raw_os_error() == Some(26) && attempt < 5 => {
                        attempt += 1;
                        tokio::time::sleep(std::time::Duration::from_millis(20 * attempt)).await;
                    }
                    Err(e) => panic!("spawn failed: {e}"),
                }
            }
        };
        let grandchild: u32 = {
            let mut pid = None;
            for _ in 0..50 {
                if let Ok(s) = std::fs::read_to_string(&pid_file) {
                    if let Ok(p) = s.trim().parse::<u32>() {
                        pid = Some(p);
                        break;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            pid.expect("wrapper did not record its grandchild's pid")
        };
        assert_ne!(grandchild, child.id().unwrap());
        kill_process_tree(&mut child).await;
        let _ = child.wait().await;
        // The grandchild must be gone, not just the wrapper.
        let mut gone = false;
        for _ in 0..50 {
            let alive = unsafe { libc::kill(grandchild as libc::pid_t, 0) } == 0;
            if !alive {
                gone = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(gone, "grandchild {grandchild} survived the tree kill");
    }

    #[test]
    fn override_for_provider_applies_only_to_the_selected_provider() {
        assert_eq!(
            override_for_provider("claude-cli", "claude-cli", "/usr/local/bin/claude"),
            "/usr/local/bin/claude"
        );
        assert_eq!(
            override_for_provider("codex-cli", "claude-cli", "/usr/local/bin/claude"),
            ""
        );
        assert_eq!(
            override_for_provider("codex-cli", "http", "/usr/local/bin/claude"),
            ""
        );
    }

    #[tokio::test]
    #[cfg(not(windows))]
    async fn probe_applies_the_configured_account_directory() {
        let dir = tempfile::tempdir().unwrap();
        let script = "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  echo \"1.2.3\"\n  exit 0\nfi\nif [ \"$CLAUDE_CONFIG_DIR\" = \"/tmp/acct\" ]; then\n  echo \"Logged in as test@example.com (Pro)\"\nelse\n  echo \"Not logged in\"\nfi\n";
        let bin = fake_bin(dir.path(), "claude", script);
        let spec = spec_for("claude-cli").unwrap();
        let probed = probe_status(&spec, &bin.to_string_lossy(), "/tmp/acct").await;
        assert_eq!(probed.logged_in, Some(true));
        assert_eq!(probed.plan.as_deref(), Some("Claude pro"));
        let unprobed = probe_status(&spec, &bin.to_string_lossy(), "").await;
        assert_eq!(unprobed.logged_in, Some(false));
    }

    #[test]
    fn claude_login_output_parses_json_status() {
        let (logged, plan) = parse_login_output(
            "claude-cli",
            r#"{"loggedIn": true, "authMethod": "claude.ai"}"#,
        );
        assert_eq!(logged, Some(true));
        assert_eq!(plan, None);
        let (logged, _) =
            parse_login_output("claude-cli", r#"{"loggedIn": false, "authMethod": "none"}"#);
        assert_eq!(logged, Some(false));
    }

    #[tokio::test]
    #[cfg(not(windows))]
    async fn claude_probe_passes_text_and_still_reads_json() {
        let dir = tempfile::tempdir().unwrap();
        let record = dir.path().join("argv.txt");
        let script = format!(
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  echo \"1.2.3\"\n  exit 0\nfi\necho \"$@\" > \"{}\"\nprintf '%s\\n' '{{\"loggedIn\": true, \"authMethod\": \"claude.ai\"}}'\n",
            record.display()
        );
        let bin = fake_bin(dir.path(), "claude", &script);
        let spec = spec_for("claude-cli").unwrap();
        let probed = probe_status(&spec, &bin.to_string_lossy(), "").await;
        let argv = std::fs::read_to_string(&record).unwrap();
        assert!(
            argv.contains("--text"),
            "the probe must ask for human-readable output: {argv}"
        );
        assert_eq!(probed.logged_in, Some(true));
    }

    #[test]
    #[cfg(not(windows))]
    fn prompt_follows_the_value_flag_for_string_prompt_providers() {
        let dir = tempfile::tempdir().unwrap();
        for (spec_id, binary) in [
            ("gemini-cli", "gemini"),
            ("qwen-cli", "qwen"),
            ("antigravity-cli", "agy"),
            ("copilot-cli", "copilot"),
        ] {
            let bin = fake_bin(dir.path(), binary, "#!/bin/sh\nexit 0\n");
            let mut cfg = test_cfg(dir.path(), spec_id);
            cfg.llm.cli_command = bin.to_string_lossy().to_string();
            let llm = CliLlm::from_config(&cfg).unwrap();
            let (_, args) = llm.argv("hello", "m").unwrap();
            assert_eq!(args[0], "-p", "{spec_id}: {args:?}");
            assert_eq!(args[1], "hello", "{spec_id}: {args:?}");
            assert_eq!(
                args[2], "--model",
                "{spec_id}: -p must not swallow the model flag: {args:?}"
            );
        }
        // Control: cursor's -p is a boolean print flag and its prompt is
        // positional, so the prompt stays at the end there.
        let bin = fake_bin(dir.path(), "cursor-agent", "#!/bin/sh\nexit 0\n");
        let mut cfg = test_cfg(dir.path(), "cursor-cli");
        cfg.llm.cli_command = bin.to_string_lossy().to_string();
        let llm = CliLlm::from_config(&cfg).unwrap();
        let (_, args) = llm.argv("hello", "m").unwrap();
        assert_eq!(args[0], "-p");
        assert_eq!(args.last().unwrap(), "hello");
    }

    #[test]
    #[cfg(not(windows))]
    fn resolve_binary_makes_a_relative_path_entry_absolute() {
        // A relative PATH entry used to return a relative path, which chat()
        // then resolved against the per-request temp dir and failed to
        // spawn, while the probes still reported the provider installed.
        struct RestorePath(String);
        impl Drop for RestorePath {
            fn drop(&mut self) {
                std::env::set_var("PATH", &self.0);
            }
        }
        fn relative_path(from: &std::path::Path, to: &std::path::Path) -> PathBuf {
            let from: Vec<_> = from.components().collect();
            let to: Vec<_> = to.components().collect();
            let mut i = 0;
            while i < from.len() && i < to.len() && from[i] == to[i] {
                i += 1;
            }
            let mut out = PathBuf::new();
            for _ in i..from.len() {
                out.push("..");
            }
            for c in &to[i..] {
                out.push(c.as_os_str());
            }
            out
        }
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("relbin");
        std::fs::create_dir(&bin_dir).unwrap();
        let fake = fake_bin(&bin_dir, "mycli", "#!/bin/sh\nexit 0\n");
        let rel = relative_path(&std::env::current_dir().unwrap(), &bin_dir);
        assert!(!rel.is_absolute(), "the test needs a relative PATH entry");
        let _guard = RestorePath(std::env::var("PATH").unwrap_or_default());
        std::env::set_var("PATH", rel.as_os_str());
        let found = resolve_binary("mycli", "").expect("fake on relative PATH");
        assert!(found.is_absolute(), "{found:?}");
        assert_eq!(
            std::fs::canonicalize(&found).unwrap(),
            std::fs::canonicalize(&fake).unwrap()
        );
        // A relative command override goes through the same lookup.
        let via_override = resolve_binary("", "mycli").expect("fake via override");
        assert!(via_override.is_absolute(), "{via_override:?}");
    }

    #[test]
    fn normalize_config_dir_expands_tilde_and_relative_paths() {
        assert_eq!(normalize_config_dir(""), "");
        // "/tmp/acct" is drive-relative on Windows, not absolute, so the
        // absolute case needs a path that is absolute on every platform.
        let dir = tempfile::tempdir().unwrap();
        let abs = dir.path().join("acct");
        assert_eq!(
            normalize_config_dir(&abs.to_string_lossy()),
            abs.to_string_lossy()
        );
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(
            normalize_config_dir("accounts/work"),
            cwd.join("accounts/work").to_string_lossy()
        );
        let home = dirs::home_dir().unwrap();
        assert_eq!(
            normalize_config_dir("~/x"),
            home.join("x").to_string_lossy()
        );
        assert_eq!(normalize_config_dir("~"), home.to_string_lossy());
    }

    #[tokio::test]
    #[cfg(not(windows))]
    async fn probe_and_request_agree_on_a_relative_config_dir() {
        // The probe runs in Heron's working directory, the request in a
        // fresh temp dir: a relative cli_config_dir used to point at two
        // different accounts, and the request's copy was deleted with it.
        let dir = tempfile::tempdir().unwrap();
        let record = dir.path().join("cfgdir.txt");
        let script = format!(
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  echo \"1.2.3\"\n  exit 0\nfi\necho \"$CLAUDE_CONFIG_DIR\" > \"{}\"\necho \"Logged in as test@example.com (Pro)\"\n",
            record.display()
        );
        let bin = fake_bin(dir.path(), "claude", &script);
        let spec = spec_for("claude-cli").unwrap();
        let probed = probe_status(&spec, &bin.to_string_lossy(), "relacct").await;
        assert_eq!(probed.logged_in, Some(true));
        let want = std::env::current_dir().unwrap().join("relacct");
        assert_eq!(
            std::fs::read_to_string(&record).unwrap().trim(),
            want.to_string_lossy()
        );
        let mut cfg = test_cfg(dir.path(), "claude-cli");
        cfg.llm.cli_command = bin.to_string_lossy().to_string();
        cfg.llm.cli_config_dir = "relacct".to_string();
        let llm = CliLlm::from_config(&cfg).unwrap();
        let env = llm.child_env();
        let got = env
            .iter()
            .find(|(k, _)| k == "CLAUDE_CONFIG_DIR")
            .map(|(_, v)| v.clone());
        assert_eq!(got.as_deref(), Some(want.to_string_lossy().as_ref()));
    }

    #[tokio::test]
    #[cfg(not(windows))]
    async fn request_model_override_wins_over_cli_model() {
        let dir = tempfile::tempdir().unwrap();
        let record = dir.path().join("argv.txt");
        let envelope = r#"{\"text\": \"hi\", \"tool_calls\": []}"#;
        let script = format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\ncat > /dev/null\nprintf '%s\\n' '{{\"type\":\"system\",\"subtype\":\"init\"}}' '{{\"type\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"content\":[{{\"type\":\"text\",\"text\":\"{envelope}\"}}]}}}}' '{{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"{envelope}\",\"usage\":{{\"input_tokens\":11,\"output_tokens\":4}}}}'\n",
            record.display()
        );
        let bin = fake_bin(dir.path(), "claude", &script);
        let mut cfg = test_cfg(dir.path(), "claude-cli");
        cfg.llm.cli_command = bin.to_string_lossy().to_string();
        cfg.llm.cli_model = "sonnet".to_string();
        let llm = CliLlm::from_config(&cfg).unwrap();
        let req = ChatRequest {
            messages: vec![Message::user("hi")],
            model: Some("haiku".to_string()),
            ..Default::default()
        };
        let r = llm.chat(req, &|_| {}).await.unwrap();
        assert_eq!(r.message.text(), "hi");
        // The reply names the model that was asked for; the CLI itself
        // ran the native form, checked on the recorded argv below.
        assert_eq!(r.model, "haiku");
        let argv = std::fs::read_to_string(&record).unwrap();
        assert!(
            argv.contains("--model claude-haiku-5-5[1m]"),
            "the CLI ran with: {argv}"
        );
        assert!(
            !argv.contains("sonnet"),
            "the configured main model leaked into the override: {argv}"
        );
    }

    #[tokio::test]
    #[cfg(not(windows))]
    async fn cancelled_chat_removes_its_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        let script = "#!/bin/sh\ncat > /dev/null\nsleep 30\n";
        let bin = fake_bin(dir.path(), "claude", script);
        let mut cfg = test_cfg(dir.path(), "claude-cli");
        cfg.llm.cli_command = bin.to_string_lossy().to_string();
        let llm = CliLlm::from_config(&cfg).unwrap();
        let tmp = dir.path().join("tmp");
        let handle = tokio::spawn(async move {
            llm.chat(
                ChatRequest {
                    messages: vec![Message::user("hi")],
                    ..Default::default()
                },
                &|_| {},
            )
            .await
        });
        // Let the request start and create its temp dir.
        let mut started = false;
        for _ in 0..50 {
            if let Ok(entries) = std::fs::read_dir(&tmp) {
                if entries.count() > 0 {
                    started = true;
                    break;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(started, "the request never created its temp dir");
        handle.abort();
        let _ = handle.await;
        let remaining = std::fs::read_dir(&tmp)
            .map(|entries| entries.count())
            .unwrap_or(0);
        assert_eq!(remaining, 0, "a cancelled request left its temp dir behind");
    }

    #[tokio::test]
    #[cfg(not(windows))]
    async fn descendant_holding_stderr_does_not_hang_the_request() {
        let dir = tempfile::tempdir().unwrap();
        let script = "#!/bin/sh\ncat > /dev/null\n(sleep 30) 1>/dev/null &\nprintf 'done\\n'\n";
        let bin = fake_bin(dir.path(), "mycmd", script);
        let mut cfg = test_cfg(dir.path(), "custom-cli");
        cfg.llm.cli_command = bin.to_string_lossy().to_string();
        let llm = CliLlm::from_config(&cfg).unwrap();
        let r = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            llm.chat(
                ChatRequest {
                    messages: vec![Message::user("hi")],
                    ..Default::default()
                },
                &|_| {},
            ),
        )
        .await
        .expect("chat hung awaiting stderr held open by a descendant");
        let r = r.unwrap();
        assert_eq!(r.message.text(), "done");
    }
}
