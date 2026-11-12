//! Time expressions for reminders and recurring jobs: cron (5-field), "in 10m",
//! "every 2h", "daily 09:00", "tomorrow 8:30", or an absolute date-time.

use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike};

#[derive(Debug, Clone, PartialEq)]
pub struct Cron {
    minutes: Vec<bool>,
    hours: Vec<bool>,
    dom: Vec<bool>,
    months: Vec<bool>,
    dow: Vec<bool>,
    dom_any: bool,
    dow_any: bool,
}

fn parse_field(field: &str, min: u32, max: u32) -> Result<Vec<bool>> {
    let mut set = vec![false; (max + 1) as usize];
    for part in field.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => (r, s.parse::<u32>().map_err(|_| anyhow!("bad step in '{part}'"))?),
            None => (part, 1),
        };
        if step == 0 {
            bail!("step cannot be 0 in '{part}'");
        }
        let (lo, hi) = if range == "*" {
            (min, max)
        } else if let Some((a, b)) = range.split_once('-') {
            (a.parse()?, b.parse()?)
        } else {
            let v: u32 = range.parse().map_err(|_| anyhow!("bad cron value '{range}'"))?;
            (v, if part.contains('/') { max } else { v })
        };
        if lo < min || hi > max || lo > hi {
            bail!("cron value out of range in '{part}' (allowed {min}-{max})");
        }
        let mut v = lo;
        while v <= hi {
            set[v as usize] = true;
            v += step;
        }
    }
    Ok(set)
}

impl Cron {
    pub fn parse(expr: &str) -> Result<Cron> {
        let f: Vec<&str> = expr.split_whitespace().collect();
        if f.len() != 5 {
            bail!("cron needs 5 fields (minute hour day-of-month month day-of-week), got {}", f.len());
        }
        let mut dow = parse_field(f[4], 0, 7)?;
        if dow[7] {
            dow[0] = true; // 7 and 0 both mean Sunday
        }
        Ok(Cron {
            minutes: parse_field(f[0], 0, 59)?,
            hours: parse_field(f[1], 0, 23)?,
            dom: parse_field(f[2], 1, 31)?,
            months: parse_field(f[3], 1, 12)?,
            dow,
            dom_any: f[2] == "*",
            dow_any: f[4] == "*",
        })
    }

    fn day_matches(&self, d: NaiveDate) -> bool {
        if !self.months[d.month() as usize] {
            return false;
        }
        let dom = self.dom[d.day() as usize];
        let dow = self.dow[d.weekday().num_days_from_sunday() as usize];
        // Standard cron: when both day fields are restricted, either may match.
        match (self.dom_any, self.dow_any) {
            (true, true) => true,
            (true, false) => dow,
            (false, true) => dom,
            (false, false) => dom || dow,
        }
    }

