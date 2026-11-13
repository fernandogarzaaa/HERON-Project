//! Setup lane views: onboarding, providers and settings, persona.
//!
//! Pure view state plus the JSON mapping for the server API. This file
//! uses only `std` and `serde_json`. It makes no HTTP calls, reads no
//! files, and knows nothing about Slint types.
//!
//! The `SetupBridge` trait is the interface this lane requests from the
//! Shell lane. It is async because the real implementation,
//! `HeronSetupBridge` in `setup_bridge.rs`, calls `HeronClient::api`,
//! which is async. The driver runs trait calls as tasks on the Bridge
//! runtime (`Bridge::spawn`) and hands results to the UI thread with
//! `Weak::upgrade_in_event_loop`. Nothing here ever blocks the UI thread.
//! The state structs are generic over the bridge so tests can use a fake
//! with no runtime and no server.
//!
//! Two halves, kept separate on purpose:
//! - `parse_*` and `*_body` are pure functions over `serde_json::Value`.
//!   They carry every server-shape decision in this lane, and they are
//!   unit-tested here against payloads copied from `src/server.rs`.
//! - `setup_bridge.rs` is a thin pass-through: one `client.api` call per
//!   trait method, then the matching `parse_*`. It holds no shape logic
//!   of its own, so the part that cannot compile in this worktree (it
//!   needs `HeronClient`, which lives in the Shell worktree) is small.
//!
//! Home: `src/bin/desktop/setup.rs`, per the plan file layout (section 5).
//! All Slint code lives in the desktop binary crate behind the `desktop`
//! feature, never in the library. Wiring (the module declarations in
//! `src/bin/desktop.rs` and the `desktop` feature dependencies) belongs to
//! the Shell lane and integration.
//!
//! Verification here: `rustc --edition 2021 --crate-type lib` and
//! `rustc --test` with `--extern serde_json=<rlib>` from the workspace
//! `target/debug/deps` (built offline with `cargo build -p serde_json`).
//! The async trait methods in tests run on a tiny `block_on` in the test
//! module. The fakes never park, so the loop always resolves at once.

use serde_json::{json, Value};

/// How much Heron does on its own. Values match the server strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Proactivity {
    Off,
    Suggest,
    Prepare,
    Takeover,
}

impl Proactivity {
    pub fn as_str(self) -> &'static str {
        match self {
            Proactivity::Off => "off",
            Proactivity::Suggest => "suggest",
            Proactivity::Prepare => "prepare",
            Proactivity::Takeover => "takeover",
        }
    }

    pub fn parse(s: &str) -> Option<Proactivity> {
        match s {
            "off" => Some(Proactivity::Off),
            "suggest" => Some(Proactivity::Suggest),
            "prepare" => Some(Proactivity::Prepare),
            "takeover" => Some(Proactivity::Takeover),
            _ => None,
        }
    }
}

/// Permission mode. Values match the server strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Readonly,
    Ask,
    Auto,
    Yolo,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Readonly => "readonly",
            Mode::Ask => "ask",
            Mode::Auto => "auto",
            Mode::Yolo => "yolo",
        }
    }

    pub fn parse(s: &str) -> Option<Mode> {
        match s {
            "readonly" => Some(Mode::Readonly),
            "ask" => Some(Mode::Ask),
            "auto" => Some(Mode::Auto),
            "yolo" => Some(Mode::Yolo),
            _ => None,
        }
    }
}

/// Where the API key comes from. Values match the server strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeySource {
    Environment,
    Saved,
    None,
}

impl KeySource {
    pub fn parse(s: &str) -> Option<KeySource> {
        match s {
            "environment" => Some(KeySource::Environment),
            "saved" => Some(KeySource::Saved),
            "none" => Some(KeySource::None),
            _ => None,
        }
    }
}

/// Per-tool permission. Values match the server strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolSetting {
    Ask,
    Allow,
    Never,
}

impl ToolSetting {
    pub fn as_str(self) -> &'static str {
        match self {
            ToolSetting::Ask => "ask",
            ToolSetting::Allow => "allow",
            ToolSetting::Never => "never",
        }
    }

    pub fn parse(s: &str) -> Option<ToolSetting> {
        match s {
            "ask" => Some(ToolSetting::Ask),
            "allow" => Some(ToolSetting::Allow),
            "never" => Some(ToolSetting::Never),
            _ => None,
        }
    }

    /// Index into the settings select: 0 ask, 1 allow, 2 never.
    pub fn index(self) -> usize {
        match self {
            ToolSetting::Ask => 0,
            ToolSetting::Allow => 1,
            ToolSetting::Never => 2,
        }
    }
}

/// A context source in onboarding. Values match the server path segments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Computer,
    Documents,
    Calendar,
    Email,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SourceKind::Computer => "computer",
            SourceKind::Documents => "documents",
            SourceKind::Calendar => "calendar",
            SourceKind::Email => "email",
        }
    }

    pub fn parse(s: &str) -> Option<SourceKind> {
        match s {
            "computer" => Some(SourceKind::Computer),
            "documents" => Some(SourceKind::Documents),
            "calendar" => Some(SourceKind::Calendar),
            "email" => Some(SourceKind::Email),
            _ => None,
        }
    }
}

/// Model preset buttons in Settings. They fill base URL and model only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    OpenRouter,
    OpenAi,
    Ollama,
    Demo,
}

impl Preset {
    pub fn base_url(self) -> &'static str {
        match self {
            Preset::OpenRouter => "https://openrouter.ai/api/v1",
            Preset::OpenAi => "https://api.openai.com/v1",
            Preset::Ollama => "http://localhost:11434/v1",
            Preset::Demo => "demo",
        }
    }

    pub fn model(self) -> &'static str {
        match self {
            Preset::OpenRouter => "anthropic/claude-sonnet-5.5",
            Preset::OpenAi => "gpt-5.5",
            Preset::Ollama => "llama3.3",
            Preset::Demo => "demo",
        }
    }
}

/// Character limit for the persona text, from `src/persona.rs`.
pub const MAX_PERSONA_CHARS: usize = 4000;

/// Name limit for the agent name, enforced by the server.
pub const MAX_NAME_CHARS: usize = 40;

/// The subset of `GET /state` the setup views read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateSnapshot {
    pub has_key: bool,
    pub demo: bool,
    pub onboarded: bool,
    pub model: String,
    pub sandbox: String,
    pub os: String,
}

/// One source row from `GET /sources`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceStatus {
    pub kind: SourceKind,
    pub connected: bool,
    pub indexed: usize,
    pub learned: usize,
    /// Documents only: folders Heron indexes.
    pub folders: Vec<String>,
    /// Calendar only: hosts of the connected ICS links.
    pub hosts: Vec<String>,
    /// Email only: the connected address.
    pub address: Option<String>,
}

/// The whole `GET /sources` response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourcesSnapshot {
    pub onboarded: bool,
    pub demo: bool,
    pub has_model: bool,
    pub model: String,
    pub about_saved: usize,
    pub about_indexed: usize,
    pub about_learned: usize,
    pub sources: Vec<SourceStatus>,
}

/// The about-yourself form in onboarding step 2.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AboutForm {
    pub name: String,
    pub work: String,
    pub goals: String,
    pub style: String,
    pub people: String,
    pub notes: String,
}

impl AboutForm {
    /// True when every field is blank. The web page then refuses to save.
    pub fn is_empty(&self) -> bool {
        self.name.trim().is_empty()
            && self.work.trim().is_empty()
            && self.goals.trim().is_empty()
            && self.style.trim().is_empty()
            && self.people.trim().is_empty()
            && self.notes.trim().is_empty()
    }
}

