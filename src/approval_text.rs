//! Why Heron is asking: the `why` text an approval carries (F2).
//!
//! Built in the core from the approval's `risk` label, the mode, the taint flag and the taint source,
//! so every surface (the app, Telegram) says the same thing. It is one or two sentences, never empty.
//! The first says why this action asks; the second, only when the conversation is tainted, says what
//! outside content restricted it.
//!
//! The first sentences for the standing-change tools and the `DANGEROUS`, `sensitive`, `external` and
//! `egress` labels are the Slint app's `approval_warning` sentences, word for word; the Slint app
//! itself does not change.

use crate::permissions::Mode;
use crate::store::Store;
use serde_json::Value;

/// The tools that change what Heron does from now on. They always ask.
const STANDING_CHANGE_TOOLS: [&str; 3] = ["save_automation", "update_persona", "save_skill"];

const STANDING_CHANGE: &str = "This changes what Heron does from now on, so it always needs your OK. Read it through before allowing.";
const OUTSIDE_READ: &str = "This reads a file outside your working folder. Reading there asks first unless you have opened that folder for Heron.";
/// The tainted sentence from before sources were recorded, for a taint whose source is unknown.
const TAINTED_NO_SOURCE: &str = "Heron has read content from outside (a web page, an email or a connected app) in this conversation. Check that this is something you asked for.";

/// What first tainted a conversation: the `kind`, `name` and `detail` of the value
/// [`crate::taint::mark`] writes. `detail` never holds content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaintSource {
    pub kind: String,
    pub name: String,
    pub detail: String,
}

impl TaintSource {
    /// The source in a stored taint value, as [`crate::taint::source`] returns it.
    pub fn from_value(v: &Value) -> Option<TaintSource> {
        let field = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
        Some(TaintSource { kind: field("kind")?, name: field("name").unwrap_or_default(), detail: field("detail").unwrap_or_default() })
    }

    /// What first tainted `session_id`; `None` when it is not tainted, or was tainted before
    /// sources were recorded.
    pub fn load(store: &Store, session_id: &str) -> Option<TaintSource> {
        crate::taint::source(store, session_id).as_ref().and_then(TaintSource::from_value)
    }

    /// What Heron read, for the sentence "Heron has read `what` in this conversation". `None` for a
    /// kind this version does not know.
    fn what(&self) -> Option<String> {
        Some(match self.kind.as_str() {
            "tool" if !self.detail.is_empty() => self.detail.clone(),
            "tool" => format!("the output of {}", self.name),
            "chat_context" => format!("text from {}", self.detail),
            "mcp" => "a request from another app over MCP".into(),
            "telegram_forward" => "a forwarded Telegram message".into(),
            "trigger" => format!("the event that started {}", self.name),
            "suggestion" => "a task Heron prepared from what it observed".into(),
            _ => return None,
        })
    }
}

/// The first sentence: why this action asks at all.
fn first_sentence(tool: &str, risk: &str, mode: Mode) -> String {
    if STANDING_CHANGE_TOOLS.contains(&tool) {
        return STANDING_CHANGE.into();
    }
    match risk {
        "DANGEROUS" => "This could delete data, reach outside your folder, or be hard to undo.".into(),
        "sensitive" => "This reads something that may hold passwords, keys or other secrets, and would show it to your AI model.".into(),
        "external" => "This acts on a website or another app. Heron cannot undo it.".into(),
        "egress" => "This sends text Heron wrote to somewhere outside your computer. Check that nothing private is in it.".into(),
        "outside" => OUTSIDE_READ.into(),
        _ => format!("Heron asks before this in {} mode.", mode.name()),
    }
}

/// The sentence for a tainted conversation: what was read, when the source is known, else the
/// tainted sentence from before sources were recorded.
pub fn taint_sentence(source: Option<&TaintSource>) -> String {
    match source.and_then(TaintSource::what) {
        Some(what) => format!("Heron has read {what} in this conversation, so changes and sends ask first."),
        None => TAINTED_NO_SOURCE.into(),
    }
}

