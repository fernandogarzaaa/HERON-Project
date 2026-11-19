//! Configuration: `~/.heron/config.toml` + environment overrides.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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
    /// The standing rules as [`Config::read_saved`] read them from `config.toml`; None when these
    /// settings were not read from the file (built in memory, or no file yet). A core starts from
    /// these as what it last read there, so a rule revoked while it starts is taken out at its first
    /// reload.
    #[serde(skip)]
    pub loaded_rules: Option<SavedRules>,
    /// Which settings `App` held when this copy was taken from it. A whole copy is saved only while
    /// that is still current (see `App::update_config`).
    #[serde(skip)]
    pub revision: u64,
}

/// `permissions.allow` and `permissions.deny` as they are in `config.toml`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SavedRules {
    pub allow: Vec<String>,
    pub deny: Vec<String>,
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
    /// Model provider: "http" (an OpenAI-compatible endpoint, the default),
    /// or one of the CLI providers from `heron providers` (e.g. "claude-cli",
    /// "codex-cli", "opencode-cli"). CLI providers run on the user's own
    /// subscription through the official CLI; Heron never reads its tokens.
    /// See docs/cli-providers.md for the full list and the safety rules.
    pub provider: String,
    /// Binary (or absolute path) for the CLI provider. Empty = resolve the
    /// provider's default binary on PATH and in the usual install dirs.
    pub cli_command: String,
    /// Model for the CLI provider. Empty = the CLI's subscription default
    /// (for Claude, "sonnet" unless set). For HTTP providers use `model`.
    pub cli_model: String,
    /// Config directory for the CLI (e.g. a second CLAUDE_CONFIG_DIR), so
    /// one account does not share state with another. Empty = the CLI default.
    pub cli_config_dir: String,
    /// Allow providers whose headless mode cannot disable their own tools
    /// (Cline, Goose) to run. Off by default; setting it is a deliberate
    /// choice, and the CLI still runs in a throwaway directory, never the
    /// workspace.
    pub cli_allow_agent_tools: bool,
    /// Which CLI providers may receive their API-key variables (`ANTHROPIC_API_KEY`,
    /// `OPENAI_API_KEY`, ...). `false` (the default) for none, a list of provider ids such as
    /// `["gemini-cli"]` for those, or `true` for every provider. A provider not allowed never sees
    /// them, so a stray key cannot flip a subscription turn into metered API billing.
    pub cli_allow_api_key: ApiKeyAllow,
    /// Let scheduled and other unattended work call a CLI provider that is a subscription login.
    /// Off by default: such work waits for the user instead.
    pub allow_unattended_subscription: bool,
    /// Extra argv for "custom-cli", with {prompt} and {model} placeholders.
    /// If no arg contains {prompt}, the prompt is appended as the last arg.
    pub cli_args: Vec<String>,
    /// Per CLI provider: is it a subscription login? A provider that is missing counts as one. Set
    /// `false` for a CLI that routes to an API key or a local model. (Last, because it is a table.)
    pub cli_subscription: BTreeMap<String, bool>,
}

/// `[llm] cli_allow_api_key`: `true`, `false`, or a list of provider ids. Kept in the form it was
/// written, so old configs that say `true` or `false` load and save unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ApiKeyAllow {
    Bool(bool),
    List(Vec<String>),
}

impl Default for ApiKeyAllow {
    fn default() -> Self {
        ApiKeyAllow::Bool(false)
    }
}

