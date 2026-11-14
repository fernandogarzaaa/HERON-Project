//! Heron desktop app: a native Slint window around the local UI.
//! The agent, its data and the HTTP server all live inside this one process;
//! the server listens on a random loopback port protected by a per-launch token.
//! The window talks to that server over HTTP through the bridge (`desktop/`),
//! exactly like the old web UI did. It never touches the backend in-process,
//! and it never touches a widget from a background thread: background tasks
//! hand results to the UI thread with `Weak::upgrade_in_event_loop`.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod bridge;
mod models;
mod setup;
mod setup_bridge;
mod setup_glue;
mod sse;
mod ui_state;
mod views_bridge;
mod views_slint;

use bridge::{Bridge, ChatEvent, HeronClient, UndoOutcome};
use heron::app::App;
use heron::config::Config;
use heron::ui::chat::ChatView;
use models::begin_send;
use setup_glue::{bind_window as bind_setup_window, SetupDriver};
use slint::{ModelRc, SharedString, VecModel};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use views_bridge::HeronViewsBridge;
use views_slint::{bind_window, ViewsController};

slint::include_modules!();

/// Start the agent backend on its own thread. Returns the launch URL, or a message explaining why it could not start.
fn start_backend() -> Result<String, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                let _ = tx.send(Err(format!("could not start the async runtime: {e}")));
                return;
            }
        };
        rt.block_on(async move {
            let started = async {
                let cfg = Config::load()?;
                let workspace = cfg.app_workspace();
                let app = Arc::new(App::build(cfg, workspace).await?);
                heron::server::start(app, "127.0.0.1", 0).await
            }
            .await;
            match started {
                Ok((url, server)) => {
                    let _ = tx.send(Ok(url));
                    let _ = server.await;
                }
                Err(e) => {
                    let _ = tx.send(Err(format!("{e:#}")));
                }
            }
        });
    });
    rx.recv()
        .unwrap_or_else(|_| Err("the background service stopped unexpectedly".into()))
}

