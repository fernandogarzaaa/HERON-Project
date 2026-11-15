//! Local-first connector discovery and custom connector registration.
//!
//! This is a metadata and credential-control plane, not an HTTP fetcher or
//! an unreviewed tool executor. Remote MCP and REST adapters must pass the
//! regular agent approval policy before they can be activated.
use crate::config::Config;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use url::{Host, Url};

static WRITES: Mutex<()> = Mutex::new(());
const MAX_CUSTOM: usize = 32;
const MAX_FILE_BYTES: u64 = 256 * 1024;
const VERSION: u32 = 1;

/// Only nonsecret metadata is persisted in connectors.json.
/// API keys are stored separately in the operating system keychain.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CustomConnector {
    pub id: String,
    pub name: String,
    pub category: String,
    /// "remote_mcp" or "rest_api"; remote execution is NOT yet enabled.
    pub transport: String,
    /// A public HTTPS URL without embedded credentials or query strings.
    pub endpoint: String,
    /// "read_only" or "approval_required"; no write actions are auto-approved.
    pub permission: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ConnectorCard {
    pub id: String,
    pub name: String,
    pub category: String,
    pub transport: String,
    pub status: String,
    pub permission: String,
    pub endpoint: Option<String>,
    pub has_credential: bool,
    pub source: String,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryFile {
    version: u32,
    custom: Vec<CustomConnector>,
}

// Catalog entries are discovery templates, NOT claims of installed integrations.
// Actual built-in connector availability is shown explicitly in status.
const CATALOG: &[(&str, &str, &str, &str)] = &[
    ("google", "Google Gmail & Calendar", "communication", "oauth_planned"),
    ("microsoft", "Microsoft Outlook & Calendar", "communication", "oauth_planned"),
    ("google_drive", "Google Drive", "files", "mcp_candidate"),
    ("onedrive", "OneDrive", "files", "mcp_candidate"),
    ("dropbox", "Dropbox", "files", "mcp_candidate"),
    ("notion", "Notion", "productivity", "mcp_candidate"),
    ("github", "GitHub", "development", "mcp_candidate"),
    ("linear", "Linear", "productivity", "mcp_candidate"),
    ("slack", "Slack", "communication", "mcp_candidate"),
    ("discord", "Discord", "social", "mcp_candidate"),
    ("telegram", "Telegram", "communication", "existing_native"),
    ("instagram", "Instagram", "social", "mcp_candidate"),
    ("facebook", "Facebook", "social", "mcp_candidate"),
    ("youtube", "YouTube", "social", "mcp_candidate"),
    ("tiktok", "TikTok", "social", "mcp_candidate"),
    ("x", "X", "social", "mcp_candidate"),
    ("salesforce", "Salesforce", "crm", "mcp_candidate"),
    ("hubspot", "HubSpot", "crm", "mcp_candidate"),
    ("shopify", "Shopify", "commerce", "mcp_candidate"),
    ("stripe", "Stripe", "commerce", "mcp_candidate"),
    ("home_assistant", "Home Assistant", "personal", "mcp_candidate"),
];

fn registry_path(cfg: &Config) -> PathBuf {
    cfg.home.join("connectors.json")
}

pub fn validate_id(id: &str) -> Result<()> {
    if !(3..=48).contains(&id.len())
        || !id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
        || id.starts_with(['-', '_'])
        || CATALOG.iter().any(|(key, _, _, _)| *key == id)
    {
        bail!("Connector ID must be a unique 3–48 character lowercase slug.");
    }
    Ok(())
}

fn validate_endpoint(endpoint: &str) -> Result<()> {
    if endpoint.len() > 2048 {
        bail!("Connector endpoint is too long.");
    }
    let url = Url::parse(endpoint).context("Use a complete HTTPS connector endpoint URL.")?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some()
        || url.fragment().is_some() || url.query().is_some() || url.port().is_some_and(|p| p != 443)
    {
        bail!("Connector endpoints must be HTTPS with no credentials, query, fragment, or nonstandard port.");
    }
    let Some(Host::Domain(hostname)) = url.host() else {
        bail!("Connector endpoints must use a public DNS hostname, not an IP address.");
    };
    let host = hostname.trim_end_matches('.').to_ascii_lowercase();
    if !host.contains('.') || host.ends_with(".localhost") || host == "localhost"
        || [".local", ".internal", ".test", ".invalid", ".example", ".home", ".lan", ".onion"]
            .iter().any(|suffix| host.ends_with(suffix))
    {
        bail!("Connector endpoints must use a public DNS hostname.");
    }
    Ok(())
}

pub fn validate(spec: &CustomConnector) -> Result<()> {
    validate_id(&spec.id)?;
    if !(2..=80).contains(&spec.name.trim().len()) || spec.name.contains(['\n', '\r']) {
        bail!("Connector name must contain 2–80 characters on one line.");
    }
    if !["communication", "files", "productivity", "development", "social", "crm",
          "commerce", "personal", "other"].contains(&spec.category.as_str()) {
        bail!("Unknown connector category.");
    }
    if !["remote_mcp", "rest_api"].contains(&spec.transport.as_str()) {
        bail!("Unsupported connector transport.");
    }
    if !["read_only", "approval_required"].contains(&spec.permission.as_str()) {
        bail!("Connector access must be read-only or require approval.");
    }
    validate_endpoint(&spec.endpoint)
}

fn load(cfg: &Config) -> Result<RegistryFile> {
    let path = registry_path(cfg);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RegistryFile { version: VERSION, custom: Vec::new() });
        }
        Err(e) => return Err(e).context("Cannot read connector registry."),
    };
    if bytes.len() as u64 > MAX_FILE_BYTES {
        bail!("Connector registry exceeds its safe size limit.");
    }
    let doc: RegistryFile = serde_json::from_slice(&bytes).context("Connector registry is damaged; refusing to overwrite it.")?;
    if doc.version != VERSION || doc.custom.len() > MAX_CUSTOM {
        bail!("Unsupported connector registry format.");
    }
    let mut ids = std::collections::HashSet::new();
    for spec in &doc.custom {
        validate(spec)?;
        if !ids.insert(spec.id.as_str()) {
            bail!("Duplicate connector IDs in registry.");
        }
    }
    Ok(doc)
}

