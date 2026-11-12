//! Provider-agnostic LLM client for any OpenAI-compatible chat-completions endpoint.
//! Streaming, tool calls, retries with backoff, and model fallback.

use crate::config::Config;
use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type", default = "function_kind")]
    pub kind: String,
    pub function: FunctionCall,
}

fn function_kind() -> String {
    "function".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

impl Message {
    pub fn system(s: impl Into<String>) -> Self {
        Message { role: "system".into(), content: Some(s.into()), tool_calls: vec![], tool_call_id: None }
    }
    pub fn user(s: impl Into<String>) -> Self {
        Message { role: "user".into(), content: Some(s.into()), tool_calls: vec![], tool_call_id: None }
    }
    pub fn assistant(s: impl Into<String>) -> Self {
        Message { role: "assistant".into(), content: Some(s.into()), tool_calls: vec![], tool_call_id: None }
    }
    pub fn tool(id: impl Into<String>, s: impl Into<String>) -> Self {
        Message { role: "tool".into(), content: Some(s.into()), tool_calls: vec![], tool_call_id: Some(id.into()) }
    }
    pub fn text(&self) -> &str {
        self.content.as_deref().unwrap_or("")
    }
}

#[derive(Debug, Clone, Default)]
pub struct ChatRequest {
    pub messages: Vec<Message>,
    /// OpenAI-format tool definitions.
    pub tools: Vec<Value>,
    /// Override the configured model (e.g. the fast model for background work).
    pub model: Option<String>,
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub message: Message,
    pub usage: Usage,
    pub model: String,
}

pub type DeltaFn<'a> = &'a (dyn Fn(&str) + Send + Sync);

#[async_trait]
pub trait Llm: Send + Sync {
    /// Run one chat completion. `on_delta` receives assistant text as it streams.
    async fn chat(&self, req: ChatRequest, on_delta: DeltaFn<'_>) -> Result<ChatResponse>;

    async fn embed(&self, _texts: &[String]) -> Result<Vec<Vec<f32>>> {
        bail!("embeddings not supported by this provider")
    }
}

pub struct HttpLlm {
    client: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
    models: Vec<String>,
    embedding_model: String,
    max_tokens: u32,
    max_retries: u32,
}

impl HttpLlm {
    pub fn from_config(cfg: &Config) -> Result<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(cfg.llm.request_timeout_secs))
            .user_agent(concat!("heron/", env!("CARGO_PKG_VERSION")))
            .build()?;
        let mut models = vec![cfg.llm.model.clone()];
        models.extend(cfg.llm.fallback_models.iter().cloned());
        Ok(HttpLlm {
            client,
            base_url: cfg.llm.base_url.trim_end_matches('/').to_string(),
            api_key: cfg.api_key(),
            models,
            embedding_model: cfg.llm.embedding_model.clone(),
            max_tokens: cfg.llm.max_tokens,
            max_retries: cfg.llm.max_retries,
        })
    }

    fn post(&self, path: &str) -> reqwest::RequestBuilder {
        let mut rb = self.client.post(format!("{}{}", self.base_url, path));
        if let Some(k) = &self.api_key {
            rb = rb.bearer_auth(k);
        }
        rb
    }

    async fn chat_once(&self, model: &str, req: &ChatRequest, on_delta: DeltaFn<'_>) -> Result<ChatResponse, CallError> {
        let mut body = json!({
            "model": model,
            "messages": req.messages,
            "stream": true,
            "stream_options": {"include_usage": true},
            "max_tokens": req.max_tokens.unwrap_or(self.max_tokens),
        });
        if !req.tools.is_empty() {
            body["tools"] = Value::Array(req.tools.clone());
        }
        let resp = self.post("/chat/completions").json(&body).send().await.map_err(|e| CallError::retryable(format!("network error: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            let retry_after = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse::<f64>().ok()).filter(|s| *s >= 0.0).map(Duration::from_secs_f64);
            let text = resp.text().await.unwrap_or_default();
            let msg = format!("{} from {}: {}", status, self.base_url, crate::util::preview(&text, 400));
            return Err(if status.as_u16() == 429 || status.is_server_error() || status.as_u16() == 408 {
                CallError { retry_after, ..CallError::retryable(msg) }
            } else {
                CallError::fatal(msg, status.as_u16())
            });
        }

        let mut acc = StreamAccumulator::default();
        let mut stream = resp.bytes_stream();
        let mut buf: Vec<u8> = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| {
                // A dropped stream after partial output is not safely retryable (text already shown).
                if acc.is_empty() { CallError::retryable(format!("stream error: {e}")) } else { CallError::fatal(format!("stream interrupted: {e}"), 0) }
            })?;
            buf.extend_from_slice(&chunk);
            while let Some(pos) = buf.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = buf.drain(..=pos).collect();
                let line = String::from_utf8_lossy(&line);
                acc.feed_line(line.trim_end(), on_delta).map_err(|e| CallError::fatal(e.to_string(), 0))?;
            }
        }
        if !buf.is_empty() {
            let line = String::from_utf8_lossy(&buf).to_string();
            acc.feed_line(line.trim_end(), on_delta).map_err(|e| CallError::fatal(e.to_string(), 0))?;
        }
        if acc.is_empty() {
            return Err(CallError::retryable("empty response from model".into()));
        }
        Ok(acc.finish(model))
    }
}

