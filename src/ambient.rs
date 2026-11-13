//! Ambient context: what has the user been working on? Heron looks at which files
//! in the working folder changed recently, so that context is ready before it is asked for.
//! Stateless and bounded: one capped directory walk per turn, no file contents read.

use crate::permissions::{classify_path, Effect};
use std::path::Path;
use std::time::{Duration, SystemTime};

const MAX_ENTRIES: usize = 20_000;

/// Files modified within `window`, newest first, as (relative path, seconds ago).
pub fn recent_changes(workspace: &Path, window: Duration, limit: usize) -> Vec<(String, u64)> {
    let now = SystemTime::now();
    let mut found: Vec<(String, u64)> = vec![];
    let walk = ignore::WalkBuilder::new(workspace)
        .max_depth(Some(6))
        .hidden(true)
        .filter_entry(|e| !matches!(e.file_name().to_str(), Some("node_modules" | "target" | "dist" | "build" | "__pycache__" | "Library" | "AppData")))
        .build();
    for entry in walk.flatten().take(MAX_ENTRIES) {
        let path = entry.path();
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) || classify_path(path, workspace, false) == Effect::SensitiveRead {
            continue;
        }
        let Ok(modified) = entry.metadata().map_err(|_| ()).and_then(|m| m.modified().map_err(|_| ())) else { continue };
        let Ok(age) = now.duration_since(modified) else { continue };
        if age <= window {
            let rel = path.strip_prefix(workspace).unwrap_or(path).to_string_lossy().replace('\\', "/");
            found.push((rel, age.as_secs()));
        }
    }
    found.sort_by_key(|(_, age)| *age);
    found.truncate(limit);
    found
}

pub fn describe_age(secs: u64) -> String {
    match secs {
        0..=89 => "just now".into(),
        90..=5399 => format!("{} min ago", (secs + 30) / 60),
        5400..=129_599 => format!("{} h ago", (secs + 1800) / 3600),
        _ => format!("{} days ago", (secs + 43_200) / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_recent_files_but_never_secrets_or_build_output() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("notes")).unwrap();
        std::fs::create_dir_all(d.path().join("target")).unwrap();
        std::fs::write(d.path().join("notes/plan.md"), "x").unwrap();
        std::fs::write(d.path().join("target/out.bin"), "x").unwrap();
        std::fs::write(d.path().join(".env"), "SECRET=1").unwrap();
        let r = recent_changes(d.path(), Duration::from_secs(3600), 10);
        assert_eq!(r.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(), vec!["notes/plan.md"]);
        assert!(recent_changes(d.path(), Duration::from_secs(0), 10).is_empty() || r[0].1 == 0);
        assert_eq!(describe_age(30), "just now");
        assert_eq!(describe_age(600), "10 min ago");
        assert_eq!(describe_age(7200), "2 h ago");
    }
}

// ---------------------------------------------------------------------------
// Background observer: a local journal of what happens in the working folder,
// and the awareness derived from it. Names and times only — never file contents.
// ---------------------------------------------------------------------------

use crate::store::Store;
use chrono::{Local, TimeZone, Timelike};
use serde::Serialize;

const DAY: i64 = 86_400;

#[derive(Debug, Clone, Default, Serialize)]
pub struct Awareness {
    /// Areas of the folder with the most activity over the last week: (area, changes).
    pub focus: Vec<(String, usize)>,
    /// Usual working hours (local, inclusive start, exclusive end), once there is enough history.
    pub active_hours: Option<(u32, u32)>,
    pub branch: Option<String>,
    /// Most recently changed files: (path, seconds ago).
    pub recent: Vec<(String, u64)>,
}

/// One observation pass: record files changed since the last pass. Returns how many new things were noticed.
pub fn observe(store: &Store, workspace: &Path) -> usize {
    let now = crate::util::now_ts();
    let last: i64 = store.kv_get("observer_last").and_then(|v| v.parse().ok()).unwrap_or(now - DAY);
    // Look back a little past the last pass so nothing slips between two scans.
    let window = Duration::from_secs(((now - last).clamp(60, 7 * DAY) + 30) as u64);
    // Heron's own edits are not the user's activity; counting them would make it react to itself.
    let own: std::collections::HashSet<String> = store
        .checkpoint_paths_since(last - 3600)
        .iter()
        .filter_map(|p| Path::new(p).strip_prefix(workspace).ok().map(|r| r.to_string_lossy().replace('\\', "/")))
        .collect();
    let mut noticed = 0;
    for (path, age) in recent_changes(workspace, window, 400) {
        if own.contains(&path) {
            continue;
        }
        if store.observe("file", &path, now - age as i64) {
            noticed += 1;
        }
    }
    let _ = store.kv_set("observer_last", &now.to_string());
    store.prune_observations(30 * DAY);
    noticed
}

/// The git branch checked out in the folder, read straight from `.git/HEAD` (no command is run).
pub fn git_branch(workspace: &Path) -> Option<String> {
    let head = std::fs::read_to_string(workspace.join(".git").join("HEAD")).ok()?;
    head.trim().strip_prefix("ref: refs/heads/").map(|b| b.to_string())
}

fn area_of(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    match parts.len() {
        0 | 1 => "(top level)".to_string(),
        2 => parts[0].to_string(),
        _ => format!("{}/{}", parts[0], parts[1]),
    }
}

