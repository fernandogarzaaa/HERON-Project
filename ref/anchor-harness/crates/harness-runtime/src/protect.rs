//! Paths that run code later, outside the sandbox, when a human uses the
//! workspace or Claude Code: git hooks and config, agent and editor config,
//! shell rc files, and Claude Code's own user settings, commands, agents,
//! skills, hooks and plugins. Sandboxed processes and file tools may read
//! them but not write them (owner decision, 2026-10-26).
//!
//! - macOS matches the workspace names at any depth by pattern, so new files
//!   are refused too. Git paths are protected for the repositories that
//!   exist when the run starts; an agent can still `git init` its own.
//! - bubblewrap can only cover paths that exist (read-only binds).
//! - File tools refuse every protected name and anything under `.git`.
//! - A snapshot taken before the executor starts and compared after it
//!   stops catches what the sandbox could not prevent: any change ends the
//!   run `inconclusive`. A repository the agent created may hold only sample
//!   hooks and plain configuration (see `NEW_REPO_CONFIG_KEYS`).

use crate::sandbox::{real_path, SandboxPolicy};
use harness_core::canonical::{content_hash, sha256_file};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

/// Names that are write-protected at any depth of the workspace.
pub const WORKSPACE_NAMES: &[&str] = &[
    ".gitmodules",
    ".husky",
    ".mcp.json",
    ".claude",
    ".vscode",
    ".idea",
    ".envrc",
    ".ripgreprc",
    ".bashrc",
    ".bash_profile",
    ".zshrc",
    ".zprofile",
    ".zshenv",
    ".profile",
];

/// Parts of an existing repository's `.git` directory that are protected.
const GIT_PARTS: &[&str] = &["hooks", "config"];

/// Claude Code's user configuration under `~/.claude` that runs or steers
/// later sessions. Transcripts, todos and caches stay writable.
pub const CLAUDE_HOME_ITEMS: &[&str] =
    &["settings.json", "settings.local.json", "CLAUDE.md", "commands", "agents", "skills", "hooks", "plugins"];

/// Directories the scan does not descend into (large, and never config).
const SKIP_DIRS: &[&str] = &["node_modules", "target", ".venv", "venv", "__pycache__"];

/// Upper bound on directory entries the scan visits.
pub const SCAN_LIMIT: usize = 100_000;

/// Keys `git init` and ordinary use write to a new repository's config.
/// Anything else (core.hooksPath, filters, aliases, includes, helpers, …)
/// can run code and counts as a change.
pub const NEW_REPO_CONFIG_KEYS: &[&str] = &[
    "core.repositoryformatversion",
    "core.filemode",
    "core.bare",
    "core.logallrefupdates",
    "core.ignorecase",
    "core.precomposeunicode",
    "core.symlinks",
    "init.defaultbranch",
    "user.name",
    "user.email",
];

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Scan {
    /// `.git` entries (directories or gitdir files).
    pub repos: Vec<PathBuf>,
    /// Entries with a protected name.
    pub named: Vec<PathBuf>,
    pub truncated: bool,
}

/// Find protected entries in the workspace without following symlinks.
pub fn scan(ws: &Path) -> Scan {
    scan_limited(ws, SCAN_LIMIT)
}

fn scan_limited(ws: &Path, limit: usize) -> Scan {
    let mut out = Scan::default();
    let mut stack = vec![ws.to_path_buf()];
    let mut seen = 0usize;
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            seen += 1;
            if seen > limit {
                out.truncated = true;
                return out;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            let path = e.path();
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if name == ".git" {
                out.repos.push(path);
            } else if WORKSPACE_NAMES.contains(&name.as_str()) {
                out.named.push(path);
            } else if is_dir && !SKIP_DIRS.contains(&name.as_str()) {
                stack.push(path);
            }
        }
    }
    out.repos.sort();
    out.named.sort();
    out
}

/// Whether a file tool may write `rel` (relative to the workspace): never
/// under `.git` or a protected name.
pub fn is_write_protected(rel: &Path) -> bool {
    rel.components().any(|c| match c {
        Component::Normal(n) => {
            let n = n.to_string_lossy();
            n == ".git" || WORKSPACE_NAMES.contains(&n.as_ref())
        }
        _ => false,
    })
}

