//! Loop guard: a turn repeating itself with nothing changing. A call's fingerprint is its tool,
//! canonical input, ok flag and output. A turn is stuck when one block of 1 to `MAX_PERIOD` calls
//! repeats `limit` times back to back. A failed call's output has its digit runs masked, so a
//! timing or a pid does not hide a repeat. A successful call's output must match exactly, because
//! changing numbers there are progress.

use crate::util::sha256_hex;
use serde_json::{json, Value};
use std::collections::VecDeque;

pub const DEFAULT_LIMIT: u32 = 4;

pub const MAX_PERIOD: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stop {
    pub period: usize,
    pub tools: Vec<String>,
    pub masked: bool,
}

#[derive(Debug, Clone, Default)]
pub struct RepeatGuard {
    recent: VecDeque<Entry>,
}

#[derive(Debug, Clone)]
struct Entry {
    tool: String,
    call: String,
    fingerprint: String,
    ok: bool,
}

pub fn mask_digits(s: &str) -> String {
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

/// Whether a tool result is a failure: an `Error:` or `Permission denied` start, or, for `shell`,
/// a nonzero `[exit code N]` line or a `[timed out after` start.
pub fn is_failure(tool: &str, text: &str) -> bool {
    if text.starts_with("Error:") || text.starts_with("Permission denied") {
        return true;
    }
    tool == "shell"
        && (text.starts_with("[timed out after")
            || text.lines().any(|line| {
                line.strip_prefix("[exit code ")
                    .and_then(|rest| rest.strip_suffix(']'))
                    .and_then(|code| code.parse::<i64>().ok())
                    .is_some_and(|code| code != 0)
            }))
}

pub fn stop_text(stop: &Stop, limit: u32) -> String {
    let subject = if stop.period == 1 { "call".to_string() } else { format!("{} calls", stop.period) };
    let mut text = format!(
        "Stopped: the same {subject} ({}) ran {limit} times in a row with the same results.",
        stop.tools.join(", ")
    );
    if stop.masked {
        text.push_str(" Failures that differ only in digits count as the same.");
    }
    text
}

impl RepeatGuard {
    /// Record one finished call. Returns a stop once a block of calls has repeated `limit` times
    /// in a row; `limit` 0 disables the stop.
    pub fn observe(&mut self, limit: u32, tool: &str, input: &Value, ok: bool, output: &str) -> Option<Stop> {
        let output = if ok { output.to_string() } else { mask_digits(output) };
        self.recent.push_back(Entry {
            tool: tool.to_string(),
            call: sha256_hex(json!([tool, input]).to_string().as_bytes()),
            fingerprint: sha256_hex(json!([tool, input, ok, output]).to_string().as_bytes()),
            ok,
        });
        if self.recent.len() > MAX_PERIOD * (limit.max(1) as usize) {
            self.recent.pop_front();
        }
        if limit == 0 {
            return None;
        }
        (1..=MAX_PERIOD).find_map(|p| self.cycle(p, limit as usize))
    }

    fn cycle(&self, period: usize, limit: usize) -> Option<Stop> {
        let n = period * limit;
        let tail: Vec<&Entry> = self.recent.iter().skip(self.recent.len().checked_sub(n)?).collect();
        if !(period..n).all(|i| tail[i].fingerprint == tail[i - period].fingerprint) {
            return None;
        }
        Some(Stop {
            period,
            tools: tail[..period].iter().map(|e| e.tool.clone()).collect(),
            masked: tail.iter().any(|e| !e.ok),
        })
    }

    /// How many calls with the same tool and input end the history, whatever their results.
    pub fn same_call_count(&self) -> u32 {
        let Some(last) = self.recent.back() else { return 0 };
        self.recent.iter().rev().take_while(|e| e.call == last.call).count() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_calls_stop_at_the_limit() {
        let mut g = RepeatGuard::default();
        let i = json!({"command": "ls"});
        assert_eq!(g.observe(3, "shell", &i, true, "a"), None);
        assert_eq!(g.observe(3, "shell", &i, true, "a"), None);
        let stop = g.observe(3, "shell", &i, true, "a").expect("third identical call stops");
        assert_eq!(stop, Stop { period: 1, tools: vec!["shell".into()], masked: false });
        assert_eq!(g.same_call_count(), 3);
    }

    #[test]
    fn any_difference_resets_the_count() {
        let i = json!({"command": "ls"});
        let other = json!({"command": "pwd"});
        for (tool, input, ok, out) in [
            ("read_file", &i, true, "a"),
            ("shell", &other, true, "a"),
            ("shell", &i, false, "a"),
            ("shell", &i, true, "b"),
        ] {
            let mut g = RepeatGuard::default();
            g.observe(2, "shell", &i, true, "a");
            assert_eq!(g.observe(2, tool, input, ok, out), None, "{tool} {input} {ok} {out}");
        }
        let mut g = RepeatGuard::default();
        g.observe(2, "t", &json!({"a": 1, "b": 2}), true, "x");
        assert!(g.observe(2, "t", &json!({"b": 2, "a": 1}), true, "x").is_some());
    }

    #[test]
    fn limit_zero_disables_and_one_stops_immediately() {
        let mut g = RepeatGuard::default();
        for _ in 0..10 {
            assert_eq!(g.observe(0, "shell", &json!({}), true, ""), None);
        }
        assert_eq!(g.same_call_count(), MAX_PERIOD as u32, "only a short window is kept");
        assert!(RepeatGuard::default().observe(1, "shell", &json!({}), true, "").is_some());
    }

    #[test]
    fn a_repeating_block_of_calls_is_a_loop() {
        let (a, b, c) = (json!({"path": "a"}), json!({"path": "b"}), json!({"path": "c"}));
        let mut g = RepeatGuard::default();
        for i in 0..7 {
            let (t, inp) = if i % 2 == 0 { ("read_file", &a) } else { ("list_dir", &b) };
            assert_eq!(g.observe(4, t, inp, true, "x"), None, "call {i}");
        }
        let stop = g.observe(4, "list_dir", &b, true, "x").unwrap();
        assert_eq!(stop, Stop { period: 2, tools: vec!["read_file".into(), "list_dir".into()], masked: false });

        let mut g = RepeatGuard::default();
        let calls = [("t", &a), ("t", &b), ("t", &c)];
        let mut out = None;
        for i in 0..9 {
            out = g.observe(3, calls[i % 3].0, calls[i % 3].1, true, "x");
            assert_eq!(out.is_some(), i == 8, "call {i}");
        }
        assert_eq!(out.unwrap().period, 3);

        let mut g = RepeatGuard::default();
        for i in 0..20 {
            let out = format!("{}", i / 2);
            let (t, inp) = if i % 2 == 0 { ("shell", &a) } else { ("list_dir", &b) };
            assert_eq!(g.observe(4, t, inp, true, &out), None, "call {i}");
        }

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
            assert_eq!(g.observe(4, "shell", &i, false, &out), None);
        }
        let stop = g.observe(4, "shell", &i, false, "1 failed in 0.93s (pid 409)").expect("masked repeat stops");
        assert!(stop.masked);

        let mut g = RepeatGuard::default();
        for n in 0..10 {
            assert_eq!(g.observe(4, "shell", &i, true, &format!("{n} passed")), None);
        }

        let mut g = RepeatGuard::default();
        for w in ["a", "b", "c", "d", "e"] {
            assert_eq!(g.observe(4, "shell", &i, false, w), None);
        }
        assert_eq!(mask_digits("t=12.5s pid 7"), "t=#.#s pid #");
        assert_eq!(mask_digits("none"), "none");
    }

    #[test]
    fn same_call_count_is_three_after_three_identical_calls() {
        let mut g = RepeatGuard::default();
        assert_eq!(g.same_call_count(), 0);
        let i = json!({"path": "a"});
        for want in 1..=3 {
            g.observe(4, "read_file", &i, true, "x");
            assert_eq!(g.same_call_count(), want);
        }
        g.observe(4, "read_file", &json!({"path": "b"}), true, "x");
        assert_eq!(g.same_call_count(), 1);
        g.observe(4, "list_dir", &json!({"path": "b"}), true, "x");
        assert_eq!(g.same_call_count(), 1, "the tool name is part of the call");
    }

    #[test]
    fn same_call_count_ignores_results() {
        let mut g = RepeatGuard::default();
        let i = json!({"command": "make"});
        g.observe(4, "shell", &i, true, "one");
        g.observe(4, "shell", &i, false, "Error: two");
        g.observe(4, "shell", &i, true, "three");
        assert_eq!(g.same_call_count(), 3);
    }

    #[test]
    fn shell_exit_code_line_counts_as_failure() {
        assert!(is_failure("shell", "boom\n[exit code 2]"));
        assert!(is_failure("shell", "[exit code -1]"));
        assert!(!is_failure("shell", "fine\n[exit code 0]"));
        assert!(!is_failure("shell", "see [exit code 2] in the docs"));
        assert!(!is_failure("read_file", "boom\n[exit code 2]"));
        assert!(is_failure("read_file", "Error: no such file"));
        assert!(is_failure("shell", "Permission denied: rm"));
        assert!(!is_failure("shell", "all good"));
    }

    #[test]
    fn shell_timeout_counts_as_failure() {
        assert!(is_failure("shell", "[timed out after 30s and was killed. For long-running work, raise timeout_secs.]"));
        assert!(!is_failure("read_file", "[timed out after 30s]"));
        assert!(!is_failure("shell", "output [timed out after 30s]"));
    }

    #[test]
    fn stop_text_uses_one_template() {
        let one = Stop { period: 1, tools: vec!["shell".into()], masked: false };
        assert_eq!(stop_text(&one, 4), "Stopped: the same call (shell) ran 4 times in a row with the same results.");
        let two = Stop { period: 2, tools: vec!["read_file".into(), "shell".into()], masked: false };
        assert_eq!(
            stop_text(&two, 3),
            "Stopped: the same 2 calls (read_file, shell) ran 3 times in a row with the same results."
        );
        let masked = Stop { masked: true, ..one };
        assert_eq!(
            stop_text(&masked, 4),
            "Stopped: the same call (shell) ran 4 times in a row with the same results. Failures that differ only in digits count as the same."
        );
    }
}
