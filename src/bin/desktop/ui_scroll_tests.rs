//! Long content must wrap and scroll in every view. Each test fills the
//! window with fixture text: prose that wraps, plus long unbroken strings
//! (a URL, a path, a hash) like the ones tool output, approvals and the
//! activity log carry. No user or account data is rendered.

use crate::window_layout_tests::{write_bmp, HeadlessPlatform};
use crate::MainWindow;
use slint::platform::software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType};
use slint::platform::{WindowAdapter, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, Model, ModelRc, PhysicalSize, SharedString, StyledText, VecModel};
use std::rc::Rc;

const VIEWS: [&str; 9] = [
    "chat", "onboarding", "awareness", "memory", "tasks", "reminders", "usage", "activity", "settings",
];

fn prose(label: &str) -> String {
    format!(
        "{label}. This fixture sentence is long on purpose, so it has to wrap across several lines \
         in the narrowest supported window instead of running past the right edge."
    )
}

fn unbroken(label: &str) -> String {
    format!("https://example.com/{label}/{}", "a1b2c3d4e5f6".repeat(30))
}

fn both(label: &str) -> SharedString {
    format!("{} {}", prose(label), unbroken(label)).into()
}

fn model<T: Clone + 'static>(rows: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(rows))
}

fn headless() -> (Rc<MinimalSoftwareWindow>, MainWindow) {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(HeadlessPlatform(window.clone()))).expect("headless platform installs");
    let ui = MainWindow::new().expect("main window builds");
    ui.show().expect("main window shows");
    ui.set_reduce_motion(true);
    (window, ui)
}

fn render(window: &MinimalSoftwareWindow, width: u32, height: u32) -> Vec<PremultipliedRgbaColor> {
    window.request_redraw();
    let mut pixels = vec![PremultipliedRgbaColor::default(); (width * height) as usize];
    let drew = window.draw_if_needed(|renderer| {
        renderer.render(&mut pixels, width as usize);
    });
    assert!(drew, "window did not render at {width}x{height}");
    pixels
}

fn snapshot(name: &str, width: u32, height: u32, pixels: &[PremultipliedRgbaColor]) {
    if let Ok(directory) = std::env::var("HERON_UI_SNAPSHOTS_DIR") {
        std::fs::create_dir_all(&directory).expect("create screenshot artifact directory");
        let path = std::path::Path::new(&directory).join(format!("{name}-{width}x{height}.bmp"));
        write_bmp(&path, width, height, pixels);
    }
}

fn chat_message(i: usize) -> crate::ChatMsg {
    let text = format!("{} {}", prose(&format!("Message {i}")), unbroken("reply"));
    let steps = vec![crate::ToolStep {
        id: format!("step-{i}").into(),
        name: "shell".into(),
        label: prose(&format!("Ran a command for message {i}")).into(),
        summary: unbroken("summary").into(),
        risk: "low".into(),
        state: "ok".into(),
        preview: format!("{}\n{}", unbroken("output"), "output line\n".repeat(20)).into(),
        expanded: true,
    }];
    let approvals = if i % 10 == 3 {
        vec![crate::ApprovalRow {
            id: format!("approval-{i}").into(),
            tool: "shell".into(),
            title: both("Run this command?"),
            warning: both("Risk notice"),
            detail: format!("{}\n{}", unbroken("command"), "echo one line of a long script\n".repeat(40)).into(),
            danger: true,
            can_session: true,
            can_always: true,
            session_label: "Allow for this conversation".into(),
            always_label: "Always allow".into(),
        }]
    } else {
        Vec::new()
    };
    crate::ChatMsg {
        role: if i.is_multiple_of(2) { "user".into() } else { "assistant".into() },
        text: text.clone().into(),
        body: StyledText::from_plain_text(&text),
        steps: model(steps),
        approvals: model(approvals),
        notes: model(vec![both("Note")]),
        failure: if i % 7 == 6 { both("Failure") } else { SharedString::new() },
        complete: true,
        live: false,
    }
}

