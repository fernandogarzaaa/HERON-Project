//! Effect journal: durable action intent with idempotency keys.
//!
//! Before a tool with side effects runs, its intent is committed. After it
//! returns, its result is committed. On restart:
//! - committed → the stored result is reused; the action is not repeated;
//! - intent without result → the effect may or may not have happened. A
//!   tool marked idempotent is re-run; anything else is UNKNOWN and the run
//!   needs reconciliation before it may continue.
//!
//! This gives at-most-once execution of non-idempotent effects per key, not
//! exactly-once: a crash between the effect and its commit is surfaced, not
//! hidden.

use crate::canonical::sha256_hex;
use crate::contracts::ToolResult;
use crate::error::{CoreError, Result};
use crate::store::{now_rfc3339, Store};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectState {
    Intent,
    Committed,
    Unknown,
    /// Reconciled as not having happened; may be retried under a new attempt.
    Abandoned,
}

impl EffectState {
    fn as_str(self) -> &'static str {
        match self {
            EffectState::Intent => "intent",
            EffectState::Committed => "committed",
            EffectState::Unknown => "unknown",
            EffectState::Abandoned => "abandoned",
        }
    }
    fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "intent" => EffectState::Intent,
            "committed" => EffectState::Committed,
            "unknown" => EffectState::Unknown,
            "abandoned" => EffectState::Abandoned,
            o => return Err(CoreError::contract("Effect", format!("unknown state {o}"))),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Effect {
    pub idempotency_key: String,
    pub run_id: String,
    pub seq: u64,
    pub tool: String,
    pub args_hash: String,
    pub destination: Option<String>,
    pub state: EffectState,
    pub result: Option<ToolResult>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EffectBegin {
    /// No prior record: run the tool.
    Fresh { key: String },
    /// Already done: reuse this result and do not run the tool again.
    AlreadyCommitted { key: String, result: ToolResult },
    /// A previous attempt recorded intent but no result.
    InDoubt { key: String },
}

/// Key derived from the run, the model's tool-call id and the exact arguments.
pub fn idempotency_key(run_id: &str, call_id: &str, tool: &str, args_hash: &str) -> String {
    sha256_hex(format!("{run_id}\x1f{call_id}\x1f{tool}\x1f{args_hash}").as_bytes())
}

type EffectRow = (String, String, i64, String, String, Option<String>, String, Option<String>);

fn row_to_effect(r: &rusqlite::Row<'_>) -> rusqlite::Result<EffectRow> {
    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?))
}

fn decode(t: EffectRow) -> Result<Effect> {
    Ok(Effect {
        idempotency_key: t.0,
        run_id: t.1,
        seq: t.2 as u64,
        tool: t.3,
        args_hash: t.4,
        destination: t.5,
        state: EffectState::parse(&t.6)?,
        result: t.7.map(|s| serde_json::from_str(&s)).transpose()?,
    })
}

const EFFECT_COLUMNS: &str = "idempotency_key, run_id, seq, tool, args_hash, destination, state, result_json";

impl Store {
    pub fn get_effect(&self, key: &str) -> Result<Option<Effect>> {
        let t = self
            .conn()
            .query_row(
                &format!("SELECT {EFFECT_COLUMNS} FROM effects WHERE idempotency_key = ?1"),
                params![key],
                row_to_effect,
            )
            .optional()?;
        t.map(decode).transpose()
    }