/// The current session id from the window, or `None` for a new conversation.
fn current_session_id(window: &MainWindow) -> Option<String> {
    let id = window.get_session_id();
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

/// Send a message in the current session: start the turn on the UI thread,
/// then stream its events onto the UI thread as they arrive. The send
/// callback, the starter cards, the continue action, and (from the Views
/// lane) an accepted suggestion all send through here, so a turn is always
/// one `ChatView::start_send` and one forwarder.
///
/// The forwarder tracks terminal events. When the channel closes without
/// one, the turn is closed with the connection note instead of staying
/// live and busy forever.
fn send_message(
    window: &MainWindow,
    chat: &Arc<Mutex<ChatView>>,
    bridge: &Bridge,
    text: &str,
    session_id: Option<String>,
) {
    let text = text.to_string();
    if text.trim().is_empty() {
        return;
    }
    let turn_id = {
        let mut chat = chat.lock().unwrap();
        // Guard all send entry points under the same mutex before adding a
        // turn: a concurrent 409 must not clear the original reply's busy state.
        if chat.busy {
            return;
        }
        begin_send(window, &mut chat, &text)
    };
    let (tx, mut rx) = mpsc::unbounded_channel::<ChatEvent>();
    let weak_events = window.as_weak();
    let chat_events = chat.clone();
    bridge.spawn(async move {
        let mut saw_terminal = false;
        while let Some(event) = rx.recv().await {
            if matches!(
                event,
                ChatEvent::Done { .. } | ChatEvent::Error { .. } | ChatEvent::Cancelled
            ) {
                saw_terminal = true;
            }
            let weak = weak_events.clone();
            let chat = chat_events.clone();
            let _ = weak.upgrade_in_event_loop(move |window| {
                let mut chat = chat.lock().unwrap();
                models::apply_chat_event(&window, &mut chat, turn_id, event);
            });
        }
        if !saw_terminal {
            let weak = weak_events.clone();
            let chat = chat_events.clone();
            let _ = weak.upgrade_in_event_loop(move |window| {
                let mut chat = chat.lock().unwrap();
                models::stream_closed_early(&window, &mut chat, turn_id);
            });
        }
    });
    // stream_chat takes the sender, so hold a clone for the error path:
    // if the turn fails before any event arrives (409 while a reply runs,
    // connection failure, broken stream), map the error into a
    // ChatEvent::Error instead of dropping it.
    let client = bridge.client().clone();
    let tx_err = tx.clone();
    bridge.spawn(async move {
        if let Err(e) = client.stream_chat(session_id.as_deref(), &text, tx).await {
            let _ = tx_err.send(ChatEvent::Error {
                message: format!("{e:#}"),
            });
        }
    });
}

/// Start a new conversation: stop the running reply, clear the thread and
/// the session id, and reload the starters for the empty state. The server
/// creates the next session lazily, on the first message sent. A forwarder
/// from the old turn drops its events: its turn id no longer exists in the
/// cleared thread.
fn start_new_conversation(window: &MainWindow, chat: &Arc<Mutex<ChatView>>, bridge: &Bridge) {
    let session_id = window.get_session_id().to_string();
    if !session_id.is_empty() {
        let client = bridge.client().clone();
        bridge.spawn(async move {
            let _ = client.cancel(&session_id).await;
        });
    }
    {
        let mut chat = chat.lock().unwrap();
        models::start_new(window, &mut chat);
    }
    load_starters(window, bridge);
}

/// Fetch the starter prompts and post them to the window. Used at startup
/// and on new conversation.
fn load_starters(window: &MainWindow, bridge: &Bridge) {
    let client = bridge.client().clone();
    let weak = window.as_weak();
    bridge.spawn(async move {
        let starters = client.starters().await;
        let _ = weak.upgrade_in_event_loop(move |window| {
            let rows: Vec<SharedString> = starters
                .iter()
                .map(|s| SharedString::from(s.as_str()))
                .collect();
            window.set_starters(ModelRc::new(VecModel::from_slice(&rows)));
        });
    });
}

fn main() {
    // Apps opened from Finder get a bare PATH; add the usual places developer tools live so that
    // commands such as git work the same as in a terminal.
    #[cfg(target_os = "macos")]
    {
        let current = std::env::var("PATH").unwrap_or_default();
        let extra = ["/opt/homebrew/bin", "/usr/local/bin"]
            .iter()
            .filter(|p| !current.split(':').any(|c| c == **p))
            .cloned()
            .collect::<Vec<_>>();
        if !extra.is_empty() {
            std::env::set_var("PATH", format!("{}:{current}", extra.join(":")));
        }
    }

    let window = MainWindow::new().expect("could not create the Heron window");
    window.set_messages(ModelRc::new(VecModel::<ChatMsg>::default()));

    // The live chat state machine. One Arc, shared by the UI-thread
    // callbacks and the forwarder closures. Every access is on the UI
    // thread and brief; no guard is ever held across an await.
    let chat = Arc::new(Mutex::new(ChatView::new()));

    // Keep the separate About window alive in the Help callback. Creating it
    // inside the callback and immediately dropping its last strong handle after
    // show() can destroy the window before the user can dismiss it.
    match AboutDialog::new() {
        Ok(dialog) => {
            dialog.set_app_version(SharedString::from(env!("CARGO_PKG_VERSION")));
            let weak_dialog = dialog.as_weak();
            dialog.on_ok_clicked(move || {
                if let Some(dialog) = weak_dialog.upgrade() {
                    let _ = dialog.hide();
                }
            });
            window.on_show_about(move || {
                if let Err(e) = dialog.show() {
                    eprintln!("could not open the About window: {e}");
                }
            });
        }
        Err(e) => eprintln!("could not create the About window: {e}"),
    }

    let bridge = match start_backend() {
        Ok(url) => match HeronClient::from_launch_url(&url) {
            Some(client) => match Bridge::new(client) {
                Ok(bridge) => {
                    window.set_backend_status(SharedString::from("Connected"));
                    Some(bridge)
                }
                Err(e) => {
                    window.set_error_message(SharedString::from(format!(
                        "Heron could not start: {e:#}"
                    )));
                    None
                }
            },
            None => {
                window.set_error_message(SharedString::from(
                    "Heron could not start: the backend URL was not recognised.",
                ));
                None
            }
        },
        Err(message) => {
            window.set_error_message(SharedString::from(format!(
                "Heron could not start: {message} If you edited the settings file by hand, check it for mistakes, then open Heron again. It lives in the .heron folder in your home folder."
            )));
            None
        }
    };

    if let Some(bridge) = bridge {
        // Refresh the status line and the chat properties from the live
        // server state, every 30 seconds.
        ui_state::poll_state(&bridge, window.as_weak(), chat.clone());

        // Starter prompts for the empty state.
        load_starters(&window, &bridge);

        // Send a message: one start_send and one forwarder per turn.
        {
            let bridge = bridge.clone();
            let chat = chat.clone();
            let weak = window.as_weak();
            window.on_send_message(move |text| {
                let Some(window) = weak.upgrade() else { return };
                let sid = current_session_id(&window);
                send_message(&window, &chat, &bridge, &text, sid);
            });
        }

        // New conversation.
        {
            let bridge = bridge.clone();
            let chat = chat.clone();
            let weak = window.as_weak();
            window.on_new_session(move || {
                if let Some(window) = weak.upgrade() {
                    start_new_conversation(&window, &chat, &bridge);
                }
            });
        }

        // Stop the running reply in the current conversation.
        {
            let bridge = bridge.clone();
            let weak = window.as_weak();
            window.on_cancel_reply(move || {
                let Some(window) = weak.upgrade() else { return };
                let session_id = window.get_session_id();
                if session_id.is_empty() {
                    return;
                }
                let client = bridge.client().clone();
                let sid = session_id.to_string();
                bridge.spawn(async move {
                    let _ = client.cancel(&sid).await;
                });
            });
        }

        // Answer a waiting approval: once, always, session, or anything else denies.
        // A failed server decision must leave the approval card actionable.
        {
            let bridge = bridge.clone();
            let chat = chat.clone();
            let weak = window.as_weak();
            window.on_approve(move |id, decision| {
                let client = bridge.client().clone();
                let weak = weak.clone();
                let chat = chat.clone();
                let id = id.to_string();
                let decision = decision.to_string();
                bridge.spawn(async move {
                    let result = client.approve(&id, &decision).await;
                    let _ = weak.upgrade_in_event_loop(move |window| {
                        let mut chat = chat.lock().unwrap();
                        match result {
                            Ok(()) => {
                                models::answer_approval(&window, &mut chat, &id, &decision, None)
                            }
                            Err(e) => {
                                models::push_note(
                                    &window,
                                    &mut chat,
                                    &format!("Approval request failed: {e:#}. Please retry or deny."),
                                );
                            }
                        }
                    });
                });
            });
        }

        // Switch permission mode. The Unrestricted switch is gated by the
        // inline confirm row in the view, so Rust sends only after confirm.
        {
            let bridge = bridge.clone();
            let chat = chat.clone();
            let weak = window.as_weak();
            window.on_set_mode(move |mode| {
                let client = bridge.client().clone();
                let weak = weak.clone();
                let chat = chat.clone();
                let mode = mode.to_string();
                bridge.spawn(async move {
                    match client.set_mode(&mode).await {
                        Ok(returned) => {
                            let _ = weak.upgrade_in_event_loop(move |window| {
                                if let Some(parsed) = heron::ui::chat::Mode::parse(&returned) {
                                    chat.lock().unwrap().mode = parsed;
                                }
                                window.set_chat_mode(SharedString::from(returned));
                            });
                        }
                        Err(e) => eprintln!("could not switch mode: {e:#}"),
                    }
                });
            });
        }

        // Undo the last file change. The outcome becomes a note row: the
        // undone description, or the reason it could not be undone. A
        // conflict needs a force restore behind a confirmation that is not
        // built yet; it reports the reason and stops there.
        {
            let bridge = bridge.clone();
            let chat = chat.clone();
            let weak = window.as_weak();
            window.on_undo(move || {
                let Some(window) = weak.upgrade() else { return };
                let session_id = window.get_session_id().to_string();
                if session_id.is_empty() {
                    return;
                }
                let client = bridge.client().clone();
                let weak = weak.clone();
                let chat = chat.clone();
                bridge.spawn(async move {
                    let outcome = client.undo(&session_id).await;
                    let _ = weak.upgrade_in_event_loop(move |window| {
                        let mut chat = chat.lock().unwrap();
                        match outcome {
                            Ok(UndoOutcome::Undone(text)) => {
                                models::push_note(&window, &mut chat, &format!("Undone: {text}"));
                            }
                            Ok(UndoOutcome::Nothing) => {
                                models::push_note(&window, &mut chat, "Nothing to undo.");
                            }
                            Ok(UndoOutcome::Stale(why)) => {
                                models::push_note(
                                    &window,
                                    &mut chat,
                                    &format!("Heron could not undo: {why}"),
                                );
                            }
                            Ok(UndoOutcome::Conflict { reason, .. }) => {
                                models::push_note(
                                    &window,
                                    &mut chat,
                                    &format!("Heron cannot undo: {reason}"),
                                );
                            }
                            Err(e) => {
                                models::push_note(
                                    &window,
                                    &mut chat,
                                    &format!("Heron could not undo: {e:#}"),
                                );
                            }
                        }
                    });
                });
            });
        }

        // Flip a tool result row open or closed. View state only.
        {
            let chat = chat.clone();
            let weak = window.as_weak();
            window.on_toggle_step(move |id| {
                let Some(window) = weak.upgrade() else { return };
                let mut chat = chat.lock().unwrap();
                models::toggle_step(&window, &mut chat, &id);
            });
        }

        // Retain the platform clipboard object for the window's lifetime.
        // Some clipboard backends transfer ownership on drop, so creating
        // and immediately dropping it can make copied text disappear.
        let clipboard = std::cell::RefCell::new(None::<arboard::Clipboard>);
        window.on_copy_requested(move |text| {
            let mut slot = clipboard.borrow_mut();
            if slot.is_none() {
                match arboard::Clipboard::new() {
                    Ok(cb) => *slot = Some(cb),
                    Err(e) => {
                        eprintln!("could not open the clipboard: {e}");
                        return;
                    }
                }
            }
            if let Some(cb) = slot.as_mut() {
                if let Err(e) = cb.set_text(text.to_string()) {
                    eprintln!("could not copy to the clipboard: {e}");
                }
            }
        });

        // Open a link from a message in the default browser.
        window.on_open_link(move |url| {
            if let Err(e) = open::that(url.as_str()) {
                eprintln!("could not open the link: {e}");
            }
        });

        // Send a starter prompt exactly like a typed message.
        {
            let bridge = bridge.clone();
            let chat = chat.clone();
            let weak = window.as_weak();
            window.on_use_starter(move |prompt| {
                let Some(window) = weak.upgrade() else { return };
                let sid = current_session_id(&window);
                send_message(&window, &chat, &bridge, &prompt, sid);
            });
        }

        // Continue an interrupted turn, exactly like the web UI does.
        {
            let bridge = bridge.clone();
            let chat = chat.clone();
            let weak = window.as_weak();
            window.on_continue_turn(move || {
                let Some(window) = weak.upgrade() else { return };
                let sid = current_session_id(&window);
                send_message(
                    &window,
                    &chat,
                    &bridge,
                    "Continue from where you left off.",
                    sid,
                );
            });
        }

        // Try the demo model, then refresh state at once.
        {
            let bridge = bridge.clone();
            let chat = chat.clone();
            let weak = window.as_weak();
            window.on_try_demo(move || {
                let client = bridge.client().clone();
                let bridge_task = bridge.clone();
                let weak = weak.clone();
                let chat = chat.clone();
                bridge.spawn(async move {
                    if let Err(e) = client.try_demo().await {
                        eprintln!("could not switch to the demo model: {e:#}");
                        return;
                    }
                    ui_state::refresh_state_now(&bridge_task, weak, chat);
                });
            });
        }

        // Resume a paused Heron, then refresh state at once.
        {
            let bridge = bridge.clone();
            let chat = chat.clone();
            let weak = window.as_weak();
            window.on_resume(move || {
                let client = bridge.client().clone();
                let bridge_task = bridge.clone();
                let weak = weak.clone();
                let chat = chat.clone();
                bridge.spawn(async move {
                    if let Err(e) = client.resume().await {
                        eprintln!("could not resume Heron: {e:#}");
                        return;
                    }
                    ui_state::refresh_state_now(&bridge_task, weak, chat);
                });
            });
        }

        // Open Settings.
        {
            let weak = window.as_weak();
            window.on_open_settings(move || {
                if let Some(window) = weak.upgrade() {
                    window.set_current_view(SharedString::from("settings"));
                }
            });
        }

        // Views: sidebar, Memory, Tasks, Reminders, Awareness, Usage,
        // Activity. The controller owns the Views lane's glue
        // (src/bin/desktop/views_slint.rs) and the HTTP bridge behind it.
        // The four Shell closures run on tokio tasks, never on the UI
        // thread, so each one posts back through upgrade_in_event_loop.
        // The controller slot breaks the circle: the closures need the
        // controller (to mark the current session), and the controller is
        // built from the closures.
        let ctrl_slot: Arc<Mutex<Option<Arc<ViewsController>>>> = Arc::new(Mutex::new(None));
        let on_accept_suggestion = {
            let bridge = bridge.clone();
            let chat = chat.clone();
            let weak = window.as_weak();
            let slot = ctrl_slot.clone();
            Arc::new(move |prompt: String, session_id: Option<String>| {
                let nav_generation = chat.lock().unwrap().begin_navigation();
                let client = bridge.client().clone();
                let bridge_task = bridge.clone();
                let chat_task = chat.clone();
                let weak_task = weak.clone();
                let slot_task = slot.clone();
                bridge.spawn(async move {
                    // Existing sessions must actually load before their
                    // suggestions can run; otherwise a separate server
                    // session would be appended to the old UI transcript.
                    let detail = match &session_id {
                        Some(id) => Some(client.get_session(id).await),
                        None => None,
                    };
                    let _ = weak_task.upgrade_in_event_loop(move |window| {
                        {
                            let mut state = chat_task.lock().unwrap();
                            if state.navigation_generation != nav_generation {
                                return;
                            }
                            if state.busy {
                                models::push_note(&window, &mut state,
                                    "Stop the current reply before opening a suggestion.");
                                return;
                            }
                            match detail {
                                Some(Ok(detail)) => {
                                    models::open_history(&window, &mut state, &detail);
                                }
                                Some(Err(e)) => {
                                    models::push_note(&window, &mut state,
                                        &format!("Could not open suggestion conversation: {e:#}"));
                                    return;
                                }
                                None => {}
                            }
                        }
                        if session_id.is_none() {
                            start_new_conversation(&window, &chat_task, &bridge_task);
                        }
                        window.set_current_view(SharedString::from("chat"));
                        if let Some(ctrl) = slot_task.lock().unwrap().as_ref() {
                            ctrl.set_current_session(session_id.clone());
                        }
                        send_message(&window, &chat_task, &bridge_task, &prompt, session_id);
                    });
                });
            }) as Arc<dyn Fn(String, Option<String>) + Send + Sync>
        };
        let on_open_session = {
            let bridge = bridge.clone();
            let chat = chat.clone();
            let weak = window.as_weak();
            let slot = ctrl_slot.clone();
            Arc::new(move |id: String| {
                // Each selection invalidates earlier pending fetches. New
                // conversation and history replay invalidate these too.
                let nav_generation = {
                    let mut state = chat.lock().unwrap();
                    if state.busy {
                        return;
                    }
                    state.begin_navigation()
                };
                let client = bridge.client().clone();
                let chat_task = chat.clone();
                let weak_task = weak.clone();
                let slot_task = slot.clone();
                bridge.spawn(async move {
                    let detail = client.get_session(&id).await.ok();
                    let _ = weak_task.upgrade_in_event_loop(move |window| {
                        // Do not switch away while a reply is running:
                        // the web UI refuses the same way.
                        let mut state = chat_task.lock().unwrap();
                        if state.busy || state.navigation_generation != nav_generation {
                            return;
                        }
                        let Some(detail) = detail else {
                            models::push_note(&window, &mut state,
                                "Could not open the selected conversation.");
                            return;
                        };
                        models::open_history(&window, &mut state, &detail);
                        drop(state);
                        window.set_current_view(SharedString::from("chat"));
                        if let Some(ctrl) = slot_task.lock().unwrap().as_ref() {
                            ctrl.set_current_session(Some(id));
                        }
                    });
                });
            }) as Arc<dyn Fn(String) + Send + Sync>
        };
        let on_navigate = {
            let weak = window.as_weak();
            let chat = chat.clone();
            Arc::new(move |view: String| {
                // A newer sidebar destination invalidates pending session
                // fetches even when it is not another conversation.
                chat.lock().unwrap().begin_navigation();
                let weak = weak.clone();
                let _ = weak.upgrade_in_event_loop(move |window| {
                    window.set_current_view(SharedString::from(view));
                });
            }) as Arc<dyn Fn(String) + Send + Sync>
        };
        let on_new_conversation = {
            let bridge = bridge.clone();
            let chat = chat.clone();
            let weak = window.as_weak();
            let slot = ctrl_slot.clone();
            Arc::new(move || {
                let bridge = bridge.clone();
                let chat = chat.clone();
                let slot = slot.clone();
                let weak = weak.clone();
                let _ = weak.upgrade_in_event_loop(move |window| {
                    start_new_conversation(&window, &chat, &bridge);
                    window.set_current_view(SharedString::from("chat"));
                    if let Some(ctrl) = slot.lock().unwrap().as_ref() {
                        ctrl.set_current_session(None);
                    }
                });
            }) as Arc<dyn Fn() + Send + Sync>
        };
        let views_bridge: Arc<dyn heron::ui::views::ViewsBridge> =
            Arc::new(HeronViewsBridge::new(bridge.client().clone()));
        let ctrl = ViewsController::new(
            views_bridge,
            bridge.handle(),
            on_accept_suggestion,
            on_open_session,
            on_navigate,
            on_new_conversation,
        );
        *ctrl_slot.lock().unwrap() = Some(ctrl.clone());
        bind_window(&window, &ctrl);

        // Sidebar refresh every 30 seconds: the tick syncs the window's
        // current session into the controller, so the sidebar highlights
        // the conversation the chat is actually in (including one the
        // Chat lane just created), and the controller reloads the badges,
        // footer, and session list with it.
        {
            let weak = window.as_weak();
            let ctrl_tick = ctrl.clone();
            let bridge_tick = bridge.clone();
            bridge_tick.spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
                loop {
                    interval.tick().await;
                    let ctrl = ctrl_tick.clone();
                    let weak = weak.clone();
                    let posted = weak.upgrade_in_event_loop(move |window| {
                        let id = current_session_id(&window);
                        ctrl.set_current_session(id);
                    });
                    if posted.is_err() {
                        break;
                    }
                }
            });
        }

        // Setup: onboarding and settings. The driver owns the two
        // state machines and installs every setup callback; the
        // finish hook starts a new conversation and switches to the
        // chat view, the way the web UI does after onboarding.
        {
            let setup_driver = SetupDriver::new(bridge.client().clone());
            let weak_finish = window.as_weak();
            let chat_finish = chat.clone();
            let bridge_finish = bridge.clone();
            let on_finish: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                let Some(window) = weak_finish.upgrade() else {
                    return;
                };
                start_new_conversation(&window, &chat_finish, &bridge_finish);
                window.set_current_view("chat".into());
            });
            bind_setup_window(&window, &bridge, &setup_driver, on_finish);
        }
    }

    let _ = window.run();
    heron::browser::kill_now(); // the browser Heron was driving closes with the app
}