/// Every string a view displays (not the text fields the user types in)
/// gets prose and an unbroken string, every list gets enough rows to
/// overflow the window.
fn fill_with_long_content(ui: &MainWindow) {
    ui.set_session_title(both("Session title"));
    ui.set_workspace_short(unbroken("workspace").into());
    ui.set_workspace_full(unbroken("workspace").into());
    ui.set_starters(model((0..6).map(|i| both(&format!("Starter {i}"))).collect()));
    ui.set_sidebar_footer_text(both("Footer"));
    ui.set_backend_status(both("Status"));
    ui.set_sidebar_sessions(model(
        (0..40)
            .map(|i| crate::SessionRow {
                id: format!("s{i}").into(),
                title: both(&format!("Conversation {i}")),
                updated: "today".into(),
                messages: 3,
                current: i == 0,
            })
            .collect(),
    ));

    let memory_rows: Vec<crate::MemoryRow> = (0..30)
        .map(|i| crate::MemoryRow {
            id: i,
            kind_label: "fact".into(),
            content: both(&format!("Memory {i}")),
            meta: both("Meta"),
            confirmed: i % 2 == 0,
        })
        .collect();
    ui.set_memory_rows(model(memory_rows.clone()));
    ui.set_memory_save_error(both("Save error"));
    ui.set_tasks_rows(model(
        (0..30)
            .map(|i| crate::TaskRow { id: i, title: both(&format!("Task {i}")), meta: both("Meta"), done: i % 3 == 0 })
            .collect(),
    ));
    ui.set_tasks_save_error(both("Save error"));

    ui.set_reminders_schedules(model(
        (0..20)
            .map(|i| crate::ScheduleRow {
                id: i,
                title: both(&format!("Reminder {i}")),
                meta: both("Meta"),
                outline: both("Outline"),
                has_outline: true,
                has_last_run: true,
                uses_model: true,
                may: "read".into(),
            })
            .collect(),
    ));
    ui.set_reminders_delivered(model(
        (0..20)
            .map(|i| crate::InboxRow {
                title: both(&format!("Delivered {i}")),
                body: StyledText::from_plain_text(&both("Body")),
                meta: both("Meta"),
            })
            .collect(),
    ));
    ui.set_reminders_last_run_title(both("Last run"));
    ui.set_reminders_last_run_steps(model(
        (0..10).map(|i| crate::RunStepRow { text: both(&format!("Step {i}")), failed: i == 3 }).collect(),
    ));
    ui.set_reminders_form_error(both("Form error"));

    ui.set_awareness_workspace(unbroken("workspace").into());
    ui.set_awareness_proactive_note(both("Proactive note"));
    ui.set_awareness_suggestions(model(
        (0..15)
            .map(|i| crate::SuggestionRow {
                id: i,
                title: both(&format!("Suggestion {i}")),
                why: both("Why"),
                result: both("Result"),
                status: if i % 2 == 0 { "new".into() } else { "done".into() },
                has_session: i % 2 == 1,
            })
            .collect(),
    ));
    let simple: Vec<crate::SimpleRow> =
        (0..15).map(|i| crate::SimpleRow { text: both(&format!("Row {i}")), meta: both("Meta") }).collect();
    ui.set_awareness_right_now(model(simple.clone()));
    ui.set_awareness_insights(model(memory_rows));
    ui.set_awareness_journal(model(simple.clone()));

    ui.set_activity_seal_text(both("Seal"));
    ui.set_activity_rows(model(
        (0..60)
            .map(|i| crate::AuditRow {
                when: "today".into(),
                action: both(&format!("Action {i}")),
                details: both("Details"),
                outcome: both("Outcome"),
                denied: i % 4 == 0,
            })
            .collect(),
    ));

    ui.set_usage_level_note(both("Level"));
    ui.set_usage_pace_note(both("Pace"));
    ui.set_usage_bg_note(both("Background"));
    ui.set_usage_fast_note(both("Fast"));
    ui.set_usage_save_error(both("Save error"));
    ui.set_usage_models(model(
        (0..30)
            .map(|i| crate::ModelRow {
                model: unbroken(&format!("model-{i}")).into(),
                sent: "1,000".into(),
                written: "2,000".into(),
                calls: "3".into(),
            })
            .collect(),
    ));
    ui.set_usage_daily_visible(true);
    ui.set_usage_daily_rows(model(simple));

    // Step 2 lists the sources, the longest onboarding step.
    ui.set_onboarding_current_step(2);
    ui.set_onboarding_model_status(both("Model status"));
    ui.set_onboarding_about_message(both("About"));
    ui.set_onboarding_source_message(both("Source"));
    ui.set_onboarding_sources(model(
        ["documents", "calendar", "email"]
            .into_iter()
            .map(|kind| crate::SourceItem {
                kind: kind.into(),
                title: both(kind),
                status: both("Status"),
                connected: true,
                learn_enabled: true,
            })
            .collect(),
    ));

    ui.set_settings_key_note(both("Key"));
    ui.set_settings_sandbox_note(both("Sandbox"));
    ui.set_settings_browser_note(both("Browser"));
    ui.set_settings_mcp_note(both("Apps"));
    ui.set_settings_oauth_status(both("Accounts"));
    ui.set_settings_connector_vault_note(both("Vault"));
    ui.set_settings_connector_feedback(both("Connector"));
    ui.set_settings_telegram_status(both("Telegram"));
    ui.set_settings_saved_message(both("Saved"));
    ui.set_settings_error_message(both("Error"));
    ui.set_settings_backup_message(both("Backup"));
    ui.set_settings_tools(model(
        (0..20)
            .map(|i| crate::ToolItem { name: unbroken(&format!("tool-{i}")).into(), about: both("About"), setting: 0 })
            .collect(),
    ));
    ui.set_settings_mcp_lines(model(
        (0..10)
            .map(|i| crate::McpLine { text: both(&format!("App {i}")), name: unbroken("app").into(), changed: true })
            .collect(),
    ));
    ui.set_settings_connector_lines(model(
        (0..10)
            .map(|i| crate::ConnectorLine {
                id: format!("c{i}").into(),
                label: both(&format!("Connector {i}")),
                detail: both("Detail"),
                custom: true,
                has_credential: true,
            })
            .collect(),
    ));
}