/// Escape a path for an SBPL deny regex. A `"` cannot be escaped inside an
/// SBPL `#"..."` literal, so it becomes `.`: the deny only widens.
fn regex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch == '"' {
            out.push('.');
            continue;
        }
        if "\\.^$|()[]{}*+?".contains(ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Add the workspace's write protections to a sandbox policy.
pub fn apply(policy: &mut SandboxPolicy, ws: &Path) {
    let s = scan(ws);
    let root = regex_escape(&real_path(ws).to_string_lossy());
    let names = WORKSPACE_NAMES.iter().map(|n| regex_escape(n)).collect::<Vec<_>>().join("|");
    policy.deny_write_regex.push(format!("^{root}/(.*/)?({names})(/.*)?$"));
    policy.deny_write.extend(s.named);
    for git in s.repos {
        if git.is_dir() {
            policy.deny_write.extend(GIT_PARTS.iter().map(|p| git.join(p)));
            // The entry itself: it cannot be renamed, removed or replaced.
            policy.deny_write_regex.push(format!("^{}$", regex_escape(&real_path(&git).to_string_lossy())));
        } else {
            policy.deny_write.push(git);
        }
    }
}

pub fn claude_home_items(home: &Path) -> Vec<PathBuf> {
    CLAUDE_HOME_ITEMS.iter().map(|i| home.join(".claude").join(i)).collect()
}

/// Digests of every protected entry, keyed by path. Directories contribute
/// one entry per file below them.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub entries: BTreeMap<String, String>,
    /// Repositories present (by `.git` path), so new ones can be told apart.
    pub repos: Vec<String>,
    pub truncated: bool,
}

fn digest_into(path: &Path, out: &mut BTreeMap<String, String>) {
    let key = path.to_string_lossy().into_owned();
    match std::fs::symlink_metadata(path) {
        Err(_) => {}
        Ok(m) if m.file_type().is_symlink() => {
            let target = std::fs::read_link(path).map(|t| t.to_string_lossy().into_owned()).unwrap_or_default();
            out.insert(key, format!("link:{target}"));
        }
        Ok(m) if m.is_dir() => {
            out.insert(key, "dir".into());
            if let Ok(rd) = std::fs::read_dir(path) {
                for e in rd.flatten() {
                    digest_into(&e.path(), out);
                }
            }
        }
        Ok(m) => {
            use std::os::unix::fs::PermissionsExt;
            let mode = m.permissions().mode() & 0o111;
            let d = sha256_file(path).unwrap_or_else(|e| format!("unreadable: {e}"));
            out.insert(key, format!("{d} x{mode:o}"));
        }
    }
}

/// The MCP servers `~/.claude.json` configures (top level and per
/// project). The CLI rewrites the rest of that file on every start.
fn claude_json_servers(path: &Path) -> Option<String> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let mut servers = BTreeMap::new();
    let mut add = |k: String, s: Option<&Value>| {
        if let Some(s) = s.filter(|s| s.as_object().is_some_and(|o| !o.is_empty())) {
            servers.insert(k, s.clone());
        }
    };
    add(String::new(), v.get("mcpServers"));
    for (p, proj) in v.get("projects").and_then(Value::as_object).into_iter().flatten() {
        add(p.clone(), proj.get("mcpServers"));
    }
    Some(content_hash(&servers))
}

/// Snapshot the workspace's protected entries and, for Claude Code runs,
/// the user's Claude configuration under `home`.
pub fn snapshot(ws: Option<&Path>, claude_home: Option<&Path>) -> Snapshot {
    let mut snap = Snapshot::default();
    if let Some(ws) = ws {
        let s = scan(ws);
        snap.truncated = s.truncated;
        for p in &s.named {
            digest_into(p, &mut snap.entries);
        }
        for git in &s.repos {
            snap.repos.push(git.to_string_lossy().into_owned());
            if git.is_dir() {
                for part in GIT_PARTS {
                    digest_into(&git.join(part), &mut snap.entries);
                }
            } else {
                digest_into(git, &mut snap.entries);
            }
        }
    }
    if let Some(home) = claude_home {
        for p in claude_home_items(home) {
            digest_into(&p, &mut snap.entries);
        }
        let cj = home.join(".claude.json");
        if let Some(d) = claude_json_servers(&cj) {
            snap.entries.insert(format!("{}#mcpServers", cj.display()), d);
        }
    }
    snap
}