/// Form fields for connecting a source. Only the fields matching the kind
/// are sent. Email also accepts `port` and `tls`, which the web page never
/// sends, so they are not here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceFields {
    pub path: String,
    pub url: String,
    pub host: String,
    pub username: String,
    pub password: String,
}

/// One result shape for every `POST /sources/{name}` response. The
/// implementation maps the per-source JSON into this: `learned` for
/// computer, `indexed` for documents, calendar, and email.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConnectResult {
    pub saved: Vec<String>,
    pub learned: Vec<String>,
    pub indexed: Option<usize>,
}

/// The `DELETE /sources/{name}` response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisconnectResult {
    pub removed_items: usize,
    pub removed_memories: usize,
}

/// One CLI provider entry from `GET /settings` field `providers`. `label`
/// is what the select shows: `{name} — {auth}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderItem {
    pub id: String,
    pub label: String,
}

/// One connected app line from `GET /settings` fields `mcp` and
/// `mcp_changed`. The parser splits `name` from the line at the first
/// colon, because the view language has no string split.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpLine {
    pub text: String,
    pub name: String,
    pub changed: bool,
}

/// The `GET /settings` response, trimmed to what the view shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsSnapshot {
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub fast_model: String,
    /// One comma-separated string on the wire, not an array.
    pub fallback_models: String,
    pub key_source: KeySource,
    pub secret_storage: String,
    pub provider: String,
    pub providers: Vec<ProviderItem>,
    pub cli_command: String,
    pub cli_model: String,
    pub mode: Mode,
    pub daemon_mode: String,
    pub auto_memory: bool,
    pub neural_memory: bool,
    pub neural_available: bool,
    /// Browser display name, or None when no supported browser was found.
    pub browser: Option<String>,
    pub browser_visible: bool,
    pub workspace: String,
    pub home: String,
    pub mcp: Vec<McpLine>,
}

/// The editable settings form. `api_key` is write-only: empty keeps the
/// stored key. `name` is not sent by the main settings save; it is saved
/// through the persona endpoints, matching the web page. (The server
/// would accept `name` in `POST /settings`, but the web form never sends
/// it, so neither does this.)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsForm {
    pub base_url: String,
    pub model: String,
    pub fallback_models: String,
    pub fast_model: String,
    pub provider: String,
    pub cli_command: String,
    pub cli_model: String,
    pub api_key: String,
    pub mode: Mode,
    pub daemon_mode: String,
    pub auto_memory: bool,
    pub neural_memory: bool,
    pub browser_visible: bool,
    pub workspace: String,
}

impl SettingsForm {
    pub fn from_snapshot(s: &SettingsSnapshot) -> SettingsForm {
        SettingsForm {
            base_url: s.base_url.clone(),
            model: s.model.clone(),
            fallback_models: s.fallback_models.clone(),
            fast_model: s.fast_model.clone(),
            provider: s.provider.clone(),
            cli_command: s.cli_command.clone(),
            cli_model: s.cli_model.clone(),
            api_key: String::new(),
            mode: s.mode,
            daemon_mode: s.daemon_mode.clone(),
            auto_memory: s.auto_memory,
            neural_memory: s.neural_memory,
            browser_visible: s.browser_visible,
            workspace: s.workspace.clone(),
        }
    }

    pub fn apply_preset(&mut self, preset: Preset) {
        self.base_url = preset.base_url().to_string();
        self.model = preset.model().to_string();
    }
}

/// The `GET /persona` response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersonaSnapshot {
    pub text: String,
    pub is_default: bool,
    pub name: String,
}

/// One row of the per-tool permission table. Only rows where the server
/// reports `always_safe` false are shown; the parser filters the rest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolRow {
    pub name: String,
    pub about: String,
    pub setting: ToolSetting,
}

/// The `GET /telegram` response. `code` is set only when configured and
/// not yet paired.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelegramSnapshot {
    pub configured: bool,
    pub paired: bool,
    pub code: Option<String>,
}

/// The interface this lane requests from the Shell lane bridge. One method
/// per server call. Every method returns the server error text on failure,
/// so the view can show it. The methods are async: the real
/// implementation awaits `HeronClient::api` on the Bridge runtime, and
/// the driver hands results to the UI thread with
/// `Weak::upgrade_in_event_loop`. Nothing here blocks the UI thread.
///
/// The `async_fn_in_trait` lint is allowed deliberately below. The trait
/// does not promise `Send` on the returned futures, but both
/// implementations deliver it: `HeronSetupBridge` holds only a
/// `HeronClient` (reqwest client plus strings), and the test fake holds
/// only strings, numbers, and bools. The driver proves it at its own
/// compile time, because `Bridge::spawn` requires a `Send` future.
#[allow(async_fn_in_trait)]
pub trait SetupBridge {
    async fn load_state(&self) -> Result<StateSnapshot, String>;
    async fn load_sources(&self) -> Result<SourcesSnapshot, String>;
    async fn connect_source(
        &self,
        kind: SourceKind,
        fields: &SourceFields,
    ) -> Result<ConnectResult, String>;
    async fn learn_source(&self, kind: SourceKind) -> Result<Vec<String>, String>;
    async fn disconnect_source(&self, kind: SourceKind) -> Result<DisconnectResult, String>;
    async fn save_about(&self, form: &AboutForm) -> Result<Vec<String>, String>;
    async fn load_proactivity(&self) -> Result<Proactivity, String>;
    async fn set_proactivity(&self, level: Proactivity) -> Result<(), String>;
    async fn use_demo(&self) -> Result<(), String>;
    async fn finish_onboarding(&self) -> Result<(), String>;
    async fn load_settings(&self) -> Result<SettingsSnapshot, String>;
    async fn save_settings(&self, form: &SettingsForm) -> Result<SettingsSnapshot, String>;
    async fn load_persona(&self) -> Result<PersonaSnapshot, String>;
    async fn save_persona(&self, name: &str, text: &str) -> Result<PersonaSnapshot, String>;
    async fn reset_persona(&self) -> Result<PersonaSnapshot, String>;
    async fn load_tools(&self) -> Result<Vec<ToolRow>, String>;
    async fn set_tool_setting(
        &self,
        name: &str,
        setting: ToolSetting,
    ) -> Result<ToolSetting, String>;
    async fn accept_mcp(&self, name: &str) -> Result<String, String>;
    async fn load_telegram(&self) -> Result<TelegramSnapshot, String>;
    async fn connect_telegram(&self, token: &str) -> Result<TelegramSnapshot, String>;
    async fn disconnect_telegram(&self) -> Result<TelegramSnapshot, String>;
    async fn make_backup(&self) -> Result<String, String>;
}

// ---------------------------------------------------------------------------
// JSON mapping. Pure functions over serde_json::Value. Every shape below
// was read from src/server.rs (handler code, not the docs), and the tests
// use payloads copied from those handlers.
// ---------------------------------------------------------------------------

fn jstr<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("")
}

fn jbool(v: &Value, key: &str) -> bool {
    v.get(key).and_then(|x| x.as_bool()).unwrap_or(false)
}

fn jnum(v: &Value, key: &str) -> usize {
    v.get(key).and_then(|x| x.as_u64()).unwrap_or(0) as usize
}