#[derive(Debug)]
struct CallError {
    msg: String,
    retryable: bool,
    status: u16,
    /// How long the provider asked us to wait before trying again.
    retry_after: Option<Duration>,
}

impl CallError {
    fn retryable(msg: String) -> Self {
        CallError { msg, retryable: true, status: 0, retry_after: None }
    }
    fn fatal(msg: String, status: u16) -> Self {
        CallError { msg, retryable: false, status, retry_after: None }
    }
}

/// How long to wait before retry number `attempt` (0-based). A provider's own Retry-After wins over
/// our backoff, within reason: hammering a rate-limited service only makes the limit last longer.
fn retry_delay(attempt: u32, retry_after: Option<Duration>) -> Duration {
    match retry_after {
        Some(asked) => asked.clamp(Duration::from_millis(200), Duration::from_secs(60)),
        None => Duration::from_millis(500 * 2u64.pow(attempt.min(6))),
    }
}

/// Assembles a streamed chat completion (SSE `data:` lines) into a single message.
#[derive(Default)]
pub struct StreamAccumulator {
    content: String,
    calls: Vec<ToolCall>,
    usage: Usage,
    saw_any: bool,
}

impl StreamAccumulator {
    pub fn is_empty(&self) -> bool {
        !self.saw_any
    }