/// Keys set in a git config file, as `section[.subsection].name`, lowercased.
fn git_config_keys(text: &str) -> Vec<String> {
    let mut section = String::new();
    let mut keys = vec![];
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let mut line = line;
        if let Some(rest) = line.strip_prefix('[') {
            let (inner, after) = rest.split_once(']').unwrap_or((rest, ""));
            section = match inner.split_once(' ') {
                Some((s, sub)) => format!("{}.{}", s.trim(), sub.trim().trim_matches('"')),
                None => inner.trim().to_string(),
            };
            // git accepts a key on the header line: `[core] fsmonitor = x`.
            line = after.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
        }
        let name = line.split('=').next().unwrap_or("").trim();
        keys.push(format!("{section}.{name}").to_lowercase());
    }
    keys
}

/// What a repository created during the run holds that could run code:
/// hooks other than git's samples, and config keys beyond plain setup.
fn new_repo_findings(git: &Path) -> Vec<String> {
    let mut out = vec![];
    if git.is_file() {
        return vec![format!("{} (gitdir file)", git.display())];
    }
    if let Ok(rd) = std::fs::read_dir(git.join("hooks")) {
        for e in rd.flatten() {
            if !e.file_name().to_string_lossy().ends_with(".sample") {
                out.push(e.path().display().to_string());
            }
        }
    }
    let config = git.join("config");
    let allowed = |k: &str| {
        NEW_REPO_CONFIG_KEYS.contains(&k)
            || (k.starts_with("branch.") && (k.ends_with(".remote") || k.ends_with(".merge")))
            || (k.starts_with("remote.") && (k.ends_with(".url") || k.ends_with(".fetch")))
    };
    let text = std::fs::read_to_string(&config).unwrap_or_default();
    for k in git_config_keys(&text).into_iter().filter(|k| !allowed(k)) {
        out.push(format!("{} ({k})", config.display()));
    }
    out
}