fn save(cfg: &Config, doc: &RegistryFile) -> Result<()> {
    cfg.ensure_home()?;
    let path = registry_path(cfg);
    let tmp = cfg.home.join(format!("connectors.json.tmp-{}", uuid::Uuid::new_v4().simple()));
    let text = serde_json::to_vec_pretty(doc)?;
    if text.len() as u64 > MAX_FILE_BYTES {
        bail!("Connector registry exceeds its safe size limit.");
    }
    let written = (|| -> Result<()> {
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut file = opts.open(&tmp)?;
        file.write_all(&text)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, &path)?;
        Ok(())
    })();
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written.context("Could not save connector registry.")
}

pub fn register(cfg: &Config, spec: CustomConnector) -> Result<()> {
    validate(&spec)?;
    let _write = WRITES.lock().unwrap_or_else(|e| e.into_inner());
    let mut doc = load(cfg)?;
    if doc.custom.len() >= MAX_CUSTOM {
        bail!("Maximum number of custom connectors reached.");
    }
    if doc.custom.iter().any(|existing| existing.id == spec.id) {
        bail!("A custom connector with this ID already exists.");
    }
    doc.custom.push(spec);
    save(cfg, &doc)
}

fn require_custom(cfg: &Config, id: &str) -> Result<()> {
    validate_id(id)?;
    if !load(cfg)?.custom.iter().any(|spec| spec.id == id) {
        bail!("Unknown custom connector.");
    }
    Ok(())
}

fn key_name(id: &str) -> String {
    format!("connector_{id}_api_key")
}

/// API keys are never exposed by a model tool or returned from this module.
pub fn put_api_key(cfg: &Config, id: &str, value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 8192 || value.contains(['\r', '\n']) {
        bail!("Supply a valid connector API key (maximum 8192 characters).");
    }
    let _write = WRITES.lock().unwrap_or_else(|e| e.into_inner());
    require_custom(cfg, id)?;
    cfg.store_vault_only_secret(&key_name(id), value)
}

pub fn remove_api_key(cfg: &Config, id: &str) -> Result<()> {
    let _write = WRITES.lock().unwrap_or_else(|e| e.into_inner());
    require_custom(cfg, id)?;
    cfg.store_secret(&key_name(id), "")
}

pub fn remove_custom(cfg: &Config, id: &str) -> Result<()> {
    let _write = WRITES.lock().unwrap_or_else(|e| e.into_inner());
    require_custom(cfg, id)?;
    // Credential removal succeeds before hiding its associated connector.
    cfg.store_secret(&key_name(id), "")?;
    let mut doc = load(cfg)?;
    doc.custom.retain(|entry| entry.id != id);
    save(cfg, &doc)
}

