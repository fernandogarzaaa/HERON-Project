//! Setup driver: onboarding and settings, wired to the Shell.
//!
//! The Setup lane owns the state machines (`OnboardingState` and
//! `SettingsState` in `setup.rs`) and the HTTP bridge
//! (`HeronSetupBridge` in `setup_bridge.rs`). This module is the Shell
//! side: it holds the two states beside the window, installs the
//! window callbacks, and copies state fields into the MainWindow
//! properties the views bind to.
//!
//! One pattern per callback, the same as the Chat and Views lanes:
//! read the form from the window on the UI thread, spawn the work on
//! the Bridge runtime, lock the state, call the state method (it
//! refreshes itself), build a plain snapshot while holding the lock,
//! drop the lock, post the snapshot to the UI thread. Nothing blocks
//! the UI thread, and no lock is held across an await that is not the
//! state's own.
//!
//! The snapshots are the only thing that crosses from the task to the
//! UI thread. A `tokio::sync::Mutex` cannot be locked from the UI
//! thread, so the post-back never touches the state directly.
//!
//! Two lane-design gaps, recorded not fixed: the onboarding view has
//! no error line on steps 1, 3 and 4, so a failed proactivity or
//! finish call leaves no visible trace (the error is in
//! `state.error`); and the SourceCard's message line is an error line
//! only, so a successful Learn shows its result as the updated count
//! in the status line, not as the list of learned items the web UI
//! shows.

use super::bridge::Bridge;
use super::setup::{
    key_placeholder, sandbox_note, telegram_status, AboutForm, KeySource, Mode, OnboardingState,
    Preset, Proactivity, SettingsForm, SettingsState, SourceKind, SourceStatus, SourcesSnapshot,
    ToolSetting,
};
use super::setup_bridge::HeronSetupBridge;
use crate::{MainWindow, McpLine, SourceItem, ToolItem};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::sync::Arc;

/// The two Setup states, held beside the window. The driver is built
/// once in `main.rs` and handed to `bind_window`; the callbacks hold
/// their own `Arc` clones, so the driver value itself can be dropped
/// after binding.
pub struct SetupDriver {
    pub onboarding: Arc<tokio::sync::Mutex<OnboardingState<HeronSetupBridge>>>,
    pub settings: Arc<tokio::sync::Mutex<SettingsState<HeronSetupBridge>>>,
}

impl SetupDriver {
    pub fn new(client: super::bridge::HeronClient) -> SetupDriver {
        let onboarding = OnboardingState::new(HeronSetupBridge::new(client.clone()));
        // The form starts empty; the startup refresh overwrites it from
        // the server before the settings view is ever shown.
        let form = SettingsForm {
            base_url: String::new(),
            model: String::new(),
            fallback_models: String::new(),
            fast_model: String::new(),
            provider: "http".to_string(),
            cli_command: String::new(),
            cli_model: String::new(),
            api_key: String::new(),
            mode: Mode::Ask,
            daemon_mode: "readonly".to_string(),
            auto_memory: false,
            neural_memory: false,
            browser_visible: false,
            workspace: String::new(),
        };
        let settings = SettingsState::new(HeronSetupBridge::new(client), form);
        SetupDriver {
            onboarding: Arc::new(tokio::sync::Mutex::new(onboarding)),
            settings: Arc::new(tokio::sync::Mutex::new(settings)),
        }
    }
}

fn model_of<T: Clone + 'static>(rows: Vec<T>) -> ModelRc<T> {
    std::rc::Rc::new(VecModel::from_slice(&rows)).into()
}

fn saturating_i32(n: i64) -> i32 {
    i32::try_from(n).unwrap_or(i32::MAX)
}

/// Plain onboarding display values. Built from the state while the
/// task holds its lock, applied to the window on the UI thread.
struct OnboardingSync {
    current_step: i32,
    progress: f32,
    model_status: String,
    model_ready: bool,
    demo: bool,
    about_name: String,
    about_work: String,
    about_goals: String,
    about_style: String,
    about_people: String,
    about_notes: String,
    about_saved: i32,
    about_message: String,
    sources: Vec<SourceItem>,
    world_done: bool,
    doc_path: String,
    cal_url: String,
    email_host: String,
    email_user: String,
    email_pass: String,
    source_message: String,
    proactivity: String,
    onboarded: bool,
    sandbox_note: String,
}