/// Why Heron is asking before `tool`, whose effect is labelled `risk`, in `mode`. One or two
/// sentences, never empty; the second is there only when the conversation is `tainted`.
pub fn why(tool: &str, risk: &str, mode: Mode, tainted: bool, source: Option<&TaintSource>) -> String {
    let mut text = first_sentence(tool, risk, mode);
    if tainted {
        text.push(' ');
        text.push_str(&taint_sentence(source));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::Effect;

    const MODES: [Mode; 4] = [Mode::ReadOnly, Mode::Ask, Mode::Auto, Mode::Yolo];
    const TOOLS: [&str; 7] = ["save_automation", "update_persona", "save_skill", "shell", "write_file", "read_file", "web_fetch"];

    fn src(kind: &str, name: &str, detail: &str) -> TaintSource {
        TaintSource { kind: kind.into(), name: name.into(), detail: detail.into() }
    }

    /// Every label an approval can carry, plus one no effect has.
    fn labels() -> Vec<&'static str> {
        Effect::ALL.iter().map(Effect::label).chain(["unknown-label", ""]).collect()
    }

    #[test]
    fn why_matches_the_old_warning_when_untainted() {
        let mut compared = 0;
        for tool in TOOLS {
            for risk in labels() {
                let old = crate::ui::chat::approval_warning(tool, risk, false);
                if old.is_empty() {
                    continue;
                }
                for mode in MODES {
                    assert_eq!(why(tool, risk, mode, false, None), old, "{tool} {risk} {mode:?}");
                }
                compared += 1;
            }
        }
        // The three standing-change tools on every label, and the four labels the old warning named
        // for each other tool.
        assert_eq!(compared, 3 * labels().len() + 4 * (TOOLS.len() - 3));
    }

    #[test]
    fn why_is_never_empty() {
        let sources = [
            None,
            Some(src("tool", "web_fetch", "example.com")),
            Some(src("tool", "read_email", "")),
            Some(src("chat_context", "selection", "Preview")),
            Some(src("mcp", "mcp", "conn-1")),
            Some(src("telegram_forward", "telegram", "")),
            Some(src("trigger", "Invoices", "email")),
            Some(src("suggestion", "suggestion", "")),
            Some(src("from_a_later_version", "x", "y")),
        ];
        for tool in TOOLS {
            for risk in labels() {
                for mode in MODES {
                    let untainted = why(tool, risk, mode, false, None);
                    assert!(!untainted.trim().is_empty() && untainted.ends_with('.'), "{tool} {risk} {mode:?}: {untainted:?}");
                    for source in &sources {
                        let tainted = why(tool, risk, mode, true, source.as_ref());
                        // The first sentence stays, and a second follows it.
                        let second = tainted.strip_prefix(&format!("{untainted} ")).unwrap_or_else(|| panic!("{tainted:?} does not start with {untainted:?}"));
                        assert!(second.starts_with("Heron has read ") && second.ends_with('.'), "{tool} {risk} {mode:?} {source:?}: {tainted:?}");
                    }
                }
            }
        }
        // Without a taint the source is ignored.
        assert_eq!(why("shell", "exec", Mode::Ask, false, Some(&src("mcp", "mcp", "c"))), "Heron asks before this in ask mode.");
        assert_eq!(why("write_file", "write", Mode::Auto, false, None), "Heron asks before this in auto mode.");
    }

    #[test]
    fn why_names_the_taint_source() {
        let second = |s: TaintSource| why("write_file", "write", Mode::Ask, true, Some(&s)).strip_prefix("Heron asks before this in ask mode. ").unwrap().to_string();
        let sentence = |what: &str| format!("Heron has read {what} in this conversation, so changes and sends ask first.");
        assert_eq!(second(src("tool", "web_fetch", "127.0.0.1")), sentence("127.0.0.1"), "the host, when there is one");
        assert_eq!(second(src("tool", "read_email", "")), sentence("the output of read_email"));
        assert_eq!(second(src("chat_context", "selection", "Preview")), "Heron has read text from Preview in this conversation, so changes and sends ask first.", "CUJ F-1, word for word");
        assert_eq!(second(src("mcp", "mcp", "conn-1")), sentence("a request from another app over MCP"));
        assert_eq!(second(src("telegram_forward", "telegram", "")), sentence("a forwarded Telegram message"));
        assert_eq!(second(src("trigger", "Invoices", "email")), sentence("the event that started Invoices"));
        assert_eq!(second(src("trigger", "Watch Downloads", "file_change")), sentence("the event that started Watch Downloads"));
        assert_eq!(second(src("suggestion", "suggestion", "")), sentence("a task Heron prepared from what it observed"));
        // No source (a value from before sources were recorded), or a kind this version does not
        // know: today's tainted sentence, word for word.
        let old = crate::ui::chat::approval_warning("write_file", "write", true);
        assert_eq!(why("write_file", "write", Mode::Ask, true, None), format!("Heron asks before this in ask mode. {old}"));
        assert_eq!(second(src("from_a_later_version", "x", "y")), old);
        // The stored value is read back into a source.
        let stored = serde_json::json!({"kind": "chat_context", "name": "selection", "detail": "Preview", "ts": 1});
        assert_eq!(TaintSource::from_value(&stored), Some(src("chat_context", "selection", "Preview")));
        assert_eq!(TaintSource::from_value(&serde_json::json!({"name": "x"})), None, "a value with no kind has no source");
    }

    #[test]
    fn why_covers_outside_reads() {
        assert_eq!(Effect::OutsideRead.label(), "outside");
        let first = "This reads a file outside your working folder. Reading there asks first unless you have opened that folder for Heron.";
        for mode in MODES {
            assert_eq!(why("read_file", "outside", mode, false, None), first, "{mode:?}");
        }
        assert_eq!(
            why("read_file", "outside", Mode::Ask, true, Some(&src("tool", "web_fetch", "example.com"))),
            format!("{first} Heron has read example.com in this conversation, so changes and sends ask first.")
        );
        // A standing change still says so, whatever its label.
        assert!(why("save_skill", "outside", Mode::Ask, false, None).starts_with("This changes what Heron does from now on"));
    }
}
