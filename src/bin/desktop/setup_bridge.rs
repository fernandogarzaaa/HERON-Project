//! Real `SetupBridge` implementation, backed by `HeronClient`.
//!
//! One `client.api` call per trait method, then the matching `parse_*`
//! function from `setup.rs`. No shape logic lives here: every server
//! shape decision is in the tested parsers and body builders, so this
//! file stays a thin pass-through.
//!
//! The driver runs these methods as tasks on the Bridge runtime
//! (`Bridge::spawn`) and hands results to the UI thread with
//! `Weak::upgrade_in_event_loop`. Nothing here touches a widget.
//!
//! This file does NOT compile in the Setup worktree: `HeronClient` lives
//! in `src/bin/desktop/bridge.rs`, which belongs to the Shell lane and
//! is not present here. It compiles in the integrated tree, where
//! `desktop.rs` declares `mod bridge; mod setup; mod setup_bridge;`.
//! Verified here with `rustfmt --check` (syntax only). The first Mac or
//! CI build of the `desktop` feature is the real check.

use super::bridge::HeronClient;
use super::setup::{
    about_body, demo_body, parse_backup_path, parse_disconnect, parse_indexed, parse_learned,
    parse_mcp_note, parse_persona, parse_proactivity, parse_saved, parse_settings, parse_sources,
    parse_state, parse_telegram, parse_tool_setting, parse_tools, persona_body, proactivity_body,
    settings_body, source_body, telegram_body, tool_body, AboutForm, ConnectResult,
    DisconnectResult, PersonaSnapshot, Proactivity, SettingsForm, SettingsSnapshot, SetupBridge,
    SourceFields, SourceKind, SourcesSnapshot, StateSnapshot, TelegramSnapshot, ToolRow,
    ToolSetting,
};
use reqwest::Method;

fn err(e: anyhow::Error) -> String {
    e.to_string()
}

/// The bridge implementation the driver constructs from the launched
/// client: `HeronSetupBridge::new(bridge.client().clone())`.
pub struct HeronSetupBridge {
    client: HeronClient,
}

impl HeronSetupBridge {
    pub fn new(client: HeronClient) -> HeronSetupBridge {
        HeronSetupBridge { client }
    }
}

impl SetupBridge for HeronSetupBridge {
    async fn load_state(&self) -> Result<StateSnapshot, String> {
        let v = self
            .client
            .api(Method::GET, "/api/state", None)
            .await
            .map_err(err)?;
        Ok(parse_state(&v))
    }

    async fn load_sources(&self) -> Result<SourcesSnapshot, String> {
        let v = self
            .client
            .api(Method::GET, "/api/sources", None)
            .await
            .map_err(err)?;
        Ok(parse_sources(&v))
    }

    async fn connect_source(
        &self,
        kind: SourceKind,
        fields: &SourceFields,
    ) -> Result<ConnectResult, String> {
        let v = self
            .client
            .api(
                Method::POST,
                &format!("/api/sources/{}", kind.as_str()),
                Some(source_body(kind, fields)),
            )
            .await
            .map_err(err)?;
        match kind {
            SourceKind::Computer => Ok(ConnectResult {
                learned: parse_learned(&v),
                ..Default::default()
            }),
            _ => Ok(ConnectResult {
                indexed: Some(parse_indexed(&v)),
                ..Default::default()
            }),
        }
    }

    async fn learn_source(&self, kind: SourceKind) -> Result<Vec<String>, String> {
        let v = self
            .client
            .api(
                Method::POST,
                &format!("/api/sources/{}/learn", kind.as_str()),
                None,
            )
            .await
            .map_err(err)?;
        Ok(parse_learned(&v))
    }

    async fn disconnect_source(&self, kind: SourceKind) -> Result<DisconnectResult, String> {
        let v = self
            .client
            .api(
                Method::DELETE,
                &format!("/api/sources/{}", kind.as_str()),
                None,
            )
            .await
            .map_err(err)?;
        Ok(parse_disconnect(&v))
    }

    async fn save_about(&self, form: &AboutForm) -> Result<Vec<String>, String> {
        let v = self
            .client
            .api(Method::POST, "/api/sources/about", Some(about_body(form)))
            .await
            .map_err(err)?;
        Ok(parse_saved(&v))
    }

