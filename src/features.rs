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

/// F2: each approval says why it was asked, and Telegram's refusal names what tainted the
/// conversation.
pub const ASK_REASONS: &str = "ask_reasons";

/// Every core key. Each feature appends its one entry.
pub static KEYS: &[FeatureKey] = &[
    FeatureKey { key: ASK_REASONS, title: "Explain approvals", default: true },
    #[cfg(test)]
    FeatureKey { key: "test_key", title: "Test feature", default: false },
];

/// The key named `key`, if there is one.
pub fn find(key: &str) -> Option<&'static FeatureKey> {
    KEYS.iter().find(|k| k.key == key)
}