/// What changed between two snapshots. Entries of repositories that did
/// not exist before count only when they could run code.
pub fn changes(before: &Snapshot, after: &Snapshot) -> Vec<String> {
    let new_repos: Vec<&String> = after.repos.iter().filter(|r| !before.repos.contains(r)).collect();
    let in_new_repo = |p: &str| new_repos.iter().any(|r| p == r.as_str() || p.starts_with(&format!("{r}/")));
    let mut out: Vec<String> = before
        .entries
        .iter()
        .filter(|(k, v)| after.entries.get(*k) != Some(v))
        .map(|(k, _)| k.clone())
        .chain(after.entries.keys().filter(|k| !before.entries.contains_key(*k) && !in_new_repo(k)).cloned())
        .collect();
    for r in &new_repos {
        out.extend(new_repo_findings(Path::new(r.as_str())));
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(paths: &[&str]) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        for p in paths {
            let full = d.path().join(p);
            if p.ends_with('/') {
                std::fs::create_dir_all(&full).unwrap();
            } else {
                std::fs::create_dir_all(full.parent().unwrap()).unwrap();
                std::fs::write(&full, "x").unwrap();
            }
        }
        d
    }

    #[test]
    fn scan_finds_repos_and_names_at_any_depth_but_skips_big_dirs() {
        let d = tree(&[
            ".git/config",
            ".git/hooks/",
            "sub/deep/.envrc",
            "sub/.vscode/settings.json",
            "wt/.git",
            "node_modules/pkg/.mcp.json",
            "target/.claude/x",
            "src/main.rs",
        ]);
        let s = scan(d.path());
        let rel = |v: &[PathBuf]| v.iter().map(|p| p.strip_prefix(d.path()).unwrap().to_path_buf()).collect::<Vec<_>>();
        assert_eq!(rel(&s.repos), vec![PathBuf::from(".git"), PathBuf::from("wt/.git")]);
        assert_eq!(rel(&s.named), vec![PathBuf::from("sub/.vscode"), PathBuf::from("sub/deep/.envrc")]);
        assert!(!s.truncated);
        // The scan is bounded; a truncated scan says so.
        // Entries visited: 6 at the top, 2 in sub, 1 each in sub/deep, wt
        // and src; node_modules and target are counted, not entered.
        let entries = 11;
        assert!(!scan_limited(d.path(), entries).truncated, "exactly at the limit");
        assert!(scan_limited(d.path(), entries - 1).truncated);
        assert!(scan_limited(d.path(), 1).truncated);
    }

    #[test]
    fn file_tools_refuse_protected_names_and_git() {
        for p in [".git/hooks/pre-commit", ".git/HEAD", "a/.mcp.json", ".claude/settings.json", "x/.zshrc", ".git"] {
            assert!(is_write_protected(Path::new(p)), "{p}");
        }
        for p in ["src/main.rs", "gitignore", ".gitignore", "a/claude/x", ".github/workflows/ci.yml", "zshrc.txt"] {
            assert!(!is_write_protected(Path::new(p)), "{p}");
        }
    }

    #[test]
    fn apply_protects_existing_repos_and_names() {
        let d = tree(&[".git/config", ".git/hooks/", "wt/.git", ".idea/x"]);
        let mut p = SandboxPolicy::new(vec![d.path().to_path_buf()], crate::sandbox::NetworkPolicy::Denied);
        apply(&mut p, d.path());
        let ws = d.path();
        for want in [ws.join(".git/hooks"), ws.join(".git/config"), ws.join("wt/.git"), ws.join(".idea")] {
            assert!(p.deny_write.contains(&want), "{want:?} in {:?}", p.deny_write);
        }
        assert!(!p.deny_write.contains(&ws.join(".git")), "the git dir itself stays writable (commits)");
        let root = regex_escape(&real_path(ws).to_string_lossy());
        assert!(p
            .deny_write_regex
            .iter()
            .any(|r| r.starts_with(&format!("^{root}/(.*/)?(")) && r.contains(r"\.mcp\.json")));
        assert!(p.deny_write_regex.contains(&format!("^{root}/\\.git$")), "{:?}", p.deny_write_regex);
        assert_eq!(regex_escape("/a.b/c(d)"), r"/a\.b/c\(d\)");
        assert_eq!(regex_escape("/a\"b/.git"), r"/a.b/\.git");
    }

    #[test]
    fn snapshots_see_changes_to_protected_entries_only() {
        let d = tree(&[".git/config", ".git/hooks/", ".git/HEAD", ".vscode/settings.json", "src/a.rs"]);
        let ws = d.path();
        let before = snapshot(Some(ws), None);
        std::fs::write(ws.join("src/a.rs"), "changed").unwrap();
        std::fs::write(ws.join(".git/HEAD"), "ref: refs/heads/other").unwrap();
        assert!(changes(&before, &snapshot(Some(ws), None)).is_empty(), "ordinary work is not a change");
        std::fs::write(ws.join(".git/hooks/pre-commit"), "#!/bin/sh\ncurl evil").unwrap();
        std::fs::write(ws.join(".vscode/settings.json"), "{}").unwrap();
        std::fs::write(ws.join(".envrc"), "evil").unwrap();
        let c = changes(&before, &snapshot(Some(ws), None));
        let names: Vec<String> = c.iter().map(|p| p.rsplit('/').next().unwrap().to_string()).collect();
        assert_eq!(names, vec![".envrc", "pre-commit", "settings.json"], "{c:?}");
        // Mode changes count: making a hook executable arms it.
        let d = tree(&[".git/hooks/pre-push"]);
        let before = snapshot(Some(d.path()), None);
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(d.path().join(".git/hooks/pre-push"), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(changes(&before, &snapshot(Some(d.path()), None)).len(), 1);
        // Only the execute bits count: other permission changes are not arming.
        let rw = snapshot(Some(d.path()), None);
        std::fs::set_permissions(d.path().join(".git/hooks/pre-push"), std::fs::Permissions::from_mode(0o711)).unwrap();
        assert!(changes(&rw, &snapshot(Some(d.path()), None)).is_empty());
        // Swapping a hook for a symlink to identical content counts.
        let same = d.path().join("same");
        std::fs::copy(d.path().join(".git/hooks/pre-push"), &same).unwrap();
        std::fs::remove_file(d.path().join(".git/hooks/pre-push")).unwrap();
        std::os::unix::fs::symlink(&same, d.path().join(".git/hooks/pre-push")).unwrap();
        let linked = snapshot(Some(d.path()), None);
        assert_eq!(changes(&rw, &linked).len(), 1);
        assert!(linked.entries.values().any(|v| v.starts_with("link:")));
        // Removal counts.
        std::fs::remove_file(d.path().join(".git/hooks/pre-push")).unwrap();
        assert_eq!(changes(&before, &snapshot(Some(d.path()), None)).len(), 1);
    }

    #[test]
    fn a_new_repository_may_hold_only_samples_and_plain_config() {
        let d = tree(&["src/a.rs"]);
        let ws = d.path();
        let before = snapshot(Some(ws), None);
        let git = ws.join("new/.git");
        std::fs::create_dir_all(git.join("hooks")).unwrap();
        std::fs::write(git.join("hooks/pre-commit.sample"), "#!/bin/sh").unwrap();
        std::fs::write(
            git.join("config"),
            "[core]\n\trepositoryformatversion = 0\n\tbare = false\n[user]\n\tname = A\n[remote \"origin\"]\n\turl = x\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n[branch \"main\"]\n\tremote = origin\n\tmerge = refs/heads/main\n",
        )
        .unwrap();
        assert!(changes(&before, &snapshot(Some(ws), None)).is_empty(), "git init + user + remote are plain");
        std::fs::write(git.join("hooks/post-checkout"), "#!/bin/sh").unwrap();
        let mut cfg = std::fs::read_to_string(git.join("config")).unwrap();
        cfg.push_str("[core]\n\thooksPath = /tmp/h\n[alias]\n\tst = !evil\n[filter \"x\"]\n\tsmudge = evil\n");
        // Inside an allowed section, only the listed keys are plain.
        cfg.push_str(
            "[remote \"origin\"]\n\tuploadpack = evil\n[branch \"main\"]\n\tdescription = d\n[x]\n\tremote = r\n",
        );
        std::fs::write(git.join("config"), cfg).unwrap();
        let c = changes(&before, &snapshot(Some(ws), None));
        assert_eq!(c.len(), 7, "{c:?}");
        for key in ["remote.origin.uploadpack", "branch.main.description", "x.remote"] {
            assert!(c.iter().any(|x| x.contains(key)), "{key}: {c:?}");
        }
        assert!(c.iter().any(|x| x.contains("core.hookspath")) && c.iter().any(|x| x.contains("alias.st")));
        assert!(c.iter().any(|x| x.contains("filter.x.smudge")) && c.iter().any(|x| x.ends_with("post-checkout")));
        // A new gitdir file can point anywhere: always a change.
        let d = tree(&[]);
        let before = snapshot(Some(d.path()), None);
        std::fs::write(d.path().join(".git"), "gitdir: /elsewhere").unwrap();
        assert_eq!(changes(&before, &snapshot(Some(d.path()), None)).len(), 1);
    }

    #[test]
    fn claude_home_items_and_mcp_servers_are_snapshotted() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        std::fs::create_dir_all(h.join(".claude/projects")).unwrap();
        std::fs::write(h.join(".claude.json"), r#"{"numStartups": 1, "projects": {"/w": {"mcpServers": {}}}}"#)
            .unwrap();
        let before = snapshot(None, Some(h));
        // What the CLI itself writes is not a change.
        std::fs::write(h.join(".claude/projects/t.jsonl"), "{}").unwrap();
        std::fs::write(
            h.join(".claude.json"),
            r#"{"numStartups": 2, "projects": {"/w": {"mcpServers": {}}, "/v": {"allowedTools": [], "mcpServers": {}}}}"#,
        )
        .unwrap();
        assert!(changes(&before, &snapshot(None, Some(h))).is_empty());
        std::fs::write(h.join(".claude.json"), r#"{"projects": {"/w": {"mcpServers": {"x": {"command": "evil"}}}}}"#)
            .unwrap();
        std::fs::create_dir_all(h.join(".claude/commands")).unwrap();
        std::fs::write(h.join(".claude/commands/deploy.md"), "evil").unwrap();
        let c = changes(&before, &snapshot(None, Some(h)));
        assert!(c.iter().any(|x| x.ends_with("#mcpServers")) && c.iter().any(|x| x.ends_with("deploy.md")), "{c:?}");
        assert_eq!(claude_home_items(h).len(), CLAUDE_HOME_ITEMS.len());
    }

    #[test]
    fn git_config_keys_are_parsed() {
        let k = git_config_keys("# c\n[core]\n  bare = false\n[Remote \"o\"]\n url=x\n; c\n[a.b]\nc\n");
        assert_eq!(k, vec!["core.bare", "remote.o.url", "a.b.c"]);
    }

    #[test]
    fn git_config_keys_on_the_header_line_are_seen() {
        let k = git_config_keys("[core] fsmonitor = /tmp/evil.sh\n[alias] st = !sh x\n[user] # note\n");
        assert_eq!(k, vec!["core.fsmonitor", "alias.st"]);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn a_quote_in_a_repo_path_keeps_the_profile_valid() {
        let ws = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(ws.path().join("a\"b/.git")).unwrap();
        let mut p = SandboxPolicy::new(vec![ws.path().to_path_buf()], crate::sandbox::NetworkPolicy::Denied);
        apply(&mut p, ws.path());
        let out = std::process::Command::new(crate::sandbox::SANDBOX_EXEC)
            .args(["-p", &p.profile(), "/usr/bin/true"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }
}