    pub fn feed_line(&mut self, line: &str, on_delta: DeltaFn<'_>) -> Result<()> {
        let Some(data) = line.strip_prefix("data:") else { return Ok(()) };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            return Ok(());
        }
        let v: Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => return Ok(()), // tolerate keep-alives / provider comments
        };
        if let Some(err) = v.get("error") {
            bail!("provider error: {}", err.get("message").and_then(|m| m.as_str()).unwrap_or(&err.to_string()));
        }
        if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
            self.usage.prompt_tokens = u["prompt_tokens"].as_u64().unwrap_or(0);
            self.usage.completion_tokens = u["completion_tokens"].as_u64().unwrap_or(0);
        }
        let Some(choice) = v.get("choices").and_then(|c| c.get(0)) else { return Ok(()) };
        // Non-streaming shape (some servers ignore `stream`): choices[0].message
        let delta = choice.get("delta").or_else(|| choice.get("message"));
        let Some(delta) = delta else { return Ok(()) };
        if let Some(text) = delta.get("content").and_then(|c| c.as_str()) {
            if !text.is_empty() {
                self.saw_any = true;
                self.content.push_str(text);
                on_delta(text);
            }
        }
        if let Some(tcs) = delta.get("tool_calls").and_then(|t| t.as_array()) {
            for (pos, tc) in tcs.iter().enumerate() {
                self.saw_any = true;
                let idx = tc.get("index").and_then(|i| i.as_u64()).map(|i| i as usize).unwrap_or(pos);
                while self.calls.len() <= idx {
                    self.calls.push(ToolCall { id: String::new(), kind: "function".into(), function: FunctionCall { name: String::new(), arguments: String::new() } });
                }
                let call = &mut self.calls[idx];
                if let Some(id) = tc.get("id").and_then(|i| i.as_str()) {
                    if !id.is_empty() {
                        call.id = id.to_string();
                    }
                }
                if let Some(f) = tc.get("function") {
                    if let Some(n) = f.get("name").and_then(|n| n.as_str()) {
                        call.function.name.push_str(n);
                    }
                    match f.get("arguments") {
                        Some(Value::String(a)) => call.function.arguments.push_str(a),
                        Some(obj @ Value::Object(_)) => call.function.arguments = obj.to_string(),
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }

    pub fn finish(mut self, model: &str) -> ChatResponse {
        self.calls.retain(|c| !c.function.name.is_empty());
        for (i, c) in self.calls.iter_mut().enumerate() {
            if c.id.is_empty() {
                c.id = format!("call_{}_{}", i, uuid::Uuid::new_v4().simple());
            }
            if c.function.arguments.trim().is_empty() {
                c.function.arguments = "{}".into();
            }
        }
        ChatResponse {
            message: Message {
                role: "assistant".into(),
                content: if self.content.is_empty() { None } else { Some(self.content) },
                tool_calls: self.calls,
                tool_call_id: None,
            },
            usage: self.usage,
            model: model.to_string(),
        }
    }
}

#[async_trait]
impl Llm for HttpLlm {
    async fn chat(&self, req: ChatRequest, on_delta: DeltaFn<'_>) -> Result<ChatResponse> {
        let models: Vec<String> = match &req.model {
            Some(m) => std::iter::once(m.clone()).chain(self.models.iter().filter(|x| *x != m).cloned()).collect(),
            None => self.models.clone(),
        };
        let mut last_err = String::new();
        for model in &models {
            for attempt in 0..=self.max_retries {
                match self.chat_once(model, &req, on_delta).await {
                    Ok(r) => return Ok(r),
                    Err(e) => {
                        last_err = format!("[{model}] {}", e.msg);
                        // Auth errors will fail identically on every model: stop immediately.
                        if e.status == 401 || e.status == 403 {
                            return Err(anyhow!("{last_err}\nCheck your API key (see `heron doctor`)."));
                        }
                        if !e.retryable {
                            break; // try the next fallback model
                        }
                        if attempt < self.max_retries {
                            tokio::time::sleep(retry_delay(attempt, e.retry_after)).await;
                        }
                    }
                }
            }
        }
        Err(anyhow!("all models failed; last error: {last_err}"))
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if self.embedding_model.is_empty() {
            bail!("no embedding model configured");
        }
        let resp = self.post("/embeddings").json(&json!({"model": self.embedding_model, "input": texts})).send().await?;
        if !resp.status().is_success() {
            bail!("embedding request failed: {}", resp.status());
        }
        let v: Value = resp.json().await?;
        let data = v["data"].as_array().ok_or_else(|| anyhow!("malformed embedding response"))?;
        Ok(data
            .iter()
            .map(|d| d["embedding"].as_array().map(|a| a.iter().filter_map(|x| x.as_f64()).map(|x| x as f32).collect()).unwrap_or_default())
            .collect())
    }
}

/// Lets the app replace the model client at runtime (e.g. after the user changes settings).
pub struct SwappableLlm(std::sync::RwLock<std::sync::Arc<dyn Llm>>);

impl SwappableLlm {
    pub fn new(inner: std::sync::Arc<dyn Llm>) -> Self {
        SwappableLlm(std::sync::RwLock::new(inner))
    }
    pub fn swap(&self, inner: std::sync::Arc<dyn Llm>) {
        *self.0.write().unwrap() = inner;
    }
    fn current(&self) -> std::sync::Arc<dyn Llm> {
        self.0.read().unwrap().clone()
    }
}

#[async_trait]
impl Llm for SwappableLlm {
    async fn chat(&self, req: ChatRequest, on_delta: DeltaFn<'_>) -> Result<ChatResponse> {
        self.current().chat(req, on_delta).await
    }
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        self.current().embed(texts).await
    }
}

/// Build the model client for a configuration. `base_url = "demo"` selects the built-in demo model.
/// With HERON_CASSETTE set, model calls are recorded to (or replayed from) that file.
pub fn build(cfg: &Config) -> Result<std::sync::Arc<dyn Llm>> {
    let cassette = std::env::var("HERON_CASSETTE").ok().filter(|p| !p.is_empty()).map(std::path::PathBuf::from);
    let replay = std::env::var("HERON_CASSETTE_MODE").map(|m| m.eq_ignore_ascii_case("replay")).unwrap_or(false);
    if let (Some(path), true) = (&cassette, replay) {
        return Ok(std::sync::Arc::new(Cassette::replay(path)?));
    }
    let inner: std::sync::Arc<dyn Llm> = if is_demo(cfg) { std::sync::Arc::new(DemoLlm) } else { std::sync::Arc::new(HttpLlm::from_config(cfg)?) };
    Ok(match cassette {
        Some(path) => std::sync::Arc::new(Cassette::record(inner, &path)),
        None => inner,
    })
}

/// Record-and-replay of model calls, so a run made once against a real model can be repeated exactly
/// (and for free) in tests. Each call is keyed by a hash of the conversation so far and the tools on
/// offer; the system prompt is left out because it contains the date and local paths.
pub struct Cassette {
    inner: Option<std::sync::Arc<dyn Llm>>,
    path: std::path::PathBuf,
    recorded: std::sync::Mutex<std::collections::HashMap<String, std::collections::VecDeque<(Message, Usage)>>>,
}

impl Cassette {
    pub fn record(inner: std::sync::Arc<dyn Llm>, path: &std::path::Path) -> Self {
        Cassette { inner: Some(inner), path: path.to_path_buf(), recorded: Default::default() }
    }

    pub fn replay(path: &std::path::Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|e| anyhow!("cannot read the recording {}: {e}", path.display()))?;
        let mut recorded: std::collections::HashMap<String, std::collections::VecDeque<(Message, Usage)>> = Default::default();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let v: Value = serde_json::from_str(line)?;
            let message: Message = serde_json::from_value(v["message"].clone())?;
            let usage: Usage = serde_json::from_value(v["usage"].clone()).unwrap_or_default();
            recorded.entry(v["key"].as_str().unwrap_or("").to_string()).or_default().push_back((message, usage));
        }
        Ok(Cassette { inner: None, path: path.to_path_buf(), recorded: std::sync::Mutex::new(recorded) })
    }

    fn key(req: &ChatRequest) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        for m in req.messages.iter().filter(|m| m.role != "system") {
            h.update(m.role.as_bytes());
            h.update([0]);
            h.update(m.text().as_bytes());
            for c in &m.tool_calls {
                h.update(c.function.name.as_bytes());
                h.update(c.function.arguments.as_bytes());
            }
            h.update([1]);
        }
        for t in &req.tools {
            h.update(t["function"]["name"].as_str().unwrap_or("").as_bytes());
            h.update([2]);
        }
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
    }
}