fn jstrs(v: &Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn jopt_str(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(str::to_string)
}

/// `GET /state`, trimmed to what the setup views read.
pub fn parse_state(v: &Value) -> StateSnapshot {
    StateSnapshot {
        has_key: jbool(v, "has_key"),
        demo: jbool(v, "demo"),
        onboarded: jbool(v, "onboarded"),
        model: jstr(v, "model").to_string(),
        sandbox: jstr(v, "sandbox").to_string(),
        os: jstr(v, "os").to_string(),
    }
}

fn parse_source(v: &Value, kind: SourceKind) -> SourceStatus {
    SourceStatus {
        kind,
        connected: jbool(v, "connected"),
        indexed: jnum(v, "indexed"),
        learned: jnum(v, "learned"),
        folders: jstrs(v, "folders"),
        hosts: jstrs(v, "hosts"),
        address: jopt_str(v, "address"),
    }
}

/// `GET /sources`. All counts are numbers on this endpoint. The `about`
/// counts come from the `about` object, not from the save response.
pub fn parse_sources(v: &Value) -> SourcesSnapshot {
    let empty = Value::Null;
    let about = v.get("about").unwrap_or(&empty);
    SourcesSnapshot {
        onboarded: jbool(v, "onboarded"),
        demo: jbool(v, "demo"),
        has_model: jbool(v, "has_model"),
        model: jstr(v, "model").to_string(),
        about_saved: jnum(about, "saved"),
        about_indexed: jnum(about, "indexed"),
        about_learned: jnum(about, "learned"),
        sources: vec![
            parse_source(v.get("computer").unwrap_or(&empty), SourceKind::Computer),
            parse_source(v.get("documents").unwrap_or(&empty), SourceKind::Documents),
            parse_source(v.get("calendar").unwrap_or(&empty), SourceKind::Calendar),
            parse_source(v.get("email").unwrap_or(&empty), SourceKind::Email),
        ],
    }
}

/// `GET /awareness`, `proactive` field only. Errors on an unknown value:
/// the server rejects unknown levels on write, so a read should never
/// produce one.
pub fn parse_proactivity(v: &Value) -> Result<Proactivity, String> {
    let s = jstr(v, "proactive");
    Proactivity::parse(s)
        .ok_or_else(|| format!("the server returned an unknown proactivity level: {s}"))
}

/// `GET /settings` (and the `POST /settings` response, same shape).
/// Provider labels are `{name} — {auth}`. MCP names split at the first
/// colon. `browser` is null when no supported browser was found.
pub fn parse_settings(v: &Value) -> SettingsSnapshot {
    let providers = v
        .get("providers")
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .map(|p| ProviderItem {
                    id: jstr(p, "id").to_string(),
                    label: format!("{} — {}", jstr(p, "name"), jstr(p, "auth")),
                })
                .collect()
        })
        .unwrap_or_default();
    let changed = jstrs(v, "mcp_changed");
    let mcp = jstrs(v, "mcp")
        .into_iter()
        .map(|text| {
            let name = text
                .split_once(':')
                .map(|(n, _)| n.to_string())
                .unwrap_or_else(|| text.clone());
            let changed = changed.iter().any(|c| c == &name);
            McpLine {
                text,
                name,
                changed,
            }
        })
        .collect();
    SettingsSnapshot {
        name: jstr(v, "name").to_string(),
        base_url: jstr(v, "base_url").to_string(),
        model: jstr(v, "model").to_string(),
        fast_model: jstr(v, "fast_model").to_string(),
        fallback_models: jstr(v, "fallback_models").to_string(),
        key_source: KeySource::parse(jstr(v, "key_source")).unwrap_or(KeySource::None),
        secret_storage: jstr(v, "secret_storage").to_string(),
        provider: jstr(v, "provider").to_string(),
        providers,
        cli_command: jstr(v, "cli_command").to_string(),
        cli_model: jstr(v, "cli_model").to_string(),
        mode: Mode::parse(jstr(v, "mode")).unwrap_or(Mode::Ask),
        daemon_mode: jstr(v, "daemon_mode").to_string(),
        auto_memory: jbool(v, "auto_memory"),
        neural_memory: jbool(v, "neural_memory"),
        neural_available: jbool(v, "neural_available"),
        browser: jopt_str(v, "browser"),
        browser_visible: jbool(v, "browser_visible"),
        workspace: jstr(v, "workspace").to_string(),
        home: jstr(v, "home").to_string(),
        mcp,
    }
}

/// `GET /persona` (and the POST/DELETE responses, same shape).
pub fn parse_persona(v: &Value) -> PersonaSnapshot {
    PersonaSnapshot {
        text: jstr(v, "text").to_string(),
        is_default: jbool(v, "is_default"),
        name: jstr(v, "name").to_string(),
    }
}