impl ApiKeyAllow {
    /// May this provider receive its API-key variables?
    pub fn allows(&self, id: &str) -> bool {
        match self {
            ApiKeyAllow::Bool(all) => *all,
            ApiKeyAllow::List(ids) => ids.iter().any(|i| i == id),
        }
    }
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
    /// Extra environment variables commands may see, by name. Commands get a short fixed list (PATH,
    /// HOME, language and time zone, temporary folders, toolchain locations) and nothing else, so
    /// tokens and cloud keys in your environment are not handed to whatever is run. Add a name here
    /// when a command genuinely needs one.
    pub shell_env: Vec<String>,
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
    /// Your own cap on tokens per day across everything Heron does. 0 = no limit. As it runs down
    /// Heron economises (fewer steps, shorter answers, the cheaper model, nothing in the background).
    pub daily_token_limit: u64,
    /// The same, per calendar month. 0 = no limit.
    pub monthly_token_limit: u64,
    /// "auto": send short, simple requests to `fast_model` when one is set, and move to the main model
    /// if the request turns out to need real work. "off": always use the main model.
    pub routing: String,
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
    /// How long a live approval waits for an answer before it is parked for later.
    pub approval_timeout_secs: u64,
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
            loaded_rules: None,
            revision: 0,
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
            provider: "http".into(),
            cli_command: String::new(),
            cli_model: String::new(),
            cli_config_dir: String::new(),
            cli_allow_agent_tools: false,
            cli_allow_api_key: ApiKeyAllow::default(),
            cli_subscription: BTreeMap::new(),
            allow_unattended_subscription: false,
            cli_args: vec![],
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
            shell_env: vec![],
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
            daily_token_limit: 0,
            monthly_token_limit: 0,
            routing: "auto".into(),
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
        ServerConfig { bind: "127.0.0.1".into(), port: 7878, approval_timeout_secs: 900 }
    }
}

/// Set in `saved` every value that differs between `old` and `new`, leaving the rest of `saved` alone.
/// Tables are compared key by key. A list is written whole when `saved` still holds `old`; otherwise
/// only the items `new` adds to or removes from `old` are applied to `saved`. Any other value (a
/// string, a number) is changed as a whole.
fn apply_changes(saved: &mut toml::Value, old: &toml::Value, new: &toml::Value) {
    use toml::Value;
    if old == new {
        return;
    }
    if let (Value::Array(old_items), Value::Array(new_items), Value::Array(saved_items)) = (old, new, &mut *saved) {
        // Another Heron on this home may have changed the list since this one read it: apply only
        // what this one added and removed, so its stale copy of the rest is never written back.
        if old != &Value::Array(saved_items.clone()) {
            saved_items.retain(|v| !(old_items.contains(v) && !new_items.contains(v)));
            for v in new_items.iter().filter(|v| !old_items.contains(v)) {
                if !saved_items.contains(v) {
                    saved_items.push(v.clone());
                }
            }
            return;
        }
    }
    let (Value::Table(old), Value::Table(new)) = (old, new) else {
        *saved = new.clone();
        return;
    };
    if !saved.is_table() {
        *saved = Value::Table(Default::default());
    }
    let Value::Table(saved) = saved else { return };
    for key in old.keys().chain(new.keys().filter(|k| !old.contains_key(*k))) {
        match (old.get(key), new.get(key)) {
            (Some(o), Some(n)) => match saved.get_mut(key) {
                Some(s) => apply_changes(s, o, n),
                None if o != n => {
                    saved.insert(key.clone(), n.clone());
                }
                None => {}
            },
            (None, Some(n)) => {
                saved.insert(key.clone(), n.clone());
            }
            (Some(_), None) => {
                saved.remove(key);
            }
            (None, None) => {}
        }
    }
}

/// The value at a dotted path (`"permissions.mode"`) in a table of settings.
fn toml_get<'a>(value: &'a toml::Value, path: &str) -> Option<&'a toml::Value> {
    path.split('.').try_fold(value, |v, key| v.get(key))
}

/// Set the value at a dotted path, creating the tables on the way.
fn toml_set(value: &mut toml::Value, path: &str, new: toml::Value) {
    let mut keys: Vec<&str> = path.split('.').collect();
    let Some(last) = keys.pop() else { return };
    let mut at = value;
    for key in keys {
        let Some(table) = at.as_table_mut() else { return };
        at = table.entry(key).or_insert_with(|| toml::Value::Table(Default::default()));
    }
    if let Some(table) = at.as_table_mut() {
        table.insert(last.to_string(), new);
    }
}

/// How long anything waits for `config.lock` before it gives up.
pub const CONFIG_LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// Take the lock on `file` (`config.lock`), waiting at most `wait`. False when another program
/// still holds it after that.
pub(crate) fn lock_within(file: &std::fs::File, wait: std::time::Duration) -> std::io::Result<bool> {
    let deadline = std::time::Instant::now() + wait;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(true),
            Err(std::fs::TryLockError::WouldBlock) => {}
            Err(std::fs::TryLockError::Error(e)) => return Err(e),
        }
        let now = std::time::Instant::now();
        if now >= deadline {
            return Ok(false);
        }
        std::thread::sleep((deadline - now).min(std::time::Duration::from_millis(20)));
    }
}