    /// Record intent before running a tool, or report what is already known.
    pub fn begin_effect(
        &self,
        run_id: &str,
        call_id: &str,
        seq: u64,
        tool: &str,
        args_hash: &str,
        destination: Option<&str>,
    ) -> Result<EffectBegin> {
        let key = idempotency_key(run_id, call_id, tool, args_hash);
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let existing = tx
            .query_row(
                &format!("SELECT {EFFECT_COLUMNS} FROM effects WHERE idempotency_key = ?1"),
                params![key],
                row_to_effect,
            )
            .optional()?
            .map(decode)
            .transpose()?;
        let outcome = match existing {
            Some(e) if e.state == EffectState::Committed => EffectBegin::AlreadyCommitted {
                key: key.clone(),
                result: e.result.ok_or_else(|| CoreError::contract("Effect", "committed effect without result"))?,
            },
            Some(e) if e.state == EffectState::Abandoned => {
                tx.execute(
                    "UPDATE effects SET state = 'intent', updated_at = ?2 WHERE idempotency_key = ?1",
                    params![key, now_rfc3339()],
                )?;
                EffectBegin::Fresh { key: key.clone() }
            }
            Some(_) => EffectBegin::InDoubt { key: key.clone() },
            None => {
                let now = now_rfc3339();
                tx.execute(
                    "INSERT INTO effects (idempotency_key, run_id, seq, tool, args_hash, destination, state, result_json, created_at, updated_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'intent', NULL, ?7, ?7)",
                    params![key, run_id, seq as i64, tool, args_hash, destination, now],
                )?;
                EffectBegin::Fresh { key: key.clone() }
            }
        };
        tx.commit()?;
        Ok(outcome)
    }

    fn set_effect(&self, key: &str, from: &[EffectState], to: EffectState, result: Option<&ToolResult>) -> Result<()> {
        let e = self.get_effect(key)?.ok_or_else(|| CoreError::NotFound(format!("effect {key}")))?;
        if !from.contains(&e.state) {
            return Err(CoreError::Conflict(format!("effect {key} is {}, expected one of {from:?}", e.state.as_str())));
        }
        let json = result.map(serde_json::to_string).transpose()?;
        self.conn().execute(
            "UPDATE effects SET state = ?2, result_json = COALESCE(?3, result_json), updated_at = ?4 WHERE idempotency_key = ?1",
            params![key, to.as_str(), json, now_rfc3339()],
        )?;
        Ok(())
    }

    pub fn commit_effect(&self, key: &str, result: &ToolResult) -> Result<()> {
        result.validate()?;
        self.set_effect(key, &[EffectState::Intent], EffectState::Committed, Some(result))
    }

    pub fn mark_effect_unknown(&self, key: &str) -> Result<()> {
        self.set_effect(key, &[EffectState::Intent, EffectState::Unknown], EffectState::Unknown, None)
    }

    /// Human or provider reconciliation of an in-doubt effect.
    pub fn reconcile_effect(&self, key: &str, happened: Option<&ToolResult>) -> Result<()> {
        match happened {
            Some(result) => {
                result.validate()?;
                self.set_effect(key, &[EffectState::Intent, EffectState::Unknown], EffectState::Committed, Some(result))
            }
            None => self.set_effect(key, &[EffectState::Intent, EffectState::Unknown], EffectState::Abandoned, None),
        }
    }

    /// Whether this exact call already has a committed result.
    pub fn effect_committed(&self, run_id: &str, call_id: &str, tool: &str, args_hash: &str) -> Result<bool> {
        Ok(self
            .get_effect(&idempotency_key(run_id, call_id, tool, args_hash))?
            .is_some_and(|e| e.state == EffectState::Committed))
    }

