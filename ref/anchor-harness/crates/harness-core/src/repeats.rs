//! Stuck-loop detection: the agent repeating itself with nothing changing.
//! Budgets bound what a looping agent can spend; this stops it as soon as
//! repetition is certain, so the run ends `exhausted` early and says why.
//!
//! A call's fingerprint is its tool, canonical input, ok/error flag and
//! output. A run is stuck when the same block of 1 to `MAX_PERIOD` calls
//! repeats `limit` times back to back: four identical calls, or A-B-A-B-...
//! Failed and denied calls are near-duplicates when only the digits in their
//! output differ (a timing, a pid, a temp path); successful output must match
//! exactly, because changing numbers there are progress.

use crate::canonical::content_hash;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Default number of repetitions that ends a run.
pub const DEFAULT_LIMIT: u32 = 4;

/// Longest repeating block detected (A, A-B, A-B-C).
pub const MAX_PERIOD: usize = 3;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepeatGuard {
    /// The most recent calls as (tool, fingerprint), oldest first.
    #[serde(default)]
    recent: Vec<(String, String)>,
}

fn mask_digits(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_digits = false;
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            if !in_digits {
                out.push('#');
            }
            in_digits = true;
        } else {
            out.push(ch);
            in_digits = false;
        }
    }
    out
}

impl RepeatGuard {
    /// Record one finished call (allowed or denied). Returns the stop reason
    /// once a block of calls has repeated `limit` times in a row; `limit` 0
    /// disables the check.
    pub fn observe(&mut self, limit: u32, tool: &str, input: &Value, ok: bool, output: &str) -> Option<String> {
        let output = if ok { output.to_string() } else { mask_digits(output) };
        self.recent.push((tool.to_string(), content_hash(&json!([tool, input, ok, output]))));
        // One call arrives at a time, so dropping the oldest keeps the window.
        if self.recent.len() > MAX_PERIOD * (limit.max(1) as usize) {
            self.recent.remove(0);
        }
        if limit == 0 {
            return None;
        }
        (1..=MAX_PERIOD).find_map(|p| self.cycle(p, limit as usize))
    }

    /// The stop reason if the last `period * limit` calls are one block of
    /// `period` calls repeated.
    fn cycle(&self, period: usize, limit: usize) -> Option<String> {
        let n = period * limit;
        let tail = self.recent.get(self.recent.len().checked_sub(n)?..)?;
        if !(period..n).all(|i| tail[i].1 == tail[i - period].1) {
            return None;
        }
        if period == 1 {
            return Some(format!("stuck: {limit} identical {} calls in a row with the same result", tail[0].0));
        }
        let tools: Vec<&str> = tail[..period].iter().map(|(t, _)| t.as_str()).collect();
        Some(format!(
            "stuck: the same {period} calls ({}) repeated {limit} times with the same results",
            tools.join(", ")
        ))
    }