/// Smallest run of hours that holds at least 80% of the activity.
fn usual_hours(hist: &[usize; 24]) -> Option<(u32, u32)> {
    let total: usize = hist.iter().sum();
    if total < 12 {
        return None; // not enough history to say anything honest
    }
    let mut best: Option<(u32, u32)> = None;
    for len in 1..=24u32 {
        for start in 0..24u32 {
            let sum: usize = (0..len).map(|i| hist[((start + i) % 24) as usize]).sum();
            if sum * 5 >= total * 4 {
                best = Some((start, (start + len) % 24));
                break;
            }
        }
        if best.is_some() {
            break;
        }
    }
    best.filter(|(s, e)| !(s == e)) // active around the clock tells us nothing
}

pub fn awareness(store: &Store, workspace: &Path) -> Awareness {
    let now = crate::util::now_ts();
    let week = store.observations_since(now - 7 * DAY, 5000).unwrap_or_default();
    let mut areas: std::collections::HashMap<String, usize> = Default::default();
    for (_, kind, subject) in &week {
        if kind == "file" {
            *areas.entry(area_of(subject)).or_default() += 1;
        }
    }
    let mut focus: Vec<(String, usize)> = areas.into_iter().collect();
    focus.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    focus.truncate(4);

    let mut hist = [0usize; 24];
    for (ts, _, _) in store.observations_since(now - 14 * DAY, 5000).unwrap_or_default() {
        if let Some(t) = Local.timestamp_opt(ts, 0).single() {
            hist[t.hour() as usize] += 1;
        }
    }
    let mut seen = std::collections::HashSet::new();
    let recent = week
        .iter()
        .filter(|(ts, kind, subject)| kind == "file" && now - ts <= 2 * DAY && seen.insert(subject.clone()))
        .take(8)
        .map(|(ts, _, subject)| (subject.clone(), (now - ts).max(0) as u64))
        .collect();
    Awareness { focus, active_hours: usual_hours(&hist), branch: git_branch(workspace), recent }
}

/// The awareness section of the system prompt. Empty when nothing has been noticed.
pub fn describe(a: &Awareness) -> String {
    let mut p = String::new();
    if a.recent.is_empty() && a.focus.is_empty() && a.branch.is_empty_or_none() {
        return p;
    }
    p.push_str("\n# What you have noticed in the background\nObserved locally from file names and times (not contents). Use it to understand what the user is working on; read a file before relying on what is in it.\n");
    if !a.recent.is_empty() {
        p.push_str("Recently changed:\n");
        for (path, age) in &a.recent {
            p.push_str(&format!("- {path} ({})\n", describe_age(*age)));
        }
    }
    if !a.focus.is_empty() {
        p.push_str(&format!("Focus this week: {}\n", a.focus.iter().map(|(area, n)| format!("{area} ({n} changes)")).collect::<Vec<_>>().join(", ")));
    }
    if let Some(b) = &a.branch {
        p.push_str(&format!("Git branch checked out: {b}\n"));
    }
    if let Some((s, e)) = a.active_hours {
        p.push_str(&format!("The user is usually active between {s}:00 and {e}:00.\n"));
    }
    p
}

trait EmptyOrNone {
    fn is_empty_or_none(&self) -> bool;
}
impl EmptyOrNone for Option<String> {
    fn is_empty_or_none(&self) -> bool {
        self.as_deref().map(str::is_empty).unwrap_or(true)
    }
}

#[cfg(test)]
mod observer_tests {
    use super::*;

    #[test]
    fn observer_builds_awareness_from_the_folder() {
        let d = tempfile::tempdir().unwrap();
        let store = Store::in_memory().unwrap();
        for f in ["book/ch1/draft.md", "book/ch1/notes.md", "book/ch2/outline.md", "taxes/2026.csv", "todo.txt"] {
            let p = d.path().join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "x").unwrap();
        }
        std::fs::create_dir_all(d.path().join(".git")).unwrap();
        std::fs::write(d.path().join(".git/HEAD"), "ref: refs/heads/chapter-two\n").unwrap();
        std::fs::write(d.path().join(".env"), "SECRET=1").unwrap();

        // a file Heron wrote itself is not treated as the user's activity
        std::fs::write(d.path().join("by-heron.md"), "x").unwrap();
        store.save_checkpoint("s", &d.path().join("by-heron.md").to_string_lossy(), None, None).unwrap();
        assert_eq!(observe(&store, d.path()), 5);
        assert_eq!(observe(&store, d.path()), 0, "the same change is not counted twice");
        let a = awareness(&store, d.path());
        assert_eq!(a.focus[0], ("book/ch1".to_string(), 2));
        assert_eq!(a.branch.as_deref(), Some("chapter-two"));
        assert_eq!(a.recent.len(), 5);
        assert!(a.active_hours.is_none(), "no claims about habits from five data points");
        let text = describe(&a);
        assert!(text.contains("book/ch1 (2 changes)") && text.contains("chapter-two"));
        assert!(!text.contains(".env") && !text.contains("SECRET"));

        store.clear_observations().unwrap();
        assert!(awareness(&store, d.path()).recent.is_empty());
        assert_eq!(describe(&Awareness::default()), "");
    }

    #[test]
    fn usual_hours_needs_evidence_and_finds_the_working_day() {
        let mut hist = [0usize; 24];
        assert_eq!(usual_hours(&hist), None);
        for h in 9..17 {
            hist[h] = 10;
        }
        hist[22] = 2;
        assert_eq!(usual_hours(&hist), Some((9, 16)));
        // night owl: the window wraps past midnight
        let mut night = [0usize; 24];
        for h in [22, 23, 0, 1] {
            night[h] = 8;
        }
        assert_eq!(usual_hours(&night), Some((22, 2)));
    }
}
