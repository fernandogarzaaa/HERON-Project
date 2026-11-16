//! Revocable remote MCP broker. Registration, discovery and stored credentials
//! never grant execution. Process-local grants expire after one hour or restart.
mod schema;
#[cfg(test)]
mod tests;
mod wire;
use crate::{
    config::Config,
    connectors::CustomConnector,
    permissions::Effect,
    tools::{Tool, ToolCtx},
};
use async_trait::async_trait;
use schema::RemoteTool;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{watch, Semaphore};
use url::Url;
type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, Clone, Serialize)]
pub struct Error {
    pub code: &'static str,
    pub message: &'static str,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Error {}
fn fail(code: &'static str, message: &'static str) -> Error {
    Error { code, message }
}
fn revoked() -> Error {
    fail(
        "revoked",
        "Connector access was revoked or changed; test and review it again.",
    )
}
fn credential_hash(key: Option<&str>) -> Option<[u8; 32]> {
    key.map(|k| Sha256::digest(k.as_bytes()).into())
}

fn contains_secret(value: &Value, key: &str) -> bool {
    match value {
        Value::String(s) => s.contains(key),
        Value::Array(a) => a.iter().any(|v| contains_secret(v, key)),
        Value::Object(o) => o
            .iter()
            .any(|(k, v)| k.contains(key) || contains_secret(v, key)),
        _ => false,
    }
}
fn redact_result(value: Value, key: &str) -> Value {
    match value {
        Value::String(s) => Value::String(s.replace(key, "[credential redacted]")),
        Value::Array(a) => Value::Array(a.into_iter().map(|v| redact_result(v, key)).collect()),
        Value::Object(o) => Value::Object(
            o.into_iter()
                .map(|(k, v)| {
                    (
                        k.replace(key, "[credential redacted]"),
                        redact_result(v, key),
                    )
                })
                .collect(),
        ),
        v => v,
    }
}

struct Connection {
    spec: CustomConnector,
    revision: String,
    credential_hash: Option<[u8; 32]>,
    peer: Arc<wire::Peer>,
    tools: Vec<RemoteTool>,
    fingerprint: String,
    manifest_hash: String,
    granted: Mutex<Option<(HashSet<String>, Instant)>>,
    busy: Semaphore,
    last_call: Mutex<Option<Instant>>,
}
struct Slot {
    nonce: String,
    cancel: watch::Sender<bool>,
    connection: Option<Arc<Connection>>,
}
pub struct Runtime {
    slots: Mutex<HashMap<String, Slot>>,
    client: reqwest::Client,
    #[cfg(test)]
    fixture: Option<Url>,
}
// Aborting an agent turn or dropping its future must fence the grant too,
// not merely drop the TCP request and leave apparently reusable authorization.
struct InvocationGuard<'a> {
    runtime: &'a Runtime,
    id: &'a str,
    nonce: String,
    armed: bool,
}
impl Drop for InvocationGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.runtime.revoke_if_current(self.id, &self.nonce);
        }
    }
}
impl Runtime {
    pub fn new() -> anyhow::Result<Arc<Self>> {
        Ok(Arc::new(Self {
            slots: Mutex::new(HashMap::new()),
            client: wire::client()?,
            #[cfg(test)]
            fixture: None,
        }))
    }
    pub fn revoke(&self, id: &str) {
        if let Some(slot) = self.slots.lock().unwrap().remove(id) {
            slot.cancel.send_replace(true);
        }
    }
    fn revoke_if_current(&self, id: &str, nonce: &str) {
        let mut slots = self.slots.lock().unwrap();
        if slots.get(id).is_some_and(|slot| slot.nonce == nonce) {
            if let Some(slot) = slots.remove(id) {
                slot.cancel.send_replace(true);
            }
        }
    }
    fn reset(&self, id: &str) -> Result<(String, watch::Receiver<bool>)> {
        let mut slots = self.slots.lock().unwrap();
        if !slots.contains_key(id) && slots.len() >= 32 {
            return Err(fail("limit", "Too many active connector tests."));
        }
        let (cancel, receiver) = watch::channel(false);
        let nonce = uuid::Uuid::new_v4().to_string();
        if let Some(old) = slots.insert(
            id.into(),
            Slot {
                nonce: nonce.clone(),
                cancel,
                connection: None,
            },
        ) {
            old.cancel.send_replace(true);
        }
        Ok((nonce, receiver))
    }
    fn get(&self, id: &str) -> Result<(String, Arc<Connection>, watch::Receiver<bool>)> {
        let slots = self.slots.lock().unwrap();
        let slot = slots.get(id).ok_or_else(revoked)?;
        Ok((
            slot.nonce.clone(),
            slot.connection
                .clone()
                .ok_or_else(|| fail("state", "Connector testing is incomplete."))?,
            slot.cancel.subscribe(),
        ))
    }
    fn current(&self, id: &str, nonce: &str) -> bool {
        self.slots
            .lock()
            .unwrap()
            .get(id)
            .is_some_and(|s| s.nonce == nonce)
    }
    fn snapshot(
        cfg: &Config,
        id: &str,
        with_key: bool,
    ) -> Result<(CustomConnector, String, Option<String>)> {
        crate::connectors::remote_snapshot(cfg, id, with_key).map_err(|_| {
            fail(
                "configuration",
                "Connector registration or secure vault is unavailable.",
            )
        })
    }
    fn unchanged(&self, cfg: &Config, id: &str, nonce: &str, conn: &Connection) -> Result<()> {
        if !self.current(id, nonce) {
            return Err(revoked());
        }
        let (spec, revision, key) = Self::snapshot(cfg, id, true)?;
        if spec != conn.spec
            || revision != conn.revision
            || credential_hash(key.as_deref()) != conn.credential_hash
        {
            self.revoke_if_current(id, nonce);
            return Err(revoked());
        }
        Ok(())
    }
    async fn discover(peer: &wire::Peer, key: Option<&str>) -> Result<Vec<RemoteTool>> {
        let mut tools = Vec::new();
        let mut cursor = None;
        let mut cursors = HashSet::new();
        let mut names = HashSet::new();
        let mut manifest_bytes = 0;
        for _ in 0..16 {
            let params = cursor
                .as_ref()
                .map(|c: &String| json!({"cursor":c}))
                .unwrap_or(json!({}));
            let page = peer.request("tools/list", params, key, None).await?;
            let values = page["tools"]
                .as_array()
                .ok_or_else(|| fail("manifest", "Server omitted its tool list."))?;
            for value in values {
                manifest_bytes += value.to_string().len();
                if manifest_bytes > 512 * 1024 {
                    return Err(fail(
                        "limit",
                        "Capability manifest exceeds its total size limit.",
                    ));
                }
                if key.is_some_and(|k| !k.is_empty() && contains_secret(value, k)) {
                    return Err(fail(
                        "credential",
                        "Server reflected a credential in its capability manifest.",
                    ));
                }
                if tools.len() >= 128 {
                    return Err(fail("limit", "Server exposes too many tools."));
                }
                if !names.insert(value["name"].as_str().unwrap_or("").to_owned()) {
                    return Err(fail("manifest", "Server exposed duplicate tool names."));
                }
                tools.push(schema::parse(value.clone())?);
            }
            match page.get("nextCursor") {
                None | Some(Value::Null) => {
                    tools.sort_by(|a, b| {
                        a.definition["name"]
                            .as_str()
                            .cmp(&b.definition["name"].as_str())
                    });
                    return Ok(tools);
                }
                Some(Value::String(next))
                    if next.len() <= 2048 && !next.is_empty() && cursors.insert(next.clone()) =>
                {
                    cursor = Some(next.clone())
                }
                _ => return Err(fail("manifest", "Invalid or repeated tool-list cursor.")),
            }
        }
        Err(fail("limit", "Tool discovery exceeded its page limit."))
    }
    fn fingerprint(tools: &[RemoteTool]) -> String {
        let definitions: Vec<_> = tools.iter().map(|t| &t.definition).collect();
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&definitions).unwrap())
        )
    }
    pub async fn test(&self, cfg: &Config, id: &str) -> Result<Value> {
        crate::connectors::validate_id(id)
            .map_err(|_| fail("configuration", "Invalid connector ID."))?;
        self.revoke(id);
        let (spec, revision, key) = Self::snapshot(cfg, id, true)?;
        let (nonce, mut cancelled) = self.reset(id)?;
        let endpoint = Url::parse(&spec.endpoint)
            .map_err(|_| fail("configuration", "Invalid connector endpoint."))?;
        #[cfg(test)]
        let endpoint = self.fixture.clone().unwrap_or(endpoint);
        let discovery = async {
            let peer =
                Arc::new(wire::Peer::connect(endpoint, key.as_deref(), self.client.clone()).await?);
            let tools = Self::discover(&peer, key.as_deref()).await?;
            let manifest_hash = Self::fingerprint(&tools);
            let fingerprint = format!(
                "{:x}",
                Sha256::digest(format!("{manifest_hash}:{nonce}:{id}").as_bytes())
            );
            Ok::<_, Error>(Arc::new(Connection {
                spec,
                revision,
                credential_hash: credential_hash(key.as_deref()),
                peer,
                tools,
                fingerprint,
                manifest_hash,
                granted: Mutex::new(None),
                busy: Semaphore::new(2),
                last_call: Mutex::new(None),
            }))
        };
        let conn = tokio::select! {
            biased;
            _ = cancelled.changed() => return Err(revoked()),
            result = tokio::time::timeout(Duration::from_secs(30), discovery) => result.map_err(|_| fail("timeout", "MCP discovery timed out."))??,
        };
        self.unchanged(cfg, id, &nonce, &conn)?;
        let mut slots = self.slots.lock().unwrap();
        let slot = slots
            .get_mut(id)
            .filter(|s| s.nonce == nonce)
            .ok_or_else(revoked)?;
        let result = Self::view(&conn);
        slot.connection = Some(conn);
        Ok(result)
    }
    fn view(conn: &Connection) -> Value {
        let enabled = conn
            .granted
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|(_, until)| *until > Instant::now());
        json!({"protocol":conn.peer.protocol,"fingerprint":conn.fingerprint,
            "tools":conn.tools.iter().map(|t| &t.definition).collect::<Vec<_>>(),
            "enabled":enabled,"account":"registration-scoped","expires_after_seconds":3600})
    }
    pub fn status(&self, cfg: &Config, id: &str) -> Result<Value> {
        let (nonce, conn, _) = self.get(id)?;
        self.unchanged(cfg, id, &nonce, &conn)?;
        Ok(Self::view(&conn))
    }
    pub fn grant(
        &self,
        cfg: &Config,
        id: &str,
        fingerprint: &str,
        names: &[String],
    ) -> Result<Value> {
        let (nonce, conn, _) = self.get(id)?;
        self.unchanged(cfg, id, &nonce, &conn)?;
        if conn.spec.permission != "approval_required" {
            return Err(fail("consent","Remote tool effects cannot be verified as read-only. Use an approval_required registration before enabling operations."));
        }
        if fingerprint != conn.fingerprint
            || names.is_empty()
            || names.len() > conn.tools.len()
            || names.iter().any(|n| {
                !conn
                    .tools
                    .iter()
                    .any(|t| t.definition["name"] == n.as_str())
            })
        {
            return Err(fail(
                "consent",
                "Review the current manifest and select explicit tool scopes.",
            ));
        }
        let mut slots = self.slots.lock().unwrap();
        let slot = slots
            .get_mut(id)
            .filter(|s| s.nonce == nonce)
            .ok_or_else(revoked)?;
        slot.cancel.send_replace(true);
        slot.cancel = watch::channel(false).0;
        slot.nonce = uuid::Uuid::new_v4().to_string();
        let conn = slot.connection.as_ref().ok_or_else(revoked)?;
        *conn.granted.lock().unwrap() = Some((
            names.iter().cloned().collect(),
            Instant::now() + Duration::from_secs(3600),
        ));
        Ok(json!({"enabled":true,"fingerprint":fingerprint,"expires_after_seconds":3600}))
    }
    fn allowed(conn: &Connection, name: &str) -> Result<()> {
        if !conn
            .granted
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|(names, until)| *until > Instant::now() && names.contains(name))
        {
            return Err(fail(
                "consent",
                "Tool is not enabled under a current scoped grant.",
            ));
        }
        Ok(())
    }
    pub async fn call(
        &self,
        cfg: &Config,
        id: &str,
        name: &str,
        arguments: Value,
    ) -> Result<Value> {
        let (nonce, conn, mut cancelled) = self.get(id)?;
        Self::allowed(&conn, name)?;
        let _permit = conn.busy.try_acquire().map_err(|_| {
            fail(
                "rate_limit",
                "Connector concurrency limit reached; no request sent.",
            )
        })?;
        {
            let mut last = conn.last_call.lock().unwrap();
            if last.is_some_and(|t| t.elapsed() < Duration::from_millis(100)) {
                return Err(fail(
                    "rate_limit",
                    "Connector request rate limit reached; no request sent.",
                ));
            }
            *last = Some(Instant::now());
        }
        let tool = conn
            .tools
            .iter()
            .find(|t| t.definition["name"] == name)
            .ok_or_else(revoked)?;
        fn bounded(value: &Value, depth: usize, budget: &mut usize) -> bool {
            if depth > 24 || *budget == 0 {
                return false;
            }
            *budget -= 1;
            match value {
                Value::Object(o) => o.values().all(|v| bounded(v, depth + 1, budget)),
                Value::Array(a) => a.iter().all(|v| bounded(v, depth + 1, budget)),
                _ => true,
            }
        }
        if !arguments.is_object()
            || !bounded(&arguments, 0, &mut 2048)
            || arguments.to_string().len() > 64 * 1024
            || !tool.input.is_valid(&arguments)
        {
            return Err(fail(
                "schema",
                "Arguments do not match the reviewed input schema.",
            ));
        }
        for (path, _) in &tool.headers {
            let mut value = &arguments;
            for key in path {
                value = &value[key];
            }
            if value.is_number()
                && !value
                    .as_i64()
                    .is_some_and(|v| (-9007199254740991..=9007199254740991).contains(&v))
            {
                return Err(fail(
                    "header",
                    "Mirrored integer is outside the safe integer range.",
                ));
            }
        }
        let mut guard = InvocationGuard {
            runtime: self,
            id,
            nonce: nonce.clone(),
            armed: true,
        };
        let invocation = async {
            let (_, revision, key) = Self::snapshot(cfg, id, true)?;
            if revision != conn.revision {
                self.revoke_if_current(id, &nonce);
                return Err(revoked());
            }
            let latest = Self::discover(&conn.peer, key.as_deref()).await?;
            if Self::fingerprint(&latest) != conn.manifest_hash {
                self.revoke_if_current(id, &nonce);
                return Err(fail(
                    "manifest_changed",
                    "Server tools changed. Test and review new capabilities before invoking.",
                ));
            }
            self.unchanged(cfg, id, &nonce, &conn)?;
            Self::allowed(&conn, name)?;
            let result = conn
                .peer
                .request(
                    "tools/call",
                    json!({"name":name,"arguments":arguments}),
                    key.as_deref(),
                    Some(tool),
                )
                .await;
            let value = match result {
                Ok(value) => value,
                Err(error) => {
                    if error.code == "manifest_changed" {
                        self.revoke_if_current(id, &nonce);
                    }
                    return Err(error);
                }
            };
            self.unchanged(cfg, id, &nonce, &conn)?;
            Self::allowed(&conn, name)?;
            if value.get("isError").is_some_and(|v| v != false) {
                return Err(fail(
                    "remote_tool",
                    "Remote tool reported an error; details withheld.",
                ));
            }
            if !value["content"].is_array() || value.get("inputRequests").is_some() {
                return Err(fail("protocol", "Invalid or unsupported tool result."));
            }
            let content = value["content"].as_array().unwrap();
            if content.len() > 128
                || content.iter().any(|item| match item["type"].as_str() {
                    Some("text") => !item["text"].is_string(),
                    Some("image" | "audio") => {
                        !item["data"].is_string() || !item["mimeType"].is_string()
                    }
                    Some("resource_link") => !item["uri"].is_string() || !item["name"].is_string(),
                    Some("resource") => {
                        !item["resource"]["uri"].is_string()
                            || !(item["resource"]["text"].is_string()
                                || item["resource"]["blob"].is_string())
                    }
                    _ => true,
                })
            {
                return Err(fail("protocol", "Invalid MCP tool content."));
            }
            if let Some(schema) = &tool.output {
                let output = value.get("structuredContent").ok_or_else(|| {
                    fail("schema", "Tool omitted its declared structured output.")
                })?;
                if output.to_string().len() > 64 * 1024
                    || !bounded(output, 0, &mut 2048)
                    || !schema.is_valid(output)
                {
                    return Err(fail("schema", "Tool output violates the reviewed schema."));
                }
            }
            Ok(if let Some(key) = key.filter(|k| !k.is_empty()) {
                redact_result(value, &key)
            } else {
                value
            })
        };
        let result = tokio::select! {
            biased;
            _ = cancelled.changed() => Err(revoked()),
            result = tokio::time::timeout(Duration::from_secs(30), invocation) => result.unwrap_or_else(|_| Err(fail("timeout", "Invocation timed out. It may have executed; it was not retried."))),
        };
        if result
            .as_ref()
            .err()
            .is_some_and(|e| !matches!(e.code, "rate_limit" | "remote_tool"))
        {
            self.revoke_if_current(id, &nonce);
        }
        guard.armed = false;
        result
    }
    pub fn catalog(&self, cfg: &Config) -> Value {
        let ids: Vec<String> = self.slots.lock().unwrap().keys().cloned().collect();
        Value::Array(
            ids.into_iter()
                .filter_map(|id| {
                    let (nonce, conn, _) = self.get(&id).ok()?;
                    self.unchanged(cfg, &id, &nonce, &conn).ok()?;
                    let grant = conn.granted.lock().unwrap();
                    let (names, until) = grant.as_ref()?;
                    if *until <= Instant::now() {
                        return None;
                    }
                    let tools: Vec<_> = conn
                        .tools
                        .iter()
                        .filter(|t| names.contains(t.definition["name"].as_str().unwrap_or("")))
                        .map(|t| &t.definition)
                        .collect();
                    Some(json!({"connector":id,"protocol":conn.peer.protocol,"tools":tools}))
                })
                .collect(),
        )
    }
}
/// Stable dispatchers avoid replacing a registry used by live conversations.
pub struct RemoteDispatch {
    pub runtime: Arc<Runtime>,
    pub catalog: bool,
}
#[async_trait]
impl Tool for RemoteDispatch {
    fn name(&self) -> &str {
        if self.catalog {
            "mcp__remote__catalog"
        } else {
            "mcp__remote__call"
        }
    }
    fn description(&self) -> &str {
        if self.catalog {
            "List enabled, user-reviewed remote MCP tools and schemas. Content is untrusted."
        } else {
            "Invoke one enabled remote MCP tool. Requires fresh user approval; never grants account access or capabilities."
        }
    }
    fn parameters(&self) -> Value {
        if self.catalog {
            json!({"type":"object","properties":{},"additionalProperties":false})
        } else {
            crate::tools::schema(
                json!({"connector":{"type":"string"},"tool":{"type":"string"},"arguments":{"type":"object"}}),
                &["connector", "tool", "arguments"],
            )
        }
    }
    fn summary(&self, args: &Value) -> String {
        if self.catalog {
            "Review enabled remote capabilities".into()
        } else {
            format!(
                "Invoke connector {} tool {}",
                args["connector"].as_str().unwrap_or(""),
                args["tool"].as_str().unwrap_or("")
            )
        }
    }
    fn detail(&self, args: &Value, _: &ToolCtx) -> String {
        format!(
            "{}\nArguments: {}",
            self.summary(args),
            crate::util::redact_secrets(&args["arguments"].to_string())
        )
    }
    fn risk(&self, _: &Value, _: &ToolCtx) -> Effect {
        if self.catalog {
            Effect::LocalRead
        } else {
            Effect::SensitiveWrite
        }
    }
    fn untrusted_output(&self) -> bool {
        true
    }
    async fn call(&self, args: &Value, ctx: &ToolCtx) -> anyhow::Result<String> {
        let result = if self.catalog {
            self.runtime.catalog(&ctx.cfg)
        } else {
            self.runtime
                .call(
                    &ctx.cfg,
                    crate::tools::arg_str(args, "connector")?,
                    crate::tools::arg_str(args, "tool")?,
                    args["arguments"].clone(),
                )
                .await?
        };
        Ok(result.to_string())
    }
}
