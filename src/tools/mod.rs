//! Tool abstraction and registry. A tool declares its own risk per call so the
//! policy layer can decide without knowing tool internals.

pub mod fs;
pub mod personal;
pub mod shell;
pub mod web;

use crate::config::Config;
use crate::memory::Memory;
use crate::permissions::Risk;
use crate::store::Store;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures_util::future::BoxFuture;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone)]
pub struct ToolCtx {
    pub workspace: PathBuf,
    pub store: Store,
    pub memory: Memory,
    pub session_id: String,
    pub cfg: Arc<Config>,
    /// Set once content from outside the user's control has entered the conversation.
    pub tainted: Arc<std::sync::atomic::AtomicBool>,
    /// Web addresses that were in the conversation before the model asked for them.
    pub seen_urls: SeenUrls,
}

/// The web addresses that have appeared in a conversation: typed by the user, or returned by a tool.
///
/// After outside content has been read, a hidden instruction can try to make the agent "fetch" an address
/// it has built out of the user's private data, which hands that data to whoever runs the site. An
/// address that was already in the conversation, exactly, cannot have been built that way, so those may
/// still be fetched freely; any other address needs the user's approval first.
#[derive(Clone, Default)]
pub struct SeenUrls(Arc<std::sync::Mutex<std::collections::HashSet<String>>>);

impl SeenUrls {
    const MAX: usize = 20_000;

    fn key(raw: &str) -> Option<String> {
        let mut url = url::Url::parse(raw.trim()).ok()?;
        url.set_fragment(None);
        Some(url.to_string())
    }

    /// Record every address found in a piece of text.
    pub fn note(&self, text: &str) {
        static FIND: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let find = FIND.get_or_init(|| regex::Regex::new(r#"https?://[^\s"'<>\\`\[\]{}|^]+"#).unwrap());
        let mut seen = self.0.lock().unwrap_or_else(|e| e.into_inner());
        for m in find.find_iter(text) {
            if seen.len() >= Self::MAX {
                return;
            }
            // Addresses in prose usually drag a full stop or bracket along; keep both readings.
            let raw = m.as_str();
            let trimmed = raw.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', '\'', '"', '*', '_']);
            for candidate in [raw, trimmed] {
                if let Some(k) = Self::key(candidate) {
                    seen.insert(k);
                }
            }
        }
    }

    pub fn contains(&self, raw: &str) -> bool {
        Self::key(raw).map(|k| self.0.lock().unwrap_or_else(|e| e.into_inner()).contains(&k)).unwrap_or(false)
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    /// JSON Schema for the arguments object.
    fn parameters(&self) -> Value;
    /// One-line description of this specific call (shown to the user, matched by permission rules).
    fn summary(&self, args: &Value) -> String;
    fn risk(&self, args: &Value, ctx: &ToolCtx) -> Risk;
    /// Longer text for the approval prompt.
    fn detail(&self, args: &Value, _ctx: &ToolCtx) -> String {
        self.summary(args)
    }
    /// Whether results come from outside the user's control (wrapped as untrusted data).
    fn untrusted_output(&self) -> bool {
        false
    }
    async fn call(&self, args: &Value, ctx: &ToolCtx) -> Result<String>;
}

/// Function-pointer based tool, to keep built-in tool definitions compact.
pub struct FnTool {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Value,
    pub summary: fn(&Value) -> String,
    pub risk: fn(&Value, &ToolCtx) -> Risk,
    pub detail: Option<fn(&Value, &ToolCtx) -> String>,
    pub untrusted: bool,
    pub run: for<'a> fn(&'a Value, &'a ToolCtx) -> BoxFuture<'a, Result<String>>,
}

#[async_trait]
impl Tool for FnTool {
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &str {
        self.description
    }
    fn parameters(&self) -> Value {
        self.parameters.clone()
    }
    fn summary(&self, args: &Value) -> String {
        (self.summary)(args)
    }
    fn risk(&self, args: &Value, ctx: &ToolCtx) -> Risk {
        (self.risk)(args, ctx)
    }
    fn detail(&self, args: &Value, ctx: &ToolCtx) -> String {
        match self.detail {
            Some(f) => f(args, ctx),
            None => (self.summary)(args),
        }
    }
    fn untrusted_output(&self) -> bool {
        self.untrusted
    }
    async fn call(&self, args: &Value, ctx: &ToolCtx) -> Result<String> {
        (self.run)(args, ctx).await
    }
}

#[derive(Default)]
pub struct ToolRegistry {
    tools: Vec<Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// All built-in tools.
    pub fn builtin() -> Self {
        let mut r = Self::new();
        for t in fs::tools().into_iter().chain(shell::tools()).chain(web::tools()).chain(personal::tools()) {
            r.register(Arc::new(t));
        }
        r
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.retain(|t| t.name() != tool.name());
        self.tools.push(tool);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.iter().find(|t| t.name() == name).cloned()
    }

    pub fn names(&self) -> Vec<String> {
        self.tools.iter().map(|t| t.name().to_string()).collect()
    }

    /// OpenAI-format tool definitions.
    pub fn schemas(&self) -> Vec<Value> {
        self.tools
            .iter()
            .map(|t| json!({"type": "function", "function": {"name": t.name(), "description": t.description(), "parameters": t.parameters()}}))
            .collect()
    }
}

// ---- argument helpers ----

pub fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key).and_then(|v| v.as_str()).ok_or_else(|| anyhow!("missing required string argument '{key}'"))
}

pub fn opt_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|v| v.as_str()).filter(|s| !s.is_empty())
}

/// Integers may arrive as numbers or numeric strings depending on the model.
pub fn opt_i64(args: &Value, key: &str) -> Option<i64> {
    match args.get(key) {
        Some(Value::Number(n)) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Some(Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    }
}

pub fn opt_f64(args: &Value, key: &str) -> Option<f64> {
    match args.get(key) {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    }
}

pub fn opt_bool(args: &Value, key: &str) -> bool {
    match args.get(key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s == "true",
        _ => false,
    }
}

pub fn schema(props: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": props, "required": required, "additionalProperties": false})
}

#[cfg(test)]
mod seen_url_tests {
    use super::SeenUrls;

    #[test]
    fn only_addresses_that_were_in_the_conversation_count_as_seen() {
        let seen = SeenUrls::default();
        seen.note("Results:\n1. Rust https://www.rust-lang.org/learn\n2. See (https://example.com/a?b=1). Also <a href=\"https://docs.rs/tokio\">tokio</a>");
        for known in ["https://www.rust-lang.org/learn", "https://example.com/a?b=1", "https://docs.rs/tokio", "https://docs.rs/tokio#section", " https://www.rust-lang.org/learn "] {
            assert!(seen.contains(known), "{known}");
        }
        // The same site with anything added is a different address: that is where data would be carried.
        for made_up in ["https://www.rust-lang.org/learn?note=secret", "https://www.rust-lang.org/learn/secret", "https://secret.rust-lang.org/learn", "https://example.com/a?b=1&c=secret", "not a url"] {
            assert!(!seen.contains(made_up), "{made_up}");
        }
    }
}
