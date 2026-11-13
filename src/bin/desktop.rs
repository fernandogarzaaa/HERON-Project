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
mod sse;
mod ui_state;

use bridge::{Bridge, ChatEvent, HeronClient};
use heron::app::App;
use heron::config::Config;
use models::{apply_chat_event, push_message};
use slint::{ModelRc, SharedString, VecModel};
use std::sync::Arc;
use tokio::sync::mpsc;

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
    window.set_app_version(SharedString::from(env!("CARGO_PKG_VERSION")));
    window.set_messages(ModelRc::new(VecModel::<ChatMsg>::default()));

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
        // Refresh the status line from the live server state, every 30 seconds.
        ui_state::poll_state(&bridge, window.as_weak());

        // Send a message: echo it, open an empty assistant reply, and stream the
        // turn's events onto the UI thread as they arrive.
        {
            let bridge = bridge.clone();
            let weak = window.as_weak();
            window.on_send_message(move |text| {
                let text = text.to_string();
                if text.trim().is_empty() {
                    return;
                }
                let Some(window) = weak.upgrade() else { return };
                push_message(&window, "user", &text);
                push_message(&window, "assistant", "");
                window.set_busy(true);
                let session_id = window.get_session_id();
                let sid = if session_id.is_empty() {
                    None
                } else {
                    Some(session_id.to_string())
                };
                let (tx, mut rx) = mpsc::unbounded_channel::<ChatEvent>();
                let weak_events = weak.clone();
                bridge.spawn(async move {
                    while let Some(event) = rx.recv().await {
                        let weak = weak_events.clone();
                        let _ = weak.upgrade_in_event_loop(move |window| {
                            apply_chat_event(window, event);
                        });
                    }
                });
                let client = bridge.client().clone();
                // stream_chat takes the sender, so hold a clone for the error
                // path: if the turn fails before any event arrives (409 while
                // a reply runs, connection failure, broken stream), map the
                // error into a ChatEvent::Error instead of dropping it. The
                // forwarder turns it into an error line and clears busy;
                // without it the empty assistant row stays live and busy
                // stays true forever.
                let tx_err = tx.clone();
                bridge.spawn(async move {
                    if let Err(e) = client.stream_chat(sid.as_deref(), &text, tx).await {
                        let _ = tx_err.send(ChatEvent::Error {
                            message: format!("{e:#}"),
                        });
                    }
                });
            });
        }

        // New conversation: forget the server session id and clear the view. The
        // server creates the next session lazily, on the first message sent.
        {
            let weak = window.as_weak();
            window.on_new_session(move || {
                if let Some(window) = weak.upgrade() {
                    window.set_session_id(SharedString::from(""));
                    window.set_messages(ModelRc::new(VecModel::<ChatMsg>::default()));
                    window.set_session_title(SharedString::from(""));
                    window.set_busy(false);
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
        {
            let bridge = bridge.clone();
            window.on_approve(move |id, decision| {
                let client = bridge.client().clone();
                let id = id.to_string();
                let decision = decision.to_string();
                bridge.spawn(async move {
                    let _ = client.approve(&id, &decision).await;
                });
            });
        }
    }

    let _ = window.run();
    heron::browser::kill_now(); // the browser Heron was driving closes with the app
}