/// `GET /tools`. The endpoint returns a bare array. Only rows where
/// `always_safe` is false are shown; the rest are filtered here so the
/// trait returns exactly what the view shows.
pub fn parse_tools(v: &Value) -> Vec<ToolRow> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter(|t| !jbool(t, "always_safe"))
                .map(|t| ToolRow {
                    name: jstr(t, "name").to_string(),
                    about: jstr(t, "about").to_string(),
                    setting: ToolSetting::parse(jstr(t, "setting")).unwrap_or(ToolSetting::Ask),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `GET /telegram` (and the POST/DELETE responses, same shape). `code` is
/// a string when configured and unpaired, else null.
pub fn parse_telegram(v: &Value) -> TelegramSnapshot {
    TelegramSnapshot {
        configured: jbool(v, "configured"),
        paired: jbool(v, "paired"),
        code: jopt_str(v, "code"),
    }
}

/// `POST /sources/about` returns `{"saved": [...]}`: memory statements.
pub fn parse_saved(v: &Value) -> Vec<String> {
    jstrs(v, "saved")
}

/// `POST /sources/computer` and `POST /sources/{name}/learn` return
/// `{"learned": [...]}`: what Heron learned, as strings. This is an
/// array, not a count: `learn` in `src/sources.rs` returns a `Vec`.
/// (The Shell bridge doc table says `{learned: n}`; the server code says
/// otherwise. The array is what the implementation uses.)
pub fn parse_learned(v: &Value) -> Vec<String> {
    jstrs(v, "learned")
}

/// `POST /sources/documents`, `/calendar`, and `/email` return
/// `{"indexed": n}`.
pub fn parse_indexed(v: &Value) -> usize {
    jnum(v, "indexed")
}

/// `DELETE /sources/{name}`.
pub fn parse_disconnect(v: &Value) -> DisconnectResult {
    DisconnectResult {
        removed_items: jnum(v, "removed_items"),
        removed_memories: jnum(v, "removed_memories"),
    }
}

/// `POST /mcp/{name}/accept` returns `{"accepted": true, "note": ...}`.
pub fn parse_mcp_note(v: &Value) -> String {
    jstr(v, "note").to_string()
}

/// `POST /backup` returns `{"path": ...}`.
pub fn parse_backup_path(v: &Value) -> String {
    jstr(v, "path").to_string()
}

/// `POST /tools/{name}` returns `{"name", "setting"}`.
pub fn parse_tool_setting(v: &Value) -> Result<ToolSetting, String> {
    let s = jstr(v, "setting");
    ToolSetting::parse(s).ok_or_else(|| format!("the server returned an unknown tool setting: {s}"))
}

/// `POST /sources/about` body: all six fields, empty when blank.
pub fn about_body(form: &AboutForm) -> Value {
    json!({
        "name": form.name,
        "work": form.work,
        "goals": form.goals,
        "style": form.style,
        "people": form.people,
        "notes": form.notes,
    })
}

/// `POST /sources/{name}` body. Only the fields for that kind are sent.
/// Computer sends an empty body.
pub fn source_body(kind: SourceKind, fields: &SourceFields) -> Value {
    match kind {
        SourceKind::Computer => json!({}),
        SourceKind::Documents => json!({ "path": fields.path }),
        SourceKind::Calendar => json!({ "url": fields.url }),
        SourceKind::Email => json!({
            "host": fields.host,
            "username": fields.username,
            "password": fields.password,
        }),
    }
}

/// `POST /awareness` body for the proactivity level.
pub fn proactivity_body(level: Proactivity) -> Value {
    json!({ "proactive": level.as_str() })
}

/// `POST /settings` body for "Use the demo for now": base URL only. The
/// server applies a partial settings update.
pub fn demo_body() -> Value {
    json!({ "base_url": "demo" })
}

/// `POST /settings` body for the whole form. Empty `api_key` keeps the
/// stored key: the server only stores a key when the value is non-empty.
pub fn settings_body(form: &SettingsForm) -> Value {
    json!({
        "base_url": form.base_url,
        "model": form.model,
        "fallback_models": form.fallback_models,
        "fast_model": form.fast_model,
        "provider": form.provider,
        "cli_command": form.cli_command,
        "cli_model": form.cli_model,
        "api_key": form.api_key,
        "mode": form.mode.as_str(),
        "daemon_mode": form.daemon_mode,
        "auto_memory": form.auto_memory,
        "neural_memory": form.neural_memory,
        "browser_visible": form.browser_visible,
        "workspace": form.workspace,
    })
}

/// `POST /persona` body.
pub fn persona_body(name: &str, text: &str) -> Value {
    json!({ "text": text, "name": name })
}

/// `POST /tools/{name}` body.
pub fn tool_body(setting: ToolSetting) -> Value {
    json!({ "setting": setting.as_str() })
}

/// `POST /telegram` body.
pub fn telegram_body(token: &str) -> Value {
    json!({ "token": token })
}

/// The note under the API key field. Pure function of the settings the
/// server returns. Matches the web page text exactly.
pub fn key_note(base_url: &str, key_source: KeySource, secret_storage: &str) -> String {
    if base_url == "demo" {
        return "Demo mode needs no key.".to_string();
    }
    match key_source {
        KeySource::Environment => {
            "A key is already set through an environment variable on this computer."
                .to_string()
        }
        KeySource::Saved => format!(
            "A key is saved in {secret_storage}. It is never shown again or sent anywhere except to the service above."
        ),
        KeySource::None => format!(
            "Kept in {secret_storage}. Not needed for models running locally, like Ollama."
        ),
    }
}

/// The placeholder for the API key field. Matches the web page.
pub fn key_placeholder(key_source: KeySource) -> &'static str {
    match key_source {
        KeySource::None => "Paste your key",
        _ => "Leave empty to keep the current key",
    }
}

/// The sandbox note in Settings. Matches the web page text.
pub fn sandbox_note(sandbox: &str, os: &str) -> String {
    if !sandbox.is_empty() && sandbox != "none" {
        "Commands run inside a sandbox on this computer: they can change files only in your working folder, and cannot read Heron's own data or your key folders, whatever the command says.".to_string()
    } else if os == "windows" {
        "Commands are not sandboxed on Windows yet, so the approval settings above are the safeguard.".to_string()
    } else {
        "No sandbox is available on this computer (on Linux, install bubblewrap), so the approval settings above are the safeguard.".to_string()
    }
}

/// The status line for the Telegram section. Matches the web page text.
pub fn telegram_status(t: &TelegramSnapshot) -> String {
    if t.paired {
        "Paired with your phone. Only that chat is answered. Replies to permission questions can be given there, except for risky actions, which always need you here. Send /stop from your phone to halt Heron.".to_string()
    } else if t.configured {
        match &t.code {
            Some(code) => format!(
                "Open your bot in Telegram and send it this code to pair your phone: {code}. Until then it answers nobody."
            ),
            None => "Configured, but no pairing code is available. Try reconnecting."
                .to_string(),
        }
    } else {
        "Talk to Heron through Telegram while this computer is on. In Telegram, message @BotFather, create a bot, and paste its token here. Your messages pass through Telegram's servers.".to_string()
    }
}

/// The result line after saving the about form. Matches the web page rule:
/// `Saved to memory` when anything was saved or the notes field is
/// non-blank, else `Fill in at least one field first`.
pub fn about_saved_message(saved: &[String], notes: &str) -> &'static str {
    if !saved.is_empty() || !notes.trim().is_empty() {
        "Saved to memory"
    } else {
        "Fill in at least one field first"
    }
}

/// True when the persona text is over the server limit.
pub fn persona_over_limit(text: &str) -> bool {
    text.chars().count() > MAX_PERSONA_CHARS
}

/// The email host auto-fill from the username domain. Matches the web
/// page table. Returns None when the domain is unknown or the host field
/// is already filled.
pub fn mail_host_for_domain(domain: &str) -> Option<&'static str> {
    match domain.to_ascii_lowercase().as_str() {
        "gmail.com" | "googlemail.com" => Some("imap.gmail.com"),
        "icloud.com" | "me.com" => Some("imap.mail.me.com"),
        "fastmail.com" => Some("imap.fastmail.com"),
        "yahoo.com" => Some("imap.mail.yahoo.com"),
        _ => None,
    }
}

/// Onboarding view state. Holds the form fields, the last snapshots, and
/// the latest message or error. Every action goes through the bridge.
pub struct OnboardingState<B: SetupBridge> {
    pub bridge: B,
    pub state: Option<StateSnapshot>,
    pub sources: Option<SourcesSnapshot>,
    pub about: AboutForm,
    pub fields: SourceFields,
    pub proactivity: Proactivity,
    pub current_step: usize,
    pub message: String,
    pub error: Option<String>,
}

impl<B: SetupBridge> OnboardingState<B> {
    pub fn new(bridge: B) -> OnboardingState<B> {
        OnboardingState {
            bridge,
            state: None,
            sources: None,
            about: AboutForm::default(),
            fields: SourceFields::default(),
            proactivity: Proactivity::Off,
            current_step: 0,
            message: String::new(),
            error: None,
        }
    }

    /// Loads state, sources, and the proactivity level. The web page
    /// loads sources and awareness together.
    pub async fn refresh(&mut self) -> Result<(), String> {
        let state = self.bridge.load_state().await?;
        let sources = self.bridge.load_sources().await?;
        self.proactivity = self.bridge.load_proactivity().await?;
        self.state = Some(state);
        self.sources = Some(sources);
        Ok(())
    }

    /// Step 1 is done when a model is connected and it is not demo mode.
    pub fn model_ready(&self) -> bool {
        match &self.sources {
            Some(s) => s.has_model && !s.demo,
            None => false,
        }
    }

    /// Demo mode counts as done for step 1: the user can proceed without
    /// a model.
    pub fn demo(&self) -> bool {
        self.sources.as_ref().map(|s| s.demo).unwrap_or(false)
    }

    /// The step 1 status line. Matches the web page text.
    pub fn model_status(&self) -> String {
        match &self.sources {
            Some(s) if s.has_model && !s.demo => format!("Connected to {}.", s.model),
            Some(s) if s.demo => "You are in demo mode: Heron can act, but it cannot think or learn yet. Connect a model whenever you are ready.".to_string(),
            Some(_) => "Heron needs an AI model to think with. Add an API key, use a model running on this computer, or look around in demo mode first.".to_string(),
            None => String::new(),
        }
    }

    /// Step 2 is done when Heron saved at least one thing from the form.
    pub fn about_done(&self) -> bool {
        self.sources
            .as_ref()
            .map(|s| s.about_saved > 0)
            .unwrap_or(false)
    }

    /// Step 3 is done when any source is connected.
    pub fn world_done(&self) -> bool {
        self.sources
            .as_ref()
            .map(|s| s.sources.iter().any(|x| x.connected))
            .unwrap_or(false)
    }

