//! Configuration: `~/.heron/config.toml` + environment overrides.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub llm: LlmConfig,
    pub agent: AgentConfig,
    pub permissions: PermissionConfig,
    pub server: ServerConfig,
    pub mcp: McpConfig,
    pub sources: SourcesConfig,
    pub telegram: TelegramConfig,
    #[serde(skip)]
    pub home: PathBuf,
}

/// Messaging gateway. The bot token itself is kept with the other secrets.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TelegramConfig {
    pub api_base: String,
}

impl Default for TelegramConfig {
    fn default() -> Self {
        TelegramConfig { api_base: "https://api.telegram.org".into() }
    }
}

/// Where Heron may look to understand the user's world. All read-only.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SourcesConfig {
    /// Folders of notes and documents to index for lookup.
    pub folders: Vec<String>,
    /// Private calendar feeds (ICS links).
    pub calendars: Vec<String>,
    pub email: Option<EmailConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EmailConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    /// Only ever false for a server on this computer (testing). Remote servers always use TLS.
    pub tls: bool,
}

impl Default for EmailConfig {
    fn default() -> Self {
        EmailConfig { host: String::new(), port: 993, username: String::new(), tls: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    /// Any OpenAI-compatible endpoint: OpenRouter, OpenAI, Ollama, LM Studio, vLLM, ...
    pub base_url: String,
    /// Name of the env var holding the API key (the key itself is never written to disk).
    pub api_key_env: String,
    pub model: String,
    /// Tried in order when the primary model fails after retries.
    pub fallback_models: Vec<String>,
    /// Cheap model for background work (memory extraction, compaction, titles). Empty = use `model`.
    pub fast_model: String,
    /// Remote embedding model. Empty = built-in offline embeddings.
    pub embedding_model: String,
    pub max_tokens: u32,
    pub request_timeout_secs: u64,
    pub max_retries: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentConfig {
    pub name: String,
    pub max_steps: usize,
    /// Approximate token budget for conversation history before compaction kicks in.
    pub context_tokens: usize,
    /// Learn durable facts about the user from conversations automatically.
    pub auto_memory: bool,
    pub max_delegate_depth: usize,
    pub tool_timeout_secs: u64,
    pub max_tool_output_chars: usize,
    /// Shell command run with HERON_MESSAGE set when a scheduled job finishes (e.g. `notify-send Heron "$HERON_MESSAGE"`).
    pub notify_command: String,
    /// Shell used for commands: "powershell", "pwsh", "cmd", "bash", "zsh", ... Empty = the system default
    /// (PowerShell on Windows, bash elsewhere). The HERON_SHELL environment variable overrides it.
    pub shell: String,
    /// "auto" (default): run commands inside the operating system's sandbox where one is available
    /// (bubblewrap on Linux, Seatbelt on macOS). "off": run them directly.
    pub sandbox: String,
    /// Browser Heron may drive to read pages and fill in forms. Empty = find Chrome, Edge, Chromium or
    /// Brave automatically; a path = use that program; "off" = no browser control. HERON_BROWSER overrides it.
    pub browser: String,
    /// Show the browser's window while Heron uses it (default: it works out of sight).
    pub browser_visible: bool,
    /// Default working folder for the desktop app / web UI. Empty = ~/Heron.
    pub workspace: String,
    /// Understand meaning, not just shared words, when searching memory: runs a small model locally.
    /// Needs a build with the `neural` feature; downloads the model (about 23 MB) once.
    pub neural_memory: bool,
    /// Observe the working folder in the background (file names and times only) so the agent is aware of what you are doing.
    pub ambient_context: bool,
    /// Periodically let the model reflect on what was observed to form lasting insights.
    /// Off by default because it sends file names to the model provider without you asking.
    pub reflect: bool,
    /// How proactive Heron is: off | suggest | prepare | takeover.
    ///  suggest  — proposes work it could do for you; never acts.
    ///  prepare  — also does the read-only groundwork in advance.
    ///  takeover — carries the work out itself, limited to `proactive_allow`; risky actions are always refused.
    pub proactive: String,
    /// Minimum minutes between proactive passes.
    pub proactive_interval_mins: u64,
    /// What Heron may do on its own initiative in takeover mode (permission rules).
    pub proactive_allow: Vec<String>,
    /// Do not come up with suggestions while the user is in the middle of something: wait until the
    /// folder has been quiet for this many seconds.
    pub proactive_quiet_secs: u64,
    /// Daily token budget for work Heron starts on its own (suggestions, preparing, taking over,
    /// reflection). 0 = no limit. Reminders you scheduled yourself always run.
    pub background_tokens_per_day: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PermissionConfig {
    /// readonly | ask | auto | yolo
    pub mode: String,
    /// Mode used by unattended scheduled jobs. With "ask", anything that needs approval is not done
    /// but kept in a "waiting for you" list; with "readonly" it is simply refused.
    pub daemon_mode: String,
    /// Glob rules "tool:summary", e.g. "shell:git status*", "write_file:notes/**".
    pub allow: Vec<String>,
    pub deny: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub bind: String,
    pub port: u16,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct McpConfig {
    pub servers: BTreeMap<String, McpServerConfig>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct McpServerConfig {
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    /// Trusted servers' tools run without asking (still subject to deny rules).
    pub trust: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            llm: LlmConfig::default(),
            agent: AgentConfig::default(),
            permissions: PermissionConfig::default(),
            server: ServerConfig::default(),
            mcp: McpConfig::default(),
            sources: SourcesConfig::default(),
            telegram: TelegramConfig::default(),
            home: default_home(),
        }
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        LlmConfig {
            base_url: "https://openrouter.ai/api/v1".into(),
            api_key_env: "OPENROUTER_API_KEY".into(),
            model: "anthropic/claude-sonnet-5.5".into(),
            fallback_models: vec![],
            fast_model: String::new(),
            embedding_model: String::new(),
            max_tokens: 8192,
            request_timeout_secs: 300,
            max_retries: 3,
        }
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        AgentConfig {
            name: "Heron".into(),
            max_steps: 40,
            context_tokens: 100_000,
            auto_memory: true,
            max_delegate_depth: 2,
            tool_timeout_secs: 120,
            max_tool_output_chars: 30_000,
            notify_command: String::new(),
            shell: String::new(),
            sandbox: "auto".into(),
            browser: String::new(),
            browser_visible: false,
            workspace: String::new(),
            neural_memory: false,
            ambient_context: true,
            reflect: false,
            proactive: "off".into(),
            proactive_interval_mins: 30,
            proactive_allow: vec!["write_file:*".into(), "edit_file:*".into()],
            proactive_quiet_secs: 180,
            background_tokens_per_day: 150_000,
        }
    }
}

impl Default for PermissionConfig {
    fn default() -> Self {
        PermissionConfig { mode: "ask".into(), daemon_mode: "ask".into(), allow: vec![], deny: vec![] }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig { bind: "127.0.0.1".into(), port: 7878 }
    }
}

pub fn default_home() -> PathBuf {
    if let Ok(h) = std::env::var("HERON_HOME") {
        return PathBuf::from(h);
    }
    dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".heron")
}

impl Config {
    /// Load config from `$HERON_HOME/config.toml` (defaults if absent), then apply env overrides.
    pub fn load() -> Result<Config> {
        let home = default_home();
        let path = home.join("config.toml");
        let mut cfg: Config = if path.exists() {
            let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?
        } else {
            let mut c = Config::default();
            // Sensible auto-detection when there is no config file yet.
            if std::env::var("OPENROUTER_API_KEY").is_err() && std::env::var("OPENAI_API_KEY").is_ok() {
                c.llm.base_url = "https://api.openai.com/v1".into();
                c.llm.api_key_env = "OPENAI_API_KEY".into();
                c.llm.model = "gpt-5.5".into();
            }
            c
        };
        cfg.home = home;
        if let Ok(v) = std::env::var("HERON_MODEL") {
            cfg.llm.model = v;
        }
        if let Ok(v) = std::env::var("HERON_BASE_URL") {
            cfg.llm.base_url = v;
        }
        if let Ok(v) = std::env::var("HERON_MODE") {
            cfg.permissions.mode = v;
        }
        Ok(cfg)
    }

    pub fn api_key(&self) -> Option<String> {
        // An empty variable counts as unset, so it cannot hide the next source.
        let env = |name: &str| std::env::var(name).ok().filter(|k| !k.trim().is_empty());
        env("HERON_API_KEY").or_else(|| env(&self.llm.api_key_env)).or_else(|| self.stored_api_key())
    }

    fn secrets_path(&self) -> PathBuf {
        self.home.join("secrets.json")
    }

    /// The fallback secrets file. A file that is there but cannot be read or understood is an error,
    /// never "no secrets": treating it as empty would let the next save wipe every other key in it.
    fn read_file_secrets(&self) -> Result<serde_json::Map<String, serde_json::Value>> {
        let path = self.secrets_path();
        match std::fs::read_to_string(&path) {
            Ok(t) if t.trim().is_empty() => Ok(Default::default()),
            Ok(t) => serde_json::from_str(&t).with_context(|| format!("{} is damaged; fix or remove it before saving a key", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Default::default()),
            Err(e) => Err(e).with_context(|| format!("could not read {}", path.display())),
        }
    }

    fn file_secrets(&self) -> serde_json::Map<String, serde_json::Value> {
        self.read_file_secrets().unwrap_or_default()
    }

    /// Replace the file in one step (write a private temporary file, then rename it over the old one),
    /// so a crash or a second writer can never leave half a file behind.
    fn write_file_secrets(&self, all: &serde_json::Map<String, serde_json::Value>) -> Result<()> {
        let path = self.secrets_path();
        if all.is_empty() {
            return match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e).with_context(|| format!("could not remove {}", path.display())),
                _ => Ok(()),
            };
        }
        let tmp = self.home.join(format!("secrets.json.tmp-{}", std::process::id()));
        let written = (|| -> Result<()> {
            use std::io::Write;
            let mut opts = std::fs::OpenOptions::new();
            opts.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            let mut f = opts.open(&tmp)?;
            f.write_all(serde_json::Value::Object(all.clone()).to_string().as_bytes())?;
            f.sync_all()?;
            drop(f);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
            }
            std::fs::rename(&tmp, &path)?;
            Ok(())
        })();
        if written.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        written.with_context(|| format!("could not save {}", path.display()))
    }

    /// A secret saved from the app. Kept in the operating system's credential store (macOS Keychain,
    /// Windows Credential Manager) where there is one; otherwise in an owner-only file in the Heron folder.
    pub fn secret(&self, name: &str) -> Option<String> {
        vault::get(&self.home, name).or_else(|| self.file_secrets().get(name).and_then(|k| k.as_str()).filter(|k| !k.is_empty()).map(|k| k.to_string()))
    }

    /// Save (or with an empty value, remove) a secret.
    pub fn store_secret(&self, name: &str, value: &str) -> Result<()> {
        // One writer at a time: two saves must not each read the file and then overwrite the other's key.
        static WRITING: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _one_at_a_time = WRITING.lock().unwrap_or_else(|e| e.into_inner());
        self.ensure_home()?;
        let value = value.trim();
        let mut file = self.read_file_secrets()?;
        if value.is_empty() {
            vault::delete(&self.home, name).map_err(|e| anyhow::anyhow!("could not remove '{name}' from {}: {e}", vault::NAME))?;
            file.remove(name);
        } else if vault::set(&self.home, name, value) {
            file.remove(name); // never keep a second copy on disk
        } else {
            file.insert(name.to_string(), serde_json::Value::String(value.to_string()));
        }
        self.write_file_secrets(&file)
    }

    /// Where secrets are kept on this computer, for display.
    pub fn secret_storage(&self) -> &'static str {
        vault::NAME
    }