impl OnboardingSync {
    fn from(st: &OnboardingState<HeronSetupBridge>) -> OnboardingSync {
        let sources = st.sources.as_ref();
        let onboarded = sources
            .map(|s| s.onboarded)
            .unwrap_or_else(|| st.state.as_ref().map(|s| s.onboarded).unwrap_or(false));
        // The view has no error line, so an error replaces the last
        // success message rather than going nowhere.
        let about_message = st.error.clone().unwrap_or_else(|| st.message.clone());
        let sandbox_note = match &st.state {
            Some(s) => sandbox_note(&s.sandbox, &s.os),
            None => String::new(),
        };
        OnboardingSync {
            current_step: st.current_step as i32,
            progress: st.progress(),
            model_status: st.model_status(),
            model_ready: st.model_ready(),
            demo: st.demo(),
            about_name: st.about.name.clone(),
            about_work: st.about.work.clone(),
            about_goals: st.about.goals.clone(),
            about_style: st.about.style.clone(),
            about_people: st.about.people.clone(),
            about_notes: st.about.notes.clone(),
            about_saved: saturating_i32(sources.map(|s| s.about_saved as i64).unwrap_or(0)),
            about_message,
            sources: sources.map(source_items).unwrap_or_default(),
            world_done: st.world_done(),
            doc_path: st.fields.path.clone(),
            cal_url: st.fields.url.clone(),
            email_host: st.fields.host.clone(),
            email_user: st.fields.username.clone(),
            email_pass: st.fields.password.clone(),
            // The SourceCard message line is styled as an error line,
            // so only errors go there. A successful connect or learn
            // shows as the updated status line on its card.
            source_message: st.error.clone().unwrap_or_default(),
            proactivity: st.proactivity.as_str().to_string(),
            onboarded,
            sandbox_note,
        }
    }

    fn apply(&self, window: &MainWindow) {
        window.set_onboarding_current_step(self.current_step);
        window.set_onboarding_progress(self.progress);
        window.set_onboarding_model_status(self.model_status.clone().into());
        window.set_onboarding_model_ready(self.model_ready);
        window.set_onboarding_demo(self.demo);
        window.set_onboarding_about_name(self.about_name.clone().into());
        window.set_onboarding_about_work(self.about_work.clone().into());
        window.set_onboarding_about_goals(self.about_goals.clone().into());
        window.set_onboarding_about_style(self.about_style.clone().into());
        window.set_onboarding_about_people(self.about_people.clone().into());
        window.set_onboarding_about_notes(self.about_notes.clone().into());
        window.set_onboarding_about_saved(self.about_saved);
        window.set_onboarding_about_message(self.about_message.clone().into());
        window.set_onboarding_sources(model_of(self.sources.clone()));
        window.set_onboarding_world_done(self.world_done);
        window.set_onboarding_doc_path(self.doc_path.clone().into());
        window.set_onboarding_cal_url(self.cal_url.clone().into());
        window.set_onboarding_email_host(self.email_host.clone().into());
        window.set_onboarding_email_user(self.email_user.clone().into());
        window.set_onboarding_email_pass(self.email_pass.clone().into());
        window.set_onboarding_source_message(self.source_message.clone().into());
        window.set_onboarding_proactivity(self.proactivity.clone().into());
        window.set_onboarding_onboarded(self.onboarded);
        // The sandbox note lives on the settings view, but its inputs
        // (sandbox, OS) come from GET /state, so this sync owns it.
        window.set_settings_sandbox_note(self.sandbox_note.clone().into());
    }
}

/// Plain settings display values. Same shape as `OnboardingSync`.
/// The sandbox note is not here: its inputs come from GET /state, so
/// `OnboardingSync::apply` sets that window property.
struct SettingsSync {
    base_url: String,
    model: String,
    fallback_models: String,
    fast_model: String,
    provider_labels: Vec<String>,
    provider_index: i32,
    cli_command: String,
    cli_model: String,
    key_note: String,
    key_placeholder: String,
    agent_name: String,
    persona_text: String,
    persona_is_default: bool,
    persona_message: String,
    mode: String,
    daemon_mode: String,
    browser_visible: bool,
    browser_available: bool,
    browser_note: String,
    tools: Vec<ToolItem>,
    mcp_lines: Vec<McpLine>,
    mcp_note: String,
    telegram_configured: bool,
    telegram_paired: bool,
    telegram_status: String,
    telegram_token: String,
    auto_memory: bool,
    neural_memory: bool,
    neural_available: bool,
    workspace: String,
    home: String,
    saved_message: String,
    error_message: String,
    backup_message: String,
}

