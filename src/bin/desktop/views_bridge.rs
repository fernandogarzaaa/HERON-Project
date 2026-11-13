//! HTTP implementation of the Views lane's bridge: every `ViewsBridge`
//! method is one call to the local server through `HeronClient::api`.
//! Shapes match `src/server.rs` exactly; the lane's types in
//! `heron::ui::views` document each one. Errors surface as plain text
//! (`BridgeResult<T>` is `Result<T, String>`), the same shape the web UI
//! shows from `data.error`.

use async_trait::async_trait;
use heron::ui::views as logic;
use reqwest::Method;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::bridge::HeronClient;

/// The HTTP client, wrapped for the Views lane.
pub struct HeronViewsBridge {
    client: HeronClient,
}

impl HeronViewsBridge {
    pub fn new(client: HeronClient) -> Self {
        Self { client }
    }
}

/// An anyhow error as the plain text the bridge returns.
fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

/// A server JSON value as the lane's type. A shape mismatch is a bridge
/// error, not a panic: the server can grow its payload, but the fields
/// the views read must be there.
fn from_value<T: DeserializeOwned>(value: Value) -> logic::BridgeResult<T> {
    serde_json::from_value(value)
        .map_err(|e| format!("the Heron server answered with an unexpected shape: {e}"))
}