    pub fn stored_api_key(&self) -> Option<String> {
        self.secret("api_key")
    }

    pub fn store_api_key(&self, key: &str) -> Result<()> {
        self.store_secret("api_key", key)
    }

    /// Persist the current settings to config.toml.
    pub fn save(&self) -> Result<()> {
        self.ensure_home()?;
        std::fs::write(self.home.join("config.toml"), toml::to_string_pretty(self)?)?;
        Ok(())
    }

    /// Folder the desktop app works in when no folder was chosen on the command line.
    pub fn app_workspace(&self) -> PathBuf {
        if !self.agent.workspace.is_empty() {
            return PathBuf::from(&self.agent.workspace);
        }
        dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join("Heron")
    }

    pub fn fast_model(&self) -> &str {
        if self.llm.fast_model.is_empty() { &self.llm.model } else { &self.llm.fast_model }
    }

    pub fn db_path(&self) -> PathBuf {
        self.home.join("heron.db")
    }

    pub fn skills_dir(&self) -> PathBuf {
        self.home.join("skills")
    }

    pub fn ensure_home(&self) -> Result<()> {
        std::fs::create_dir_all(&self.home)?;
        std::fs::create_dir_all(self.skills_dir())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // Keys, memories and conversations live here. If the folder cannot be made private
            // (and is not already), stop rather than keep them where other accounts can read them.
            let _ = std::fs::set_permissions(&self.home, std::fs::Permissions::from_mode(0o700));
            let mode = std::fs::metadata(&self.home)?.permissions().mode();
            if mode & 0o077 != 0 {
                anyhow::bail!("{} is readable by other accounts on this computer and could not be made private (mode {:o}). Move it to a disk that supports file permissions, or set HERON_HOME.", self.home.display(), mode & 0o777);
            }
        }
        Ok(())
    }

    /// Write a commented starter config if none exists. Returns the path.
    pub fn write_default(&self) -> Result<PathBuf> {
        self.ensure_home()?;
        let path = self.home.join("config.toml");
        if !path.exists() {
            std::fs::write(&path, DEFAULT_CONFIG)?;
        }
        Ok(path)
    }
}

