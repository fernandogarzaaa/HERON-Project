//! Paths that run code later, outside the sandbox, when a human uses the workspace or their own
//! machine: git hooks and config, agent and editor config, shell rc files, launch agents and Heron's
//! own settings. File tools and sandboxed commands may read them but not write them.
//!
//! - macOS matches the workspace names at any depth by pattern, so new files are refused too. Git
//!   config is protected for the repositories that exist when the command starts; hooks are denied at
//!   any depth, so a repository the command makes gets no hooks either.
//! - File tools refuse every protected name and anything under `.git`.
//! - A snapshot taken before a run and compared after it catches what the sandbox could not prevent.
//!   A repository made during the run may hold only sample hooks and plain configuration (see
//!   [`NEW_REPO_CONFIG_KEYS`]).

pub mod scan;

pub use scan::{scan, Repo, Scan, SCAN_LIMIT};

use crate::sandbox::{data_volume_alias, literals, seatbelt_regex, subpaths};
use serde_json::Value;
use sha2::{Digest, Sha256};
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

/// The names above that are folders, so everything under them is protected too.
const DIRECTORY_NAMES: &[&str] = &[".husky", ".claude", ".vscode", ".idea"];

/// Parts of a repository's git directory that are protected.
pub const GIT_PARTS: &[&str] = &["hooks", "config"];

/// Protected paths under the user's home folder.
pub const HOME_PATHS: &[&str] = &[
    ".bashrc",
    ".bash_profile",
    ".zshrc",
    ".zprofile",
    ".zshenv",
    ".zlogin",
    ".profile",
    "Library/LaunchAgents",
    ".gitconfig",
    ".config/git",
    ".cargo/config.toml",
];

/// The folders among [`HOME_PATHS`].
const HOME_DIRECTORIES: &[&str] = &["Library/LaunchAgents", ".config/git"];

/// Heron's files, in each Heron home folder. The folder itself stays writable.
pub const HERON_FILES: &[&str] = &["config.toml", "heron.db", "heron.db-wal", "heron.db-shm", "secrets.json"];

/// Claude Code's user configuration under `~/.claude` that runs or steers later sessions.
pub const CLAUDE_HOME_ITEMS: &[&str] =
    &["settings.json", "settings.local.json", "CLAUDE.md", "commands", "agents", "skills", "hooks", "plugins"];

/// Directories the scan does not descend into (large, and never config).
const SKIP_DIRS: &[&str] = &["node_modules", "target", ".venv", "venv", "__pycache__"];

/// Keys `git init` and ordinary use write to a new repository's config. Anything else (core.hooksPath,
/// filters, aliases, includes, helpers) can run code and counts as a change.
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

const LABEL_HOOK: &str = "a git hook";
const LABEL_GIT_CONFIG: &str = "a repository's git config";
const LABEL_GIT_INTERNALS: &str = "a git repository's internals";
const LABEL_SHELL: &str = "a shell startup file";
const LABEL_LAUNCH_AGENT: &str = "a launch agent";
const LABEL_EDITOR: &str = "an editor or agent settings folder";
const LABEL_SETTINGS: &str = "a git or cargo settings file";
const LABEL_HERON: &str = "Heron's own settings or data";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub path: PathBuf,
    pub label: &'static str,
}

impl Refusal {
    pub fn message(&self) -> String {
        format!(
            "Refused: {} is a protected file ({}, which runs code later). Heron's file tools can't change it in any mode.",
            self.path.display(),
            self.label
        )
    }
}

fn names(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|c| match c {
            Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

/// `.` and `..` resolved without touching the disk.
pub(crate) fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            c => out.push(c),
        }
    }
    out
}