    pub fn effects_for_run(&self, run_id: &str) -> Result<Vec<Effect>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!("SELECT {EFFECT_COLUMNS} FROM effects WHERE run_id = ?1 ORDER BY seq"))?;
        let rows = stmt.query_map(params![run_id], row_to_effect)?;
        rows.map(|r| decode(r?)).collect()
    }

    pub fn unresolved_effects(&self, run_id: &str) -> Result<Vec<Effect>> {
        Ok(self
            .effects_for_run(run_id)?
            .into_iter()
            .filter(|e| matches!(e.state, EffectState::Intent | EffectState::Unknown))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::ToolErrorKind;

    #[test]
    fn fresh_then_committed_is_not_repeated() {
        let s = Store::open_in_memory().unwrap();
        let b = s.begin_effect("r", "call-1", 0, "http_fetch", "ah", Some("x.com")).unwrap();
        let EffectBegin::Fresh { key } = b else { panic!("expected fresh") };
        s.commit_effect(&key, &ToolResult::ok("call-1", "http_fetch", "200 OK")).unwrap();
        // Restart: the same call must reuse the stored result.
        match s.begin_effect("r", "call-1", 0, "http_fetch", "ah", Some("x.com")).unwrap() {
            EffectBegin::AlreadyCommitted { result, .. } => assert_eq!(result.content, "200 OK"),
            other => panic!("expected committed, got {other:?}"),
        }
        assert!(s.unresolved_effects("r").unwrap().is_empty());
        assert!(s.effect_committed("r", "call-1", "http_fetch", "ah").unwrap());
        assert!(!s.effect_committed("r", "call-1", "http_fetch", "other").unwrap());
        assert!(!s.effect_committed("r", "call-2", "http_fetch", "ah").unwrap());
    }

    #[test]
    fn crash_after_intent_is_in_doubt() {
        let s = Store::open_in_memory().unwrap();
        let EffectBegin::Fresh { key } = s.begin_effect("r", "c", 3, "pay", "ah", None).unwrap() else { panic!() };
        // Simulated crash: no commit. On restart:
        assert_eq!(s.begin_effect("r", "c", 3, "pay", "ah", None).unwrap(), EffectBegin::InDoubt { key: key.clone() });
        s.mark_effect_unknown(&key).unwrap();
        assert_eq!(s.unresolved_effects("r").unwrap().len(), 1);
        assert_eq!(s.get_effect(&key).unwrap().unwrap().state, EffectState::Unknown);
    }

    #[test]
    fn reconciliation_paths() {
        let s = Store::open_in_memory().unwrap();
        let EffectBegin::Fresh { key } = s.begin_effect("r", "c", 0, "pay", "ah", None).unwrap() else { panic!() };
        s.mark_effect_unknown(&key).unwrap();
        // Provider says it did not happen: retry is allowed.
        s.reconcile_effect(&key, None).unwrap();
        assert!(matches!(s.begin_effect("r", "c", 0, "pay", "ah", None).unwrap(), EffectBegin::Fresh { .. }));
        // This time it happened but we crashed again; provider confirms.
        s.mark_effect_unknown(&key).unwrap();
        s.reconcile_effect(&key, Some(&ToolResult::ok("c", "pay", "receipt 9"))).unwrap();
        assert!(matches!(
            s.begin_effect("r", "c", 0, "pay", "ah", None).unwrap(),
            EffectBegin::AlreadyCommitted { .. }
        ));
        // Committed effects cannot be reconciled again.
        assert!(s.reconcile_effect(&key, None).is_err());
        assert!(s.mark_effect_unknown(&key).is_err());
    }

    #[test]
    fn keys_bind_arguments() {
        assert_ne!(idempotency_key("r", "c", "t", "a1"), idempotency_key("r", "c", "t", "a2"));
        assert_ne!(idempotency_key("r", "c1", "t", "a"), idempotency_key("r", "c2", "t", "a"));
        assert_ne!(idempotency_key("r1", "c", "t", "a"), idempotency_key("r2", "c", "t", "a"));
        assert_ne!(idempotency_key("r", "c", "t1", "a"), idempotency_key("r", "c", "t2", "a"));
        // The separator prevents concatenation collisions.
        assert_ne!(idempotency_key("ab", "c", "t", "a"), idempotency_key("a", "bc", "t", "a"));
    }

    #[test]
    fn failed_results_are_committed_too() {
        let s = Store::open_in_memory().unwrap();
        let EffectBegin::Fresh { key } = s.begin_effect("r", "c", 0, "t", "a", None).unwrap() else { panic!() };
        let bad = ToolResult::ok("c", "t", "x");
        let mut invalid = bad.clone();
        invalid.error_kind = Some(ToolErrorKind::Failed);
        assert!(s.commit_effect(&key, &invalid).is_err());
        s.commit_effect(&key, &ToolResult::err("c", "t", ToolErrorKind::Failed, "boom")).unwrap();
        assert!(s.commit_effect(&key, &bad).is_err(), "commit twice refused");
        let effects = s.effects_for_run("r").unwrap();
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].result.as_ref().unwrap().content, "boom");
    }
}