    async fn load_proactivity(&self) -> Result<Proactivity, String> {
        let v = self
            .client
            .api(Method::GET, "/api/awareness", None)
            .await
            .map_err(err)?;
        parse_proactivity(&v)
    }

    async fn set_proactivity(&self, level: Proactivity) -> Result<(), String> {
        self.client
            .api(
                Method::POST,
                "/api/awareness",
                Some(proactivity_body(level)),
            )
            .await
            .map_err(err)?;
        Ok(())
    }

    async fn use_demo(&self) -> Result<(), String> {
        self.client
            .api(Method::POST, "/api/settings", Some(demo_body()))
            .await
            .map_err(err)?;
        Ok(())
    }

    async fn finish_onboarding(&self) -> Result<(), String> {
        self.client
            .api(Method::POST, "/api/onboarded", None)
            .await
            .map_err(err)?;
        Ok(())
    }

    async fn load_settings(&self) -> Result<SettingsSnapshot, String> {
        let v = self
            .client
            .api(Method::GET, "/api/settings", None)
            .await
            .map_err(err)?;
        Ok(parse_settings(&v))
    }

    async fn save_settings(&self, form: &SettingsForm) -> Result<SettingsSnapshot, String> {
        let v = self
            .client
            .api(Method::POST, "/api/settings", Some(settings_body(form)))
            .await
            .map_err(err)?;
        Ok(parse_settings(&v))
    }

    async fn load_persona(&self) -> Result<PersonaSnapshot, String> {
        let v = self
            .client
            .api(Method::GET, "/api/persona", None)
            .await
            .map_err(err)?;
        Ok(parse_persona(&v))
    }

    async fn save_persona(&self, name: &str, text: &str) -> Result<PersonaSnapshot, String> {
        let v = self
            .client
            .api(Method::POST, "/api/persona", Some(persona_body(name, text)))
            .await
            .map_err(err)?;
        Ok(parse_persona(&v))
    }

    async fn reset_persona(&self) -> Result<PersonaSnapshot, String> {
        let v = self
            .client
            .api(Method::DELETE, "/api/persona", None)
            .await
            .map_err(err)?;
        Ok(parse_persona(&v))
    }

    async fn load_tools(&self) -> Result<Vec<ToolRow>, String> {
        let v = self
            .client
            .api(Method::GET, "/api/tools", None)
            .await
            .map_err(err)?;
        Ok(parse_tools(&v))
    }

    async fn set_tool_setting(
        &self,
        name: &str,
        setting: ToolSetting,
    ) -> Result<ToolSetting, String> {
        let v = self
            .client
            .api(
                Method::POST,
                &format!("/api/tools/{name}"),
                Some(tool_body(setting)),
            )
            .await
            .map_err(err)?;
        parse_tool_setting(&v)
    }

    async fn accept_mcp(&self, name: &str) -> Result<String, String> {
        let v = self
            .client
            .api(Method::POST, &format!("/api/mcp/{name}/accept"), None)
            .await
            .map_err(err)?;
        Ok(parse_mcp_note(&v))
    }

    async fn load_telegram(&self) -> Result<TelegramSnapshot, String> {
        let v = self
            .client
            .api(Method::GET, "/api/telegram", None)
            .await
            .map_err(err)?;
        Ok(parse_telegram(&v))
    }

    async fn connect_telegram(&self, token: &str) -> Result<TelegramSnapshot, String> {
        let v = self
            .client
            .api(Method::POST, "/api/telegram", Some(telegram_body(token)))
            .await
            .map_err(err)?;
        Ok(parse_telegram(&v))
    }

    async fn disconnect_telegram(&self) -> Result<TelegramSnapshot, String> {
        let v = self
            .client
            .api(Method::DELETE, "/api/telegram", None)
            .await
            .map_err(err)?;
        Ok(parse_telegram(&v))
    }

    async fn make_backup(&self) -> Result<String, String> {
        let v = self
            .client
            .api(Method::POST, "/api/backup", None)
            .await
            .map_err(err)?;
        Ok(parse_backup_path(&v))
    }
}