#[test]
fn long_content_wraps_and_scrolls_in_every_view() {
    let (window, ui) = headless();
    fill_with_long_content(&ui);
    let mut failures = Vec::new();
    for (width, height) in [(880, 560), (1100, 700)] {
        window.set_size(PhysicalSize::new(width, height));
        for thread in ["empty", "long"] {
            let messages = if thread == "long" { (0..25).map(chat_message).collect() } else { Vec::new() };
            ui.set_messages(model(messages));
            ui.set_thread_version(ui.get_thread_version() + 1);
            for view in VIEWS {
                if thread == "empty" && view != "chat" {
                    continue;
                }
                ui.set_current_view(view.into());
                if view == "chat" {
                    // Start each pass on the newest message, as a new thread does.
                    render(&window, width, height);
                    scroll_thread(&window, -1_000_000.0);
                }
                let pixels = render(&window, width, height);
                snapshot(&format!("heron-long-{view}-{thread}"), width, height, &pixels);

                let shell_height = ui.get_debug_window_height();
                let overflow_x = ui.get_debug_view_overflow_x();
                let overflow_y = ui.get_debug_view_overflow_y();
                eprintln!(
                    "HERON_UI_SCROLL view={view} thread={thread} {width}x{height} overflow_x={overflow_x} overflow_y={overflow_y} shell={shell_height}"
                );
                if shell_height > height as f32 + 1.0 {
                    failures.push(format!("{view} ({thread}, {width}x{height}): window content is {shell_height}px tall"));
                }
                if overflow_x > 1.0 {
                    failures.push(format!(
                        "{view} ({thread}, {width}x{height}): content runs {overflow_x}px past the right edge"
                    ));
                }
                if view == "chat" && !thread_margin_is_clear(&ui, width, &pixels) {
                    failures.push(format!("chat ({thread}, {width}x{height}): message text is drawn outside its bubble"));
                }
                if view == "chat" && thread == "long" {
                    // Page up through the whole thread: every message
                    // must stay inside its bubble, not only the newest.
                    for _ in 0..500 {
                        if ui.get_debug_chat_scroll_y() >= 0.0 {
                            break;
                        }
                        // Half a visible page per step, so no message is skipped.
                        scroll_thread(&window, ui.get_debug_chat_thread_height() / 2.0);
                        let pixels = render(&window, width, height);
                        if !thread_margin_is_clear(&ui, width, &pixels) {
                            let at = ui.get_debug_chat_scroll_y();
                            failures.push(format!("chat ({width}x{height}, scrolled to {at}): message text is drawn outside its bubble"));
                            break;
                        }
                    }
                }
                if thread == "long" && overflow_y <= 0.0 {
                    failures.push(format!("{view} ({width}x{height}): long content fits, expected it to scroll"));
                }
                if view != "chat" {
                    ui.invoke_debug_scroll_view_to_end();
                    let pixels = render(&window, width, height);
                    snapshot(&format!("heron-long-{view}-end"), width, height, &pixels);
                }
            }
            let sessions_x = ui.get_debug_sessions_overflow_x();
            if sessions_x > 1.0 {
                failures.push(format!("sidebar ({width}x{height}): conversation list runs {sessions_x}px past the edge"));
            }
            if ui.get_debug_sessions_overflow_y() <= 0.0 {
                failures.push(format!("sidebar ({width}x{height}): 40 conversations fit, expected the list to scroll"));
            }
        }
    }
    assert!(failures.is_empty(), "views clip or overflow:\n{}", failures.join("\n"));
}