pub fn list(cfg: &Config) -> Result<Vec<ConnectorCard>> {
    let mut result = Vec::new();
    for &(id, name, category, status) in CATALOG {
        result.push(ConnectorCard {
            id: id.into(), name: name.into(), category: category.into(),
            transport: if status == "oauth_planned" { "oauth" } else if status == "existing_native" { "native" } else { "mcp" }.into(),
            status: status.into(), permission: "approval_required".into(),
            endpoint: None, has_credential: false, source: "catalog".into(),
        });
    }
    for (name, server) in &cfg.mcp.servers {
        result.push(ConnectorCard {
            id: format!("mcp_{name}"), name: name.clone(), category: "other".into(),
            transport: "mcp_stdio".into(), status: "configured".into(),
            permission: if server.trust { "trusted_by_user" } else { "approval_required" }.into(),
            endpoint: None, has_credential: false, source: "configured_mcp".into(),
        });
    }
    for spec in load(cfg)?.custom {
        let credential = cfg.vault_only_secret_present(&key_name(&spec.id));
        result.push(ConnectorCard {
            id: spec.id, name: spec.name, category: spec.category,
            transport: spec.transport, status: "saved_not_active".into(),
            permission: spec.permission, endpoint: Some(spec.endpoint),
            has_credential: credential, source: "custom".into(),
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> CustomConnector {
        CustomConnector {
            id: "my_api".into(), name: "My Private API".into(),
            category: "other".into(), transport: "rest_api".into(),
            endpoint: "https://example.org/v1".into(),
            permission: "read_only".into(),
        }
    }

    #[test]
    fn validates_public_https_only() {
        assert!(validate(&sample()).is_ok());
        for url in [
            "http://example.org", "https://localhost/x",
            "https://metadata.internal./", "https://127.0.0.1/x",
            "https://169.254.169.254/", "https://user:secret@example.org/",
            "https://example.org/?token=secret", "https://example.org/#fragment",
            "https://example.org:8443/", "https://service.local/",
        ] {
            let mut c = sample();
            c.endpoint = url.into();
            assert!(validate(&c).is_err(), "{url}");
        }
        let mut c = sample();
        c.id = "google".into();
        assert!(validate(&c).is_err(), "must not shadow built-in OAuth");
        c.id = "../escape".into();
        assert!(validate(&c).is_err(), "must not escape vault namespace");
    }

    #[test]
    fn registry_is_atomic_private_and_never_contains_keys() {
        let temp = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.home = temp.path().join("heron");
        let c = sample();
        register(&cfg, c.clone()).unwrap();
        assert!(register(&cfg, c).is_err(), "duplicate must fail");
        let cards = list(&cfg).unwrap();
        let custom = cards.iter().find(|card| card.id == "my_api").unwrap();
        assert_eq!(custom.status, "saved_not_active");
        assert!(!custom.has_credential);
        let file = fs::read_to_string(registry_path(&cfg)).unwrap();
        assert!(!file.contains("api_key"));
        assert!(!file.contains("secret"));
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(registry_path(&cfg)).unwrap().permissions().mode() & 0o777, 0o600);
        }
        remove_custom(&cfg, "my_api").unwrap();
        assert!(list(&cfg).unwrap().iter().all(|card| card.id != "my_api"));
    }

    #[test]
    fn damaged_registry_is_not_ignored_or_overwritten() {
        let temp = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.home = temp.path().to_path_buf();
        fs::write(registry_path(&cfg), b"{bad-json").unwrap();
        let before = fs::read(registry_path(&cfg)).unwrap();
        assert!(register(&cfg, sample()).is_err());
        assert_eq!(fs::read(registry_path(&cfg)).unwrap(), before);
    }

    #[test]
    fn connector_registry_never_activates_unreviewed_remote_tools() {
        let temp = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.home = temp.path().to_path_buf();
        let mut c = sample();
        c.transport = "remote_mcp".into();
        c.permission = "approval_required".into();
        register(&cfg, c).unwrap();
        let card = list(&cfg).unwrap().into_iter().find(|x| x.id == "my_api").unwrap();
        assert_eq!(card.status, "saved_not_active");
        assert_eq!(card.permission, "approval_required");
    }
}
