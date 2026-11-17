//! Local-first connector discovery and custom connector registration.
//!
//! This is a metadata and credential-control plane, not an HTTP fetcher or
//! an unreviewed tool executor. Remote MCP and REST adapters must pass the
//! regular agent approval policy before they can be activated.
use crate::config::Config;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
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

/// A process-local mutex cannot serialize separate HERON instances sharing
/// one home. Hold an OS file lock throughout each read-modify-write and vault
/// mutation, preventing silent loss of successfully registered connectors.
fn lock_registry(cfg: &Config) -> Result<std::fs::File> {
    cfg.ensure_home()?;
    let mut opts = OpenOptions::new();
    opts.write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let file = opts.open(cfg.home.join("connectors.lock"))?;
    // A child process started by any thread briefly holds a copy of this open file between fork and
    // exec, keeping the lock alive after we close it. Wait out that moment; only a lock still held
    // after a second means another HERON instance is mid-change.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(e) => return Err(e).context("Connector registry is busy in another HERON instance; please retry."),
        }
    }
}

pub fn validate_id(id: &str) -> Result<()> {
    if !(3..=48).contains(&id.len())
        || !id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
        || id.starts_with('-') || id.starts_with('_') || id.starts_with("mcp_")
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
    if !(2..=80).contains(&spec.name.trim().len()) || spec.name.chars().any(char::is_control) {
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
    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RegistryFile { version: VERSION, custom: Vec::new() });
        }
        Err(e) => return Err(e).context("Cannot read connector registry."),
    };
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)
        .context("Cannot read connector registry.")?;
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
    let _file_lock = lock_registry(cfg)?;
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
    if value.is_empty() || value.len() > 8192 || value.contains('\r') || value.contains('\n') {
        bail!("Supply a valid connector API key (maximum 8192 characters).");
    }
    let _write = WRITES.lock().unwrap_or_else(|e| e.into_inner());
    let _file_lock = lock_registry(cfg)?;
    require_custom(cfg, id)?;
    cfg.store_vault_only_secret(&key_name(id), value)
}

pub fn remove_api_key(cfg: &Config, id: &str) -> Result<()> {
    let _write = WRITES.lock().unwrap_or_else(|e| e.into_inner());
    let _file_lock = lock_registry(cfg)?;
    require_custom(cfg, id)?;
    cfg.delete_vault_only_secret(&key_name(id))
}

pub fn remove_custom(cfg: &Config, id: &str) -> Result<()> {
    let _write = WRITES.lock().unwrap_or_else(|e| e.into_inner());
    let _file_lock = lock_registry(cfg)?;
    require_custom(cfg, id)?;
    // Credential removal succeeds before hiding its associated connector.
    cfg.delete_vault_only_secret(&key_name(id))?;
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
        c.id = "mcp_filesystem".into();
        assert!(validate(&c).is_err(), "must not shadow configured MCP server IDs");
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
    fn another_process_lock_refuses_racy_mutations() {
        let temp = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.home = temp.path().join("heron");
        let held = lock_registry(&cfg).unwrap();
        // The OS lock protects multiple processes, and also prevents a
        // second independent handle from overwriting another transaction.
        assert!(lock_registry(&cfg).is_err());
        drop(held);
        assert!(lock_registry(&cfg).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn a_lock_kept_alive_for_a_moment_by_a_spawning_child_does_not_fail_the_next_change() {
        use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
        let temp = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.home = temp.path().join("heron");
        let held = lock_registry(&cfg).unwrap();
        // Between fork and exec, a child started by another thread holds a copy of every open
        // file, this lock included, so closing ours does not release it until the child execs.
        let child_copy = unsafe { OwnedFd::from_raw_fd(libc::fcntl(held.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0)) };
        drop(held);
        let exec = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            drop(child_copy);
        });
        register(&cfg, sample()).unwrap();
        exec.join().unwrap();
        assert!(list(&cfg).unwrap().iter().any(|card| card.id == "my_api"));
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

    // Spawn the test executable itself, not a shell or an in-process second
    // file handle. This exercises the actual OS lock on every CI platform.
    #[test]
    fn registry_child_process() {
        let Ok(home) = std::env::var("HERON_REGISTRY_TEST_HOME") else { return };
        let mut cfg = Config::default();
        cfg.home = home.into();
        match std::env::var("HERON_REGISTRY_TEST_ACTION").unwrap().as_str() {
            "blocked" => {
                let err = register(&cfg, sample()).unwrap_err();
                assert!(format!("{err:#}").contains("busy in another HERON instance"));
            }
            "register" => register(&cfg, sample()).unwrap(),
            "hold" => {
                let _held = lock_registry(&cfg).unwrap();
                fs::write(cfg.home.join("child-ready"), b"ready").unwrap();
                // Parent terminates us after observing the readiness marker.
                std::thread::sleep(std::time::Duration::from_secs(30));
                panic!("parent failed to terminate lock owner");
            }
            _ => panic!("unknown test action"),
        }
    }

    fn child(cfg: &Config, action: &str) -> std::process::Child {
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "connectors::tests::registry_child_process", "--nocapture"])
            .env("HERON_REGISTRY_TEST_HOME", &cfg.home)
            .env("HERON_REGISTRY_TEST_ACTION", action)
            .spawn().unwrap()
    }

    #[test]
    fn two_process_transactions_fail_closed_then_preserve_both_registrations() {
        let temp = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.home = temp.path().join("heron");
        let mut first = sample();
        first.id = "first_api".into();
        register(&cfg, first).unwrap();
        let before = fs::read(registry_path(&cfg)).unwrap();
        let held = lock_registry(&cfg).unwrap();
        assert!(child(&cfg, "blocked").wait().unwrap().success());
        assert_eq!(fs::read(registry_path(&cfg)).unwrap(), before);
        drop(held);
        assert!(child(&cfg, "register").wait().unwrap().success());
        let cards = list(&cfg).unwrap();
        for id in ["first_api", "my_api"] {
            assert!(cards.iter().any(|card| card.id == id));
        }
    }

    #[test]
    fn terminated_process_releases_registry_lock_without_stale_lock_cleanup() {
        let temp = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.home = temp.path().join("heron");
        cfg.ensure_home().unwrap();
        let mut owner = child(&cfg, "hold");
        let ready = cfg.home.join("child-ready");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !ready.exists() && std::time::Instant::now() < deadline {
            assert!(owner.try_wait().unwrap().is_none(), "lock owner exited before readiness");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let was_ready = ready.exists();
        let was_locked = lock_registry(&cfg).is_err();
        owner.kill().unwrap();
        owner.wait().unwrap();
        assert!(was_ready, "child never acquired registry lock");
        assert!(was_locked, "another process bypassed the lock");
        register(&cfg, sample()).unwrap();
    }

    #[test]
    fn oversized_registry_and_control_characters_fail_closed() {
        let temp = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.home = temp.path().into();
        let path = registry_path(&cfg);
        fs::File::create(&path).unwrap().set_len(MAX_FILE_BYTES + 1).unwrap();
        assert!(list(&cfg).is_err());
        assert!(register(&cfg, sample()).is_err());
        assert_eq!(fs::metadata(path).unwrap().len(), MAX_FILE_BYTES + 1);
        for c in ['\0', '\t', '\u{1b}', '\u{7f}'] {
            let mut spec = sample();
            spec.name.push(c);
            assert!(validate(&spec).is_err());
        }
    }
}
