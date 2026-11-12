//! Model Context Protocol over stdio, both directions:
//! - client: connect to any MCP server and expose its tools to the agent;
//! - server: expose Heron's memory and tasks to other MCP-capable apps.

use crate::config::McpServerConfig;
use crate::permissions::Risk;
use crate::tools::{Tool, ToolCtx, ToolRegistry};
use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::oneshot;

const PROTOCOL_VERSION: &str = "2024-11-05";

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>;

pub struct McpClient {
    pub name: String,
    stdin: tokio::sync::Mutex<tokio::process::ChildStdin>,
    pending: Pending,
    next_id: AtomicU64,
    _child: Mutex<tokio::process::Child>,
}

impl McpClient {
    pub async fn connect(name: &str, cfg: &McpServerConfig) -> Result<Arc<McpClient>> {
        // On Windows, launchers such as npx are .cmd shims that must go through cmd.exe.
        #[cfg(windows)]
        let mut cmd = {
            let mut c = tokio::process::Command::new("cmd");
            c.arg("/C").arg(&cfg.command);
            c
        };
        #[cfg(not(windows))]
        let mut cmd = tokio::process::Command::new(&cfg.command);
        cmd.args(&cfg.args).envs(&cfg.env).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
        let mut child = cmd.spawn().with_context(|| format!("could not start MCP server '{name}' ({})", cfg.command))?;
        let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
        let pending: Pending = Default::default();

        let p = pending.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                if let Some(id) = v.get("id").and_then(|i| i.as_u64()) {
                    if v.get("method").is_none() {
                        if let Some(tx) = p.lock().unwrap().remove(&id) {
                            let _ = tx.send(v);
                        }
                    }
                }
            }
            p.lock().unwrap().clear(); // server exited: wake all waiters with an error
        });

        let client = Arc::new(McpClient { name: name.to_string(), stdin: tokio::sync::Mutex::new(stdin), pending, next_id: AtomicU64::new(1), _child: Mutex::new(child) });
        client
            .request("initialize", json!({"protocolVersion": PROTOCOL_VERSION, "capabilities": {}, "clientInfo": {"name": "heron", "version": env!("CARGO_PKG_VERSION")}}), 20)
            .await
            .with_context(|| format!("MCP server '{name}' did not complete initialization"))?;
        client.notify("notifications/initialized", json!({})).await?;
        Ok(client)
    }

    async fn send(&self, msg: &Value) -> Result<()> {
        let mut stdin = self.stdin.lock().await;
        stdin.write_all(format!("{msg}\n").as_bytes()).await?;
        stdin.flush().await?;
        Ok(())
    }

    async fn notify(&self, method: &str, params: Value) -> Result<()> {
        self.send(&json!({"jsonrpc": "2.0", "method": method, "params": params})).await
    }

    async fn request(&self, method: &str, params: Value, timeout_secs: u64) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})).await?;
        let resp = match tokio::time::timeout(Duration::from_secs(timeout_secs), rx).await {
            Ok(Ok(v)) => v,
            Ok(Err(_)) => bail!("MCP server '{}' exited", self.name),
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                bail!("MCP server '{}' timed out on {method}", self.name)
            }
        };
        if let Some(err) = resp.get("error") {
            bail!("MCP error: {}", err.get("message").and_then(|m| m.as_str()).unwrap_or("unknown"));
        }
        Ok(resp.get("result").cloned().unwrap_or(Value::Null))
    }

    pub async fn list_tools(&self) -> Result<Vec<Value>> {
        let r = self.request("tools/list", json!({}), 20).await?;
        Ok(r.get("tools").and_then(|t| t.as_array()).cloned().unwrap_or_default())
    }

    pub async fn call_tool(&self, name: &str, args: &Value, timeout_secs: u64) -> Result<String> {
        let r = self.request("tools/call", json!({"name": name, "arguments": args}), timeout_secs).await?;
        let text = r
            .get("content")
            .and_then(|c| c.as_array())
            .map(|items| {
                items
                    .iter()
                    .map(|i| match i.get("type").and_then(|t| t.as_str()) {
                        Some("text") => i.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string(),
                        Some(other) => format!("[{other} content omitted]"),
                        None => String::new(),
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        if r.get("isError").and_then(|e| e.as_bool()).unwrap_or(false) {
            bail!("{}", if text.is_empty() { "tool reported an error".to_string() } else { text });
        }
        Ok(text)
    }
}

/// An MCP server tool exposed to the agent as `mcp__<server>__<tool>`.
pub struct McpTool {
    client: Arc<McpClient>,
    full_name: String,
    tool_name: String,
    description: String,
    schema: Value,
    trust: bool,
    /// The server's tool definitions differ from what the user first connected.
    changed: bool,
}

#[async_trait]
impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.full_name
    }
    fn description(&self) -> &str {
        &self.description
    }
    fn parameters(&self) -> Value {
        self.schema.clone()
    }
    fn summary(&self, args: &Value) -> String {
        crate::util::preview(&args.to_string(), 160)
    }
    fn risk(&self, _args: &Value, _ctx: &ToolCtx) -> Risk {
        // We cannot know what a third-party tool does, so it needs approval unless the user marked the server trusted.
        if self.changed {
            Risk::Danger // a tool that silently changed its description must be re-approved every time
        } else if self.trust {
            Risk::Network
        } else {
            Risk::Exec
        }
    }
    fn untrusted_output(&self) -> bool {
        true
    }
    async fn call(&self, args: &Value, ctx: &ToolCtx) -> Result<String> {
        self.client.call_tool(&self.tool_name, args, ctx.cfg.agent.tool_timeout_secs).await
    }
}

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

pub struct Attached {
    pub tools: usize,
    /// Fingerprint of every tool's name, description and input schema.
    pub fingerprint: String,
    /// True when `pinned` was given and no longer matches.
    pub changed: bool,
}

fn fingerprint(tools: &[Value]) -> String {
    let mut parts: Vec<String> = tools.iter().map(|t| format!("{}\u{0}{}\u{0}{}", t["name"], t["description"], t["inputSchema"])).collect();
    parts.sort();
    let mut h: u64 = 0xcbf29ce484222325;
    for b in parts.join("\u{1}").as_bytes() {
        h = (h ^ *b as u64).wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// Connect to a server and register all of its tools. `pinned` is the fingerprint recorded when the user
/// first connected it; if the server's tool definitions have changed since (a "rug pull": a tool that
/// behaved until it was trusted, then rewrote its own description), every call needs approval again.
pub async fn attach(registry: &mut ToolRegistry, name: &str, cfg: &McpServerConfig, pinned: Option<&str>) -> Result<Attached> {
    let client = McpClient::connect(name, cfg).await?;
    let tools = client.list_tools().await?;
    let fingerprint = fingerprint(&tools);
    let changed = pinned.map(|p| p != fingerprint).unwrap_or(false);
    let mut n = 0;
    for t in tools {
        let Some(tool_name) = t.get("name").and_then(|n| n.as_str()) else { continue };
        let mut full_name = format!("mcp__{}__{}", sanitize(name), sanitize(tool_name));
        full_name.truncate(64); // provider limit on function names
        registry.register(Arc::new(McpTool {
            client: client.clone(),
            full_name,
            tool_name: tool_name.to_string(),
            description: format!("[MCP:{name}] {}", crate::util::preview(t.get("description").and_then(|d| d.as_str()).unwrap_or(""), 600)),
            schema: t.get("inputSchema").cloned().filter(|s| s.is_object()).unwrap_or_else(|| json!({"type": "object", "properties": {}})),
            trust: cfg.trust,
            changed,
        }));
        n += 1;
    }
    Ok(Attached { tools: n, fingerprint, changed })
}

/// Tools Heron offers to other apps when acting as an MCP server.
pub const SERVED_TOOLS: &[&str] = &["search_knowledge", "remember", "recall", "forget", "task_add", "task_list", "task_update", "schedule_add", "schedule_list", "schedule_remove"];

/// Serve MCP on the given streams until EOF.
pub async fn serve<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin>(reader: R, mut writer: W, registry: &ToolRegistry, ctx: &ToolCtx) -> Result<()> {
    let mut lines = reader.lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => {
                writer.write_all(format!("{}\n", json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "parse error"}})).as_bytes()).await?;
                writer.flush().await?;
                continue;
            }
        };
        let Some(id) = msg.get("id").cloned().filter(|i| !i.is_null()) else { continue }; // notification
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let result: std::result::Result<Value, (i64, String)> = match method {
            "initialize" => Ok(json!({"protocolVersion": PROTOCOL_VERSION, "capabilities": {"tools": {}}, "serverInfo": {"name": "heron", "version": env!("CARGO_PKG_VERSION")}})),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": SERVED_TOOLS.iter().filter_map(|n| registry.get(n)).map(|t| json!({"name": t.name(), "description": t.description(), "inputSchema": t.parameters()})).collect::<Vec<_>>()})),
            "tools/call" => {
                let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                match registry.get(name).filter(|_| SERVED_TOOLS.contains(&name)) {
                    None => Err((-32602, format!("unknown tool '{name}'"))),
                    Some(tool) => Ok(match tool.call(&args, ctx).await {
                        Ok(text) => json!({"content": [{"type": "text", "text": text}], "isError": false}),
                        Err(e) => json!({"content": [{"type": "text", "text": format!("{e:#}")}], "isError": true}),
                    }),
                }
            }
            other => Err((-32601, format!("method not found: {other}"))),
        };
        let resp = match result {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err((code, message)) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
        };
        writer.write_all(format!("{resp}\n").as_bytes()).await?;
        writer.flush().await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::memory::Memory;
    use crate::store::Store;

    #[tokio::test]
    async fn server_speaks_mcp() {
        let store = Store::in_memory().unwrap();
        let ctx = ToolCtx { workspace: ".".into(), memory: Memory::new(store.clone(), None), store, session_id: "mcp".into(), cfg: Arc::new(Config::default()), tainted: Default::default() };
        let input = [
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"remember","arguments":{"content":"The user's cat is named Miso"}}}),
            json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"recall","arguments":{"query":"cat name"}}}),
            json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"shell","arguments":{"command":"id"}}}),
            json!({"jsonrpc":"2.0","id":6,"method":"nope"}),
        ]
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        let mut out = Vec::new();
        serve(BufReader::new(input.as_bytes()), &mut out, &ToolRegistry::builtin(), &ctx).await.unwrap();
        let resps: Vec<Value> = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(resps.len(), 6);
        assert_eq!(resps[0]["result"]["serverInfo"]["name"], "heron");
        assert_eq!(resps[1]["result"]["tools"].as_array().unwrap().len(), SERVED_TOOLS.len());
        assert!(resps[3]["result"]["content"][0]["text"].as_str().unwrap().contains("Miso"));
        // the shell is never exposed over MCP
        assert_eq!(resps[4]["error"]["code"], -32602);
        assert_eq!(resps[5]["error"]["code"], -32601);
    }
}