    /// Identical calls (same fingerprint) at the end, in a row.
    pub fn count(&self) -> u32 {
        let Some((_, last)) = self.recent.last() else { return 0 };
        self.recent.iter().rev().take_while(|(_, f)| f == last).count() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_calls_stop_at_the_limit() {
        let mut g = RepeatGuard::default();
        let i = json!({"command": "ls"});
        assert_eq!(g.observe(3, "bash", &i, true, "a"), None);
        assert_eq!(g.observe(3, "bash", &i, true, "a"), None);
        let r = g.observe(3, "bash", &i, true, "a").expect("third identical call stops");
        assert!(r.contains("3 identical bash calls"), "{r}");
        assert_eq!(g.count(), 3);
    }

    #[test]
    fn any_difference_resets_the_count() {
        let i = json!({"command": "ls"});
        let other = json!({"command": "pwd"});
        // tool, input, ok flag and output each break the run of repeats.
        for (tool, input, ok, out) in [
            ("read_file", &i, true, "a"),
            ("bash", &other, true, "a"),
            ("bash", &i, false, "a"),
            ("bash", &i, true, "b"),
        ] {
            let mut g = RepeatGuard::default();
            g.observe(2, "bash", &i, true, "a");
            assert_eq!(g.observe(2, tool, input, ok, out), None, "{tool} {input} {ok} {out}");
            assert_eq!(g.count(), 1);
        }
        // Key order is not a difference: inputs compare canonically.
        let mut g = RepeatGuard::default();
        g.observe(2, "t", &json!({"a": 1, "b": 2}), true, "x");
        assert!(g.observe(2, "t", &json!({"b": 2, "a": 1}), true, "x").is_some());
    }

    #[test]
    fn limit_zero_disables_and_one_stops_immediately() {
        let mut g = RepeatGuard::default();
        for _ in 0..10 {
            assert_eq!(g.observe(0, "bash", &json!({}), true, ""), None);
        }
        assert_eq!(g.count(), MAX_PERIOD as u32, "only a short window is kept");
        assert!(RepeatGuard::default().observe(1, "bash", &json!({}), true, "").is_some());
    }

    #[test]
    fn state_survives_serialization() {
        let mut g = RepeatGuard::default();
        g.observe(3, "bash", &json!({}), true, "");
        g.observe(3, "bash", &json!({}), true, "");
        let mut back: RepeatGuard = serde_json::from_str(&serde_json::to_string(&g).unwrap()).unwrap();
        assert!(back.observe(3, "bash", &json!({}), true, "").is_some(), "a resumed run keeps counting");
        // State saved before cycles existed ({last, count}) still loads.
        let old: RepeatGuard = serde_json::from_str(r#"{"last": "x", "count": 2}"#).unwrap();
        assert_eq!(old.count(), 0);
    }

    #[test]
    fn a_repeating_block_of_calls_is_a_loop() {
        let (a, b, c) = (json!({"path": "a"}), json!({"path": "b"}), json!({"path": "c"}));
        // A-B repeated: stops on the 4th repetition (8th call), not before.
        let mut g = RepeatGuard::default();
        for i in 0..7 {
            let (t, inp) = if i % 2 == 0 { ("read_file", &a) } else { ("list_dir", &b) };
            assert_eq!(g.observe(4, t, inp, true, "x"), None, "call {i}");
        }
        let r = g.observe(4, "list_dir", &b, true, "x").unwrap();
        assert_eq!(r, "stuck: the same 2 calls (read_file, list_dir) repeated 4 times with the same results");
        // A-B-C repeated 3 times with limit 3.
        let mut g = RepeatGuard::default();
        let calls = [("t", &a), ("t", &b), ("t", &c)];
        let mut out = None;
        for i in 0..9 {
            out = g.observe(3, calls[i % 3].0, calls[i % 3].1, true, "x");
            assert_eq!(out.is_some(), i == 8, "call {i}");
        }
        assert!(out.unwrap().contains("same 3 calls (t, t, t)"));
        // A-B-A-B with a changing result is progress.
        let mut g = RepeatGuard::default();
        for i in 0..20 {
            let out = format!("{}", i / 2);
            let (t, inp) = if i % 2 == 0 { ("bash", &a) } else { ("list_dir", &b) };
            assert_eq!(g.observe(4, t, inp, true, &out), None, "call {i}");
        }
        // A block longer than MAX_PERIOD is not detected.
        let mut g = RepeatGuard::default();
        let four = [json!(1), json!(2), json!(3), json!(4)];
        for i in 0..32 {
            assert_eq!(g.observe(4, "t", &four[i % 4], true, "x"), None);
        }
    }

    #[test]
    fn failures_that_differ_only_in_digits_are_repeats() {
        let i = json!({"command": "pytest"});
        let mut g = RepeatGuard::default();
        for n in 0..3 {
            let out = format!("1 failed in 0.{n}3s (pid 40{n})");
            assert_eq!(g.observe(4, "bash", &i, false, &out), None);
        }
        assert!(g.observe(4, "bash", &i, false, "1 failed in 0.93s (pid 409)").is_some());
        // Successful output must match exactly: changing numbers are progress.
        let mut g = RepeatGuard::default();
        for n in 0..10 {
            assert_eq!(g.observe(4, "bash", &i, true, &format!("{n} passed")), None);
        }
        // Text that differs is still different when the call failed.
        let mut g = RepeatGuard::default();
        for w in ["a", "b", "c", "d", "e"] {
            assert_eq!(g.observe(4, "bash", &i, false, w), None);
        }
        assert_eq!(mask_digits("t=12.5s pid 7"), "t=#.#s pid #");
        assert_eq!(mask_digits("none"), "none");
    }
}
