//! Token awareness: what has been used, what is left, where the month is heading, and how Heron
//! should behave given that.
//!
//! Every model call is recorded per day and per model (see `SwappableLlm`). From that record and the
//! limits the user chose come three things:
//! - the figures shown in the app (used, left, projected);
//! - an effort level: as the allowance runs down, Heron takes fewer steps, answers more briefly,
//!   stops work nobody asked for, and prefers the cheaper model;
//! - routing: simple requests go to the cheaper model when one is configured, and a request that
//!   turns out to need real work is moved to the main model.
//!
//! No extra model call is made to decide any of this: spending tokens to save tokens defeats the point.

use crate::config::Config;
use crate::store::Store;
use chrono::{Datelike, Duration, NaiveDate};
use serde::Serialize;

/// How carefully to spend, from the share of the allowance that is left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// Plenty left (or no limit set).
    Normal,
    /// A quarter or less left: fewer steps, shorter answers, nothing in the background.
    Careful,
    /// Nearly gone: the cheapest model and the least work that answers the question.
    Minimal,
    /// The allowance is used up. A person asking directly is still answered, as cheaply as possible;
    /// nothing runs on its own.
    Spent,
}

impl Level {
    pub fn label(&self) -> &'static str {
        match self {
            Level::Normal => "Normal",
            Level::Careful => "Economy",
            Level::Minimal => "Strict economy",
            Level::Spent => "Allowance used up",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Window {
    pub used: u64,
    /// 0 means no limit.
    pub limit: u64,
    pub left: Option<u64>,
}

impl Window {
    fn new(used: u64, limit: u64) -> Window {
        Window { used, limit, left: (limit > 0).then(|| limit.saturating_sub(used)) }
    }
    /// Share of the limit still available, 1.0 when there is no limit.
    fn share_left(&self) -> f64 {
        if self.limit == 0 { 1.0 } else { self.left.unwrap_or(0) as f64 / self.limit as f64 }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Budget {
    pub level: Level,
    pub today: Window,
    pub month: Window,
    /// Average tokens per day over the last seven days (days with no use count as zero).
    pub daily_average: u64,
    /// Where this month ends up if the recent pace continues.
    pub projected_month: u64,
    /// The day the monthly limit would be reached at the recent pace, if that is within this month.
    pub limit_reached_on: Option<String>,
}

pub fn day_key(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

fn days_in_month(d: NaiveDate) -> u32 {
    let first_of_next = if d.month() == 12 { NaiveDate::from_ymd_opt(d.year() + 1, 1, 1) } else { NaiveDate::from_ymd_opt(d.year(), d.month() + 1, 1) };
    first_of_next.map(|n| (n - Duration::days(1)).day()).unwrap_or(30)
}

impl Budget {
    /// The budget as of `today`, from the stored record and the user's limits.
    pub fn on(store: &Store, cfg: &Config, today: NaiveDate) -> Budget {
        let per_day = store.usage_per_day(&day_key(today - Duration::days(40))).unwrap_or_default();
        let used_on = |d: NaiveDate| per_day.iter().find(|(k, _)| *k == day_key(d)).map(|(_, t)| *t).unwrap_or(0);
        let month_start = today.with_day(1).unwrap_or(today);
        let month_used: u64 = per_day.iter().filter(|(k, _)| k.as_str() >= day_key(month_start).as_str() && k.as_str() <= day_key(today).as_str()).map(|(_, t)| *t).sum();
        let today_w = Window::new(used_on(today), cfg.agent.daily_token_limit);
        let month_w = Window::new(month_used, cfg.agent.monthly_token_limit);

        // Pace: the last seven days including today. Early in a record, use only the days there are.
        let first_day = per_day.first().and_then(|(k, _)| NaiveDate::parse_from_str(k, "%Y-%m-%d").ok()).unwrap_or(today);
        let span = ((today - first_day).num_days() + 1).clamp(1, 7);
        let recent: u64 = (0..span).map(|i| used_on(today - Duration::days(i))).sum();
        let daily_average = recent / span as u64;
        let days_left = (days_in_month(today) - today.day()) as u64;
        let projected_month = month_used + daily_average * days_left;
        let limit_reached_on = match (month_w.left, daily_average) {
            (Some(left), avg) if avg > 0 && left > 0 => {
                let days = left / avg + 1;
                (days <= days_left).then(|| day_key(today + Duration::days(days as i64)))
            }
            (Some(0), _) => Some(day_key(today)),
            _ => None,
        };

        let share = today_w.share_left().min(month_w.share_left());
        let level = if share <= 0.0 {
            Level::Spent
        } else if share <= 0.08 {
            Level::Minimal
        } else if share <= 0.25 {
            Level::Careful
        } else {
            Level::Normal
        };
        Budget { level, today: today_w, month: month_w, daily_average, projected_month, limit_reached_on }
    }

    pub fn now(store: &Store, cfg: &Config) -> Budget {
        Budget::on(store, cfg, chrono::Local::now().date_naive())
    }

    /// Most steps one request may take at this level.
    pub fn max_steps(&self, configured: usize) -> usize {
        match self.level {
            Level::Normal => configured,
            Level::Careful => (configured * 3 / 5).max(8).min(configured),
            Level::Minimal | Level::Spent => configured.min(8),
        }
    }

    /// Largest reply to ask the model for at this level (`None` = whatever is configured).
    pub fn reply_tokens(&self) -> Option<u32> {
        match self.level {
            Level::Normal => None,
            Level::Careful => Some(4096),
            Level::Minimal | Level::Spent => Some(2048),
        }
    }

    /// May Heron start work nobody asked for (reflection, suggestions, learning from a conversation)?
    pub fn background_allowed(&self) -> bool {
        self.level == Level::Normal
    }

    /// What to tell the model, so its own choices are economical too.
    pub fn guidance(&self) -> Option<String> {
        let pct = (self.today.share_left().min(self.month.share_left()) * 100.0).round() as u64;
        match self.level {
            Level::Normal => None,
            Level::Careful => Some(format!("The user's token allowance is running low (about {pct}% left). Be economical: answer briefly, use tools only when the answer needs them, and do not hand work to a sub-agent.")),
            Level::Minimal => Some(format!("The user's token allowance is almost used up (about {pct}% left). Give the shortest answer that is correct and useful. Use a tool only if the request cannot be answered without it. Do not hand work to a sub-agent.")),
            Level::Spent => Some("The user's token allowance for this period is used up. Give the shortest answer that is correct and useful, avoid tools unless the request is impossible without one, and mention once that the allowance is used up and can be raised in Usage.".into()),
        }
    }

    /// One line for the person, shown when the level is not normal.
    pub fn notice(&self) -> Option<String> {
        let pct = (self.today.share_left().min(self.month.share_left()) * 100.0).round() as u64;
        match self.level {
            Level::Normal => None,
            Level::Careful => Some(format!("Economy mode: about {pct}% of your token allowance is left, so Heron is taking fewer steps and keeping answers short.")),
            Level::Minimal => Some(format!("Strict economy: about {pct}% of your token allowance is left. Heron is using its cheapest model and the least work that answers you.")),
            Level::Spent => Some("Your token allowance is used up. Heron will still answer you, as cheaply as it can, and has stopped everything it does on its own. You can raise the limit in Usage.".into()),
        }
    }
}

/// Which model should handle a request.
#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    /// `None` = the main model.
    pub model: Option<String>,
    /// Move to the main model if the request turns out to need more than this many steps.
    pub escalate_after: Option<usize>,
    pub why: &'static str,
}

/// Does this look like something a small, cheap model handles well? Judged from the request alone:
/// short, no code, and none of the words that signal real work.
pub fn looks_simple(input: &str) -> bool {
    let text = input.trim();
    if text.chars().count() > 280 || text.lines().count() > 4 || text.contains("```") {
        return false;
    }
    let lower = text.to_lowercase();
    const HEAVY: &[&str] = &[
        "write a", "write me", "draft", "implement", "refactor", "debug", "fix ", "build ", "create a", "design", "analy", "research", "compare", "plan ", "plan.", "summar", "review", "explain why",
        "step by step", "report", "investigate", "translate", "rewrite", "automate", "browser", "website", "spreadsheet", "code", "script", "essay", "strategy", "evaluate", "prove", "calculate",
    ];
    !HEAVY.iter().any(|w| lower.contains(w))
}

/// Choose the model for a request. Routing only happens when a separate cheaper model is configured;
/// otherwise everything goes to the main model as before.
pub fn route(cfg: &Config, level: Level, input: &str) -> Route {
    let fast = cfg.llm.fast_model.trim();
    let has_fast = !fast.is_empty() && fast != cfg.llm.model;
    if !has_fast {
        return Route { model: None, escalate_after: None, why: "one model is configured" };
    }
    if level >= Level::Minimal {
        return Route { model: Some(fast.to_string()), escalate_after: None, why: "the token allowance is nearly used up" };
    }
    if cfg.agent.routing.eq_ignore_ascii_case("off") {
        return Route { model: None, escalate_after: None, why: "routing is switched off" };
    }
    if looks_simple(input) {
        // A wrong guess corrects itself: real work shows up as steps, and then the main model takes over.
        return Route { model: Some(fast.to_string()), escalate_after: Some(if level == Level::Careful { 4 } else { 2 }), why: "a short, simple request" };
    }
    Route { model: None, escalate_after: None, why: "a request that needs the main model" }
}

/// Everything the Usage page shows.
#[derive(Debug, Serialize)]
pub struct Report {
    pub budget: Budget,
    pub level_label: &'static str,
    /// (day, tokens) for the last 30 days, oldest first; days without use are included as zero.
    pub days: Vec<(String, u64)>,
    /// Per model this month: (model, tokens in, tokens out, calls).
    pub models: Vec<(String, u64, u64, u64)>,
    pub main_model: String,
    pub fast_model: String,
    pub routing: bool,
    pub background_today: u64,
    pub background_limit: u64,
}

pub fn report(store: &Store, cfg: &Config, background_today: u64) -> Report {
    let today = chrono::Local::now().date_naive();
    let budget = Budget::on(store, cfg, today);
    let per_day = store.usage_per_day(&day_key(today - Duration::days(29))).unwrap_or_default();
    let days = (0..30).rev().map(|i| today - Duration::days(i)).map(|d| (day_key(d), per_day.iter().find(|(k, _)| *k == day_key(d)).map(|(_, t)| *t).unwrap_or(0))).collect();
    let models = store.usage_per_model(&day_key(today.with_day(1).unwrap_or(today))).unwrap_or_default();
    let fast = cfg.llm.fast_model.trim();
    Report {
        level_label: budget.level.label(),
        budget,
        days,
        models,
        main_model: cfg.llm.model.clone(),
        fast_model: fast.to_string(),
        routing: !fast.is_empty() && fast != cfg.llm.model && !cfg.agent.routing.eq_ignore_ascii_case("off"),
        background_today,
        background_limit: cfg.agent.background_tokens_per_day,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn used_left_and_projection_follow_the_record() {
        let store = Store::in_memory().unwrap();
        let mut cfg = Config::default();
        cfg.agent.monthly_token_limit = 1_000_000;
        cfg.agent.daily_token_limit = 100_000;
        // 20,000 a day for the first ten days of a 30-day month (two models on the last day).
        for d in 1..=9 {
            store.record_usage(&format!("2026-11-{d:02}"), "big", 15_000, 5_000).unwrap();
        }
        store.record_usage("2026-11-10", "big", 9_000, 3_000).unwrap();
        store.record_usage("2026-11-10", "small", 6_000, 2_000).unwrap();
        store.record_usage("2026-10-31", "big", 500_000, 0).unwrap(); // last month does not count
        let b = Budget::on(&store, &cfg, day("2026-11-10"));
        assert_eq!((b.today.used, b.today.left), (20_000, Some(80_000)));
        assert_eq!((b.month.used, b.month.left), (200_000, Some(800_000)));
        assert_eq!(b.daily_average, 20_000);
        assert_eq!(b.projected_month, 200_000 + 20 * 20_000);
        assert_eq!(b.limit_reached_on, None, "600,000 projected is within the limit");
        assert_eq!(b.level, Level::Normal);

        // With a tighter monthly limit the same pace runs out on the 21st, and effort drops as it nears.
        cfg.agent.monthly_token_limit = 400_000;
        let b = Budget::on(&store, &cfg, day("2026-11-10"));
        assert_eq!(b.limit_reached_on.as_deref(), Some("2026-11-21"));
        cfg.agent.monthly_token_limit = 250_000; // 20% left
        assert_eq!(Budget::on(&store, &cfg, day("2026-11-10")).level, Level::Careful);
        cfg.agent.monthly_token_limit = 210_000; // under 5% left
        assert_eq!(Budget::on(&store, &cfg, day("2026-11-10")).level, Level::Minimal);
        cfg.agent.monthly_token_limit = 200_000;
        let spent = Budget::on(&store, &cfg, day("2026-11-10"));
        assert_eq!((spent.level, spent.month.left), (Level::Spent, Some(0)));
        assert!(!spent.background_allowed() && spent.max_steps(40) == 8 && spent.reply_tokens() == Some(2048));

        // No limits: figures are still reported, and nothing is restricted.
        let free = Budget::on(&store, &Config::default(), day("2026-11-10"));
        assert_eq!((free.level, free.month.left, free.month.used), (Level::Normal, None, 200_000));
        assert!(free.guidance().is_none() && free.max_steps(40) == 40);

        // The per-model breakdown for the month.
        let models = store.usage_per_model("2026-11-01").unwrap();
        assert_eq!(models[0], ("big".to_string(), 9 * 15_000 + 9_000, 9 * 5_000 + 3_000, 10));
        assert_eq!(models[1], ("small".to_string(), 6_000, 2_000, 1));
    }

    #[test]
    fn simple_requests_go_to_the_cheaper_model_and_real_work_does_not() {
        let mut cfg = Config::default();
        cfg.llm.model = "big".into();
        // With one model there is nothing to route between.
        assert_eq!(route(&cfg, Level::Normal, "hi").model, None);
        cfg.llm.fast_model = "small".into();
        for simple in ["hi", "what time is my dentist appointment?", "remind me to call mum tomorrow at 9", "what do you remember about me?", "thanks!"] {
            let r = route(&cfg, Level::Normal, simple);
            assert_eq!((r.model.as_deref(), r.escalate_after), (Some("small"), Some(2)), "{simple}");
        }
        for heavy in [
            "Research the best way to learn Rust in a month and save a plan as plan.md",
            "refactor the parser module so errors carry line numbers",
            "Write a cover letter for the attached job description",
            "compare these three laptops and tell me which to buy",
            &"long ".repeat(80),
            "run this:\n```\ncargo test\n```",
        ] {
            assert_eq!(route(&cfg, Level::Normal, heavy).model, None, "{heavy}");
        }
        // When the allowance is nearly gone everything goes to the cheaper model, and stays there.
        let r = route(&cfg, Level::Minimal, "refactor the parser module");
        assert_eq!((r.model.as_deref(), r.escalate_after), (Some("small"), None));
        // The user can switch routing off; economy still applies at the very end.
        cfg.agent.routing = "off".into();
        assert_eq!(route(&cfg, Level::Normal, "hi").model, None);
        assert_eq!(route(&cfg, Level::Spent, "hi").model.as_deref(), Some("small"));
    }
}