/// The error for a change that gave up waiting for `config.lock`.
pub(crate) fn lock_timed_out(wait: std::time::Duration) -> anyhow::Error {
    anyhow::anyhow!("config.lock is still held by another program after {} s (perhaps a Heron that stopped in the middle of a save), so nothing was changed; try again, or close the other Heron", wait.as_secs())
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
        let mut cfg = Self::read_saved(&default_home())?;
        if let Ok(v) = std::env::var("HERON_MODEL") {
            cfg.llm.model = v;
        }
        if let Ok(v) = std::env::var("HERON_BASE_URL") {
            cfg.llm.base_url = v;
        }
        if let Ok(v) = std::env::var("HERON_PROVIDER") {
            cfg.llm.provider = v;
        }
        if let Ok(v) = std::env::var("HERON_CLI_COMMAND") {
            cfg.llm.cli_command = v;
        }
        if let Ok(v) = std::env::var("HERON_CLI_MODEL") {
            cfg.llm.cli_model = v;
        }
        if let Ok(v) = std::env::var("HERON_MODE") {
            cfg.permissions.mode = v;
        }
        Ok(cfg)
    }

    /// The settings as saved in `<home>/config.toml` (defaults if absent), without the overrides from
    /// the environment or the command line.
    pub fn read_saved(home: &Path) -> Result<Config> {
        let home = home.to_path_buf();
        let path = home.join("config.toml");
        let mut cfg: Config = if path.exists() {
            let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
            let mut cfg: Config = toml::from_str(&text).with_context(|| {
                let previous = home.join("config.toml.previous");
                if previous.exists() {
                    format!("parsing {} (the settings from before the last save are in {}; copy that file over it to go back)", path.display(), previous.display())
                } else {
                    format!("parsing {}", path.display())
                }
            })?;
            cfg.loaded_rules = Some(SavedRules { allow: cfg.permissions.allow.clone(), deny: cfg.permissions.deny.clone() });
            cfg
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
        Ok(cfg)
    }

    /// Apply `heron --mode` and `--model`. They hold for this run only: [`Config::save_changes`]
    /// never writes them.
    pub fn with_cli_overrides(mut self, mode: Option<&str>, model: Option<&str>) -> Result<Config> {
        if let Some(m) = mode {
            crate::permissions::Mode::parse(m)?;
            self.permissions.mode = m.to_string();
        }
        if let Some(m) = model {
            self.llm.model = m.to_string();
        }
        Ok(self)
    }

    /// Save the settings that differ between `before` and `self`, and only those: the file is read
    /// again and each changed field is set in it. A value that came from the command line or the
    /// environment (`--mode yolo`, `--model`, `HERON_MODEL`) is in both and is never written.
    ///
    /// `chosen` names fields the user set explicitly (`"permissions.mode"`): they are written as they
    /// are in `self` even when `before` already held the same value, as it does under an override.
    /// A list field is merged (see [`apply_changes`]), so this never writes back a whole list that
    /// another Heron on this home has changed since. The file is read and written under
    /// [`Config::lock_file`]. Returns the settings as written.
    pub fn save_changes(&self, before: &Config, chosen: &[&str]) -> Result<Config> {
        let _lock = self.lock_file()?;
        self.save_changes_held(before, chosen, &[])
    }

    /// [`Config::save_changes`], with [`Config::lock_file`] already held. Each of `members` is
    /// `(list, item, present)`: after the merge, `item` is put in the list at the dotted path `list`
    /// when `present` (at the end, if it is not there yet) and taken out of it otherwise, whatever
    /// `before` held. A choice the user just made is written this way even when this copy of the
    /// settings already matched it and another Heron has changed the file since.
    ///
    /// The "never" rules in `self` are not written for being in memory: `App` passes the ones saved
    /// in `heron.db` as `members`, so a copy that is out of date cannot put back a lifted one.
    pub(crate) fn save_changes_held(&self, before: &Config, chosen: &[&str], members: &[(&str, &str, bool)]) -> Result<Config> {
        let (old, new) = (toml::Value::try_from(before)?, toml::Value::try_from(self)?);
        let mut saved = toml::Value::try_from(Self::read_saved(&self.home)?)?;
        apply_changes(&mut saved, &old, &new);
        for path in chosen {
            if let Some(value) = toml_get(&new, path) {
                toml_set(&mut saved, path, value.clone());
            }
        }
        for (path, item, present) in members {
            let mut list = toml_get(&saved, path).and_then(|v| v.as_array()).cloned().unwrap_or_default();
            let item = toml::Value::String(item.to_string());
            if *present {
                if !list.contains(&item) {
                    list.push(item);
                }
            } else {
                list.retain(|v| *v != item);
            }
            toml_set(&mut saved, path, toml::Value::Array(list));
        }
        let mut merged: Config = saved.try_into().context("the changed settings could not be combined with the saved ones; nothing was saved")?;
        merged.home = self.home.clone();
        merged.write_file()?;
        Ok(merged)
    }

    /// Hold the lock that every Heron on this home takes to read-and-write `config.toml`, until the
    /// returned file is dropped. It is a file of its own: `config.toml` is replaced by a rename on each
    /// save, so a lock on it would not outlive the save. A second handle on it blocks even within one
    /// program, so whoever holds it calls [`Config::save_changes_held`], not `save_changes`.
    ///
    /// It is waited for at most [`CONFIG_LOCK_WAIT`]: a Heron stopped in the middle of a save must not
    /// stall every other one. Still held after that, it is an error, and nothing is saved.
    pub(crate) fn lock_file(&self) -> Result<std::fs::File> {
        self.lock_file_within(CONFIG_LOCK_WAIT)
    }

    /// [`Config::lock_file`], waiting at most `wait`.
    pub(crate) fn lock_file_within(&self, wait: std::time::Duration) -> Result<std::fs::File> {
        let file = self.open_lock_file()?;
        let path = self.home.join("config.lock");
        if !lock_within(&file, wait).with_context(|| format!("locking {}", path.display()))? {
            return Err(lock_timed_out(wait));
        }
        Ok(file)
    }

    /// The file [`Config::lock_file`] locks, opened (and the home folder made) but not yet locked,
    /// for a caller that waits for it in its own way.
    pub(crate) fn open_lock_file(&self) -> Result<std::fs::File> {
        self.ensure_home()?;
        let path = self.home.join("config.lock");
        std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path).with_context(|| format!("opening {}", path.display()))
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

    /// Store a connector credential only in a verified OS credential vault.
    ///
    /// Config::store_secret intentionally has a file fallback for legacy model
    /// keys. Connector/web-login credentials MUST NOT use that fallback:
    /// on Linux, until a native Secret Service or encrypted vault backend is
    /// implemented, storing a new connector API key is refused.
    pub fn store_vault_only_secret(&self, name: &str, value: &str) -> Result<()> {
        if !cfg!(any(target_os = "macos", windows)) {
            anyhow::bail!("Secure connector vault is not available on this OS. A native Secret Service or encrypted vault must be configured first.");
        }
        self.ensure_home()?;
        // Inspect the legacy store BEFORE writing to Keychain/Credential
        // Manager. A damaged file must never turn a failed API response into
        // a newly installed secret that the user believes was rejected.
        let mut file = self.read_file_secrets()?;
        let mut store = NativeCredential { home: &self.home, name };
        crate::credential_transaction::replace(&mut store, value, || {
            if file.remove(name).is_some() {
                self.write_file_secrets(&file)?;
            }
            Ok(())
        })
    }

    /// Revoke and verify absence without depending on the legacy plaintext file.
    pub fn delete_vault_only_secret(&self, name: &str) -> Result<()> {
        #[cfg(any(target_os = "macos", windows))]
        {
            let mut store = NativeCredential { home: &self.home, name };
            crate::credential_transaction::delete_verified(&mut store)
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            // No connector key can have been stored in this unsupported backend.
            vault::delete(&self.home, name).map_err(anyhow::Error::msg)
        }
    }

    /// Metadata-only status; never exposes the actual credential to an API.
    pub fn vault_only_secret_present(&self, name: &str) -> bool {
        vault::get(&self.home, name).is_some()
    }

    pub fn stored_api_key(&self) -> Option<String> {
        self.secret("api_key")
    }

    pub fn store_api_key(&self, key: &str) -> Result<()> {
        self.store_secret("api_key", key)
    }

    /// Persist the current settings to config.toml.
    ///
    /// The new file is written beside the old one, flushed, and renamed over it, so stopping half-way
    /// (a crash, a full disk) cannot leave a cut-off settings file that Heron then refuses to start
    /// with. The settings that were there are kept as `config.toml.previous`, so one bad save can be
    /// taken back by hand. Nothing is written unless it reads back as valid settings.
    pub fn save(&self) -> Result<()> {
        let _lock = self.lock_file()?;
        self.write_file()
    }

    /// [`Config::save`], with the lock already held.
    fn write_file(&self) -> Result<()> {
        use std::io::Write;
        self.ensure_home()?;
        let path = self.home.join("config.toml");
        let text = toml::to_string_pretty(self)?;
        toml::from_str::<Config>(&text).context("these settings could not be written in a form Heron can read back; nothing was saved")?;
        let tmp = self.home.join(format!("config.toml.tmp-{}-{}", std::process::id(), uuid::Uuid::new_v4().simple()));
        let written = (|| -> Result<()> {
            let mut opts = std::fs::OpenOptions::new();
            opts.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            let mut f = opts.open(&tmp)?;
            f.write_all(text.as_bytes())?;
            f.sync_all()?;
            drop(f);
            if path.exists() {
                // Best effort: failing to keep the old copy is no reason to refuse the new settings.
                let _ = std::fs::copy(&path, self.home.join("config.toml.previous"));
            }
            std::fs::rename(&tmp, &path)?;
            #[cfg(unix)]
            if let Ok(dir) = std::fs::File::open(&self.home) {
                let _ = dir.sync_all();
            }
            Ok(())
        })();
        if written.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        written.with_context(|| format!("could not save {}", path.display()))
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

struct NativeCredential<'a> {
    home: &'a std::path::Path,
    name: &'a str,
}

impl crate::credential_transaction::CredentialStore for NativeCredential<'_> {
    fn read(&mut self) -> Result<Option<String>> {
        vault::get_checked(self.home, self.name).map_err(anyhow::Error::msg)
    }
    fn write(&mut self, value: &str) -> Result<()> {
        vault::set_verified(self.home, self.name, value).map_err(anyhow::Error::msg)
    }
    fn delete(&mut self) -> Result<()> {
        vault::delete(self.home, self.name).map_err(anyhow::Error::msg)
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
        // Legacy model keys may fall back to an owner-only file.
        set_verified(home, name, value).is_ok()
    }
    pub fn get_checked(home: &Path, name: &str) -> Result<Option<String>, String> {
        let e = entry(home, name).ok_or_else(|| "OS vault entry cannot be opened".to_string())?;
        match e.get_password() {
            Ok(value) => Ok(Some(value).filter(|v| !v.is_empty())),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }
    pub fn set_verified(home: &Path, name: &str, value: &str) -> Result<(), String> {
        let e = entry(home, name).ok_or_else(|| "OS vault entry cannot be opened".to_string())?;
        e.set_password(value).map_err(|error| format!("OS credential write rejected: {error}"))?;
        let saved = e.get_password().map_err(|error| format!("OS credential read-back failed after write: {error}"))?;
        if saved != value {
            return Err("OS credential read-back did not match the attempted value.".into());
        }
        Ok(())
    }
    pub fn delete(home: &Path, name: &str) -> Result<(), String> {
        match entry(home, name).map(|e| e.delete_credential()) {
            // Nothing saved under that name is what the caller wanted anyway.
            Some(Ok(())) | Some(Err(keyring::Error::NoEntry)) => Ok(()),
            None => Err("OS vault entry cannot be opened".into()),
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
    pub fn get_checked(_: &Path, _: &str) -> Result<Option<String>, String> {
        Err("Secure OS vault is not available on this platform.".into())
    }
    pub fn set_verified(_: &Path, _: &str, _: &str) -> Result<(), String> {
        Err("Secure OS vault is not available on this platform.".into())
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
# Use a subscription CLI instead of an API key: provider = "claude-cli",
# "codex-cli", "cursor-cli", "grok-cli", "opencode-cli", "antigravity-cli",
# "cline-cli", "copilot-cli", "qwen-cli", "gemini-cli", "pi-cli", "fx-cli",
# "kiro-cli", "muse-cli", "goose-cli", or "custom-cli". Run `heron providers`
# to see install status. Full list, login steps and safety rules are in
# docs/cli-providers.md. Login happens in the CLI itself, never in Heron.
provider = "http"                    # "http" = the endpoint above
cli_command = ""                     # empty = resolve the provider's binary
cli_model = ""                       # empty = the CLI's subscription default
cli_config_dir = ""                  # e.g. a second CLAUDE_CONFIG_DIR
cli_allow_agent_tools = false        # only for Cline / Goose, which cannot disable their own tools
cli_args = []                        # extra argv for "custom-cli"; {prompt} and {model} placeholders

[agent]
name = "Heron"
max_steps = 40
context_tokens = 100000
auto_memory = true
notify_command = ""                  # e.g. 'notify-send Heron "$HERON_MESSAGE"'
shell_env = []                       # extra variables commands may see, e.g. ["GITHUB_TOKEN"]; others are withheld
daily_token_limit = 0                # your own cap per day; 0 = none. Heron economises as it runs down
monthly_token_limit = 0              # the same, per calendar month
routing = "auto"                     # send short, simple requests to fast_model when one is set; "off" = never
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

    /// Run explicitly on an authorized interactive macOS/Windows desktop.
    /// A mock backend cannot establish the OS vault's availability or ACLs.
    #[test]
    #[cfg(any(target_os = "macos", windows))]
    #[ignore = "requires an authorized, unlocked native credential vault"]
    fn connector_vault_live_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config { home: dir.path().join("native-vault-qualification"), ..Default::default() };
        let name = format!("connector_{}_api_key", uuid::Uuid::new_v4().simple());
        cfg.store_vault_only_secret(&name, "disposable-first").unwrap();
        let first = vault::get_checked(&cfg.home, &name).unwrap();
        cfg.store_vault_only_secret(&name, "disposable-replacement").unwrap();
        let second = vault::get_checked(&cfg.home, &name).unwrap();
        // Unrelated legacy corruption must not prevent emergency revocation.
        std::fs::write(cfg.home.join("secrets.json"), b"{broken").unwrap();
        cfg.delete_vault_only_secret(&name).unwrap();
        let absent = vault::get_checked(&cfg.home, &name).unwrap().is_none();
        assert!(first.as_deref() == Some("disposable-first"));
        assert!(second.as_deref() == Some("disposable-replacement"));
        assert!(absent);
        assert_eq!(std::fs::read(cfg.home.join("secrets.json")).unwrap(), b"{broken");
    }

    #[test]
    fn default_config_text_parses_to_defaults() {
        let parsed: Config = toml::from_str(DEFAULT_CONFIG).unwrap();
        let d = Config::default();
        assert_eq!(parsed.llm.model, d.llm.model);
        assert_eq!(parsed.permissions.mode, "ask");
        assert_eq!(parsed.server.port, 7878);
    }

    #[test]
    fn a_saved_list_takes_only_this_ones_changes() {
        let dir = tempfile::tempdir().unwrap();
        let mut first = Config { home: dir.path().join("home"), ..Default::default() };
        first.permissions.allow = vec!["shell:git *".into(), "shell:make *".into()];
        first.save().unwrap();
        let on_disk = || Config::read_saved(&first.home).unwrap();
        // Two Herons read the same settings. One removes a rule.
        let stale = first.clone();
        let mut a = first.clone();
        a.permissions.allow = vec!["shell:make *".into()];
        a.save_changes(&first, &[]).unwrap();
        // The other never read that change. It removes the second rule from its old copy: the first
        // stays removed, and only its own removal is applied.
        let mut b = stale.clone();
        b.permissions.allow = vec!["shell:git *".into()];
        let written = b.save_changes(&stale, &[]).unwrap();
        assert!(on_disk().permissions.allow.is_empty(), "{:?}", on_disk().permissions.allow);
        assert_eq!(written.permissions.allow, on_disk().permissions.allow);
        // What it adds from its old copy is added to what is there.
        let mut c = b.clone();
        c.permissions.allow.push("shell:ls *".into());
        c.save_changes(&b, &[]).unwrap();
        assert_eq!(on_disk().permissions.allow, vec!["shell:ls *".to_string()]);

        // When nobody else changed it, a list is written as given, order included.
        let before = on_disk();
        let mut d = before.clone();
        d.permissions.deny = vec!["write_file:*".into(), "shell:*".into()];
        d.save_changes(&before, &[]).unwrap();
        let mut e = d.clone();
        e.permissions.deny = vec!["shell:*".into(), "write_file:*".into()];
        e.save_changes(&d, &[]).unwrap();
        assert_eq!(on_disk().permissions.deny, vec!["shell:*".to_string(), "write_file:*".to_string()]);

        // A field named as chosen is written even when it is unchanged in memory.
        let held = on_disk().with_cli_overrides(Some("yolo"), None).unwrap();
        held.save_changes(&held, &[]).unwrap();
        assert_eq!(on_disk().permissions.mode, "ask", "an override is not written");
        held.save_changes(&held, &["permissions.mode"]).unwrap();
        assert_eq!(on_disk().permissions.mode, "yolo", "a choice is");
    }

    #[test]
    fn saving_settings_replaces_the_file_whole_and_keeps_the_previous_one() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config { home: dir.path().join("home"), ..Default::default() };
        cfg.llm.model = "first-model".into();
        cfg.save().unwrap();
        let path = cfg.home.join("config.toml");
        assert!(!cfg.home.join("config.toml.previous").exists(), "nothing came before the first save");

        cfg.llm.model = "second-model".into();
        cfg.save().unwrap();
        let read = |p: &std::path::Path| toml::from_str::<Config>(&std::fs::read_to_string(p).unwrap()).unwrap().llm.model;
        assert_eq!(read(&path), "second-model");
        assert_eq!(read(&cfg.home.join("config.toml.previous")), "first-model", "the settings before the last save can be put back by hand");
        let stray: Vec<_> = std::fs::read_dir(&cfg.home).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().contains(".tmp-")).collect();
        assert!(stray.is_empty(), "no half-written file is left behind");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }

        // A save that cannot complete (the settings file's place is taken by a folder) changes nothing
        // and cleans up after itself.
        let blocked = dir.path().join("blocked");
        std::fs::create_dir_all(blocked.join("config.toml")).unwrap();
        let other = Config { home: blocked.clone(), ..Default::default() };
        assert!(other.save().is_err());
        assert!(blocked.join("config.toml").is_dir());
        let stray: Vec<_> = std::fs::read_dir(&blocked).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().contains(".tmp-")).collect();
        assert!(stray.is_empty());
    }

    #[test]
    fn a_held_config_lock_is_waited_for_a_bounded_time() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config { home: dir.path().join("home"), ..Default::default() };
        let held = cfg.lock_file().unwrap();
        // Still held after the wait (none at all here, so the test does not sleep): an error, not a hang.
        let said = format!("{:#}", cfg.lock_file_within(std::time::Duration::ZERO).expect_err("the lock is held"));
        assert!(said.contains("config.lock") && said.contains("nothing was changed"), "{said}");
        drop(held);
        cfg.lock_file_within(std::time::Duration::ZERO).unwrap();
    }

    #[test]
    fn secrets_round_trip_and_never_touch_the_config_file() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config { home: dir.path().join("home"), ..Default::default() };
        assert_eq!(cfg.secret("api_key"), None);
        cfg.store_secret("api_key", "  sk-test-123  ").unwrap();
        cfg.store_secret("email_password", "app pass").unwrap();
        assert_eq!(cfg.secret("api_key").as_deref(), Some("sk-test-123"));
        assert_eq!(cfg.secret("email_password").as_deref(), Some("app pass"));
        cfg.save().unwrap();
        assert!(!std::fs::read_to_string(cfg.home.join("config.toml")).unwrap().contains("sk-test"));
        // a different profile cannot see them
        let other = Config { home: dir.path().join("other"), ..Default::default() };
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
    #[cfg(not(any(target_os = "macos", windows)))]
    fn connector_vault_fails_closed_without_an_os_backend() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.home = tmp.path().join("home");
        assert!(cfg.store_vault_only_secret("connector_test_api_key", "never-write-me").is_err());
        assert!(!cfg.vault_only_secret_present("connector_test_api_key"));
        assert!(!cfg.home.join("secrets.json").exists());
    }

    #[test]
    fn a_damaged_secrets_file_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config { home: dir.path().join("home"), ..Default::default() };
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
        let mut cfg = Config { home: dir.path().join("home"), ..Default::default() };
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