/// The `{"id": n}` most write calls answer with.
fn id_of(value: &Value) -> logic::BridgeResult<i64> {
    value
        .get("id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "the Heron server answered without an id".to_string())
}

/// Percent-encode a query value: everything except the unreserved set.
/// The memory search is the only call that puts user text in a query
/// string (`GET /api/memories?q=...`).
fn encode_query(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[async_trait]
impl logic::ViewsBridge for HeronViewsBridge {
    async fn get_state(&self) -> logic::BridgeResult<logic::AppState> {
        let value = self
            .client
            .api(Method::GET, "/api/state", None)
            .await
            .map_err(err)?;
        from_value(value)
    }

    async fn list_sessions(&self) -> logic::BridgeResult<Vec<logic::SessionInfo>> {
        let rows = self.client.list_sessions().await.map_err(err)?;
        Ok(rows
            .into_iter()
            .map(|r| logic::SessionInfo {
                id: r.id,
                title: r.title,
                created_at: r.created_at,
                updated_at: r.updated_at,
                messages: r.messages,
            })
            .collect())
    }

    async fn delete_session(&self, id: &str) -> logic::BridgeResult<()> {
        self.client.delete_session(id).await.map_err(err)
    }

    async fn list_memories(
        &self,
        query: Option<&str>,
    ) -> logic::BridgeResult<Vec<logic::MemoryItem>> {
        let path = match query {
            Some(q) if !q.trim().is_empty() => format!("/api/memories?q={}", encode_query(q)),
            _ => "/api/memories".to_string(),
        };
        let value = self
            .client
            .api(Method::GET, &path, None)
            .await
            .map_err(err)?;
        from_value(value)
    }

    async fn add_memory(&self, content: &str, kind: &str) -> logic::BridgeResult<i64> {
        let value = self
            .client
            .api(
                Method::POST,
                "/api/memories",
                Some(serde_json::json!({"content": content, "kind": kind})),
            )
            .await
            .map_err(err)?;
        id_of(&value)
    }

    async fn delete_memory(&self, id: i64) -> logic::BridgeResult<()> {
        self.client
            .api(Method::DELETE, &format!("/api/memories/{id}"), None)
            .await
            .map_err(err)?;
        Ok(())
    }

    async fn confirm_memory(&self, id: i64) -> logic::BridgeResult<()> {
        self.client
            .api(Method::POST, &format!("/api/memories/{id}/confirm"), None)
            .await
            .map_err(err)?;
        Ok(())
    }

    async fn list_tasks(&self) -> logic::BridgeResult<Vec<logic::TaskItem>> {
        let value = self
            .client
            .api(Method::GET, "/api/tasks", None)
            .await
            .map_err(err)?;
        from_value(value)
    }

    async fn add_task(&self, title: &str) -> logic::BridgeResult<i64> {
        let value = self
            .client
            .api(
                Method::POST,
                "/api/tasks",
                Some(serde_json::json!({"title": title})),
            )
            .await
            .map_err(err)?;
        id_of(&value)
    }

    async fn set_task_status(&self, id: i64, status: &str) -> logic::BridgeResult<()> {
        self.client
            .api(
                Method::POST,
                &format!("/api/tasks/{id}"),
                Some(serde_json::json!({"status": status})),
            )
            .await
            .map_err(err)?;
        Ok(())
    }

    async fn delete_task(&self, id: i64) -> logic::BridgeResult<()> {
        self.client
            .api(Method::DELETE, &format!("/api/tasks/{id}"), None)
            .await
            .map_err(err)?;
        Ok(())
    }

    async fn list_schedules(&self) -> logic::BridgeResult<Vec<logic::ScheduleItem>> {
        let value = self
            .client
            .api(Method::GET, "/api/schedules", None)
            .await
            .map_err(err)?;
        from_value(value)
    }

    async fn add_schedule(
        &self,
        prompt: &str,
        when: &str,
        may: &str,
        quiet: bool,
    ) -> logic::BridgeResult<i64> {
        let value = self
            .client
            .api(
                Method::POST,
                "/api/schedules",
                Some(serde_json::json!({
                    "prompt": prompt,
                    "when": when,
                    "may": may,
                    "quiet": quiet,
                })),
            )
            .await
            .map_err(err)?;
        id_of(&value)
    }

    async fn set_schedule_may(&self, id: i64, may: &str) -> logic::BridgeResult<()> {
        self.client
            .api(
                Method::POST,
                &format!("/api/schedules/{id}"),
                Some(serde_json::json!({"may": may})),
            )
            .await
            .map_err(err)?;
        Ok(())
    }

    async fn run_schedule_now(&self, id: i64) -> logic::BridgeResult<()> {
        self.client
            .api(Method::POST, &format!("/api/schedules/{id}/run"), None)
            .await
            .map_err(err)?;
        Ok(())
    }

    async fn schedule_runs(&self, id: i64) -> logic::BridgeResult<Vec<logic::ScheduleRun>> {
        let value = self
            .client
            .api(Method::GET, &format!("/api/schedules/{id}/runs"), None)
            .await
            .map_err(err)?;
        from_value(value)
    }

    async fn delete_schedule(&self, id: i64) -> logic::BridgeResult<()> {
        self.client
            .api(Method::DELETE, &format!("/api/schedules/{id}"), None)
            .await
            .map_err(err)?;
        Ok(())
    }

    async fn get_inbox(&self) -> logic::BridgeResult<Vec<logic::InboxItem>> {
        let value = self
            .client
            .api(Method::GET, "/api/inbox", None)
            .await
            .map_err(err)?;
        from_value(value)
    }

    async fn read_inbox(&self) -> logic::BridgeResult<()> {
        self.client
            .api(Method::POST, "/api/inbox/read", None)
            .await
            .map_err(err)?;
        Ok(())
    }

    async fn get_awareness(&self) -> logic::BridgeResult<logic::Awareness> {
        let value = self
            .client
            .api(Method::GET, "/api/awareness", None)
            .await
            .map_err(err)?;
        from_value(value)
    }

    async fn set_awareness_enabled(&self, enabled: bool) -> logic::BridgeResult<logic::Awareness> {
        let value = self
            .client
            .api(
                Method::POST,
                "/api/awareness",
                Some(serde_json::json!({"enabled": enabled})),
            )
            .await
            .map_err(err)?;
        from_value(value)
    }

    async fn set_awareness_reflect(&self, reflect: bool) -> logic::BridgeResult<logic::Awareness> {
        let value = self
            .client
            .api(
                Method::POST,
                "/api/awareness",
                Some(serde_json::json!({"reflect": reflect})),
            )
            .await
            .map_err(err)?;
        from_value(value)
    }

    async fn set_awareness_proactive(&self, level: &str) -> logic::BridgeResult<logic::Awareness> {
        let value = self
            .client
            .api(
                Method::POST,
                "/api/awareness",
                Some(serde_json::json!({"proactive": level})),
            )
            .await
            .map_err(err)?;
        from_value(value)
    }

    async fn clear_awareness(&self) -> logic::BridgeResult<i64> {
        let value = self
            .client
            .api(Method::DELETE, "/api/awareness", None)
            .await
            .map_err(err)?;
        value
            .get("removed")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| "the Heron server answered without a removed count".to_string())
    }

    async fn set_paused(&self, paused: bool) -> logic::BridgeResult<bool> {
        let value = self
            .client
            .api(
                Method::POST,
                "/api/pause",
                Some(serde_json::json!({"paused": paused})),
            )
            .await
            .map_err(err)?;
        value
            .get("paused")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| "the Heron server answered without a paused flag".to_string())
    }

    async fn list_suggestions(&self) -> logic::BridgeResult<Vec<logic::Suggestion>> {
        let value = self
            .client
            .api(Method::GET, "/api/suggestions", None)
            .await
            .map_err(err)?;
        from_value(value)
    }

    async fn answer_suggestion(
        &self,
        id: i64,
        action: &str,
    ) -> logic::BridgeResult<(String, Option<String>)> {
        let value = self
            .client
            .api(
                Method::POST,
                &format!("/api/suggestions/{id}"),
                Some(serde_json::json!({"action": action})),
            )
            .await
            .map_err(err)?;
        let prompt = value
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "the Heron server answered without a prompt".to_string())?
            .to_string();
        let session_id = value
            .get("session_id")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        Ok((prompt, session_id))
    }

    async fn list_approvals(&self) -> logic::BridgeResult<Vec<logic::ParkedApproval>> {
        let value = self
            .client
            .api(Method::GET, "/api/approvals", None)
            .await
            .map_err(err)?;
        from_value(value)
    }

    async fn answer_approval(&self, id: i64, action: &str) -> logic::BridgeResult<Option<String>> {
        let value = self
            .client
            .api(
                Method::POST,
                &format!("/api/approvals/{id}"),
                Some(serde_json::json!({"action": action})),
            )
            .await
            .map_err(err)?;
        Ok(match value.get("result") {
            Some(Value::String(s)) => Some(s.clone()),
            Some(other) => Some(other.to_string()),
            None => None,
        })
    }

    async fn get_usage(&self) -> logic::BridgeResult<logic::UsageReport> {
        let value = self
            .client
            .api(Method::GET, "/api/usage", None)
            .await
            .map_err(err)?;
        from_value(value)
    }

    async fn save_usage(
        &self,
        limits: &logic::UsageLimits,
    ) -> logic::BridgeResult<logic::UsageReport> {
        let body = serde_json::to_value(limits)
            .map_err(|e| format!("the usage limits could not be encoded: {e}"))?;
        let value = self
            .client
            .api(Method::POST, "/api/usage", Some(body))
            .await
            .map_err(err)?;
        from_value(value)
    }

    async fn get_audit(&self) -> logic::BridgeResult<logic::AuditLog> {
        let value = self
            .client
            .api(Method::GET, "/api/audit", None)
            .await
            .map_err(err)?;
        from_value(value)
    }
}