    /// First matching time strictly after `after`.
    pub fn next_after(&self, after: DateTime<Local>) -> Option<DateTime<Local>> {
        let start = (after + Duration::minutes(1)).with_second(0)?.with_nanosecond(0)?.naive_local();
        let mut date = start.date();
        for day in 0..(366 * 5) {
            if self.day_matches(date) {
                for h in 0..24u32 {
                    if !self.hours[h as usize] {
                        continue;
                    }
                    for m in 0..60u32 {
                        if !self.minutes[m as usize] {
                            continue;
                        }
                        let cand = NaiveDateTime::new(date, NaiveTime::from_hms_opt(h, m, 0)?);
                        if day == 0 && cand < start {
                            continue;
                        }
                        // Skips times that do not exist locally (DST spring-forward).
                        if let Some(t) = Local.from_local_datetime(&cand).earliest() {
                            return Some(t);
                        }
                    }
                }
            }
            date = date.succ_opt()?;
        }
        None
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct When {
    /// Cron expression if recurring, empty if one-shot.
    pub cron: String,
    pub next_run: i64,
}

fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim().to_lowercase();
    let split = s.find(|c: char| !c.is_ascii_digit() && c != ' ')?;
    let n: i64 = s[..split].trim().parse().ok()?;
    if n <= 0 {
        return None;
    }
    match s[split..].trim() {
        "s" | "sec" | "secs" | "second" | "seconds" => Some(Duration::seconds(n)),
        "m" | "min" | "mins" | "minute" | "minutes" => Some(Duration::minutes(n)),
        "h" | "hr" | "hrs" | "hour" | "hours" => Some(Duration::hours(n)),
        "d" | "day" | "days" => Some(Duration::days(n)),
        "w" | "week" | "weeks" => Some(Duration::weeks(n)),
        _ => None,
    }
}

fn parse_time(s: &str) -> Option<NaiveTime> {
    let s = s.trim().to_lowercase().replace(' ', "");
    let (body, pm) = match (s.strip_suffix("am"), s.strip_suffix("pm")) {
        (Some(b), _) => (b, Some(false)),
        (_, Some(b)) => (b, Some(true)),
        _ => (s.as_str(), None),
    };
    let (h, m) = match body.split_once(':') {
        Some((h, m)) => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        None => (body.parse::<u32>().ok()?, 0),
    };
    let h = match pm {
        Some(_) if !(1..=12).contains(&h) => return None,
        Some(true) => h % 12 + 12,
        Some(false) => h % 12,
        None => h,
    };
    NaiveTime::from_hms_opt(h, m, 0)
}

fn local_ts(dt: NaiveDateTime) -> Result<i64> {
    Local.from_local_datetime(&dt).earliest().map(|t| t.timestamp()).ok_or_else(|| anyhow!("that local time does not exist (DST gap)"))
}

/// Parse a human or cron time expression relative to `now`.
pub fn parse_when(expr: &str, now: DateTime<Local>) -> Result<When> {
    let e = expr.trim();
    let lower = e.to_lowercase();

    if let Some(rest) = lower.strip_prefix("in ") {
        let d = parse_duration(rest).ok_or_else(|| anyhow!("could not parse duration '{rest}' (try 'in 10m', 'in 2 hours')"))?;
        return Ok(When { cron: String::new(), next_run: (now + d).timestamp() });
    }
    if let Some(rest) = lower.strip_prefix("every ") {
        let rest = rest.trim();
        let cron = if let Some(t) = rest.strip_prefix("day at ").or_else(|| rest.strip_prefix("day ")).and_then(parse_time) {
            format!("{} {} * * *", t.minute(), t.hour())
        } else {
            let d = parse_duration(rest).or_else(|| parse_duration(&format!("1 {rest}"))).ok_or_else(|| anyhow!("could not parse interval '{rest}'"))?;
            let mins = d.num_minutes();
            match mins {
                1..=59 if 60 % mins == 0 => format!("*/{mins} * * * *"),
                60..=1439 if mins % 60 == 0 && 24 % (mins / 60) == 0 => format!("0 */{} * * *", mins / 60),
                1440 => format!("{} {} * * *", now.minute(), now.hour()),
                10080 => format!("{} {} * * {}", now.minute(), now.hour(), now.weekday().num_days_from_sunday()),
                _ => bail!("interval '{rest}' does not divide evenly into a clock schedule; use a cron expression instead"),
            }
        };
        return cron_when(&cron, now);
    }
    if let Some(t) = lower.strip_prefix("daily at ").or_else(|| lower.strip_prefix("daily ")).and_then(parse_time) {
        return cron_when(&format!("{} {} * * *", t.minute(), t.hour()), now);
    }
    for (prefix, offset) in [("today", 0), ("tomorrow", 1)] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            let rest = rest.trim().trim_start_matches("at ").trim();
            let t = if rest.is_empty() { NaiveTime::from_hms_opt(9, 0, 0) } else { parse_time(rest) }.ok_or_else(|| anyhow!("could not parse time '{rest}'"))?;
            let ts = local_ts(NaiveDateTime::new(now.date_naive() + Duration::days(offset), t))?;
            if ts <= now.timestamp() {
                bail!("that time is already in the past");
            }
            return Ok(When { cron: String::new(), next_run: ts });
        }
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(e) {
        return one_shot(dt.timestamp(), now);
    }
    for fmt in ["%Y-%m-%d %H:%M", "%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S"] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(e, fmt) {
            return one_shot(local_ts(dt)?, now);
        }
    }
    if let Ok(d) = NaiveDate::parse_from_str(e, "%Y-%m-%d") {
        return one_shot(local_ts(d.and_hms_opt(9, 0, 0).unwrap())?, now);
    }
    // Only report cron details when the input actually looks like a cron expression.
    let looks_like_cron = e.split_whitespace().count() == 5;
    cron_when(e, now).map_err(|err| {
        if looks_like_cron {
            anyhow!("That schedule is not valid: {err}.")
        } else {
            anyhow!("Could not understand \"{expr}\" as a time. Try \"in 30m\", \"tomorrow 9:00\", \"daily 08:00\", \"every 2h\" or \"2026-12-01 14:00\".")
        }
    })
}