    /// The finish button label. Matches the web page.
    pub fn finish_label(&self) -> &'static str {
        match &self.sources {
            Some(s) if s.onboarded => "Back to the conversation",
            _ => "Finish setup",
        }
    }

    /// Whether the step's done condition holds, for the indicator.
    pub fn step_done(&self, step: usize) -> bool {
        match step {
            0 => self.model_ready() || self.demo(),
            1 => self.about_done(),
            2 => self.world_done(),
            3 => self.sources.as_ref().map(|s| s.onboarded).unwrap_or(false),
            _ => false,
        }
    }

    /// Fraction of steps done, for the progress bar. 0.0 to 1.0.
    pub fn progress(&self) -> f32 {
        (0..4).filter(|s| self.step_done(*s)).count() as f32 / 4.0
    }

    /// Whether the user may leave this step forward. Only the model step
    /// gates: about and world are both skippable in the web UI.
    pub fn can_advance_from(&self, step: usize) -> bool {
        match step {
            0 => self.model_ready() || self.demo(),
            _ => true,
        }
    }

    /// Moves to another step. Back is always allowed. Forward requires
    /// every step in between to be passable. Out of range is an error.
    pub fn goto_step(&mut self, step: usize) -> Result<(), String> {
        if step > 3 {
            return Err("There is no step after the fourth.".to_string());
        }
        if step > self.current_step {
            for s in self.current_step..step {
                if !self.can_advance_from(s) {
                    return Err("Connect a model first, or use the demo.".to_string());
                }
            }
        }
        self.current_step = step;
        Ok(())
    }

    /// Feeds the folder picker result into the documents form. The picker
    /// itself is not this file's job: the driver opens
    /// `rfd::AsyncFileDialog` inside `slint::spawn_local` when the view
    /// fires `pick_folder_requested`, and calls this with the picked path.
    /// A cancelled picker calls nothing, and the previous path stays.
    pub fn set_document_path(&mut self, path: String) {
        self.fields.path = path;
    }

    /// Saves the about form. Sets `message` to the result line. The
    /// button is "Save and continue", so a real save advances from step
    /// 2 to step 3. An empty save stays put and shows the message.
    pub async fn save_about(&mut self) -> Result<(), String> {
        self.error = None;
        match self.bridge.save_about(&self.about).await {
            Ok(saved) => {
                self.message = about_saved_message(&saved, &self.about.notes).to_string();
                self.refresh().await?;
                if self.message == "Saved to memory" && self.current_step == 1 {
                    self.current_step = 2;
                }
                Ok(())
            }
            Err(e) => {
                self.error = Some(e.clone());
                Err(e)
            }
        }
    }

    /// Connects one source with the fields held in `self.fields`.
    pub async fn connect(&mut self, kind: SourceKind) -> Result<ConnectResult, String> {
        self.error = None;
        match self.bridge.connect_source(kind, &self.fields).await {
            Ok(r) => {
                self.refresh().await?;
                Ok(r)
            }
            Err(e) => {
                self.error = Some(e.clone());
                Err(e)
            }
        }
    }

    /// Asks Heron to learn from a connected source.
    pub async fn learn(&mut self, kind: SourceKind) -> Result<Vec<String>, String> {
        self.error = None;
        match self.bridge.learn_source(kind).await {
            Ok(learned) => {
                self.refresh().await?;
                Ok(learned)
            }
            Err(e) => {
                self.error = Some(e.clone());
                Err(e)
            }
        }
    }

    /// Disconnects a source and forgets what came from it.
    pub async fn disconnect(&mut self, kind: SourceKind) -> Result<DisconnectResult, String> {
        self.error = None;
        match self.bridge.disconnect_source(kind).await {
            Ok(r) => {
                self.refresh().await?;
                Ok(r)
            }
            Err(e) => {
                self.error = Some(e.clone());
                Err(e)
            }
        }
    }

    /// Sets the proactivity level. The takeover choice needs a confirm
    /// dialog in the view before this is called.
    pub async fn set_proactivity(&mut self, level: Proactivity) -> Result<(), String> {
        self.error = None;
        match self.bridge.set_proactivity(level).await {
            Ok(()) => {
                self.proactivity = level;
                Ok(())
            }
            Err(e) => {
                self.error = Some(e.clone());
                Err(e)
            }
        }
    }

    /// The `Use the demo for now` button: the bridge posts only
    /// `{"base_url": "demo"}`. The server applies a partial settings
    /// update, so no full form is needed. Then reloads.
    pub async fn use_demo(&mut self) -> Result<(), String> {
        self.error = None;
        match self.bridge.use_demo().await {
            Ok(()) => {
                self.refresh().await?;
                Ok(())
            }
            Err(e) => {
                self.error = Some(e.clone());
                Err(e)
            }
        }
    }

    /// Marks onboarding done. The view then starts a new conversation.
    pub async fn finish(&mut self) -> Result<(), String> {
        self.error = None;
        match self.bridge.finish_onboarding().await {
            Ok(()) => {
                self.refresh().await?;
                Ok(())
            }
            Err(e) => {
                self.error = Some(e.clone());
                Err(e)
            }
        }
    }
}

/// Settings view state. Holds the form, the persona editor state, the
/// tool table, the Telegram state, and the latest message or error.
pub struct SettingsState<B: SetupBridge> {
    pub bridge: B,
    pub settings: Option<SettingsSnapshot>,
    pub form: SettingsForm,
    pub persona: Option<PersonaSnapshot>,
    pub persona_text: String,
    pub persona_name: String,
    pub tools: Vec<ToolRow>,
    pub telegram: Option<TelegramSnapshot>,
    pub telegram_token: String,
    pub message: String,
    pub error: Option<String>,
    pub backup_message: String,
    pub mcp_note: String,
}

impl<B: SetupBridge> SettingsState<B> {
    pub fn new(bridge: B, form: SettingsForm) -> SettingsState<B> {
        SettingsState {
            bridge,
            settings: None,
            form,
            persona: None,
            persona_text: String::new(),
            persona_name: String::new(),
            tools: Vec::new(),
            telegram: None,
            telegram_token: String::new(),
            message: String::new(),
            error: None,
            backup_message: String::new(),
            mcp_note: String::new(),
        }
    }

    /// Loads settings, persona, tools, and Telegram state. The web page
    /// loads settings first, then persona and tools in parallel.
    pub async fn refresh(&mut self) -> Result<(), String> {
        let settings = self.bridge.load_settings().await?;
        self.form = SettingsForm::from_snapshot(&settings);
        self.settings = Some(settings);
        self.persona = Some(self.bridge.load_persona().await?);
        if let Some(p) = &self.persona {
            self.persona_text = p.text.clone();
            self.persona_name = p.name.clone();
        }
        self.tools = self.bridge.load_tools().await?;
        self.telegram = Some(self.bridge.load_telegram().await?);
        Ok(())
    }

    /// The provider select labels: `API key (endpoint above)` first, then
    /// one `{name} — {auth}` label per CLI provider.
    pub fn provider_labels(&self) -> Vec<String> {
        let mut labels = vec!["API key (endpoint above)".to_string()];
        if let Some(s) = &self.settings {
            labels.extend(s.providers.iter().map(|p| p.label.clone()));
        }
        labels
    }

    /// Index of the current provider in `provider_labels`. 0 is `http`.
    pub fn provider_index(&self) -> usize {
        match &self.settings {
            Some(s) => s
                .providers
                .iter()
                .position(|p| p.id == s.provider)
                .map(|i| i + 1)
                .unwrap_or(0),
            None => 0,
        }
    }

    /// The provider id for a select index. 0 is `http`.
    pub fn provider_id_for_index(&self, index: usize) -> String {
        if index == 0 {
            return "http".to_string();
        }
        match &self.settings {
            Some(s) => s
                .providers
                .get(index - 1)
                .map(|p| p.id.clone())
                .unwrap_or_else(|| "http".to_string()),
            None => "http".to_string(),
        }
    }