#[async_trait]
impl Llm for Cassette {
    async fn chat(&self, req: ChatRequest, on_delta: DeltaFn<'_>) -> Result<ChatResponse> {
        let key = Self::key(&req);
        match &self.inner {
            Some(inner) => {
                let resp = inner.chat(req, on_delta).await?;
                let line = json!({"key": key, "message": resp.message, "usage": resp.usage, "model": resp.model}).to_string();
                use std::io::Write;
                let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&self.path)?;
                writeln!(file, "{line}")?;
                Ok(resp)
            }
            None => {
                // Identical requests are replayed in the order they were recorded.
                let next = self.recorded.lock().unwrap().get_mut(&key).and_then(|q| q.pop_front());
                let (message, usage) = next.ok_or_else(|| anyhow!("the recording has no answer for this request: the conversation differs from the one that was recorded (re-record with HERON_CASSETTE_MODE=record)"))?;
                if let Some(text) = &message.content {
                    on_delta(text);
                }
                Ok(ChatResponse { message, usage, model: "replay".into() })
            }
        }
    }
}

pub fn is_demo(cfg: &Config) -> bool {
    cfg.llm.base_url.trim().eq_ignore_ascii_case("demo")
}

/// Offline demo model: no key, no network. Replies are canned, but it issues real tool calls,
/// so permissions, the action track, memory and reminders can all be tried before connecting a model.
pub struct DemoLlm;

