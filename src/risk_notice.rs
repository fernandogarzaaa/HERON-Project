//! The research-preview risk notice (B2, M16). The owner-signed register of accepted risks is
//! compiled into the core, so the notice the user accepts can never drift from the binary. On macOS
//! (Q11) no turn and no model call runs until its current version is accepted.

use crate::store::Store;
use anyhow::{bail, Result};

/// The register the user reads and accepts.
pub const TEXT: &str = include_str!("../docs/ACCEPTED_RISKS.md");

/// The kv key that holds the version the user accepted.
const KEY: &str = "risk_notice_version";

/// The register's version, from its first line, `Version: <integer>`. A register without that line
/// is version 0, which `register_has_a_version_line` rejects, so such a build never ships.
pub fn version() -> u32 {
    parse_version(TEXT).unwrap_or(0)
}

/// `^Version: (\d+)$` on the first line.
fn parse_version(text: &str) -> Option<u32> {
    let first = text.lines().next()?;
    let digits = first.strip_prefix("Version: ")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Whether turns wait for the notice on this platform (Q11): macOS only.
pub fn required() -> bool {
    cfg!(target_os = "macos")
}

/// The version the user accepted, if any.
pub fn accepted_version(store: &Store) -> Option<u32> {
    store.kv_get(KEY).and_then(|v| v.trim().parse().ok())
}

/// Whether the current version has been accepted. A newer register shows the notice again.
pub fn accepted(store: &Store) -> bool {
    accepted_version(store) == Some(version())
}

/// Turns are blocked: the notice is required here and its current version is not accepted.
pub fn blocks(store: &Store) -> bool {
    required() && !accepted(store)
}

/// Record that the user accepted `version`. Anything but the current version is refused, so a client
/// that showed an older register cannot accept a newer one.
pub fn accept(store: &Store, version: u32) -> Result<()> {
    if version != self::version() {
        bail!("risk notice version {version} is not the current one ({})", self::version());
    }
    store.kv_set(KEY, &version.to_string())
}

/// Accept the current version. Tests and the eval runner use this.
pub fn accept_current(store: &Store) -> Result<()> {
    accept(store, version())
}

/// Withdraw the acceptance (tests start from a store that has accepted).
#[cfg(test)]
pub(crate) fn revoke(store: &Store) {
    store.kv_set(KEY, "").unwrap();
}

/// A turn or model call was refused because the risk notice has not been accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RiskNoticeNotAccepted;

impl std::fmt::Display for RiskNoticeNotAccepted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Heron's risk notice has not been accepted yet. Accept it in the Mac app or by running `heron` in a terminal.")
    }
}

impl std::error::Error for RiskNoticeNotAccepted {}

/// `Err(RiskNoticeNotAccepted)` when turns are blocked.
pub fn check(store: &Store) -> std::result::Result<(), RiskNoticeNotAccepted> {
    if blocks(store) {
        Err(RiskNoticeNotAccepted)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_has_a_version_line() {
        let first = TEXT.lines().next().unwrap_or("");
        assert!(parse_version(TEXT).is_some(), "the register's first line must be `Version: <integer>`, found {first:?}");
        assert!(version() >= 1, "{first:?}");
        assert_eq!(first, format!("Version: {}", version()));
        // The pattern is anchored: anything else on the line, or on a later line, is not a version.
        assert_eq!(parse_version("Version: 12"), Some(12));
        for bad in ["Version: ", "Version: 1a", " Version: 1", "Version: 1 ", "version: 1", "# Risks\nVersion: 1"] {
            assert_eq!(parse_version(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn only_the_current_version_can_be_accepted() {
        let store = Store::in_memory().unwrap();
        assert_eq!(accepted_version(&store), None);
        assert!(!accepted(&store));
        assert_eq!(blocks(&store), required());
        assert!(accept(&store, version() + 1).is_err());
        assert!(accept(&store, version() - 1).is_err());
        assert!(!accepted(&store));
        accept_current(&store).unwrap();
        assert!(accepted(&store) && !blocks(&store));
        assert_eq!(accepted_version(&store), Some(version()));
        // An older acceptance does not cover a newer register.
        store.kv_set(KEY, &(version() - 1).to_string()).unwrap();
        assert!(!accepted(&store));
        assert_eq!(check(&store).is_err(), required());
    }
}