    /// The key note line, from the loaded settings.
    pub fn key_note(&self) -> String {
        match &self.settings {
            Some(s) => key_note(&s.base_url, s.key_source, &s.secret_storage),
            None => String::new(),
        }
    }

    /// Applies a preset to the form. It does not save.
    pub fn apply_preset(&mut self, preset: Preset) {
        self.form.apply_preset(preset);
    }

    /// Saves the whole settings form, then reloads settings.
    pub async fn save(&mut self) -> Result<(), String> {
        self.error = None;
        match self.bridge.save_settings(&self.form).await {
            Ok(settings) => {
                self.form = SettingsForm::from_snapshot(&settings);
                self.settings = Some(settings);
                self.message = "Settings saved".to_string();
                Ok(())
            }
            Err(e) => {
                self.error = Some(e.clone());
                Err(e)
            }
        }
    }

    /// Saves the persona text and name. The server enforces the limits.
    pub async fn save_persona(&mut self) -> Result<(), String> {
        self.error = None;
        if persona_over_limit(&self.persona_text) {
            let e = format!(
                "Keep it under {MAX_PERSONA_CHARS} characters: a long character sheet crowds out the actual work."
            );
            self.error = Some(e.clone());
            return Err(e);
        }
        match self
            .bridge
            .save_persona(&self.persona_name, &self.persona_text)
            .await
        {
            Ok(p) => {
                self.persona = Some(p);
                self.message = "Personality saved".to_string();
                Ok(())
            }
            Err(e) => {
                self.error = Some(e.clone());
                Err(e)
            }
        }
    }

    /// Resets the persona to the original. The view confirms first.
    pub async fn reset_persona(&mut self) -> Result<(), String> {
        self.error = None;
        match self.bridge.reset_persona().await {
            Ok(p) => {
                self.persona_text = p.text.clone();
                self.persona_name = p.name.clone();
                self.persona = Some(p);
                self.message = "Back to the original".to_string();
                Ok(())
            }
            Err(e) => {
                self.error = Some(e.clone());
                Err(e)
            }
        }
    }

    /// Changes one tool permission immediately. The server persists it.
    pub async fn set_tool(&mut self, name: &str, setting: ToolSetting) -> Result<(), String> {
        self.error = None;
        match self.bridge.set_tool_setting(name, setting).await {
            Ok(back) => {
                if let Some(row) = self.tools.iter_mut().find(|r| r.name == name) {
                    row.setting = back;
                }
                Ok(())
            }
            Err(e) => {
                self.error = Some(e.clone());
                Err(e)
            }
        }
    }

    /// Accepts a changed MCP app. The server note says a restart is needed.
    pub async fn accept_mcp(&mut self, name: &str) -> Result<(), String> {
        self.error = None;
        match self.bridge.accept_mcp(name).await {
            Ok(note) => {
                self.mcp_note = format!("{name}: accepted. {note}");
                Ok(())
            }
            Err(e) => {
                self.error = Some(e.clone());
                Err(e)
            }
        }
    }

    /// Connects Telegram with the token in `telegram_token`, then clears it.
    pub async fn connect_telegram(&mut self) -> Result<(), String> {
        self.error = None;
        let token = self.telegram_token.clone();
        match self.bridge.connect_telegram(&token).await {
            Ok(t) => {
                self.telegram_token.clear();
                self.telegram = Some(t);
                Ok(())
            }
            Err(e) => {
                self.error = Some(e.clone());
                Err(e)
            }
        }
    }

    /// Disconnects the phone. The view confirms first. The server response
    /// carries no note, so the view shows its own disconnected text.
    pub async fn disconnect_telegram(&mut self) -> Result<(), String> {
        self.error = None;
        match self.bridge.disconnect_telegram().await {
            Ok(t) => {
                self.telegram = Some(t);
                Ok(())
            }
            Err(e) => {
                self.error = Some(e.clone());
                Err(e)
            }
        }
    }