/// The path with the deepest existing ancestor resolved through symlinks.
fn real_path(path: &Path) -> PathBuf {
    let mut tail = vec![];
    let mut cur = path;
    loop {
        if let Ok(mut real) = dunce::canonicalize(cur) {
            real.extend(tail.iter().rev());
            return real;
        }
        match (cur.parent(), cur.file_name()) {
            (Some(parent), Some(name)) => {
                tail.push(name.to_owned());
                cur = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// A path as written, through its symlinks, and under the other macOS name of each.
fn forms(path: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    for base in [lexical(path), real_path(&lexical(path))] {
        for p in [data_volume_alias(&base), Some(base)].into_iter().flatten() {
            if !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

fn relative_to(path_forms: &[PathBuf], base: &Path) -> Option<PathBuf> {
    forms(base).iter().find_map(|b| path_forms.iter().find_map(|p| p.strip_prefix(b).ok().map(Path::to_path_buf)))
}

fn is_name(component: &str, list: &[&str]) -> bool {
    list.iter().any(|n| n.eq_ignore_ascii_case(component))
}

/// Whether a path has a `.git` component. Case does not matter: the file systems on macOS and Windows
/// do not tell `.GIT` from `.git`.
pub fn has_git_component(path: &Path) -> bool {
    names(path).iter().any(|n| n.eq_ignore_ascii_case(".git"))
}

fn is_worktrees_at(components: &[String], i: usize) -> bool {
    components[i].eq_ignore_ascii_case(".claude") && components.get(i + 1).is_some_and(|c| c.eq_ignore_ascii_case("worktrees"))
}

fn git_label(after_git: &[String]) -> &'static str {
    if after_git.iter().any(|c| c.eq_ignore_ascii_case("hooks")) {
        LABEL_HOOK
    } else if after_git.last().is_some_and(|c| c.eq_ignore_ascii_case("config") || c.eq_ignore_ascii_case("config.worktree")) {
        LABEL_GIT_CONFIG
    } else {
        LABEL_GIT_INTERNALS
    }
}

fn name_label(name: &str) -> &'static str {
    if is_name(name, &[".bashrc", ".bash_profile", ".zshrc", ".zprofile", ".zshenv", ".profile"]) {
        LABEL_SHELL
    } else if name.eq_ignore_ascii_case(".gitmodules") {
        LABEL_SETTINGS
    } else if name.eq_ignore_ascii_case(".husky") {
        LABEL_HOOK
    } else {
        LABEL_EDITOR
    }
}

/// The label of the first protected component of a path, or `Err` when `.claude/worktrees` comes first,
/// which is allowed. `names` is false outside the working folder, where only `.git` counts.
fn component_label(components: &[String], names: bool) -> Result<Option<&'static str>, ()> {
    for (i, c) in components.iter().enumerate() {
        if c.eq_ignore_ascii_case(".git") {
            return Ok(Some(git_label(&components[i + 1..])));
        }
        if is_worktrees_at(components, i) {
            return Err(());
        }
        if names && is_name(c, WORKSPACE_NAMES) {
            return Ok(Some(name_label(c)));
        }
    }
    Ok(None)
}

fn home_label(rel: &Path) -> Option<&'static str> {
    let item = HOME_PATHS.iter().find(|item| {
        let item = Path::new(item);
        let lower = |p: &Path| names(p).iter().map(|n| n.to_ascii_lowercase()).collect::<Vec<_>>();
        let (rel, item_names) = (lower(rel), lower(item));
        if HOME_DIRECTORIES.iter().any(|d| Path::new(d) == item) {
            rel.starts_with(&item_names)
        } else {
            rel == item_names
        }
    })?;
    Some(if item.starts_with("Library/") {
        LABEL_LAUNCH_AGENT
    } else if item.contains("git") || item.contains("cargo") {
        LABEL_SETTINGS
    } else {
        LABEL_SHELL
    })
}

fn heron_files(heron_homes: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = vec![];
    for file in heron_homes.iter().flat_map(|h| HERON_FILES.iter().map(move |f| h.join(f))) {
        if !out.contains(&file) {
            out.push(file);
        }
    }
    out
}

fn label_of_form(form: &Path, workspace: &Path, user_home: &Path, heron_homes: &[PathBuf]) -> Option<&'static str> {
    let as_forms = [form.to_path_buf()];
    let in_workspace = relative_to(&as_forms, workspace);
    if let Some(rel) = &in_workspace {
        match component_label(&names(rel), true) {
            Err(()) => return None,
            Ok(Some(label)) => return Some(label),
            Ok(None) => {}
        }
    }
    if let Some(rel) = relative_to(&as_forms, user_home) {
        if let Some(label) = home_label(&rel) {
            return Some(label);
        }
    }
    let lower = form.to_string_lossy().to_ascii_lowercase();
    let is_heron_file = heron_files(heron_homes).iter().flat_map(|f| forms(f)).any(|f| f.to_string_lossy().to_ascii_lowercase() == lower);
    if is_heron_file {
        return Some(LABEL_HERON);
    }
    if in_workspace.is_none() {
        return component_label(&names(form), false).unwrap_or(None);
    }
    None
}

/// Why a file tool may not write `path`, or `None` when it may. Every form of the path is checked: as
/// written, through its symlinks, and under the other macOS name of each. A relative path is taken
/// relative to the working folder.
pub fn refusal_for(path: &Path, workspace: &Path, user_home: &Path, heron_homes: &[PathBuf]) -> Option<Refusal> {
    let absolute = if path.is_absolute() { path.to_path_buf() } else { workspace.join(path) };
    let label = forms(&absolute).iter().find_map(|form| label_of_form(form, workspace, user_home, heron_homes))?;
    Some(Refusal { path: path.to_path_buf(), label })
}

pub fn is_protected(path: &Path, workspace: &Path, user_home: &Path, heron_homes: &[PathBuf]) -> bool {
    refusal_for(path, workspace, user_home, heron_homes).is_some()
}

/// Escape a path for an SBPL deny regex. A `"` cannot be escaped inside an SBPL `#"..."` literal, so it
/// becomes `.`: the deny only widens.
pub fn regex_escape(s: &str) -> String {
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

fn inside_any(path: &Path, writable: &[PathBuf]) -> bool {
    let writable: Vec<PathBuf> = writable.iter().flat_map(|w| forms(w)).collect();
    forms(path).iter().any(|p| writable.iter().any(|w| p.starts_with(w)))
}

fn repo_paths(repo: &Repo) -> Vec<PathBuf> {
    let mut out = vec![repo.entry.clone()];
    out.extend(repo.parts());
    out.extend(repo.worktree_admin());
    out
}

/// The Seatbelt rules that stop a command writing protected paths, and the paths to pass to
/// `refused_ancestors` so a parent folder can't be renamed out from under them. Seatbelt applies the
/// last rule that matches: the names are denied, `.claude/worktrees` is allowed back, hooks are denied
/// again at any depth with only git's samples allowed, then the literals follow.
pub fn seatbelt_rules(workspace: &Path, user_home: &Path, heron_homes: &[PathBuf], scan: &Scan, writable: &[PathBuf]) -> (String, Vec<PathBuf>) {
    let roots: Vec<String> = forms(workspace).iter().map(|f| regex_escape(&f.to_string_lossy())).collect();
    let regexes = |tails: &[String]| -> String {
        roots.iter().flat_map(|root| tails.iter().map(move |t| format!(" (regex #\"{}\")", seatbelt_regex(&format!("^{root}/{t}"))))).collect()
    };
    let name_tails: Vec<String> = WORKSPACE_NAMES
        .iter()
        .map(|n| {
            let tail = if DIRECTORY_NAMES.contains(n) { "(/.*)?$" } else { "$" };
            format!("(.*/)?{}{tail}", regex_escape(n))
        })
        .collect();
    let mut rules = format!("(deny file-write*{})\n", regexes(&name_tails));
    rules.push_str(&format!("(allow file-write*{})\n", regexes(&["(.*/)?\\.claude/worktrees(/.*)?$".to_string()])));
    let under_protected_folder: Vec<String> =
        DIRECTORY_NAMES.iter().map(|n| format!("(.*/)?{}/(.*/)?\\.claude/worktrees(/.*)?$", regex_escape(n))).collect();
    rules.push_str(&format!("(deny file-write*{})\n", regexes(&under_protected_folder)));
    rules.push_str(&format!("(deny file-write*{})\n", regexes(&["(.*/)?\\.git/(.*/)?hooks/.+$".to_string()])));
    rules.push_str(&format!("(allow file-write*{})\n", regexes(&["(.*/)?\\.git/(.*/)?hooks/[^/]+\\.sample$".to_string()])));
    // The files that tell git where a repository's hooks and config are, for it and for each linked
    // worktree. `git worktree add` writes them too, so it can't run in a sandboxed command.
    rules.push_str(&format!("(deny file-write*{})\n", regexes(&["(.*/)?\\.git/(.*/)?(commondir|gitdir|config\\.worktree)$".to_string()])));

    let mut refused: Vec<PathBuf> = vec![];
    for repo in &scan.repos {
        refused.extend(repo_paths(repo));
    }
    let mut literal_paths = refused.clone();

    // The rule above covers hooks inside a `.git` folder in the workspace. A `commondir` can name hooks
    // anywhere else, and those are denied whole.
    let in_reach_of_the_rule = |h: &PathBuf| has_git_component(h) && inside_any(h, &[workspace.to_path_buf()]);
    let mut whole_folders: Vec<PathBuf> = scan.repos.iter().map(|r| r.common_dir.join("hooks")).filter(|h| !in_reach_of_the_rule(h)).collect();
    for item in HOME_PATHS {
        let path = user_home.join(item);
        if !inside_any(&path, writable) {
            continue;
        }
        if HOME_DIRECTORIES.contains(item) {
            whole_folders.push(path.clone());
        } else {
            literal_paths.push(path.clone());
        }
        refused.push(path);
    }
    for file in heron_files(heron_homes) {
        if inside_any(&file, writable) {
            literal_paths.push(file.clone());
            refused.push(file);
        }
    }
    if !literal_paths.is_empty() {
        rules.push_str(&format!("(deny file-write*{})\n", literals(&as_written_and_real(literal_paths))));
    }
    if !whole_folders.is_empty() {
        rules.push_str(&format!("(deny file-write*{})\n", subpaths(&as_written_and_real(whole_folders))));
    }
    (rules, as_written_and_real(refused))
}

/// Each path as written and with its deepest existing ancestor resolved. `literals` and `subpaths`
/// resolve only paths that exist, and Seatbelt sees `/private/tmp/x`, never `/tmp/x`.
fn as_written_and_real(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = vec![];
    for p in paths {
        for form in [real_path(&p), p] {
            if !out.contains(&form) {
                out.push(form);
            }
        }
    }
    out
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChangeKind {
    #[default]
    Changed,
    New,
    Deleted,
    NewRepository,
    ScanTruncated,
}

impl ChangeKind {
    fn text(self) -> &'static str {
        match self {
            ChangeKind::Changed => "changed",
            ChangeKind::New => "new",
            ChangeKind::Deleted => "deleted",
            ChangeKind::NewRepository => "new repository",
            ChangeKind::ScanTruncated => "scan hit its limit",
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ProtectedChange {
    pub path: PathBuf,
    pub kind: ChangeKind,
}

/// Digests of every protected entry, keyed by path. Directories contribute one entry per file below
/// them.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Snapshot {
    entries: BTreeMap<String, String>,
    /// Repositories present (by `.git` path), so new ones can be told apart.
    repos: Vec<String>,
    /// What each repository holds that could run code, if it turns out to be new.
    findings: BTreeMap<String, Vec<PathBuf>>,
    workspace: PathBuf,
    truncated: bool,
}

/// A protected name found by the scan. The scan never enters a protected folder, so the name is the
/// first protected component of its path, and `.claude/worktrees` directly under a `.claude` is the one
/// place the carve-out applies.
fn digest_named(path: &Path, out: &mut BTreeMap<String, String>) {
    let is_claude_dir = path.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(".claude"))
        && std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir());
    if !is_claude_dir {
        return digest_into(path, out);
    }
    out.insert(path.to_string_lossy().into_owned(), "dir".into());
    for e in std::fs::read_dir(path).into_iter().flatten().flatten() {
        if !e.file_name().to_string_lossy().eq_ignore_ascii_case("worktrees") {
            digest_into(&e.path(), out);
        }
    }
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    std::io::copy(&mut std::fs::File::open(path)?, &mut hasher)?;
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
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
            let digest = sha256_file(path).unwrap_or_else(|e| format!("unreadable: {e}"));
            #[cfg(unix)]
            let digest = {
                use std::os::unix::fs::PermissionsExt;
                format!("{digest} x{:o}", m.permissions().mode() & 0o111)
            };
            #[cfg(not(unix))]
            let _ = m;
            out.insert(key, digest);
        }
    }
}

/// The MCP servers `~/.claude.json` configures (top level and per project). The CLI rewrites the rest
/// of that file on every start.
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
    let text = serde_json::to_vec(&servers).ok()?;
    Some(Sha256::digest(text).iter().map(|b| format!("{b:02x}")).collect())
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

/// What a repository could run later: hooks other than git's samples, and config keys beyond plain
/// setup. A `.git` file can point anywhere, so it always counts.
fn repo_findings(repo: &Repo) -> Vec<PathBuf> {
    if !repo.entry.is_dir() {
        return vec![repo.entry.clone()];
    }
    // A redirect sends git to hooks and config this function does not read.
    let mut out: Vec<PathBuf> = ["commondir", "gitdir"].iter().map(|f| repo.entry.join(f)).filter(|f| f.exists()).collect();
    if let Ok(rd) = std::fs::read_dir(repo.entry.join("hooks")) {
        for e in rd.flatten() {
            if !e.file_name().to_string_lossy().ends_with(".sample") {
                out.push(e.path());
            }
        }
    }
    let allowed = |k: &str| {
        NEW_REPO_CONFIG_KEYS.contains(&k)
            || (k.starts_with("branch.") && (k.ends_with(".remote") || k.ends_with(".merge")))
            || (k.starts_with("remote.") && (k.ends_with(".url") || k.ends_with(".fetch")))
    };
    let config = repo.entry.join("config");
    let text = std::fs::read_to_string(&config).unwrap_or_default();
    if git_config_keys(&text).iter().any(|k| !allowed(k)) {
        out.push(config);
    }
    out
}

impl Snapshot {
    /// Digest every protected entry: the working folder's names and repositories, the user's protected
    /// home paths, and Heron's settings. `heron.db*`, the journal's lease files and `background.lock`
    /// are left out: Heron writes them on every call.
    pub fn take(workspace: &Path, user_home: &Path, heron_homes: &[PathBuf]) -> Snapshot {
        Snapshot::take_limited(workspace, user_home, heron_homes, SCAN_LIMIT)
    }

    fn take_limited(workspace: &Path, user_home: &Path, heron_homes: &[PathBuf], limit: usize) -> Snapshot {
        let mut snap = Snapshot { workspace: workspace.to_path_buf(), ..Snapshot::default() };
        let s = scan::scan_limited(workspace, limit);
        snap.truncated = s.truncated;
        for p in &s.named {
            digest_named(p, &mut snap.entries);
        }
        for repo in &s.repos {
            let key = repo.entry.to_string_lossy().into_owned();
            snap.repos.push(key.clone());
            snap.findings.insert(key, repo_findings(repo));
            if !repo.entry.is_dir() {
                digest_into(&repo.entry, &mut snap.entries);
            }
            for part in repo.parts().into_iter().chain(repo.worktree_admin()) {
                digest_into(&part, &mut snap.entries);
            }
        }
        for item in HOME_PATHS {
            digest_into(&user_home.join(item), &mut snap.entries);
        }
        for file in heron_files(heron_homes).iter().filter(|f| !f.file_name().is_some_and(|n| n.to_string_lossy().starts_with("heron.db"))) {
            digest_into(file, &mut snap.entries);
        }
        snap
    }

    /// Digest the user's Claude Code configuration under `home`, for runs of that CLI.
    pub fn take_claude_home(home: &Path) -> Snapshot {
        let mut snap = Snapshot::default();
        for item in CLAUDE_HOME_ITEMS {
            digest_into(&home.join(".claude").join(item), &mut snap.entries);
        }
        let json = home.join(".claude.json");
        if let Some(d) = claude_json_servers(&json) {
            snap.entries.insert(format!("{}#mcpServers", json.display()), d);
        }
        snap
    }

    /// What changed since `self`. Entries of a repository that did not exist before count only when
    /// they could run code. Paths only, never contents.
    pub fn diff(&self, later: &Snapshot) -> Vec<ProtectedChange> {
        let new_repos: Vec<&String> = later.repos.iter().filter(|r| !self.repos.contains(r)).collect();
        let in_new_repo = |p: &str| new_repos.iter().any(|r| Path::new(p).starts_with(r.as_str()));
        let change = |p: &str, kind| ProtectedChange { path: PathBuf::from(p), kind };
        let mut out: Vec<ProtectedChange> = vec![];
        for (k, v) in &self.entries {
            match later.entries.get(k) {
                None => out.push(change(k, ChangeKind::Deleted)),
                Some(now) if now != v => out.push(change(k, ChangeKind::Changed)),
                Some(_) => {}
            }
        }
        for k in later.entries.keys().filter(|k| !self.entries.contains_key(*k) && !in_new_repo(k)) {
            out.push(change(k, ChangeKind::New));
        }
        for r in &new_repos {
            for path in later.findings.get(*r).into_iter().flatten() {
                out.push(ProtectedChange { path: path.clone(), kind: ChangeKind::NewRepository });
            }
        }
        if self.truncated || later.truncated {
            let workspace = if later.truncated { &later.workspace } else { &self.workspace };
            out.push(ProtectedChange { path: workspace.clone(), kind: ChangeKind::ScanTruncated });
        }
        out.sort_by(|a, b| a.path.cmp(&b.path).then(a.kind.cmp(&b.kind)));
        out.dedup();
        out
    }
}

const REPORT_LIMIT: usize = 20;

/// The notice for a run that changed protected paths. It names paths, never contents.
pub fn report_text(changes: &[ProtectedChange]) -> String {
    let mut listed: Vec<String> = changes.iter().take(REPORT_LIMIT).map(|c| format!("{} ({})", c.path.display(), c.kind.text())).collect();
    if changes.len() > REPORT_LIMIT {
        listed.push(format!("and {} more", changes.len() - REPORT_LIMIT));
    }
    format!(
        "Protected files changed while Heron was working: {}. If you didn't change them, check them before you next use this folder.",
        listed.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn tree(paths: &[&str]) -> tempfile::TempDir {
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

    fn snap(ws: &Path) -> Snapshot {
        Snapshot::take(ws, Path::new("/nonexistent-home"), &[])
    }

    fn ws_paths(ws: &Path, changes: &[ProtectedChange]) -> Vec<String> {
        changes.iter().map(|c| format!("{}:{:?}", c.path.strip_prefix(ws).unwrap().display().to_string().replace('\\', "/"), c.kind)).collect()
    }

    fn rules_for(ws: &Path) -> (String, Vec<PathBuf>) {
        seatbelt_rules(ws, Path::new("/nonexistent-home"), &[], &scan(ws), &[ws.to_path_buf()])
    }

    #[test]
    fn file_tools_refuse_protected_names_and_git() {
        let ws = Path::new("/work/proj");
        let home = Path::new("/home/u");
        for p in [".git/hooks/pre-commit", ".git/HEAD", "a/.mcp.json", ".claude/settings.json", "x/.zshrc", ".git"] {
            assert!(is_protected(&ws.join(p), ws, home, &[]), "{p}");
        }
        for p in ["src/main.rs", "gitignore", ".gitignore", "a/claude/x", ".github/workflows/ci.yml", "zshrc.txt"] {
            assert!(!is_protected(&ws.join(p), ws, home, &[]), "{p}");
        }
    }

    #[test]
    fn protected_names_match_at_any_depth() {
        let ws = Path::new("/work/proj");
        let home = Path::new("/home/u");
        for name in WORKSPACE_NAMES {
            for p in [name.to_string(), format!("a/{name}"), format!("a/b/c/{name}"), format!("a/{name}/child")] {
                assert!(is_protected(&ws.join(&p), ws, home, &[]), "{p}");
            }
        }
        assert!(is_protected(Path::new("src/.vscode/settings.json"), ws, home, &[]), "a relative path is taken from the workspace");
        assert!(is_protected(&ws.join("a/.ENVRC"), ws, home, &[]), "names match in any case");
        assert!(is_protected(&ws.join("a/.Git/hooks/x"), ws, home, &[]));
        assert!(is_protected(&ws.join("a/../.envrc"), ws, home, &[]), "dot-dot is resolved");
        assert!(!is_protected(Path::new("/elsewhere/.envrc"), ws, home, &[]), "names are protected inside the working folder");
    }

    #[test]
    fn git_component_in_a_write_path_is_protected() {
        let ws = Path::new("/work/proj");
        let home = Path::new("/home/u");
        assert!(has_git_component(Path::new("/a/.git/b")));
        assert!(!has_git_component(Path::new("/a/.github/b")));
        assert!(!has_git_component(Path::new("/a/x.git/b")));
        for p in [".git/modules/m/hooks/pre-push", ".git/refs/heads/main", "sub/.git"] {
            assert!(is_protected(&ws.join(p), ws, home, &[]), "{p}");
        }
        let outside = Path::new("/elsewhere/repo/.git/hooks/pre-commit");
        assert_eq!(refusal_for(outside, ws, home, &[]).unwrap().label, "a git hook", "any write path with a .git component");
        assert!(!is_protected(Path::new("/elsewhere/repo/src/a.rs"), ws, home, &[]));
    }

    #[test]
    fn refusal_for_names_the_path_and_label() {
        let ws = Path::new("/work/proj");
        let home = Path::new("/home/u");
        let heron = [PathBuf::from("/home/u/.heron")];
        let label = |p: &str| refusal_for(Path::new(p), ws, home, &heron).map(|r| r.label);
        assert_eq!(label("/work/proj/.git/hooks/pre-commit"), Some("a git hook"));
        assert_eq!(label("/work/proj/.husky/pre-commit"), Some("a git hook"));
        assert_eq!(label("/work/proj/sub/.git/config"), Some("a repository's git config"));
        assert_eq!(label("/work/proj/.git/worktrees/w/config.worktree"), Some("a repository's git config"));
        assert_eq!(label("/work/proj/.git/HEAD"), Some("a git repository's internals"));
        assert_eq!(label("/work/proj/.git"), Some("a git repository's internals"));
        assert_eq!(label("/work/proj/.zshrc"), Some("a shell startup file"));
        assert_eq!(label("/home/u/.zlogin"), Some("a shell startup file"));
        assert_eq!(label("/home/u/Library/LaunchAgents/x.plist"), Some("a launch agent"));
        assert_eq!(label("/work/proj/.vscode/settings.json"), Some("an editor or agent settings folder"));
        assert_eq!(label("/work/proj/.claude/CLAUDE.md"), Some("an editor or agent settings folder"));
        assert_eq!(label("/work/proj/.gitmodules"), Some("a git or cargo settings file"));
        assert_eq!(label("/home/u/.gitconfig"), Some("a git or cargo settings file"));
        assert_eq!(label("/home/u/.config/git/ignore"), Some("a git or cargo settings file"));
        assert_eq!(label("/home/u/.cargo/config.toml"), Some("a git or cargo settings file"));
        assert_eq!(label("/home/u/.heron/secrets.json"), Some("Heron's own settings or data"));
        assert_eq!(label("/work/proj/src/lib.rs"), None);
        assert_eq!(label("/home/u/.cargo/registry/x"), None);
        assert_eq!(label("/home/u/.config/other/x"), None);

        let r = refusal_for(&ws.join(".envrc"), ws, home, &[]).unwrap();
        assert_eq!(r.path, ws.join(".envrc"));
        assert_eq!(
            r.message(),
            format!(
                "Refused: {} is a protected file (an editor or agent settings folder, which runs code later). Heron's file tools can't change it in any mode.",
                ws.join(".envrc").display()
            )
        );
    }

    #[test]
    fn a_write_under_claude_worktrees_is_allowed() {
        let ws = Path::new("/work/proj");
        let home = Path::new("/home/u");
        for p in [".claude/worktrees/w/src/a.rs", ".claude/worktrees/w/.claude/settings.json", ".claude/worktrees"] {
            assert!(!is_protected(&ws.join(p), ws, home, &[]), "{p}");
        }
        for p in [".claude/settings.json", ".claude/hooks/x", ".claude/other/worktrees/x"] {
            assert!(is_protected(&ws.join(p), ws, home, &[]), "{p}");
        }

        let d = tree(&[".claude/worktrees/w/a.rs", ".claude/settings.json"]);
        let (rules, _) = rules_for(d.path());
        let allow = rules.lines().position(|l| l.starts_with("(allow") && l.contains(r"\.[cC][lL][aA][uU][dD][eE]/[wW][oO][rR][kK][tT][rR][eE][eE][sS]")).expect(&rules);
        let deny = rules.lines().position(|l| l.starts_with("(deny") && l.contains(r"\.[cC][lL][aA][uU][dD][eE](/.*)?$")).expect(&rules);
        assert!(deny < allow, "the allow comes after the deny, so it wins: {rules}");

        let root = real_path(d.path());
        let home = Path::new("/nonexistent-home");
        // The shell rules are Seatbelt's, so they only exist where the shell is sandboxed.
        for p in [
            ".claude/settings.json",
            ".CLAUDE/settings.json",
            ".claude/commands/.claude/worktrees/x.md",
            ".vscode/.claude/worktrees/x",
            ".husky/.claude/worktrees/w/a",
            ".idea/a/.claude/worktrees",
            ".envrc",
            "sub/.zshrc",
            "sub/.husky/h",
            ".claude/worktrees",
            ".claude/worktrees/w/a.rs",
            ".claude/worktrees/w/.claude/settings.json",
            ".Claude/Worktrees/w/b",
            "a/b/.claude/worktrees/w/c",
            "src/a.rs",
            ".github/workflows/ci.yml",
        ]
        .into_iter()
        .filter(|_| cfg!(unix))
        {
            let full = root.join(p);
            assert_eq!(denied_by(&rules, &full.to_string_lossy()), is_protected(&full, &root, home, &[]), "{p}: the shell and the file tools agree\n{rules}");
        }

        let before = snap(d.path());
        std::fs::write(d.path().join(".claude/worktrees/w/a.rs"), "changed").unwrap();
        std::fs::write(d.path().join(".claude/worktrees/w/new.rs"), "new").unwrap();
        assert!(before.diff(&snap(d.path())).is_empty(), "the snapshot skips .claude/worktrees");
        std::fs::write(d.path().join(".claude/settings.json"), "changed").unwrap();
        assert_eq!(ws_paths(d.path(), &before.diff(&snap(d.path()))), vec![".claude/settings.json:Changed"]);

        let nested = tree(&[".vscode/.claude/worktrees/w/a.rs", ".claude/commands/.claude/worktrees/x.md"]);
        let before = snap(nested.path());
        std::fs::write(nested.path().join(".vscode/.claude/worktrees/w/a.rs"), "changed").unwrap();
        std::fs::write(nested.path().join(".claude/commands/.claude/worktrees/x.md"), "changed").unwrap();
        assert_eq!(
            ws_paths(nested.path(), &before.diff(&snap(nested.path()))),
            vec![".claude/commands/.claude/worktrees/x.md:Changed", ".vscode/.claude/worktrees/w/a.rs:Changed"]
        );
    }

    /// The verdict of the regex rules for a write to `path`, as Seatbelt gives it: the last rule that
    /// matches decides, and a path no rule matches is allowed.
    fn denied_by(rules: &str, path: &str) -> bool {
        let mut denied = false;
        for line in rules.lines().filter(|l| l.contains("(regex")) {
            let matches = line
                .split("(regex #\"")
                .skip(1)
                .any(|part| regex::Regex::new(part.split("\")").next().unwrap()).unwrap().is_match(path));
            if matches {
                denied = line.starts_with("(deny");
            }
        }
        denied
    }

    #[test]
    fn default_heron_home_files_are_protected_when_heron_home_is_overridden() {
        let ws = Path::new("/work/proj");
        let home = Path::new("/home/u");
        let homes = [PathBuf::from("/data/heron"), PathBuf::from("/home/u/.heron")];
        for file in HERON_FILES {
            for h in &homes {
                let r = refusal_for(&h.join(file), ws, home, &homes).unwrap_or_else(|| panic!("{h:?}/{file}"));
                assert_eq!(r.label, "Heron's own settings or data");
            }
        }
        assert!(!is_protected(&homes[1].join("config.toml"), ws, home, &homes[..1]), "only the homes passed are covered");
    }

    #[test]
    fn heron_home_folder_itself_is_not_protected() {
        let ws = Path::new("/work/proj");
        let home = Path::new("/home/u");
        let homes = [PathBuf::from("/data/heron"), PathBuf::from("/home/u/.heron")];
        for p in ["/data/heron", "/home/u/.heron", "/home/u/.heron/selfheal/runs/1/out.txt", "/data/heron/journal/a.lease", "/data/heron/background.lock"] {
            assert!(!is_protected(Path::new(p), ws, home, &homes), "{p}");
        }
    }

    #[test]
    #[cfg_attr(not(unix), ignore = "Seatbelt rules are written for unix paths")]
    fn apply_protects_existing_repos_and_names() {
        let d = tree(&[".git/config", ".git/hooks/", "wt/.git", ".idea/x", "sub/.envrc"]);
        let ws = d.path();
        let (rules, refused) = rules_for(ws);
        let root = regex_escape(&real_path(ws).to_string_lossy());
        let want = |name: &str, tail: &str| seatbelt_regex(&format!("^{root}/(.*/)?{name}{tail}"));
        assert!(rules.contains(&format!("(regex #\"{}\")", want(r"\.mcp\.json", "$"))), "{rules}");
        assert!(rules.contains(&format!("(regex #\"{}\")", want(r"\.idea", "(/.*)?$"))), "{rules}");
        for lit in [ws.join(".git/hooks"), ws.join(".git/config"), ws.join("wt/.git"), ws.join(".git")] {
            assert!(rules.contains(&format!("(literal \"{}\")", real_path(&lit).display())), "{lit:?}: {rules}");
        }
        assert!(refused.contains(&ws.join(".git/config")), "{refused:?}");
        assert!(!refused.contains(&ws.join(".idea")), "a name's own rule already denies a rename of it: {refused:?}");
        assert_eq!(regex_escape("/a.b/c(d)"), r"/a\.b/c\(d\)");
        assert_eq!(regex_escape("/a\"b/.git"), r"/a.b/\.git");
        assert_eq!(regex_escape(r"/a+b\c"), r"/a\+b\\c");
    }

    #[test]
    fn rules_return_repo_paths_for_refused_ancestors() {
        let d = tree(&["x/.git/config", "x/.git/hooks/", "w/.git", "w/.vscode/s.json"]);
        let ws = d.path();
        let (_, refused) = rules_for(ws);
        for want in [
            ws.join("x/.git"),
            ws.join("x/.git/hooks"),
            ws.join("x/.git/config"),
            ws.join("x/.git/config.worktree"),
            ws.join("w/.git"),
        ] {
            assert!(refused.contains(&want), "{want:?} in {refused:?}");
        }
        assert!(!refused.contains(&ws.join("x")), "ancestors are for refused_ancestors to find");
    }

    #[test]
    #[cfg_attr(not(unix), ignore = "Seatbelt rules are written for unix paths")]
    fn git_admin_files_are_denied_at_any_depth() {
        let d = tree(&["a.txt"]);
        let (rules, _) = rules_for(d.path());
        let root = real_path(d.path());
        let at = |p: &str| format!("{}/{p}", root.display());
        for p in [".git/commondir", ".git/gitdir", ".git/config.worktree", ".git/worktrees/feature/commondir", ".git/worktrees/feature/gitdir", ".git/worktrees/feature/config.worktree", "sub/.git/worktrees/x/commondir"] {
            assert!(denied_by(&rules, &at(p)), "{p}\n{rules}");
        }
        for p in [".git/worktrees/feature/HEAD", ".git/worktrees/feature/index", "src/commondir", "notes/gitdir"] {
            assert!(!denied_by(&rules, &at(p)), "{p}\n{rules}");
        }
    }

    #[test]
    #[cfg_attr(not(unix), ignore = "Seatbelt rules are written for unix paths")]
    fn hooks_a_commondir_names_outside_the_workspace_are_denied_whole() {
        let root = tempfile::tempdir().unwrap();
        let ws = root.path().join("ws");
        std::fs::create_dir_all(ws.join(".git")).unwrap();
        std::fs::create_dir_all(root.path().join("other/.git/hooks")).unwrap();
        std::fs::write(ws.join(".git/commondir"), "../../other/.git\n").unwrap();
        let (rules, _) = rules_for(&ws);
        let hooks = real_path(&root.path().join("other/.git/hooks"));
        let whole = |p: &Path| rules.lines().any(|l| l.starts_with("(deny") && l.contains(&format!("(subpath \"{}\")", p.display())));
        assert!(whole(&hooks), "{rules}");
        assert!(!whole(&real_path(&ws.join(".git/hooks"))), "git's samples in the workspace stay writable: {rules}");
    }

    #[test]
    fn a_linked_worktrees_admin_files_are_snapshotted() {
        let ws = tempfile::tempdir().unwrap();
        let admin = ws.path().join(".git/worktrees/feature");
        std::fs::create_dir_all(ws.path().join(".git/hooks")).unwrap();
        std::fs::create_dir_all(&admin).unwrap();
        std::fs::write(admin.join("commondir"), "../..\n").unwrap();
        std::fs::write(admin.join("gitdir"), "/elsewhere/feature/.git\n").unwrap();
        for file in ["commondir", "gitdir", "config.worktree"] {
            let before = snap(ws.path());
            std::fs::write(admin.join(file), "/tmp/g\n").unwrap();
            let changed = before.diff(&snap(ws.path()));
            assert_eq!(ws_paths(ws.path(), &changed).len(), 1, "{file}: {changed:?}");
            assert!(changed[0].path.ends_with(file), "{file}: {changed:?}");
        }
    }

    #[test]
    fn hooks_rule_allows_sample_names() {
        let d = tree(&["a.txt"]);
        let (rules, _) = rules_for(d.path());
        let root = regex_escape(&real_path(d.path()).to_string_lossy());
        let deny = seatbelt_regex(&format!(r"^{root}/(.*/)?\.git/(.*/)?hooks/.+$"));
        let allow = seatbelt_regex(&format!(r"^{root}/(.*/)?\.git/(.*/)?hooks/[^/]+\.sample$"));
        let line = |prefix: &str, pattern: &str| {
            rules.lines().position(|l| l.starts_with(prefix) && l.contains(&format!("(regex #\"{pattern}\")"))).unwrap_or_else(|| panic!("{prefix} {pattern} in {rules}"))
        };
        let (d_at, a_at) = (line("(deny", &deny), line("(allow", &allow));
        assert!(d_at < a_at, "the sample allow comes last: {rules}");
        assert!(!rules.contains("hooks\")"), "no rule names a hooks folder for a repository the scan has not seen");
        assert!(!rules.lines().any(|l| l == "(deny file-write*)"), "a deny with no filter denies every write: {rules}");
        let compiled = regex::Regex::new(&deny).unwrap();
        let sample = regex::Regex::new(&allow).unwrap();
        let hook = format!("{}/.git/hooks/pre-commit", real_path(d.path()).display());
        let sample_hook = format!("{}/sub/.git/hooks/pre-commit.sample", real_path(d.path()).display());
        let modules = format!("{}/.git/modules/m/hooks/post-merge", real_path(d.path()).display());
        assert!(compiled.is_match(&hook) && !sample.is_match(&hook));
        assert!(compiled.is_match(&sample_hook) && sample.is_match(&sample_hook));
        assert!(compiled.is_match(&modules) && !sample.is_match(&modules));
        let folder = format!("{}/.git/hooks", real_path(d.path()).display());
        assert!(!compiled.is_match(&folder), "git init must be able to create the folder");
        let nested = format!("{}/.git/hooks/sub/x.sample", real_path(d.path()).display());
        assert!(!sample.is_match(&nested));
    }

    #[test]
    #[cfg_attr(not(unix), ignore = "Seatbelt rules are written for unix paths")]
    fn workspace_path_is_escaped_in_rules() {
        let d = tempfile::tempdir().unwrap();
        let ws = d.path().join("a+b (c).d\"e");
        std::fs::create_dir_all(ws.join(".git")).unwrap();
        let (rules, _) = rules_for(&ws);
        let root = real_path(&ws).to_string_lossy().into_owned();
        let escaped = regex_escape(&root);
        assert!(escaped.contains(r"a\+b \(c\)\.d.e"), "{escaped}");
        assert!(rules.contains(&seatbelt_regex(&format!("^{escaped}/(.*/)?\\.envrc$"))), "{rules}");
        let envrc = rules
            .lines()
            .find(|l| l.starts_with("(deny") && l.contains(&seatbelt_regex(&format!("^{escaped}/(.*/)?\\.envrc$"))))
            .expect(&rules);
        let pattern = seatbelt_regex(&format!("^{escaped}/(.*/)?\\.envrc$"));
        let compiled = regex::Regex::new(&pattern).unwrap();
        assert!(envrc.contains(&pattern));
        assert!(compiled.is_match(&format!("{root}/sub/.envrc")), "the quote in the path matches any character");
        assert!(!compiled.is_match(&format!("{}/.envrc", root.replace("a+b", "aab"))), "a + in the path is not a quantifier");
        assert!(!compiled.is_match(&format!("{}/.envrc", root.replace("(c)", "c"))), "parentheses in the path are not a group");
        for line in rules.lines().filter(|l| l.contains("(regex")) {
            for part in line.split("(regex #\"").skip(1) {
                let pattern = part.split("\")").next().unwrap();
                assert!(regex::Regex::new(pattern).is_ok(), "{pattern}");
            }
        }
    }

    #[test]
    #[cfg_attr(not(unix), ignore = "Seatbelt rules are written for unix paths")]
    fn home_rules_only_cover_paths_inside_writable_folders() {
        let ws = tree(&["a.txt"]);
        let home = tree(&["Library/LaunchAgents/x.plist", ".zshrc"]);
        let heron = tempfile::tempdir().unwrap();
        let homes = [heron.path().to_path_buf()];
        let rules = |writable: &[PathBuf]| seatbelt_rules(ws.path(), home.path(), &homes, &scan(ws.path()), writable);

        let (outside, refused) = rules(&[ws.path().to_path_buf()]);
        assert!(!outside.contains(".zshrc") && !outside.contains("LaunchAgents") && !outside.contains("secrets.json"), "{outside}");
        assert!(refused.iter().all(|p| p.starts_with(ws.path())), "{refused:?}");

        let (inside, refused) = rules(&[ws.path().to_path_buf(), home.path().to_path_buf(), heron.path().to_path_buf()]);
        assert!(inside.contains(&format!("(literal \"{}\")", real_path(&home.path().join(".zshrc")).display())), "{inside}");
        assert!(inside.contains(&format!("(subpath \"{}\")", real_path(&home.path().join("Library/LaunchAgents")).display())), "{inside}");
        assert!(inside.contains(&format!("(literal \"{}\")", real_path(&heron.path().join("secrets.json")).display())), "{inside}");
        assert!(inside.contains(&format!("(literal \"{}\")", real_path(&heron.path().join("heron.db-wal")).display())), "{inside}");
        assert!(!inside.contains(&format!("\"{}\")", real_path(heron.path()).display())), "the folder itself stays writable: {inside}");
        for p in [home.path().join(".zshrc"), home.path().join(".gitconfig"), heron.path().join("config.toml")] {
            assert!(refused.contains(&p), "{p:?} in {refused:?}");
        }

        let (only_home, _) = rules(&[home.path().join("Library")]);
        assert!(only_home.contains("LaunchAgents") && !only_home.contains(".zshrc"), "{only_home}");
    }

    #[test]
    fn snapshots_see_changes_to_protected_entries_only() {
        let d = tree(&[".git/config", ".git/hooks/", ".git/HEAD", ".vscode/settings.json", "src/a.rs"]);
        let ws = d.path();
        let before = snap(ws);
        std::fs::write(ws.join("src/a.rs"), "changed").unwrap();
        std::fs::write(ws.join(".git/HEAD"), "ref: refs/heads/other").unwrap();
        assert!(before.diff(&snap(ws)).is_empty(), "ordinary work is not a change");
        std::fs::write(ws.join(".git/hooks/pre-commit"), "#!/bin/sh\ncurl evil").unwrap();
        std::fs::write(ws.join(".vscode/settings.json"), "{}").unwrap();
        std::fs::write(ws.join(".envrc"), "evil").unwrap();
        let c = before.diff(&snap(ws));
        assert_eq!(ws_paths(ws, &c), vec![".envrc:New", ".git/hooks/pre-commit:New", ".vscode/settings.json:Changed"], "{c:?}");

        let d = tree(&[".git/hooks/pre-push"]);
        let before = snap(d.path());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let hook = d.path().join(".git/hooks/pre-push");
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert_eq!(before.diff(&snap(d.path())).len(), 1, "making a hook executable arms it");
            let rw = snap(d.path());
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o711)).unwrap();
            assert!(rw.diff(&snap(d.path())).is_empty(), "only the execute bits count");
            let same = d.path().join("same");
            std::fs::copy(&hook, &same).unwrap();
            std::fs::remove_file(&hook).unwrap();
            std::os::unix::fs::symlink(&same, &hook).unwrap();
            let linked = snap(d.path());
            assert_eq!(rw.diff(&linked).len(), 1, "a symlink to identical content counts");
            assert!(linked.entries.values().any(|v| v.starts_with("link:")));
        }
        std::fs::remove_file(d.path().join(".git/hooks/pre-push")).unwrap();
        let c = before.diff(&snap(d.path()));
        assert_eq!(ws_paths(d.path(), &c), vec![".git/hooks/pre-push:Deleted"]);
    }

    #[test]
    fn a_new_commondir_or_gitdir_file_is_a_change() {
        let ws = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(ws.path().join(".git/hooks")).unwrap();
        let before = Snapshot::take(ws.path(), home.path(), &[]);
        std::fs::write(ws.path().join(".git/commondir"), "../g\n").unwrap();
        let after = Snapshot::take(ws.path(), home.path(), &[]);
        let changed = before.diff(&after);
        assert!(changed.iter().any(|c| c.path.ends_with("commondir")), "{changed:?}");
        std::fs::remove_file(ws.path().join(".git/commondir")).unwrap();
        let before = Snapshot::take(ws.path(), home.path(), &[]);
        std::fs::write(ws.path().join(".git/gitdir"), "elsewhere\n").unwrap();
        let after = Snapshot::take(ws.path(), home.path(), &[]);
        let changed = before.diff(&after);
        assert!(changed.iter().any(|c| c.path.ends_with("gitdir")), "{changed:?}");
    }

    #[test]
    fn a_git_folder_with_a_commondir_protects_the_folder_it_names() {
        let ws = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(ws.path().join(".git")).unwrap();
        std::fs::write(ws.path().join(".git/commondir"), "../shared\n").unwrap();
        let repo = scan::Repo::from_entry(ws.path().join(".git"));
        let parts = repo.parts();
        assert!(parts.contains(&ws.path().join("shared/hooks")), "{parts:?}");
        assert!(parts.contains(&ws.path().join(".git/commondir")) && parts.contains(&ws.path().join(".git/gitdir")), "{parts:?}");
    }

    #[test]
    fn snapshot_diff_reports_paths_only() {
        let d = tree(&[".envrc", "new/.keep"]);
        let before = snap(d.path());
        std::fs::write(d.path().join(".envrc"), "SECRET_TOKEN=hunter2").unwrap();
        let git = d.path().join("new/.git");
        std::fs::create_dir_all(git.join("hooks")).unwrap();
        std::fs::write(git.join("config"), "[core]\n\thookspath = /tmp/SECRET_TOKEN\n").unwrap();
        let c = before.diff(&snap(d.path()));
        assert_eq!(ws_paths(d.path(), &c), vec![".envrc:Changed", "new/.git/config:NewRepository"]);
        let text = report_text(&c);
        let config = Path::new("new").join(".git").join("config");
        assert!(text.contains(".envrc (changed)") && text.contains(&format!("{} (new repository)", config.display())), "{text}");
        assert!(!text.contains("SECRET_TOKEN") && !text.contains("hunter2") && !text.contains("hookspath"), "{text}");
        assert!(text.starts_with("Protected files changed while Heron was working: "));
        assert!(text.ends_with("If you didn't change them, check them before you next use this folder."));

        let many: Vec<ProtectedChange> = (0..25).map(|i| ProtectedChange { path: PathBuf::from(format!("/p/{i}")), kind: ChangeKind::Deleted }).collect();
        let text = report_text(&many);
        assert!(text.contains("/p/19 (deleted)") && !text.contains("/p/20") && text.contains("and 5 more"), "{text}");
    }

    #[test]
    fn snapshot_leaves_out_heron_db_and_leases() {
        let ws = tree(&["a.txt"]);
        let home = tree(&["config.toml", "secrets.json", "heron.db", "heron.db-wal", "heron.db-shm", "background.lock", "journal/1.lease"]);
        let homes = [home.path().to_path_buf()];
        let take = || Snapshot::take(ws.path(), Path::new("/nonexistent-home"), &homes);
        let before = take();
        for f in ["heron.db", "heron.db-wal", "heron.db-shm", "background.lock", "journal/1.lease"] {
            std::fs::write(home.path().join(f), "changed by Heron").unwrap();
        }
        assert!(before.diff(&take()).is_empty());
        std::fs::write(home.path().join("config.toml"), "changed").unwrap();
        std::fs::remove_file(home.path().join("secrets.json")).unwrap();
        let c = before.diff(&take());
        let names: Vec<String> = c.iter().map(|c| format!("{}:{:?}", c.path.file_name().unwrap().to_string_lossy(), c.kind)).collect();
        assert_eq!(names, vec!["config.toml:Changed", "secrets.json:Deleted"]);
    }

    #[test]
    fn truncated_scan_is_a_change() {
        let d = tree(&["c.txt", "d.txt"]);
        let no_home = Path::new("/nonexistent-home");
        let full = Snapshot::take(d.path(), no_home, &[]);
        let cut = Snapshot::take_limited(d.path(), no_home, &[], 1);
        assert!(!full.truncated && cut.truncated);
        for c in [full.diff(&cut), cut.diff(&full), cut.diff(&cut)] {
            assert_eq!(c, vec![ProtectedChange { path: d.path().to_path_buf(), kind: ChangeKind::ScanTruncated }]);
        }
        assert!(full.diff(&full).is_empty());
        assert!(report_text(&full.diff(&cut)).contains("(scan hit its limit)"));
    }

    #[test]
    fn a_new_repository_may_hold_only_samples_and_plain_config() {
        let d = tree(&["src/a.rs"]);
        let ws = d.path();
        let before = snap(ws);
        let git = ws.join("new/.git");
        std::fs::create_dir_all(git.join("hooks")).unwrap();
        std::fs::write(git.join("hooks/pre-commit.sample"), "#!/bin/sh").unwrap();
        let plain = "[core]\n\trepositoryformatversion = 0\n\tbare = false\n[user]\n\tname = A\n[remote \"origin\"]\n\turl = x\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n[branch \"main\"]\n\tremote = origin\n\tmerge = refs/heads/main\n";
        std::fs::write(git.join("config"), plain).unwrap();
        assert!(before.diff(&snap(ws)).is_empty(), "git init + user + remote are plain");

        for bad in [
            "[core]\n\thooksPath = /tmp/h\n",
            "[alias]\n\tst = !evil\n",
            "[filter \"x\"]\n\tsmudge = evil\n",
            "[remote \"origin\"]\n\tuploadpack = evil\n",
            "[branch \"main\"]\n\tdescription = d\n",
            "[x]\n\tremote = r\n",
            "[core] fsmonitor = /tmp/evil.sh\n",
        ] {
            std::fs::write(git.join("config"), format!("{plain}{bad}")).unwrap();
            let c = before.diff(&snap(ws));
            assert_eq!(ws_paths(ws, &c), vec!["new/.git/config:NewRepository"], "{bad}");
        }

        std::fs::write(git.join("config"), plain).unwrap();
        std::fs::write(git.join("hooks/post-checkout"), "#!/bin/sh").unwrap();
        let c = before.diff(&snap(ws));
        assert_eq!(ws_paths(ws, &c), vec!["new/.git/hooks/post-checkout:NewRepository"]);

        let d = tree(&[]);
        let before = snap(d.path());
        std::fs::write(d.path().join(".git"), "gitdir: /elsewhere").unwrap();
        assert_eq!(ws_paths(d.path(), &before.diff(&snap(d.path()))), vec![".git:NewRepository"], "a new gitdir file can point anywhere");
    }

    #[test]
    fn new_repo_with_sample_hooks_is_not_a_change() {
        let d = tree(&["src/a.rs"]);
        let before = snap(d.path());
        let hooks = d.path().join("r/.git/hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        for sample in ["pre-commit.sample", "pre-push.sample", "update.sample"] {
            std::fs::write(hooks.join(sample), "#!/bin/sh").unwrap();
        }
        assert!(before.diff(&snap(d.path())).is_empty());
        std::fs::write(hooks.join("pre-commit"), "#!/bin/sh").unwrap();
        assert_eq!(ws_paths(d.path(), &before.diff(&snap(d.path()))), vec!["r/.git/hooks/pre-commit:NewRepository"]);
    }

    #[test]
    fn cloned_repo_with_branch_and_remote_keys_is_not_a_change() {
        let d = tree(&["src/a.rs"]);
        let before = snap(d.path());
        let git = d.path().join("c/.git");
        std::fs::create_dir_all(git.join("hooks")).unwrap();
        std::fs::write(
            git.join("config"),
            "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = true\n\tignorecase = true\n\tprecomposeunicode = true\n[remote \"origin\"]\n\turl = https://example.com/r.git\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n[branch \"main\"]\n\tremote = origin\n\tmerge = refs/heads/main\n[branch \"feature/x\"]\n\tremote = origin\n\tmerge = refs/heads/feature/x\n",
        )
        .unwrap();
        std::fs::write(git.join("hooks/applypatch-msg.sample"), "#!/bin/sh").unwrap();
        assert!(before.diff(&snap(d.path())).is_empty());
    }

    #[test]
    fn new_repo_with_hooks_path_is_a_change() {
        let d = tree(&["src/a.rs"]);
        let before = snap(d.path());
        let git = d.path().join("n/.git");
        std::fs::create_dir_all(&git).unwrap();
        std::fs::write(git.join("config"), "[core]\n\tbare = false\n\thooksPath = ../hooks\n").unwrap();
        let c = before.diff(&snap(d.path()));
        assert_eq!(c, vec![ProtectedChange { path: git.join("config"), kind: ChangeKind::NewRepository }]);
        let existing = tree(&[".git/config"]);
        let before = snap(existing.path());
        std::fs::write(existing.path().join(".git/config"), "[core]\n\thooksPath = ../hooks\n").unwrap();
        assert_eq!(ws_paths(existing.path(), &before.diff(&snap(existing.path()))), vec![".git/config:Changed"]);
    }

    #[test]
    fn claude_home_items_and_mcp_servers_are_snapshotted() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        std::fs::create_dir_all(h.join(".claude/projects")).unwrap();
        std::fs::write(h.join(".claude.json"), r#"{"numStartups": 1, "projects": {"/w": {"mcpServers": {}}}}"#).unwrap();
        let before = Snapshot::take_claude_home(h);
        std::fs::write(h.join(".claude/projects/t.jsonl"), "{}").unwrap();
        std::fs::write(
            h.join(".claude.json"),
            r#"{"numStartups": 2, "projects": {"/w": {"mcpServers": {}}, "/v": {"allowedTools": [], "mcpServers": {}}}}"#,
        )
        .unwrap();
        assert!(before.diff(&Snapshot::take_claude_home(h)).is_empty(), "what the CLI itself writes is not a change");
        std::fs::write(h.join(".claude.json"), r#"{"projects": {"/w": {"mcpServers": {"x": {"command": "evil"}}}}}"#).unwrap();
        std::fs::create_dir_all(h.join(".claude/commands")).unwrap();
        std::fs::write(h.join(".claude/commands/deploy.md"), "evil").unwrap();
        let c = before.diff(&Snapshot::take_claude_home(h));
        assert!(c.iter().any(|x| x.path.to_string_lossy().ends_with("#mcpServers")) && c.iter().any(|x| x.path.ends_with("deploy.md")), "{c:?}");
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
        let (rules, _) = rules_for(ws.path());
        let profile = format!("(version 1)\n(allow default)\n{rules}");
        let out = std::process::Command::new("sandbox-exec").args(["-p", &profile, "/usr/bin/true"]).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }
}