/// Message text that does not wrap inside its bubble is drawn wider than
/// the bubble and lands in the thread's left margin. Every pixel of that
/// margin should be plain background.
fn thread_margin_is_clear(ui: &MainWindow, width: u32, pixels: &[PremultipliedRgbaColor]) -> bool {
    let x = ui.get_debug_chat_thread_x().ceil() as u32 + 1;
    let top = ui.get_debug_chat_thread_y().ceil() as u32;
    // Clamp to the window: a layout that does not fit can place the
    // thread partly below it.
    let window_height = (pixels.len() / width as usize) as u32;
    let bottom = (top + ui.get_debug_chat_thread_height().floor() as u32).min(window_height);
    let rgb = |x: u32, y: u32| {
        let p = pixels[(y * width + x) as usize];
        (p.red, p.green, p.blue)
    };
    let background = rgb(x, top);
    (top..bottom).all(|y| (x..x + 4).all(|x| rgb(x, y) == background))
}

fn scroll_thread(window: &MinimalSoftwareWindow, delta_y: f32) {
    window.window().dispatch_event(WindowEvent::PointerMoved { position: LogicalPosition::new(560.0, 300.0) });
    window.window().dispatch_event(WindowEvent::PointerScrolled {
        position: LogicalPosition::new(560.0, 300.0),
        delta_x: 0.0,
        delta_y,
    });
}

fn at_newest(ui: &MainWindow) -> bool {
    (ui.get_debug_chat_scroll_y() + ui.get_debug_view_overflow_y()).abs() <= 1.0
}

#[test]
fn chat_thread_stays_on_the_newest_message_until_scrolled_up() {
    let (window, ui) = headless();
    let (width, height) = (880, 560);
    window.set_size(PhysicalSize::new(width, height));
    ui.set_current_view("chat".into());
    let messages = Rc::new(VecModel::from((0..12).map(chat_message).collect::<Vec<_>>()));
    ui.set_messages(ModelRc::from(messages.clone()));
    ui.set_thread_version(1);
    render(&window, width, height);
    assert!(ui.get_debug_view_overflow_y() > 0.0, "12 messages fit; the thread should scroll");
    assert!(at_newest(&ui), "a new thread opens on its newest message");

    messages.push(chat_message(messages.row_count()));
    ui.set_thread_version(2);
    render(&window, width, height);
    assert!(at_newest(&ui), "a new message keeps the thread on the newest message");

    scroll_thread(&window, 400.0);
    render(&window, width, height);
    assert!(!at_newest(&ui), "scrolling up leaves the newest message");
    let reading_at = ui.get_debug_chat_scroll_y();
    messages.push(chat_message(messages.row_count()));
    ui.set_thread_version(3);
    let pixels = render(&window, width, height);
    snapshot("heron-chat-scrolled-up", width, height, &pixels);
    assert!(thread_margin_is_clear(&ui, width, &pixels), "message text is drawn outside its bubble");
    assert_eq!(ui.get_debug_chat_scroll_y(), reading_at, "a new message does not move a reader who scrolled up");

    scroll_thread(&window, -100_000.0);
    render(&window, width, height);
    messages.push(chat_message(messages.row_count()));
    ui.set_thread_version(4);
    render(&window, width, height);
    assert!(at_newest(&ui), "scrolling back down follows new messages again");
}
