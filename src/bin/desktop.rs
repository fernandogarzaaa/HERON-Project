//! Heron desktop app: a native window around the local UI.
//! The agent, its data and the HTTP server all live inside this one process;
//! the server listens on a random loopback port protected by a per-launch token.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use heron::app::App;
use heron::config::Config;
use std::sync::Arc;
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop};
use tao::window::WindowBuilder;
use wry::WebViewBuilder;

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
    rx.recv().unwrap_or_else(|_| Err("the background service stopped unexpectedly".into()))
}

fn error_page(message: &str) -> String {
    let safe = message.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    format!(
        "<!doctype html><meta charset=utf-8><body style=\"font:16px/1.5 system-ui;max-width:640px;margin:12vh auto;padding:0 24px;color:#16212B;background:#EDF1F4\">\
         <h1 style=\"font:600 28px Georgia,serif\">Heron could not start</h1><p>{safe}</p>\
         <p>If you edited the settings file by hand, check it for mistakes, then open Heron again. It lives in the <code>.heron</code> folder in your home folder.</p></body>"
    )
}

/// Standard app and Edit menus. On macOS these are what make ⌘C / ⌘V / ⌘Q work inside the window.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn build_menu() -> muda::Menu {
    use muda::{Menu, PredefinedMenuItem, Submenu};
    let menu = Menu::new();
    let app_menu = Submenu::new("Heron", true);
    let _ = app_menu.append_items(&[
        &PredefinedMenuItem::about(None, None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::hide(None),
        &PredefinedMenuItem::hide_others(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::quit(None),
    ]);
    let edit = Submenu::new("Edit", true);
    let _ = edit.append_items(&[
        &PredefinedMenuItem::undo(None),
        &PredefinedMenuItem::redo(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::cut(None),
        &PredefinedMenuItem::copy(None),
        &PredefinedMenuItem::paste(None),
        &PredefinedMenuItem::select_all(None),
    ]);
    let window = Submenu::new("Window", true);
    let _ = window.append_items(&[&PredefinedMenuItem::minimize(None), &PredefinedMenuItem::fullscreen(None), &PredefinedMenuItem::close_window(None)]);
    let _ = menu.append_items(&[&app_menu, &edit, &window]);
    menu
}

fn main() {
    // Apps opened from Finder get a bare PATH; add the usual places developer tools live so that
    // commands such as git work the same as in a terminal.
    #[cfg(target_os = "macos")]
    {
        let current = std::env::var("PATH").unwrap_or_default();
        let extra = ["/opt/homebrew/bin", "/usr/local/bin"].iter().filter(|p| !current.split(':').any(|c| c == **p)).cloned().collect::<Vec<_>>();
        if !extra.is_empty() {
            std::env::set_var("PATH", format!("{}:{current}", extra.join(":")));
        }
    }
    let backend = start_backend();
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("Heron")
        .with_inner_size(LogicalSize::new(1180.0, 800.0))
        .with_min_inner_size(LogicalSize::new(420.0, 480.0))
        .build(&event_loop)
        .expect("could not create the Heron window");

    #[cfg(target_os = "macos")]
    let _menu = {
        let menu = build_menu();
        menu.init_for_nsapp();
        menu
    };

    let builder = match &backend {
        Ok(url) => {
            let origin = url.split("/?").next().unwrap_or("").to_string();
            WebViewBuilder::new().with_url(url).with_navigation_handler(move |target: String| {
                // Stay inside the app; anything else (links in replies) opens in the default browser.
                if target.starts_with(&origin) || target.starts_with("about:") || target.starts_with("data:") {
                    true
                } else {
                    if target.starts_with("http://") || target.starts_with("https://") {
                        let _ = open::that(&target);
                    }
                    false
                }
            })
        }
        Err(message) => WebViewBuilder::new().with_html(error_page(message)),
    };

    #[cfg(not(target_os = "linux"))]
    let _webview = builder.build(&window).expect("could not create the web view");
    #[cfg(target_os = "linux")]
    let _webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder.build_gtk(window.default_vbox().expect("window has no container")).expect("could not create the web view")
    };

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        if let Event::WindowEvent { event: WindowEvent::CloseRequested, .. } = event {
            *control_flow = ControlFlow::Exit;
        }
    });
}
