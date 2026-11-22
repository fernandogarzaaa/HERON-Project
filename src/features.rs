//! Core feature keys: the `[features]` table in `config.toml`.
//!
//! A per-ability switch only denies a call. A feature that is off must also be hidden from the model
//! and from automations, and must start off, so a tool that belongs to a feature declares its key
//! (see [`crate::tools::Tool::feature`]) and the registry's list of tool names is filtered by the
//! keys when each turn starts ([`crate::tools::ToolRegistry::names_for`]).

/// One core key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeatureKey {
    /// The name in `[features]` and in `/api/features/{key}`.
    pub key: &'static str,
    /// What Settings › Features shows.
    pub title: &'static str,
    /// Whether the feature is on when `config.toml` does not say.
    pub default: bool,
}

/// Every core key. Each feature appends its one entry.
pub static KEYS: &[FeatureKey] = &[
    #[cfg(test)]
    FeatureKey { key: "test_key", title: "Test feature", default: false },
    // F8: `draft_email`.
    FeatureKey { key: "email_drafts", title: "Email drafts", default: true },
];

/// The key named `key`, if there is one.
pub fn find(key: &str) -> Option<&'static FeatureKey> {
    KEYS.iter().find(|k| k.key == key)
}