    /// Saves a copy of the data. Returns the full path; the view shows
    /// only the file name.
    pub async fn backup(&mut self) -> Result<String, String> {
        self.error = None;
        match self.bridge.make_backup().await {
            Ok(path) => {
                let file = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string();
                self.backup_message = format!("Saved in the backups folder as {file}");
                Ok(path)
            }
            Err(e) => {
                self.error = Some(e.clone());
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};

    struct NopWaker;

    impl Wake for NopWaker {
        fn wake(self: Arc<Self>) {}
    }

    /// Drives a future to completion on the calling thread. Only for
    /// tests: the fakes never park, so the first poll always resolves.
    /// A future that parks forever would spin here, so never use this
    /// outside tests.
    fn block_on<F: Future>(f: F) -> F::Output {
        let waker = Waker::from(Arc::new(NopWaker));
        let mut cx = Context::from_waker(&waker);
        let mut f = Box::pin(f);
        loop {
            match f.as_mut().poll(&mut cx) {
                Poll::Ready(v) => return v,
                Poll::Pending => std::thread::yield_now(),
            }
        }
    }

    struct FakeBridge {
        state: StateSnapshot,
        sources: SourcesSnapshot,
        settings: SettingsSnapshot,
        persona: PersonaSnapshot,
        tools: Vec<ToolRow>,
        telegram: TelegramSnapshot,
    }

    fn fake() -> FakeBridge {
        FakeBridge {
            state: StateSnapshot {
                has_key: true,
                demo: false,
                onboarded: false,
                model: "gpt-5.5".to_string(),
                sandbox: "none".to_string(),
                os: "linux".to_string(),
            },
            sources: SourcesSnapshot {
                onboarded: false,
                demo: false,
                has_model: true,
                model: "gpt-5.5".to_string(),
                about_saved: 2,
                about_indexed: 2,
                about_learned: 1,
                sources: vec![
                    SourceStatus {
                        kind: SourceKind::Computer,
                        connected: true,
                        indexed: 12,
                        learned: 3,
                        folders: vec![],
                        hosts: vec![],
                        address: None,
                    },
                    SourceStatus {
                        kind: SourceKind::Email,
                        connected: false,
                        indexed: 0,
                        learned: 0,
                        folders: vec![],
                        hosts: vec![],
                        address: None,
                    },
                ],
            },
            settings: SettingsSnapshot {
                name: "Heron".to_string(),
                base_url: "https://openrouter.ai/api/v1".to_string(),
                model: "anthropic/claude-sonnet-5.5".to_string(),
                fast_model: String::new(),
                fallback_models: "gpt-5.5".to_string(),
                key_source: KeySource::Saved,
                secret_storage: "the system keychain".to_string(),
                provider: "http".to_string(),
                providers: vec![ProviderItem {
                    id: "claude-cli".to_string(),
                    label: "Claude Code (subscription) — Claude Pro/Max subscription".to_string(),
                }],
                cli_command: String::new(),
                cli_model: String::new(),
                mode: Mode::Ask,
                daemon_mode: "auto".to_string(),
                auto_memory: true,
                neural_memory: false,
                neural_available: true,
                browser: Some("Google Chrome".to_string()),
                browser_visible: false,
                workspace: "/home/user".to_string(),
                home: "/home/user/.heron".to_string(),
                mcp: vec![],
            },
            persona: PersonaSnapshot {
                text: "Be helpful.".to_string(),
                is_default: false,
                name: "Heron".to_string(),
            },
            tools: vec![ToolRow {
                name: "shell".to_string(),
                about: "Ran a command".to_string(),
                setting: ToolSetting::Ask,
            }],
            telegram: TelegramSnapshot {
                configured: false,
                paired: false,
                code: None,
            },
        }
    }

    impl SetupBridge for FakeBridge {
        async fn load_state(&self) -> Result<StateSnapshot, String> {
            Ok(self.state.clone())
        }
        async fn load_sources(&self) -> Result<SourcesSnapshot, String> {
            Ok(self.sources.clone())
        }
        async fn connect_source(
            &self,
            _kind: SourceKind,
            _fields: &SourceFields,
        ) -> Result<ConnectResult, String> {
            Ok(ConnectResult {
                indexed: Some(5),
                ..Default::default()
            })
        }
        async fn learn_source(&self, _kind: SourceKind) -> Result<Vec<String>, String> {
            Ok(vec!["Learned one thing.".to_string()])
        }
        async fn disconnect_source(&self, _kind: SourceKind) -> Result<DisconnectResult, String> {
            Ok(DisconnectResult {
                removed_items: 3,
                removed_memories: 1,
            })
        }
        async fn save_about(&self, form: &AboutForm) -> Result<Vec<String>, String> {
            if form.is_empty() {
                Ok(vec![])
            } else {
                Ok(vec!["You are a designer.".to_string()])
            }
        }
        async fn load_proactivity(&self) -> Result<Proactivity, String> {
            Ok(Proactivity::Suggest)
        }
        async fn set_proactivity(&self, _level: Proactivity) -> Result<(), String> {
            Ok(())
        }
        async fn use_demo(&self) -> Result<(), String> {
            Ok(())
        }
        async fn finish_onboarding(&self) -> Result<(), String> {
            Ok(())
        }
        async fn load_settings(&self) -> Result<SettingsSnapshot, String> {
            Ok(self.settings.clone())
        }
        async fn save_settings(&self, _form: &SettingsForm) -> Result<SettingsSnapshot, String> {
            Ok(self.settings.clone())
        }
        async fn load_persona(&self) -> Result<PersonaSnapshot, String> {
            Ok(self.persona.clone())
        }
        async fn save_persona(&self, name: &str, text: &str) -> Result<PersonaSnapshot, String> {
            Ok(PersonaSnapshot {
                text: text.to_string(),
                is_default: false,
                name: name.to_string(),
            })
        }
        async fn reset_persona(&self) -> Result<PersonaSnapshot, String> {
            Ok(PersonaSnapshot {
                text: "Default.".to_string(),
                is_default: true,
                name: "Heron".to_string(),
            })
        }
        async fn load_tools(&self) -> Result<Vec<ToolRow>, String> {
            Ok(self.tools.clone())
        }
        async fn set_tool_setting(
            &self,
            _name: &str,
            setting: ToolSetting,
        ) -> Result<ToolSetting, String> {
            Ok(setting)
        }
        async fn accept_mcp(&self, _name: &str) -> Result<String, String> {
            Ok("Restart Heron for the change to take effect.".to_string())
        }
        async fn load_telegram(&self) -> Result<TelegramSnapshot, String> {
            Ok(self.telegram.clone())
        }
        async fn connect_telegram(&self, _token: &str) -> Result<TelegramSnapshot, String> {
            Ok(TelegramSnapshot {
                configured: true,
                paired: false,
                code: Some("123456".to_string()),
            })
        }
        async fn disconnect_telegram(&self) -> Result<TelegramSnapshot, String> {
            Ok(TelegramSnapshot {
                configured: false,
                paired: false,
                code: None,
            })
        }
        async fn make_backup(&self) -> Result<String, String> {
            Ok("/home/user/.heron/backups/heron-2026-11-13.db".to_string())
        }
    }

    #[test]
    fn onboarding_ready_states_match_the_web_page() {
        let mut view = OnboardingState::new(fake());
        block_on(view.refresh()).unwrap();
        assert!(view.model_ready());
        assert!(view.about_done());
        assert!(view.world_done());
        assert_eq!(view.model_status(), "Connected to gpt-5.5.");
        assert_eq!(view.finish_label(), "Finish setup");
    }

    #[test]
    fn about_save_message_follows_the_web_rule() {
        let mut view = OnboardingState::new(fake());
        view.about.name = "Fernando".to_string();
        block_on(view.save_about()).unwrap();
        assert_eq!(view.message, "Saved to memory");
        let empty = AboutForm::default();
        assert!(empty.is_empty());
        assert_eq!(
            about_saved_message(&[], ""),
            "Fill in at least one field first"
        );
        assert_eq!(about_saved_message(&[], "some notes"), "Saved to memory");
    }

    #[test]
    fn key_note_variants_match_the_web_page() {
        assert_eq!(
            key_note("demo", KeySource::None, "the keychain"),
            "Demo mode needs no key."
        );
        assert_eq!(
            key_note("https://x", KeySource::Environment, "the keychain"),
            "A key is already set through an environment variable on this computer."
        );
        assert_eq!(
            key_note("https://x", KeySource::Saved, "the system keychain"),
            "A key is saved in the system keychain. It is never shown again or sent anywhere except to the service above."
        );
        assert_eq!(key_placeholder(KeySource::None), "Paste your key");
        assert_eq!(
            key_placeholder(KeySource::Saved),
            "Leave empty to keep the current key"
        );
    }

    #[test]
    fn presets_fill_url_and_model_only() {
        assert_eq!(
            Preset::OpenRouter.base_url(),
            "https://openrouter.ai/api/v1"
        );
        assert_eq!(Preset::OpenRouter.model(), "anthropic/claude-sonnet-5.5");
        assert_eq!(Preset::Ollama.base_url(), "http://localhost:11434/v1");
        assert_eq!(Preset::Demo.model(), "demo");
        let mut form = SettingsForm::from_snapshot(&fake().settings);
        form.apply_preset(Preset::Demo);
        assert_eq!(form.base_url, "demo");
        assert_eq!(form.model, "demo");
        assert_eq!(form.provider, "http");
    }

    #[test]
    fn provider_index_round_trips_through_http_and_cli() {
        let settings = fake().settings;
        let mut view = SettingsState::new(fake(), SettingsForm::from_snapshot(&settings));
        block_on(view.refresh()).unwrap();
        assert_eq!(
            view.provider_labels(),
            vec![
                "API key (endpoint above)".to_string(),
                "Claude Code (subscription) — Claude Pro/Max subscription".to_string()
            ]
        );
        assert_eq!(view.provider_index(), 0);
        assert_eq!(view.provider_id_for_index(1), "claude-cli");
        assert_eq!(view.provider_id_for_index(0), "http");
    }

    #[test]
    fn persona_limit_is_enforced_before_the_bridge() {
        let mut view = SettingsState::new(fake(), SettingsForm::from_snapshot(&fake().settings));
        block_on(view.refresh()).unwrap();
        view.persona_text = "x".repeat(MAX_PERSONA_CHARS + 1);
        assert!(persona_over_limit(&view.persona_text));
        assert!(block_on(view.save_persona()).is_err());
        view.persona_text = "Short.".to_string();
        block_on(view.save_persona()).unwrap();
        assert_eq!(view.message, "Personality saved");
        block_on(view.reset_persona()).unwrap();
        assert_eq!(view.message, "Back to the original");
    }

    #[test]
    fn backup_message_shows_the_file_name_only() {
        let mut view = SettingsState::new(fake(), SettingsForm::from_snapshot(&fake().settings));
        let path = block_on(view.backup()).unwrap();
        assert_eq!(path, "/home/user/.heron/backups/heron-2026-11-13.db");
        assert_eq!(
            view.backup_message,
            "Saved in the backups folder as heron-2026-11-13.db"
        );
    }

    #[test]
    fn mail_host_table_matches_the_web_page() {
        assert_eq!(mail_host_for_domain("gmail.com"), Some("imap.gmail.com"));
        assert_eq!(mail_host_for_domain("me.com"), Some("imap.mail.me.com"));
        assert_eq!(mail_host_for_domain("outlook.com"), None);
    }

    #[test]
    fn tool_setting_index_matches_the_select_order() {
        assert_eq!(ToolSetting::Ask.index(), 0);
        assert_eq!(ToolSetting::Allow.index(), 1);
        assert_eq!(ToolSetting::Never.index(), 2);
    }

    #[test]
    fn step_flow_gates_only_on_the_model_step() {
        let mut view = OnboardingState::new(fake());
        block_on(view.refresh()).unwrap();
        assert_eq!(view.current_step, 0);
        assert!(view.step_done(0));
        assert!(view.step_done(1));
        assert!(view.step_done(2));
        assert!(!view.step_done(3));
        assert_eq!(view.progress(), 0.75);
        view.goto_step(3).unwrap();
        assert_eq!(view.current_step, 3);
        view.goto_step(0).unwrap();
        assert_eq!(view.current_step, 0);
        assert!(view.goto_step(4).is_err());
    }

    #[test]
    fn step_flow_blocks_without_a_model() {
        let mut bridge = fake();
        bridge.sources.has_model = false;
        bridge.sources.demo = false;
        let mut view = OnboardingState::new(bridge);
        block_on(view.refresh()).unwrap();
        assert!(!view.step_done(0));
        assert_eq!(view.progress(), 0.5);
        assert!(view.goto_step(1).is_err());
        assert_eq!(view.current_step, 0);
    }

    #[test]
    fn save_and_continue_advances_only_on_a_real_save() {
        let mut view = OnboardingState::new(fake());
        block_on(view.refresh()).unwrap();
        view.current_step = 1;
        block_on(view.save_about()).unwrap();
        assert_eq!(view.message, "Fill in at least one field first");
        assert_eq!(view.current_step, 1);
        view.about.name = "Fernando".to_string();
        block_on(view.save_about()).unwrap();
        assert_eq!(view.message, "Saved to memory");
        assert_eq!(view.current_step, 2);
    }

    #[test]
    fn folder_picker_result_feeds_the_documents_form() {
        let mut view = OnboardingState::new(fake());
        assert_eq!(view.fields.path, "");
        view.set_document_path("/home/user/Documents/Notes".to_string());
        assert_eq!(view.fields.path, "/home/user/Documents/Notes");
        block_on(view.connect(SourceKind::Documents)).unwrap();
    }

    #[test]
    fn parse_state_reads_the_server_shape() {
        let v = json!({"name":"Heron","version":"0.2.0","model":"gpt-5.5","mode":"ask","workspace":"/home/u","has_key":true,"demo":false,"memories":3,"unread":0,"paused":false,"waiting":0,"onboarded":true,"suggestions":0,"mcp":[],"usage":{},"tools":5,"os":"linux","sandbox":"none"});
        let s = parse_state(&v);
        assert_eq!(s.model, "gpt-5.5");
        assert!(s.has_key);
        assert!(!s.demo);
        assert!(s.onboarded);
        assert_eq!(s.os, "linux");
    }

    #[test]
    fn parse_sources_reads_counts_folders_hosts_and_address() {
        let v = json!({"onboarded":false,"demo":false,"has_model":true,"model":"gpt-5.5",
            "about":{"saved":2,"indexed":2,"learned":1},
            "computer":{"connected":true,"indexed":12,"learned":3},
            "documents":{"connected":true,"folders":["/home/u/Documents"],"indexed":4,"learned":1},
            "calendar":{"connected":false,"hosts":[],"indexed":0,"learned":0},
            "email":{"connected":true,"address":"me@example.com","indexed":7,"learned":0}});
        let s = parse_sources(&v);
        assert_eq!(s.about_saved, 2);
        assert_eq!(s.about_indexed, 2);
        assert_eq!(s.sources.len(), 4);
        assert_eq!(s.sources[0].kind, SourceKind::Computer);
        assert_eq!(s.sources[1].folders, vec!["/home/u/Documents".to_string()]);
        assert_eq!(s.sources[3].address.as_deref(), Some("me@example.com"));
        assert_eq!(s.sources[3].indexed, 7);
    }

    #[test]
    fn parse_settings_builds_provider_labels_and_mcp_lines() {
        let v = json!({"name":"Heron","base_url":"https://openrouter.ai/api/v1","model":"anthropic/claude-sonnet-5.5","fast_model":"","fallback_models":"gpt-5.5","key_source":"saved","provider":"http","cli_command":"","cli_model":"","providers":[{"id":"claude-cli","name":"Claude Code","auth":"subscription","cost":"","login":"","experimental":false}],"mode":"ask","daemon_mode":"auto","auto_memory":true,"neural_memory":false,"neural_available":true,"browser":"Google Chrome","browser_visible":false,"workspace":"/home/u","home":"/home/u/.heron","secret_storage":"the keychain","mcp":["slack:3 tools","notion:2 tools"],"mcp_changed":["slack"]});
        let s = parse_settings(&v);
        assert_eq!(s.providers.len(), 1);
        assert_eq!(s.providers[0].label, "Claude Code — subscription");
        assert_eq!(s.mcp.len(), 2);
        assert_eq!(s.mcp[0].name, "slack");
        assert!(s.mcp[0].changed);
        assert_eq!(s.mcp[1].name, "notion");
        assert!(!s.mcp[1].changed);
        assert_eq!(s.browser.as_deref(), Some("Google Chrome"));
        assert_eq!(s.fallback_models, "gpt-5.5");
    }

    #[test]
    fn parse_settings_tolerates_a_missing_browser() {
        let v = json!({"name":"Heron","base_url":"demo","model":"demo","fast_model":"","fallback_models":"","key_source":"none","provider":"http","cli_command":"","cli_model":"","providers":[],"mode":"ask","daemon_mode":"auto","auto_memory":false,"neural_memory":false,"neural_available":false,"browser":null,"browser_visible":false,"workspace":"/home/u","home":"/home/u/.heron","secret_storage":"the keychain","mcp":[],"mcp_changed":[]});
        let s = parse_settings(&v);
        assert_eq!(s.browser, None);
        assert_eq!(s.key_source, KeySource::None);
    }

    #[test]
    fn parse_tools_keeps_only_rows_the_view_shows() {
        let v = json!([{"name":"shell","about":"Ran a command","setting":"ask","always_safe":false},{"name":"read_file","about":"Read a file","setting":"allow","always_safe":true}]);
        let rows = parse_tools(&v);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "shell");
        assert_eq!(rows[0].setting, ToolSetting::Ask);
    }

    #[test]
    fn parse_learn_is_an_array_not_a_count() {
        let v = json!({"learned":["You use metric units.","You run a design studio."]});
        assert_eq!(parse_learned(&v).len(), 2);
        let v = json!({"indexed": 12});
        assert_eq!(parse_indexed(&v), 12);
    }

    #[test]
    fn body_builders_match_the_server_shapes() {
        let mut form = AboutForm::default();
        form.name = "Fernando".to_string();
        assert_eq!(about_body(&form)["name"], "Fernando");
        assert_eq!(about_body(&form)["notes"], "");
        let mut fields = SourceFields::default();
        fields.host = "imap.gmail.com".to_string();
        fields.username = "me@example.com".to_string();
        assert_eq!(
            source_body(SourceKind::Email, &fields)["host"],
            "imap.gmail.com"
        );
        assert_eq!(source_body(SourceKind::Computer, &fields), json!({}));
        assert_eq!(source_body(SourceKind::Documents, &fields)["path"], "");
        assert_eq!(demo_body()["base_url"], "demo");
        assert_eq!(
            proactivity_body(Proactivity::Takeover)["proactive"],
            "takeover"
        );
        assert_eq!(tool_body(ToolSetting::Never)["setting"], "never");
        assert_eq!(telegram_body("1:abc")["token"], "1:abc");
    }
}