#[async_trait]
impl Llm for DemoLlm {
    async fn chat(&self, req: ChatRequest, on_delta: DeltaFn<'_>) -> Result<ChatResponse> {
        let last = req.messages.last().cloned().unwrap_or_else(|| Message::user(""));
        let user = req.messages.iter().rev().find(|m| m.role == "user").map(|m| m.text().to_lowercase()).unwrap_or_default();
        let system = req.messages.first().filter(|m| m.role == "system").map(|m| m.text().to_string()).unwrap_or_default();
        let call = |name: &str, args: Value| ScriptedLlm::tool_call(name, args);
        let reply = if last.role == "tool" {
            let result = last.text();
            if result.starts_with("Permission denied") {
                Message::assistant("That was not allowed, so I left everything as it was.")
            } else {
                Message::assistant(format!("Done. Here is the result:\n\n```\n{}\n```", crate::util::truncate_middle(result, 600)))
            }
        } else if req.tools.is_empty() {
            // Background requests (memory extraction, summaries).
            let fact = ["vegetarian", "vegan", "allergic"].iter().find(|w| user.contains(*w));
            Message::assistant(match fact {
                Some(w) if user.contains("durable facts") => format!("[{{\"content\":\"The user mentioned being {w}\",\"kind\":\"profile\",\"importance\":0.8}}]"),
                _ if user.contains("durable facts") => "[]".to_string(),
                _ => "- (demo mode) earlier conversation summarized".to_string(),
            })
        } else if user.contains("remind") {
            call("schedule_add", json!({"when": "tomorrow 9:00", "prompt": "Remind the user: call the dentist", "name": "Call the dentist"}))
        } else if user.contains("folder") || user.contains("list") {
            call("list_dir", json!({"path": "."}))
        } else if user.contains("plan.md") || user.contains("write") || user.contains("save") {
            call("write_file", json!({"path": "plan.md", "content": "# Plan\n\n1. Connect a real model in Settings\n2. Ask Heron for the real plan\n"}))
        } else if user.contains("run") || user.contains("command") {
            call("shell", json!({"command": if cfg!(windows) { "Get-Date" } else { "date" }}))
        } else if user.contains("task") || user.contains("todo") {
            call("task_add", json!({"title": "Try Heron with a real model"}))
        } else if user.contains("remember about me") || user.contains("know about me") {
            let known: Vec<&str> = system.lines().skip_while(|l| !l.starts_with("## About the user")).skip(1).take_while(|l| l.starts_with("- ")).collect();
            Message::assistant(if known.is_empty() { "Nothing yet. Tell me something about yourself, or add it under Memory.".to_string() } else { format!("Here is what I know about you:\n\n{}", known.join("\n")) })
        } else {
            Message::assistant("This is **demo mode**: my replies are canned, but the actions are real. Try asking me to *list this folder*, *save a plan as plan.md*, *run a command*, or *remind me to call the dentist*. For real answers, connect a model in Settings.")
        };
        // Stream word by word so the interface behaves as it does with a real model.
        if let Some(text) = &reply.content {
            for word in text.split_inclusive(' ') {
                on_delta(word);
                tokio::time::sleep(Duration::from_millis(12)).await;
            }
        }
        Ok(ChatResponse { message: reply, usage: Usage::default(), model: "demo".into() })
    }
}

/// Deterministic scripted model for tests and offline demos.
pub struct ScriptedLlm {
    responses: std::sync::Mutex<std::collections::VecDeque<Message>>,
    pub requests: std::sync::Mutex<Vec<ChatRequest>>,
}

impl ScriptedLlm {
    pub fn new(responses: Vec<Message>) -> Self {
        ScriptedLlm { responses: std::sync::Mutex::new(responses.into()), requests: Default::default() }
    }

    pub fn tool_call(name: &str, args: Value) -> Message {
        Message {
            role: "assistant".into(),
            content: None,
            tool_calls: vec![ToolCall { id: format!("call_{}", uuid::Uuid::new_v4().simple()), kind: "function".into(), function: FunctionCall { name: name.into(), arguments: args.to_string() } }],
            tool_call_id: None,
        }
    }
}