#[cfg(test)]
mod window_layout_tests {
    use super::MainWindow;
    use slint::platform::software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType};
    use slint::platform::{Platform, WindowAdapter};
    use slint::{ComponentHandle, PhysicalSize, PlatformError};
    use std::rc::Rc;

    struct HeadlessPlatform(Rc<MinimalSoftwareWindow>);

    impl Platform for HeadlessPlatform {
        fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
            Ok(self.0.clone())
        }
    }

    // Uncompressed 32-bit BMP output requires no extra dependencies and
    // can be opened by macOS Preview and Windows Photos for visual review.
    fn write_bmp(
        path: &std::path::Path,
        width: u32,
        height: u32,
        pixels: &[PremultipliedRgbaColor],
    ) {
        let image_size = width * height * 4;
        let file_size = image_size + 54;
        let mut bytes = Vec::with_capacity(file_size as usize);
        bytes.extend_from_slice(b"BM");
        bytes.extend_from_slice(&file_size.to_le_bytes());
        bytes.extend_from_slice(&[0u8; 4]); // reserved
        bytes.extend_from_slice(&54u32.to_le_bytes()); // pixel data offset
        bytes.extend_from_slice(&40u32.to_le_bytes()); // BITMAPINFOHEADER size
        bytes.extend_from_slice(&(width as i32).to_le_bytes());
        bytes.extend_from_slice(&(height as i32).to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes()); // color planes
        bytes.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        bytes.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB, uncompressed
        bytes.extend_from_slice(&image_size.to_le_bytes());
        bytes.extend_from_slice(&[0u8; 16]); // DPI and palette
        for row in pixels.chunks_exact(width as usize).rev() {
            for pixel in row {
                bytes.extend_from_slice(&[pixel.blue, pixel.green, pixel.red, 0]);
            }
        }
        std::fs::write(path, bytes).expect("write headless UI screenshot");
    }

    #[test]
    fn main_window_lays_out_and_renders_without_a_binding_loop() {
        let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        slint::platform::set_platform(Box::new(HeadlessPlatform(window.clone()))).expect("headless platform installs");
        let ui = MainWindow::new().expect("main window builds");
        ui.show().expect("main window shows");

        // Exercise resize transitions in one test: Slint's platform can only
        // be installed once per process, and a single initial render cannot
        // detect resize-triggered binding loops or renderer failures.
        for (width, height) in [(1100, 700), (800, 600), (640, 480), (1440, 900), (1100, 700)] {
            window.set_size(PhysicalSize::new(width, height));
            eprintln!(
                "HERON_UI_GEOMETRY requested={}x{} window={:?} content={:?} composer_y={:?}",
                width, height,
                ui.get_debug_window_height(),
                ui.get_debug_content_height(),
                ui.get_debug_composer_y()
            );
            // For viewport sizes within the supported native minimum,
            // reject an intrinsic Slint root taller than the actual frame.
            // The previous missing-composer regression had a 2752px root
            // for a requested 700px window while every draw call succeeded.
            if width >= 880 && height >= 560 {
                let shell_height = ui.get_debug_window_height();
                let composer_y = ui.get_debug_composer_y();
                assert!(
                    shell_height <= height as f32 + 1.0,
                    "Slint shell height {shell_height} overflows requested {height}px frame"
                );
                assert!(
                    composer_y >= 0.0 && composer_y + 94.0 <= height as f32,
                    "composer at y={composer_y} is outside the {height}px frame"
                );
            }
            let mut pixels = vec![PremultipliedRgbaColor::default(); (width * height) as usize];
            let drew = window.draw_if_needed(|renderer| {
                renderer.render(&mut pixels, width as usize);
            });
            assert!(drew, "main window did not render after resizing to {width}x{height}");
            // A draw call alone can succeed while presenting a completely
            // blank window. Require at least two distinct RGB values so a
            // broken compositor or hidden top-level view is caught.
            let reference = (pixels[0].red, pixels[0].green, pixels[0].blue);
            assert!(
                pixels.iter().any(|pixel| (pixel.red, pixel.green, pixel.blue) != reference),
                "main window drew a uniform/blank frame at {width}x{height}"
            );
            if let Ok(directory) = std::env::var("HERON_UI_SNAPSHOTS_DIR") {
                // CI saves an intentionally empty-session screen only.
                // No runtime account, message, or credential data is rendered.
                std::fs::create_dir_all(&directory).expect("create screenshot artifact directory");
                let path = std::path::Path::new(&directory)
                    .join(format!("heron-main-{width}x{height}.bmp"));
                write_bmp(&path, width, height, &pixels);
            }
        }
    }
}