impl SettingsSync {
    fn from(st: &SettingsState<HeronSetupBridge>) -> SettingsSync {
        let settings = st.settings.as_ref();
        let persona = st.persona.as_ref();
        let browser = settings.and_then(|s| s.browser.clone());
        SettingsSync {
            base_url: st.form.base_url.clone(),
            model: st.form.model.clone(),
            fallback_models: st.form.fallback_models.clone(),
            fast_model: st.form.fast_model.clone(),
            provider_labels: st.provider_labels(),
            provider_index: st.provider_index() as i32,
            cli_command: st.form.cli_command.clone(),
            cli_model: st.form.cli_model.clone(),
            // The key is write-only: empty keeps the stored key, and
            // the input is always cleared on load.
            key_note: st.key_note(),
            key_placeholder: key_placeholder(
                settings.map(|s| s.key_source).unwrap_or(KeySource::None),
            )
            .to_string(),
            agent_name: st.persona_name.clone(),
            persona_text: st.persona_text.clone(),
            persona_is_default: persona.map(|p| p.is_default).unwrap_or(false),
            // The state holds one message for both saves; both lines
            // show it. A persona save reads "Personality saved" in
            // the save row too, which is true, just redundant.
            persona_message: st.message.clone(),
            mode: st.form.mode.as_str().to_string(),
            daemon_mode: st.form.daemon_mode.clone(),
            browser_visible: st.form.browser_visible,
            browser_available: browser.is_some(),
            browser_note: match &browser {
                Some(_) => "Heron can open websites, follow links and fill in forms using its own copy of your browser, kept apart from yours: it has none of your logins unless you sign in there. Typing and pressing buttons ask you first. Normally it works out of sight; switch this on to watch, or to sign in to a site yourself.".to_string(),
                None => "Heron can use websites for you (open pages, fill in forms) when Chrome, Edge, Chromium or Brave is installed. None was found on this computer.".to_string(),
            },
            tools: st
                .tools
                .iter()
                .map(|r| ToolItem {
                    name: r.name.clone().into(),
                    about: r.about.clone().into(),
                    setting: r.setting.index() as i32,
                })
                .collect(),
            mcp_lines: settings
                .map(|s| {
                    s.mcp
                        .iter()
                        .map(|l| McpLine {
                            text: l.text.clone().into(),
                            name: l.name.clone().into(),
                            changed: l.changed,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            mcp_note: st.mcp_note.clone(),
            telegram_configured: st.telegram.as_ref().map(|t| t.configured).unwrap_or(false),
            telegram_paired: st.telegram.as_ref().map(|t| t.paired).unwrap_or(false),
            telegram_status: st
                .telegram
                .as_ref()
                .map(telegram_status)
                .unwrap_or_default(),
            telegram_token: st.telegram_token.clone(),
            auto_memory: st.form.auto_memory,
            neural_memory: st.form.neural_memory,
            neural_available: settings.map(|s| s.neural_available).unwrap_or(false),
            workspace: st.form.workspace.clone(),
            home: settings.map(|s| s.home.clone()).unwrap_or_default(),
            saved_message: st.message.clone(),
            error_message: st.error.clone().unwrap_or_default(),
            backup_message: st.backup_message.clone(),
        }
    }

    fn apply(&self, window: &MainWindow) {
        window.set_settings_base_url(self.base_url.clone().into());
        window.set_settings_model(self.model.clone().into());
        window.set_settings_fallback_models(self.fallback_models.clone().into());
        window.set_settings_fast_model(self.fast_model.clone().into());
        window.set_settings_provider_labels(model_of(
            self.provider_labels
                .iter()
                .map(|s| s.clone().into())
                .collect(),
        ));
        window.set_settings_provider_index(self.provider_index);
        window.set_settings_cli_command(self.cli_command.clone().into());
        window.set_settings_cli_model(self.cli_model.clone().into());
        window.set_settings_api_key("".into());
        window.set_settings_key_note(self.key_note.clone().into());
        window.set_settings_key_placeholder(self.key_placeholder.clone().into());
        window.set_settings_agent_name(self.agent_name.clone().into());
        window.set_settings_persona_text(self.persona_text.clone().into());
        window.set_settings_persona_is_default(self.persona_is_default);
        window.set_settings_persona_message(self.persona_message.clone().into());
        window.set_settings_mode(self.mode.clone().into());
        window.set_settings_daemon_mode(self.daemon_mode.clone().into());
        window.set_settings_browser_visible(self.browser_visible);
        window.set_settings_browser_available(self.browser_available);
        window.set_settings_browser_note(self.browser_note.clone().into());
        window.set_settings_tools(model_of(self.tools.clone()));
        // The choice labels are fixed: index 0 ask, 1 allow, 2 never.
        window.set_settings_tool_settings(model_of(vec![
            "Ask as usual".into(),
            "Never ask".into(),
            "Never allow".into(),
        ]));
        window.set_settings_mcp_lines(model_of(self.mcp_lines.clone()));
        window.set_settings_mcp_note(self.mcp_note.clone().into());
        window.set_settings_telegram_configured(self.telegram_configured);
        window.set_settings_telegram_paired(self.telegram_paired);
        window.set_settings_telegram_status(self.telegram_status.clone().into());
        window.set_settings_telegram_token(self.telegram_token.clone().into());
        window.set_settings_auto_memory(self.auto_memory);
        window.set_settings_neural_memory(self.neural_memory);
        window.set_settings_neural_available(self.neural_available);
        window.set_settings_workspace(self.workspace.clone().into());
        window.set_settings_home(self.home.clone().into());
        window.set_settings_saved_message(self.saved_message.clone().into());
        window.set_settings_error_message(self.error_message.clone().into());
        window.set_settings_backup_message(self.backup_message.clone().into());
    }
}

/// The four source cards, in the web UI's order, from the loaded
/// sources snapshot. Titles and off-texts are verbatim from the
/// `SOURCES` table in `assets/index.html`; the connected status line
/// follows its `sourceCard` function.
fn source_items(snap: &SourcesSnapshot) -> Vec<SourceItem> {
    fn row(snap: &SourcesSnapshot, kind: SourceKind) -> Option<&SourceStatus> {
        snap.sources.iter().find(|s| s.kind == kind)
    }
    let mut out = Vec::with_capacity(4);
    for kind in [
        SourceKind::Computer,
        SourceKind::Documents,
        SourceKind::Calendar,
        SourceKind::Email,
    ] {
        let (title, off): (&str, &str) = match kind {
            SourceKind::Computer => (
                "This computer",
                "Heron looks at your system, time zone, installed tools and the names of your folders. Nothing is opened.",
            ),
            SourceKind::Documents => (
                "Notes and documents",
                "A folder of notes or documents (Markdown and text files). Heron can then find things in them.",
            ),
            SourceKind::Calendar => (
                "Calendar",
                "Paste the private ICS link of your calendar. Google Calendar: Settings → your calendar → “Secret address in iCal format”. iCloud and Outlook offer a similar link when you share or publish a calendar.",
            ),
            SourceKind::Email => (
                "Email",
                "Heron reads your recent inbox so it knows who you are talking to and what is pending. It can never send, delete or mark anything. Use an app password, not your normal password: Gmail, iCloud, Fastmail and Yahoo create one under their security settings. Outlook.com and Microsoft 365 no longer allow this kind of sign-in, so they cannot be connected yet.",
            ),
        };
        let st = row(snap, kind);
        let connected = st.map(|s| s.connected).unwrap_or(false);
        let status = match st {
            Some(s) if s.connected => {
                if kind == SourceKind::Computer {
                    "Connected. Heron knows its way around this computer.".to_string()
                } else {
                    let noun = match kind {
                        SourceKind::Email => "recent messages",
                        SourceKind::Calendar => "events",
                        _ => "files",
                    };
                    let mut text =
                        format!("Connected. {} {} indexed on this computer", s.indexed, noun);
                    if let Some(address) = &s.address {
                        text.push_str(&format!(" from {address}"));
                    }
                    if !s.folders.is_empty() {
                        text.push_str(&format!(" from {}", s.folders.join(", ")));
                    }
                    if !s.hosts.is_empty() {
                        text.push_str(&format!(" from {}", s.hosts.join(", ")));
                    }
                    text.push('.');
                    if s.learned > 0 {
                        text.push_str(&format!(
                            " Heron learned {} thing{} from it.",
                            s.learned,
                            if s.learned == 1 { "" } else { "s" }
                        ));
                    }
                    text
                }
            }
            _ => off.to_string(),
        };
        out.push(SourceItem {
            kind: kind.as_str().into(),
            title: title.into(),
            status: status.into(),
            connected,
            // Learn is disabled in demo mode, and when there is no
            // model and not demo. The server refuses it in both cases.
            learn_enabled: connected
                && kind != SourceKind::Computer
                && !snap.demo
                && snap.has_model,
        });
    }
    out
}

/// The settings form as the user left it in the view. Read on the UI
/// thread, applied to the state inside the spawned task. The provider
/// is an index into the labels; the task maps it back to an id.
struct SettingsFormInput {
    base_url: String,
    model: String,
    fallback_models: String,
    fast_model: String,
    provider_index: i32,
    cli_command: String,
    cli_model: String,
    api_key: String,
    mode: String,
    daemon_mode: String,
    auto_memory: bool,
    neural_memory: bool,
    browser_visible: bool,
    workspace: String,
}

impl SettingsFormInput {
    fn read(window: &MainWindow) -> SettingsFormInput {
        SettingsFormInput {
            base_url: window.get_settings_base_url().to_string(),
            model: window.get_settings_model().to_string(),
            fallback_models: window.get_settings_fallback_models().to_string(),
            fast_model: window.get_settings_fast_model().to_string(),
            provider_index: window.get_settings_provider_index(),
            cli_command: window.get_settings_cli_command().to_string(),
            cli_model: window.get_settings_cli_model().to_string(),
            api_key: window.get_settings_api_key().to_string(),
            mode: window.get_settings_mode().to_string(),
            daemon_mode: window.get_settings_daemon_mode().to_string(),
            auto_memory: window.get_settings_auto_memory(),
            neural_memory: window.get_settings_neural_memory(),
            browser_visible: window.get_settings_browser_visible(),
            workspace: window.get_settings_workspace().to_string(),
        }
    }

    fn apply_to(&self, st: &mut SettingsState<HeronSetupBridge>) {
        st.form.base_url = self.base_url.clone();
        st.form.model = self.model.clone();
        st.form.fallback_models = self.fallback_models.clone();
        st.form.fast_model = self.fast_model.clone();
        st.form.provider =
            st.provider_id_for_index(usize::try_from(self.provider_index).unwrap_or(0));
        st.form.cli_command = self.cli_command.clone();
        st.form.cli_model = self.cli_model.clone();
        st.form.api_key = self.api_key.clone();
        st.form.mode = Mode::parse(&self.mode).unwrap_or(Mode::Ask);
        st.form.daemon_mode = self.daemon_mode.clone();
        st.form.auto_memory = self.auto_memory;
        st.form.neural_memory = self.neural_memory;
        st.form.browser_visible = self.browser_visible;
        st.form.workspace = self.workspace.clone();
    }
}

/// Install every Setup callback on the window and load both states.
///
/// `on_finish` runs on the UI thread after a successful finish, so
/// the Shell can start a new conversation and switch to the chat
/// view. It is the Shell's closure, not the driver's.
pub fn bind_window(
    window: &MainWindow,
    bridge: &Bridge,
    driver: &SetupDriver,
    on_finish: Arc<dyn Fn() + Send + Sync>,
) {
    bind_onboarding(window, bridge, driver);
    bind_settings(window, bridge, driver);
    bind_finish(window, bridge, driver, on_finish);

    // Startup load: refresh both states, post both snapshots, and
    // switch to onboarding when the server says setup is not done.
    // The switch happens only when the refresh succeeded: a failed
    // load means the server is unreachable, and the chat view already
    // shows that error.
    {
        let onb = driver.onboarding.clone();
        let set = driver.settings.clone();
        let weak = window.as_weak();
        window.set_onboarding_busy(true);
        window.set_settings_busy(true);
        bridge.spawn(async move {
            let (onb_snap, refreshed) = {
                let mut st = onb.lock().await;
                let refreshed = st.refresh().await.is_ok();
                (OnboardingSync::from(&st), refreshed)
            };
            let set_snap = {
                let mut st = set.lock().await;
                let _ = st.refresh().await;
                SettingsSync::from(&st)
            };
            let onboarded = onb_snap.onboarded;
            let _ = weak.upgrade_in_event_loop(move |window| {
                onb_snap.apply(&window);
                set_snap.apply(&window);
                window.set_onboarding_busy(false);
                window.set_settings_busy(false);
                if refreshed && !onboarded {
                    window.set_current_view("onboarding".into());
                }
            });
        });
    }
}

fn bind_onboarding(window: &MainWindow, bridge: &Bridge, driver: &SetupDriver) {
    // Step 1: "Connect a model" opens Settings. No server call.
    {
        let weak = window.as_weak();
        window.on_onboarding_open_settings(move || {
            if let Some(w) = weak.upgrade() {
                w.set_current_view("settings".into());
            }
        });
    }

    // Step 1: "Use the demo for now". Posts {"base_url": "demo"}.
    {
        let onb = driver.onboarding.clone();
        let bridge = bridge.clone();
        let weak = window.as_weak();
        window.on_onboarding_use_demo(move || {
            let Some(w) = weak.upgrade() else { return };
            w.set_onboarding_busy(true);
            let weak_task = weak.clone();
            let onb_task = onb.clone();
            bridge.spawn(async move {
                let snap = {
                    let mut st = onb_task.lock().await;
                    let _ = st.use_demo().await;
                    OnboardingSync::from(&st)
                };
                let _ = weak_task.upgrade_in_event_loop(move |window| {
                    snap.apply(&window);
                    window.set_onboarding_busy(false);
                });
            });
        });
    }

    // Step 2: "Save and continue". Reads the six about fields from
    // the window first; a real save advances to step 3 by itself.
    {
        let onb = driver.onboarding.clone();
        let bridge = bridge.clone();
        let weak = window.as_weak();
        window.on_onboarding_save_about(move || {
            let Some(w) = weak.upgrade() else { return };
            let about = AboutForm {
                name: w.get_onboarding_about_name().to_string(),
                work: w.get_onboarding_about_work().to_string(),
                goals: w.get_onboarding_about_goals().to_string(),
                style: w.get_onboarding_about_style().to_string(),
                people: w.get_onboarding_about_people().to_string(),
                notes: w.get_onboarding_about_notes().to_string(),
            };
            w.set_onboarding_busy(true);
            let weak_task = weak.clone();
            let onb_task = onb.clone();
            bridge.spawn(async move {
                let snap = {
                    let mut st = onb_task.lock().await;
                    st.about = about;
                    let _ = st.save_about().await;
                    OnboardingSync::from(&st)
                };
                let _ = weak_task.upgrade_in_event_loop(move |window| {
                    snap.apply(&window);
                    window.set_onboarding_busy(false);
                });
            });
        });
    }

    // Step 3: one card per source. The kind string comes from the
    // card; an unknown kind is ignored, never posted.
    for (callback, action) in [("connect", 0u8), ("learn", 1u8), ("disconnect", 2u8)] {
        let onb = driver.onboarding.clone();
        let bridge = bridge.clone();
        let weak = window.as_weak();
        let install = move |kind: String| {
            let Some(kind) = SourceKind::parse(&kind) else {
                return;
            };
            let Some(w) = weak.upgrade() else { return };
            // Read the form the card needs before the call: documents
            // sends the picked folder, calendar the URL, email the
            // host, username and app password. Computer sends nothing.
            let fields = (
                w.get_onboarding_doc_path().to_string(),
                w.get_onboarding_cal_url().to_string(),
                w.get_onboarding_email_host().to_string(),
                w.get_onboarding_email_user().to_string(),
                w.get_onboarding_email_pass().to_string(),
            );
            w.set_onboarding_busy(true);
            let weak_task = weak.clone();
            let onb_task = onb.clone();
            let bridge_task = bridge.clone();
            bridge_task.spawn(async move {
                let snap = {
                    let mut st = onb_task.lock().await;
                    st.fields.path = fields.0;
                    st.fields.url = fields.1;
                    st.fields.host = fields.2;
                    st.fields.username = fields.3;
                    st.fields.password = fields.4;
                    match action {
                        0 => {
                            let _ = st.connect(kind).await;
                        }
                        1 => {
                            let _ = st.learn(kind).await;
                        }
                        _ => {
                            let _ = st.disconnect(kind).await;
                        }
                    }
                    OnboardingSync::from(&st)
                };
                let _ = weak_task.upgrade_in_event_loop(move |window| {
                    snap.apply(&window);
                    window.set_onboarding_busy(false);
                });
            });
        };
        match callback {
            "connect" => window.on_onboarding_connect_source(install),
            "learn" => window.on_onboarding_learn_source(install),
            _ => window.on_onboarding_disconnect_source(install),
        }
    }

    // Step 3: the documents card's "Choose folder" button. The
    // picker runs on the Slint event loop; a cancelled picker calls
    // nothing and the previous path stays.
    {
        let onb = driver.onboarding.clone();
        let bridge = bridge.clone();
        let weak = window.as_weak();
        window.on_onboarding_pick_folder_requested(move || {
            let onb = onb.clone();
            let bridge = bridge.clone();
            let weak = weak.clone();
            let _ = slint::spawn_local(async move {
                let picked = rfd::AsyncFileDialog::new().pick_folder().await;
                let Some(path) = picked else { return };
                let path = path.path().to_string_lossy().into_owned();
                let weak_task = weak.clone();
                bridge.spawn(async move {
                    let snap = {
                        let mut st = onb.lock().await;
                        st.set_document_path(path);
                        OnboardingSync::from(&st)
                    };
                    let _ = weak_task.upgrade_in_event_loop(move |window| {
                        snap.apply(&window);
                    });
                });
            });
        });
    }

    // Step 4: proactivity choice. Takeover reaches here only after
    // the view's inline confirm; declining it calls nothing.
    {
        let onb = driver.onboarding.clone();
        let bridge = bridge.clone();
        let weak = window.as_weak();
        window.on_onboarding_set_proactivity(move |level| {
            let Some(level) = Proactivity::parse(&level) else {
                return;
            };
            let Some(w) = weak.upgrade() else { return };
            w.set_onboarding_busy(true);
            let weak_task = weak.clone();
            let onb_task = onb.clone();
            bridge.spawn(async move {
                let snap = {
                    let mut st = onb_task.lock().await;
                    let _ = st.set_proactivity(level).await;
                    OnboardingSync::from(&st)
                };
                let _ = weak_task.upgrade_in_event_loop(move |window| {
                    snap.apply(&window);
                    window.set_onboarding_busy(false);
                });
            });
        });
    }

    // Step navigation. Back is always allowed; forward past the model
    // step needs a model or demo mode. The view hides the Continue
    // button when the step cannot be left, so a refused move cannot
    // happen from the buttons.
    {
        let onb = driver.onboarding.clone();
        let bridge = bridge.clone();
        let weak = window.as_weak();
        window.on_onboarding_goto_step(move |step| {
            let Ok(step) = usize::try_from(step) else {
                return;
            };
            let Some(w) = weak.upgrade() else { return };
            w.set_onboarding_busy(true);
            let weak_task = weak.clone();
            let onb_task = onb.clone();
            bridge.spawn(async move {
                let snap = {
                    let mut st = onb_task.lock().await;
                    let _ = st.goto_step(step);
                    OnboardingSync::from(&st)
                };
                let _ = weak_task.upgrade_in_event_loop(move |window| {
                    snap.apply(&window);
                    window.set_onboarding_busy(false);
                });
            });
        });
    }
}

/// The API key field is write-only: every settings sync clears it.
/// A callback that posts a sync without saving the key must hand the
/// typed-but-unsaved key back to the window, or it silently vanishes:
/// the next save reads the cleared field and keeps the stored key.
fn restore_api_key(window: &MainWindow, key: &str) {
    if !key.is_empty() {
        window.set_settings_api_key(key.into());
    }
}

fn bind_settings(window: &MainWindow, bridge: &Bridge, driver: &SetupDriver) {
    // Preset buttons fill the service address and model only. They
    // do not save.
    {
        let set = driver.settings.clone();
        let bridge = bridge.clone();
        let weak = window.as_weak();
        window.on_settings_apply_preset(move |name| {
            let preset = match name.as_str() {
                "openrouter" => Preset::OpenRouter,
                "openai" => Preset::OpenAi,
                "ollama" => Preset::Ollama,
                "demo" => Preset::Demo,
                _ => return,
            };
            let Some(w) = weak.upgrade() else { return };
            // Read the form first: the preset fills only the service
            // address and model, and a full sync built from the stale
            // backing state would silently discard the user's unsaved
            // edits everywhere else. The key is write-only, so hand the
            // typed-but-unsaved key back after the sync clears it.
            let input = SettingsFormInput::read(&w);
            w.set_settings_busy(true);
            let weak_task = weak.clone();
            let set_task = set.clone();
            bridge.spawn(async move {
                let snap = {
                    let mut st = set_task.lock().await;
                    input.apply_to(&mut st);
                    st.apply_preset(preset);
                    SettingsSync::from(&st)
                };
                let _ = weak_task.upgrade_in_event_loop(move |window| {
                    snap.apply(&window);
                    restore_api_key(&window, &input.api_key);
                    window.set_settings_busy(false);
                });
            });
        });
    }

    // The main save. Reads the whole form from the window first;
    // an empty API key keeps the stored key.
    {
        let set = driver.settings.clone();
        let onb = driver.onboarding.clone();
        let bridge = bridge.clone();
        let weak = window.as_weak();
        window.on_settings_save_settings(move || {
            let Some(w) = weak.upgrade() else { return };
            let input = SettingsFormInput::read(&w);
            w.set_settings_busy(true);
            let weak_task = weak.clone();
            let set_task = set.clone();
            let onb_task = onb.clone();
            bridge.spawn(async move {
                // A first-time user saves a key here and returns to
                // onboarding: step 1 must see the new model, so the
                // onboarding state refreshes too, but only when the
                // save itself succeeded.
                let (snap, onb_snap) = {
                    let mut st = set_task.lock().await;
                    input.apply_to(&mut st);
                    let saved = st.save().await.is_ok();
                    let snap = SettingsSync::from(&st);
                    let onb_snap = if saved {
                        let mut onb = onb_task.lock().await;
                        let _ = onb.refresh().await;
                        Some(OnboardingSync::from(&onb))
                    } else {
                        None
                    };
                    (snap, onb_snap)
                };
                let _ = weak_task.upgrade_in_event_loop(move |window| {
                    snap.apply(&window);
                    if let Some(onb_snap) = onb_snap {
                        onb_snap.apply(&window);
                    }
                    window.set_settings_busy(false);
                });
            });
        });
    }

    // Persona save and reset are separate from the settings save.
    // The view enforces the 4000 character limit before calling;
    // the state enforces it again.
    {
        let set = driver.settings.clone();
        let bridge = bridge.clone();
        let weak = window.as_weak();
        window.on_settings_save_persona(move || {
            let Some(w) = weak.upgrade() else { return };
            let name = w.get_settings_agent_name().to_string();
            let text = w.get_settings_persona_text().to_string();
            let api_key = w.get_settings_api_key().to_string();
            w.set_settings_busy(true);
            let weak_task = weak.clone();
            let set_task = set.clone();
            bridge.spawn(async move {
                let snap = {
                    let mut st = set_task.lock().await;
                    st.persona_name = name;
                    st.persona_text = text;
                    let _ = st.save_persona().await;
                    SettingsSync::from(&st)
                };
                let _ = weak_task.upgrade_in_event_loop(move |window| {
                    snap.apply(&window);
                    restore_api_key(&window, &api_key);
                    window.set_settings_busy(false);
                });
            });
        });
    }
    {
        let set = driver.settings.clone();
        let bridge = bridge.clone();
        let weak = window.as_weak();
        window.on_settings_reset_persona(move || {
            let Some(w) = weak.upgrade() else { return };
            let api_key = w.get_settings_api_key().to_string();
            w.set_settings_busy(true);
            let weak_task = weak.clone();
            let set_task = set.clone();
            bridge.spawn(async move {
                let snap = {
                    let mut st = set_task.lock().await;
                    let _ = st.reset_persona().await;
                    SettingsSync::from(&st)
                };
                let _ = weak_task.upgrade_in_event_loop(move |window| {
                    snap.apply(&window);
                    restore_api_key(&window, &api_key);
                    window.set_settings_busy(false);
                });
            });
        });
    }

    // One tool row changed. The index is the row's position in the
    // view's fixed labels: 0 ask, 1 allow, 2 never. The change saves
    // immediately, one call per change.
    {
        let set = driver.settings.clone();
        let bridge = bridge.clone();
        let weak = window.as_weak();
        window.on_settings_set_tool(move |name, index| {
            let setting = match index {
                0 => ToolSetting::Ask,
                1 => ToolSetting::Allow,
                2 => ToolSetting::Never,
                _ => return,
            };
            let Some(w) = weak.upgrade() else { return };
            let api_key = w.get_settings_api_key().to_string();
            w.set_settings_busy(true);
            let weak_task = weak.clone();
            let set_task = set.clone();
            bridge.spawn(async move {
                let snap = {
                    let mut st = set_task.lock().await;
                    let _ = st.set_tool(&name, setting).await;
                    SettingsSync::from(&st)
                };
                let _ = weak_task.upgrade_in_event_loop(move |window| {
                    snap.apply(&window);
                    restore_api_key(&window, &api_key);
                    window.set_settings_busy(false);
                });
            });
        });
    }

    // Accept a changed MCP app. The server note says a restart is
    // needed; the view shows "{name}: accepted. {note}".
    {
        let set = driver.settings.clone();
        let bridge = bridge.clone();
        let weak = window.as_weak();
        window.on_settings_accept_mcp(move |name| {
            let Some(w) = weak.upgrade() else { return };
            let api_key = w.get_settings_api_key().to_string();
            w.set_settings_busy(true);
            let weak_task = weak.clone();
            let set_task = set.clone();
            bridge.spawn(async move {
                let snap = {
                    let mut st = set_task.lock().await;
                    let _ = st.accept_mcp(&name).await;
                    SettingsSync::from(&st)
                };
                let _ = weak_task.upgrade_in_event_loop(move |window| {
                    snap.apply(&window);
                    restore_api_key(&window, &api_key);
                    window.set_settings_busy(false);
                });
            });
        });
    }

    // Phone. Connect reads the token from the window; the state
    // clears it on success. Disconnect shows the view's own text,
    // because the server response carries no note.
    {
        let set = driver.settings.clone();
        let bridge = bridge.clone();
        let weak = window.as_weak();
        window.on_settings_connect_telegram(move || {
            let Some(w) = weak.upgrade() else { return };
            let token = w.get_settings_telegram_token().to_string();
            let api_key = w.get_settings_api_key().to_string();
            w.set_settings_busy(true);
            let weak_task = weak.clone();
            let set_task = set.clone();
            bridge.spawn(async move {
                let snap = {
                    let mut st = set_task.lock().await;
                    st.telegram_token = token;
                    let _ = st.connect_telegram().await;
                    SettingsSync::from(&st)
                };
                let _ = weak_task.upgrade_in_event_loop(move |window| {
                    snap.apply(&window);
                    restore_api_key(&window, &api_key);
                    window.set_settings_busy(false);
                });
            });
        });
    }
    {
        let set = driver.settings.clone();
        let bridge = bridge.clone();
        let weak = window.as_weak();
        window.on_settings_disconnect_telegram(move || {
            let Some(w) = weak.upgrade() else { return };
            let api_key = w.get_settings_api_key().to_string();
            w.set_settings_busy(true);
            let weak_task = weak.clone();
            let set_task = set.clone();
            bridge.spawn(async move {
                let snap = {
                    let mut st = set_task.lock().await;
                    let _ = st.disconnect_telegram().await;
                    SettingsSync::from(&st)
                };
                let _ = weak_task.upgrade_in_event_loop(move |window| {
                    snap.apply(&window);
                    restore_api_key(&window, &api_key);
                    window.set_settings_busy(false);
                });
            });
        });
    }

    // "Save a copy of my data". The state keeps the full path and
    // the file name for the message.
    {
        let set = driver.settings.clone();
        let bridge = bridge.clone();
        let weak = window.as_weak();
        window.on_settings_make_backup(move || {
            let Some(w) = weak.upgrade() else { return };
            let api_key = w.get_settings_api_key().to_string();
            w.set_settings_busy(true);
            let weak_task = weak.clone();
            let set_task = set.clone();
            bridge.spawn(async move {
                let snap = {
                    let mut st = set_task.lock().await;
                    let _ = st.backup().await;
                    SettingsSync::from(&st)
                };
                let _ = weak_task.upgrade_in_event_loop(move |window| {
                    snap.apply(&window);
                    restore_api_key(&window, &api_key);
                    window.set_settings_busy(false);
                });
            });
        });
    }
}

fn bind_finish(
    window: &MainWindow,
    bridge: &Bridge,
    driver: &SetupDriver,
    on_finish: Arc<dyn Fn() + Send + Sync>,
) {
    // Step 4: "Finish setup" (or "Back to the conversation" when
    // already onboarded). Posts /onboarded, reloads, then the Shell
    // starts a new conversation and switches to the chat view.
    let onb = driver.onboarding.clone();
    let bridge = bridge.clone();
    let weak = window.as_weak();
    window.on_onboarding_finish(move || {
        let Some(w) = weak.upgrade() else { return };
        w.set_onboarding_busy(true);
        let weak_task = weak.clone();
        let onb_task = onb.clone();
        let on_finish_task = on_finish.clone();
        bridge.spawn(async move {
            let (snap, finished_ok) = {
                let mut st = onb_task.lock().await;
                let finished_ok = st.finish().await.is_ok();
                (OnboardingSync::from(&st), finished_ok)
            };
            let _ = weak_task.upgrade_in_event_loop(move |window| {
                snap.apply(&window);
                window.set_onboarding_busy(false);
                if finished_ok {
                    on_finish_task();
                }
            });
        });
    });
}