#[async_trait]
impl Llm for ScriptedLlm {
    async fn chat(&self, req: ChatRequest, on_delta: DeltaFn<'_>) -> Result<ChatResponse> {
        self.requests.lock().unwrap().push(req);
        let msg = self.responses.lock().unwrap().pop_front().unwrap_or_else(|| Message::assistant("(script exhausted)"));
        if let Some(c) = &msg.content {
            on_delta(c);
        }
        Ok(ChatResponse { message: msg, usage: Usage { prompt_tokens: 10, completion_tokens: 5 }, model: "scripted".into() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulates_streamed_text_and_tool_calls() {
        let mut acc = StreamAccumulator::default();
        let seen = std::sync::Mutex::new(String::new());
        let cb = |s: &str| seen.lock().unwrap().push_str(s);
        for line in [
            r#"data: {"choices":[{"delta":{"role":"assistant","content":"Hel"}}]}"#,
            r#"data: {"choices":[{"delta":{"content":"lo"}}]}"#,
            ": keep-alive",
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"read_file","arguments":"{\"pa"}}]}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"th\":\"a\"}"}}]}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":1,"id":"c2","function":{"name":"now","arguments":""}}]}}]}"#,
            r#"data: {"choices":[],"usage":{"prompt_tokens":7,"completion_tokens":3}}"#,
            "data: [DONE]",
        ] {
            acc.feed_line(line, &cb).unwrap();
        }
        let r = acc.finish("m");
        assert_eq!(r.message.content.as_deref(), Some("Hello"));
        assert_eq!(*seen.lock().unwrap(), "Hello");
        assert_eq!(r.message.tool_calls.len(), 2);
        assert_eq!(r.message.tool_calls[0].function.arguments, r#"{"path":"a"}"#);
        assert_eq!(r.message.tool_calls[1].function.arguments, "{}");
        assert_eq!(r.usage, Usage { prompt_tokens: 7, completion_tokens: 3 });
    }

    #[test]
    fn a_providers_retry_after_is_respected_within_reason() {
        assert_eq!(retry_delay(0, None), Duration::from_millis(500));
        assert_eq!(retry_delay(3, None), Duration::from_secs(4));
        assert_eq!(retry_delay(0, Some(Duration::from_secs(7))), Duration::from_secs(7));
        assert_eq!(retry_delay(5, Some(Duration::from_secs(3600))), Duration::from_secs(60), "never sleeps for an hour");
        assert_eq!(retry_delay(0, Some(Duration::ZERO)), Duration::from_millis(200));
    }

    #[tokio::test]
    async fn recorded_runs_replay_exactly_and_refuse_a_different_conversation() {
        let dir = tempfile::tempdir().unwrap();
        let tape = dir.path().join("run.jsonl");
        let real = std::sync::Arc::new(ScriptedLlm::new(vec![ScriptedLlm::tool_call("list_dir", json!({"path": "."})), Message::assistant("Two files.")]));
        let ask = |text: &str, extra: Option<Message>| {
            let mut messages = vec![Message::system("the date and paths here change every run"), Message::user(text)];
            messages.extend(extra);
            ChatRequest { messages, tools: vec![json!({"function": {"name": "list_dir"}})], ..Default::default() }
        };
        // Record two calls against the "real" model.
        let rec = Cassette::record(real, &tape);
        let first = rec.chat(ask("what is here?", None), &|_| {}).await.unwrap();
        let second = rec.chat(ask("what is here?", Some(first.message.clone())), &|_| {}).await.unwrap();
        assert_eq!(second.message.text(), "Two files.");
        // Replay needs no model at all, and the changing system prompt does not matter.
        let replay = Cassette::replay(&tape).unwrap();
        let mut req = ask("what is here?", None);
        req.messages[0] = Message::system("a different date and different paths");
        let again = replay.chat(req, &|_| {}).await.unwrap();
        assert_eq!(again.message.tool_calls[0].function.name, "list_dir");
        assert_eq!(replay.chat(ask("what is here?", Some(again.message.clone())), &|_| {}).await.unwrap().message.text(), "Two files.");
        // A conversation that was never recorded is an error, not a made-up answer.
        let err = replay.chat(ask("something else entirely", None), &|_| {}).await.unwrap_err();
        assert!(err.to_string().contains("no answer for this request"));
    }

    #[test]
    fn stream_error_is_surfaced() {
        let mut acc = StreamAccumulator::default();
        let e = acc.feed_line(r#"data: {"error":{"message":"overloaded"}}"#, &|_| {}).unwrap_err();
        assert!(e.to_string().contains("overloaded"));
    }

    #[test]
    fn message_serialization_is_openai_shaped() {
        let m = Message::tool("c1", "ok");
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v, json!({"role":"tool","content":"ok","tool_call_id":"c1"}));
    }
}