fn one_shot(ts: i64, now: DateTime<Local>) -> Result<When> {
    if ts <= now.timestamp() {
        bail!("that time is already in the past");
    }
    Ok(When { cron: String::new(), next_run: ts })
}

fn cron_when(cron: &str, now: DateTime<Local>) -> Result<When> {
    let c = Cron::parse(cron)?;
    let next = c.next_after(now).ok_or_else(|| anyhow!("cron expression never fires"))?;
    Ok(When { cron: cron.to_string(), next_run: next.timestamp() })
}

/// Next run for a stored cron string, if it is recurring.
pub fn next_run(cron: &str, now: DateTime<Local>) -> Option<i64> {
    if cron.is_empty() {
        return None;
    }
    Cron::parse(cron).ok()?.next_after(now).map(|t| t.timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> DateTime<Local> {
        Local.from_local_datetime(&NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()).unwrap()
    }
    fn next(cron: &str, from: &str) -> String {
        Cron::parse(cron).unwrap().next_after(at(from)).unwrap().format("%Y-%m-%d %H:%M").to_string()
    }

    #[test]
    fn cron_next() {
        // 2026-11-12 is a Thursday
        assert_eq!(next("0 9 * * *", "2026-11-12 08:00"), "2026-11-12 09:00");
        assert_eq!(next("0 9 * * *", "2026-11-12 09:00"), "2026-11-13 09:00");
        assert_eq!(next("*/15 * * * *", "2026-11-12 08:07"), "2026-11-12 08:15");
        assert_eq!(next("0 9 * * 1-5", "2026-11-13 10:00"), "2026-11-16 09:00");
        assert_eq!(next("30 18 * * 0", "2026-11-12 00:00"), "2026-11-15 18:30");
        assert_eq!(next("30 18 * * 7", "2026-11-12 00:00"), "2026-11-15 18:30");
        assert_eq!(next("0 0 1 1 *", "2026-11-12 00:00"), "2027-01-01 00:00");
        assert_eq!(next("0 12 29 2 *", "2026-11-12 00:00"), "2028-02-29 12:00");
        assert_eq!(next("0 8 1,15 * *", "2026-11-12 00:00"), "2026-11-15 08:00");
        // dom OR dow when both restricted
        assert_eq!(next("0 8 20 * 5", "2026-11-12 09:00"), "2026-11-13 08:00");
    }

    #[test]
    fn cron_rejects_garbage() {
        for bad in ["* * * *", "60 * * * *", "* 24 * * *", "*/0 * * * *", "a b c d e", "5-1 * * * *", "* * 0 * *"] {
            assert!(Cron::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn human_expressions() {
        let now = at("2026-11-12 10:00");
        assert_eq!(parse_when("in 10m", now).unwrap(), When { cron: "".into(), next_run: now.timestamp() + 600 });
        assert_eq!(parse_when("in 2 hours", now).unwrap().next_run, now.timestamp() + 7200);
        assert_eq!(parse_when("every 30m", now).unwrap().cron, "*/30 * * * *");
        assert_eq!(parse_when("every 2h", now).unwrap().cron, "0 */2 * * *");
        assert_eq!(parse_when("every hour", now).unwrap().cron, "0 */1 * * *");
        assert_eq!(parse_when("every day at 7:30", now).unwrap().cron, "30 7 * * *");
        assert_eq!(parse_when("daily 9am", now).unwrap().cron, "0 9 * * *");
        assert_eq!(parse_when("tomorrow 8:30", now).unwrap().next_run, at("2026-11-13 08:30").timestamp());
        assert_eq!(parse_when("today at 5pm", now).unwrap().next_run, at("2026-11-12 17:00").timestamp());
        assert_eq!(parse_when("2026-12-01 14:00", now).unwrap().next_run, at("2026-12-01 14:00").timestamp());
        assert_eq!(parse_when("0 9 * * 1-5", now).unwrap().cron, "0 9 * * 1-5");
        assert!(parse_when("today 9:00", now).is_err());
        assert!(parse_when("2020-01-01 00:00", now).is_err());
        assert!(parse_when("every 7m", now).is_err());
        assert!(parse_when("whenever", now).is_err());
    }
}