/// The operating system's credential store. Entries are namespaced by the Heron folder so that
/// separate profiles (and tests) never see each other's secrets.
#[cfg(any(target_os = "macos", windows))]
mod vault {
    use std::path::Path;

    pub const NAME: &str = if cfg!(windows) { "Windows Credential Manager" } else { "macOS Keychain" };

    fn entry(home: &Path, name: &str) -> Option<keyring::Entry> {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in home.to_string_lossy().as_bytes() {
            h = (h ^ *b as u64).wrapping_mul(0x100000001b3);
        }
        keyring::Entry::new("Heron", &format!("{name}@{h:x}")).ok()
    }
    pub fn get(home: &Path, name: &str) -> Option<String> {
        entry(home, name)?.get_password().ok().filter(|v| !v.is_empty())
    }
    pub fn set(home: &Path, name: &str, value: &str) -> bool {
        // Only report success if the value really reads back, so a broken store falls back to the file.
        entry(home, name).map(|e| e.set_password(value).is_ok() && e.get_password().map(|v| v == value).unwrap_or(false)).unwrap_or(false)
    }
    pub fn delete(home: &Path, name: &str) -> Result<(), String> {
        match entry(home, name).map(|e| e.delete_credential()) {
            // Nothing saved under that name is what the caller wanted anyway.
            None | Some(Ok(())) | Some(Err(keyring::Error::NoEntry)) => Ok(()),
            Some(Err(e)) => Err(e.to_string()),
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod vault {
    use std::path::Path;

    pub const NAME: &str = "an owner-only file in the Heron folder";

    pub fn get(_: &Path, _: &str) -> Option<String> {
        None
    }
    pub fn set(_: &Path, _: &str, _: &str) -> bool {
        false
    }
    pub fn delete(_: &Path, _: &str) -> Result<(), String> {
        Ok(())
    }
}

pub const DEFAULT_CONFIG: &str = r#"# Heron configuration. Every value is optional; these are the defaults.

[llm]
# Any OpenAI-compatible endpoint works:
#   OpenRouter  https://openrouter.ai/api/v1      (OPENROUTER_API_KEY)
#   OpenAI      https://api.openai.com/v1         (OPENAI_API_KEY)
#   Ollama      http://localhost:11434/v1         (no key; fully offline)
#   demo        the word "demo" — built-in canned replies, no key or network
base_url = "https://openrouter.ai/api/v1"
api_key_env = "OPENROUTER_API_KEY"   # the key is read from this env var, never stored here
model = "anthropic/claude-sonnet-5.5"
fallback_models = []                 # tried in order if the primary fails
fast_model = ""                      # cheap model for background work; empty = same as model
embedding_model = ""                 # empty = built-in offline embeddings
max_tokens = 8192

[agent]
name = "Heron"
max_steps = 40
context_tokens = 100000
auto_memory = true
notify_command = ""                  # e.g. 'notify-send Heron "$HERON_MESSAGE"'
browser = ""                         # empty = find Chrome/Edge/Chromium/Brave; a path; or "off"
browser_visible = false              # true = show the browser window while Heron uses it

[permissions]
mode = "ask"                         # readonly | ask | auto | yolo
daemon_mode = "ask"                  # unattended jobs: anything needing approval waits for you
allow = []                           # e.g. ["shell:git status*", "shell:cargo *"]
deny = []                            # e.g. ["shell:*sudo*"]

[server]
bind = "127.0.0.1"
port = 7878

# Connect any MCP server; its tools become available to the agent.
# [mcp.servers.filesystem]
# command = "npx"
# args = ["-y", "@modelcontextprotocol/server-filesystem", "/home/me/Documents"]
# trust = false
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_text_parses_to_defaults() {
        let parsed: Config = toml::from_str(DEFAULT_CONFIG).unwrap();
        let d = Config::default();
        assert_eq!(parsed.llm.model, d.llm.model);
        assert_eq!(parsed.permissions.mode, "ask");
        assert_eq!(parsed.server.port, 7878);
    }

    #[test]
    fn secrets_round_trip_and_never_touch_the_config_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.home = dir.path().join("home");
        assert_eq!(cfg.secret("api_key"), None);
        cfg.store_secret("api_key", "  sk-test-123  ").unwrap();
        cfg.store_secret("email_password", "app pass").unwrap();
        assert_eq!(cfg.secret("api_key").as_deref(), Some("sk-test-123"));
        assert_eq!(cfg.secret("email_password").as_deref(), Some("app pass"));
        cfg.save().unwrap();
        assert!(!std::fs::read_to_string(cfg.home.join("config.toml")).unwrap().contains("sk-test"));
        // a different profile cannot see them
        let mut other = Config::default();
        other.home = dir.path().join("other");
        assert_eq!(other.secret("api_key"), None);
        // removing one leaves the other
        cfg.store_secret("api_key", "").unwrap();
        assert_eq!(cfg.secret("api_key"), None);
        assert_eq!(cfg.secret("email_password").as_deref(), Some("app pass"));
        cfg.store_secret("email_password", "").unwrap();
        assert!(!cfg.home.join("secrets.json").exists(), "no empty secrets file is left behind");
        // Where the OS credential store works, nothing is written to disk at all. (On a machine where it is
        // locked or unavailable, such as some build servers, the owner-only file is used instead.)
        cfg.store_secret("api_key", "sk-again").unwrap();
        assert_eq!(cfg.secret("api_key").as_deref(), Some("sk-again"));
        if cfg!(windows) {
            assert!(!cfg.home.join("secrets.json").exists());
        }
        cfg.store_secret("api_key", "").unwrap();
        assert_eq!(cfg.secret("api_key"), None);
    }

    #[test]
    fn mcp_servers_parse() {
        let c: Config = toml::from_str("[mcp.servers.fs]\ncommand='x'\nargs=['a']\ntrust=true\n").unwrap();
        assert!(c.mcp.servers["fs"].trust);
    }

    #[test]
    fn a_damaged_secrets_file_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.home = dir.path().join("home");
        cfg.ensure_home().unwrap();
        let path = cfg.home.join("secrets.json");
        std::fs::write(&path, "{\"email_password\": \"keep me\", ").unwrap(); // cut off mid-write
        if cfg!(not(any(target_os = "macos", windows))) {
            let err = cfg.store_secret("api_key", "sk-new").unwrap_err();
            assert!(format!("{err:#}").contains("damaged"), "{err:#}");
        } else {
            let _ = cfg.store_secret("api_key", "sk-new");
        }
        assert!(std::fs::read_to_string(&path).unwrap().contains("keep me"), "the old contents are still there to recover");
        // nothing temporary is left lying around either
        let stray: Vec<_> = std::fs::read_dir(&cfg.home).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().contains(".tmp-")).collect();
        assert!(stray.is_empty());
    }

    #[test]
    fn an_empty_key_variable_does_not_hide_the_configured_one() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.home = dir.path().join("home");
        cfg.llm.api_key_env = "HERON_TEST_CONFIGURED_KEY".into();
        std::env::set_var("HERON_TEST_CONFIGURED_KEY", "sk-configured");
        let had = std::env::var("HERON_API_KEY").ok();
        std::env::set_var("HERON_API_KEY", "");
        let got = cfg.api_key();
        match had {
            Some(v) => std::env::set_var("HERON_API_KEY", v),
            None => std::env::remove_var("HERON_API_KEY"),
        }
        assert_eq!(got.as_deref(), Some("sk-configured"));
    }
}
