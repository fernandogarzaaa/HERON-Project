//! The Rust toolchain hash check (§3.4): a manifest of `~/.rustup/toolchains`, a stat-first check
//! against the trusted baseline in kv, and the pieces the shell tool calls around a command.

use crate::sandbox::subpaths;
use crate::store::Store;
use crate::util::{now_ts, truncate_middle};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

const BASELINE_KEY: &str = "toolchain:baseline";
const CHANGED_KEY: &str = "toolchain:changed";
const FORMAT: u32 = 1;
const NOT_COVERED: [&str; 2] = ["lib/rustlib/src", "share/doc"];
const NEW_TOOLCHAIN: &str = "(new toolchain)";
const REMOVED_TOOLCHAIN: &str = "(removed)";
const UNREADABLE: &str = "(unreadable)";
const BASELINE_UNREADABLE: &str = "(baseline unreadable)";
const COMMAND_CHARS: usize = 200;
const SHOWN_FILES: usize = 5;
const RUST_TOOLS: [&str; 7] = ["cargo", "rustc", "rustup", "rustdoc", "rustfmt", "clippy-driver", "rust-analyzer"];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ChangedFile {
    pub toolchain: String,
    pub path: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ToolchainState {
    #[default]
    Trusted,
    NoBaseline,
    Changed { toolchains: Vec<String>, files: Vec<ChangedFile>, since: i64, during: Option<String> },
}

/// The owner's `since` no longer matches the change they were shown.
#[derive(Debug)]
pub struct StaleSince;

impl std::fmt::Display for StaleSince {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("The toolchain changed again since you looked. Check it again before you trust it.")
    }
}

impl std::error::Error for StaleSince {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Stat {
    size: u64,
    mtime: i64,
    #[serde(default)]
    ino: u64,
    #[serde(default)]
    ctime: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    size: u64,
    mtime: i64,
    sha256: String,
    #[serde(default)]
    ino: u64,
    #[serde(default)]
    ctime: i64,
}

impl Entry {
    fn new(stat: &Stat, sha256: String) -> Entry {
        Entry { size: stat.size, mtime: stat.mtime, sha256, ino: stat.ino, ctime: stat.ctime }
    }

    fn stat(&self) -> Stat {
        Stat { size: self.size, mtime: self.mtime, ino: self.ino, ctime: self.ctime }
    }
}

/// Every covered file of every installed toolchain, with its stat data and SHA-256.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    format: u32,
    toolchains: BTreeMap<String, BTreeMap<String, Entry>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Record {
    toolchains: Vec<String>,
    files: Vec<ChangedFile>,
    since: i64,
    during: Option<String>,
    reported: bool,
}

type Hasher<'a> = &'a dyn Fn(&Path) -> io::Result<String>;

fn toolchains_dir(user_home: &Path) -> PathBuf {
    user_home.join(".rustup").join("toolchains")
}

fn stat_of(md: &fs::Metadata) -> Stat {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Stat {
            size: md.len(),
            mtime: md.mtime() * 1_000_000_000 + md.mtime_nsec(),
            ino: md.ino(),
            ctime: md.ctime() * 1_000_000_000 + md.ctime_nsec(),
        }
    }
    #[cfg(not(unix))]
    {
        let nanos = md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos() as i64);
        Stat { size: md.len(), mtime: nanos, ino: 0, ctime: 0 }
    }
}

fn hash_file(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut sha = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        sha.update(&buf[..n]);
    }
    Ok(sha.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn installed(user_home: &Path) -> Vec<String> {
    let Ok(rd) = fs::read_dir(toolchains_dir(user_home)) else { return vec![] };
    let mut names: Vec<String> = rd
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn walk(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf, Stat)>) -> io::Result<()> {
    for e in fs::read_dir(dir)? {
        let e = e?;
        let name = e.file_name().to_string_lossy().into_owned();
        let rel = if rel.is_empty() { name } else { format!("{rel}/{name}") };
        if NOT_COVERED.contains(&rel.as_str()) {
            continue;
        }
        let md = e.metadata()?;
        if md.is_dir() {
            walk(&e.path(), &rel, out)?;
        } else if md.is_file() {
            out.push((rel, e.path(), stat_of(&md)));
        }
    }
    Ok(())
}

fn files_of(user_home: &Path, name: &str) -> io::Result<Vec<(String, PathBuf, Stat)>> {
    let mut out = vec![];
    walk(&toolchains_dir(user_home).join(name), "", &mut out)?;
    Ok(out)
}

impl Manifest {
    pub fn scan(user_home: &Path) -> Result<Manifest> {
        Manifest::scan_with(user_home, &hash_file)
    }

    fn scan_with(user_home: &Path, hash: Hasher) -> Result<Manifest> {
        let mut toolchains = BTreeMap::new();
        for name in installed(user_home) {
            let mut files = BTreeMap::new();
            for (rel, abs, stat) in files_of(user_home, &name).with_context(|| format!("reading toolchain {name}"))? {
                let sha = hash(&abs).with_context(|| format!("hashing {}", abs.display()))?;
                files.insert(rel, Entry::new(&stat, sha));
            }
            toolchains.insert(name, files);
        }
        Ok(Manifest { format: FORMAT, toolchains })
    }

    fn parse(json: &str) -> Result<Manifest> {
        let m: Manifest = serde_json::from_str(json).context("the toolchain baseline is not valid JSON")?;
        if m.format != FORMAT {
            bail!("the toolchain baseline has format {}, not {FORMAT}", m.format);
        }
        Ok(m)
    }

    fn store(&self, store: &Store) -> Result<()> {
        store.kv_set(BASELINE_KEY, &serde_json::to_string(self)?)
    }
}

fn read_record(store: &Store) -> Option<Record> {
    serde_json::from_str(&store.kv_read(CHANGED_KEY).ok()??).ok()
}

fn write_record(store: &Store, rec: &Record) {
    if let Ok(json) = serde_json::to_string(rec) {
        let _ = store.kv_set(CHANGED_KEY, &json);
    }
}

fn unreadable_baseline(user_home: &Path) -> BTreeSet<ChangedFile> {
    let mut toolchains = installed(user_home);
    if toolchains.is_empty() {
        toolchains.push(String::new());
    }
    toolchains.into_iter().map(|toolchain| ChangedFile { toolchain, path: BASELINE_UNREADABLE.into() }).collect()
}

/// Compare the toolchains on disk with `baseline`. Only a file whose stat data differs is hashed.
/// A file whose hash still matches gets its stat data refreshed in `baseline`, when a second stat
/// taken after hashing equals the first. Returns the changed files and whether `baseline` changed.
fn diff(user_home: &Path, baseline: &mut Manifest, hash: Hasher) -> (BTreeSet<ChangedFile>, bool) {
    let mut changed = BTreeSet::new();
    let mut refreshed = false;
    let mark = |set: &mut BTreeSet<ChangedFile>, t: &str, p: &str| {
        set.insert(ChangedFile { toolchain: t.to_string(), path: p.to_string() });
    };
    let now_installed = installed(user_home);
    for name in &now_installed {
        let Some(base) = baseline.toolchains.get_mut(name) else {
            mark(&mut changed, name, NEW_TOOLCHAIN);
            continue;
        };
        let Ok(files) = files_of(user_home, name) else {
            mark(&mut changed, name, UNREADABLE);
            continue;
        };
        let mut seen = BTreeSet::new();
        for (rel, abs, stat) in files {
            seen.insert(rel.clone());
            let Some(entry) = base.get_mut(&rel) else {
                mark(&mut changed, name, &rel);
                continue;
            };
            if entry.stat() == stat {
                continue;
            }
            match hash(&abs) {
                Ok(sha) if sha == entry.sha256 => {
                    let after = fs::symlink_metadata(&abs).map(|md| stat_of(&md));
                    if after.is_ok_and(|a| a == stat) {
                        *entry = Entry::new(&stat, sha);
                        refreshed = true;
                    }
                }
                _ => mark(&mut changed, name, &rel),
            }
        }
        for rel in base.keys().filter(|r| !seen.contains(*r)) {
            mark(&mut changed, name, rel);
        }
    }
    for name in baseline.toolchains.keys().filter(|n| !now_installed.contains(n)) {
        mark(&mut changed, name, REMOVED_TOOLCHAIN);
    }
    (changed, refreshed)
}

pub fn check(store: &Store, user_home: &Path) -> ToolchainState {
    check_with(store, user_home, &hash_file)
}

fn check_with(store: &Store, user_home: &Path, hash: Hasher) -> ToolchainState {
    if cfg!(not(unix)) {
        return ToolchainState::Trusted;
    }
    let baseline = match store.kv_read(BASELINE_KEY) {
        Ok(None) => {
            return match Manifest::scan_with(user_home, hash) {
                Ok(m) if m.store(store).is_ok() => ToolchainState::Trusted,
                _ => ToolchainState::NoBaseline,
            };
        }
        Ok(Some(json)) => Manifest::parse(&json).ok(),
        Err(_) => None,
    };
    let found = match baseline {
        Some(mut baseline) => {
            let (found, refreshed) = diff(user_home, &mut baseline, hash);
            if refreshed {
                let _ = baseline.store(store);
            }
            found
        }
        None => unreadable_baseline(user_home),
    };
    let record = read_record(store);
    if found.is_empty() && record.is_none() {
        return ToolchainState::Trusted;
    }
    let known: BTreeSet<ChangedFile> = record.iter().flat_map(|r| r.files.iter().cloned()).collect();
    let rec = match record {
        Some(r) if found.is_subset(&known) => r,
        old => {
            let files: Vec<ChangedFile> = known.into_iter().chain(found).collect::<BTreeSet<_>>().into_iter().collect();
            let toolchains: Vec<String> = files.iter().map(|f| f.toolchain.clone()).collect::<BTreeSet<_>>().into_iter().collect();
            let since = old.map_or(now_ts(), |r| now_ts().max(r.since + 1));
            let rec = Record { toolchains, files, since, during: None, reported: false };
            write_record(store, &rec);
            rec
        }
    };
    ToolchainState::Changed { toolchains: rec.toolchains, files: rec.files, since: rec.since, during: rec.during }
}

/// Make what is installed now the trusted baseline. `seen_since` is the `since` of the `Changed`
/// state the owner was shown, or `None` when they were shown none.
pub fn trust_current(store: &Store, user_home: &Path, seen_since: Option<i64>) -> Result<ToolchainState> {
    if let ToolchainState::Changed { since, .. } = check(store, user_home) {
        if seen_since != Some(since) {
            return Err(StaleSince.into());
        }
    }
    Manifest::scan(user_home)?.store(store)?;
    store.kv_delete(CHANGED_KEY)?;
    Ok(ToolchainState::Trusted)
}

pub fn export_baseline(store: &Store) -> Result<String> {
    let json = store.kv_read(BASELINE_KEY)?.ok_or_else(|| anyhow!("there is no toolchain baseline to export"))?;
    Manifest::parse(&json)?;
    Ok(json)
}

pub fn import_baseline(store: &Store, json: &str) -> Result<()> {
    let manifest = Manifest::parse(json)?;
    let mut existed = false;
    let fresh = serde_json::to_string(&manifest)?;
    store.kv_update(BASELINE_KEY, |old| match old {
        Some(old) => {
            existed = true;
            old
        }
        None => fresh,
    })?;
    if existed {
        bail!("this store already has a toolchain baseline");
    }
    Ok(())
}

fn file_list(toolchains: &[String], files: &[ChangedFile]) -> String {
    let name = |f: &ChangedFile| if toolchains.len() > 1 { format!("{}:{}", f.toolchain, f.path) } else { f.path.clone() };
    let mut shown: Vec<String> = files.iter().take(SHOWN_FILES).map(name).collect();
    if files.len() > SHOWN_FILES {
        shown.push(format!("and {} more", files.len() - SHOWN_FILES));
    }
    shown.join(", ")
}

/// What the shell tool says when it refuses a Rust tool because the toolchain changed.
pub fn refusal(state: &ToolchainState) -> Option<String> {
    let ToolchainState::Changed { toolchains, files, .. } = state else { return None };
    Some(format!(
        "Heron won't run the Rust toolchain: {} changed since you last trusted it ({}). Check it, then trust it in Settings › Sandbox.",
        toolchains.join(", "),
        file_list(toolchains, files)
    ))
}

/// The notice for a change the check after a command found, once: the first call after a change
/// is recorded returns the text, and later calls return `None`.
pub fn note_after(store: &Store, before: &ToolchainState, after: &ToolchainState, command: &str) -> Option<String> {
    let ToolchainState::Changed { toolchains, files, .. } = after else { return None };
    let mut rec = read_record(store)?;
    if rec.reported {
        return None;
    }
    rec.reported = true;
    let during = matches!(before, ToolchainState::Trusted);
    if during {
        rec.during = Some(truncate_middle(command, COMMAND_CHARS));
    }
    write_record(store, &rec);
    let when = if during { "during this command" } else { "since Heron last checked" };
    Some(format!(
        "[toolchain] the Rust toolchain {} changed {when} ({}). Heron won't use it until you trust it in Settings › Sandbox.",
        toolchains.join(", "),
        file_list(toolchains, files)
    ))
}

/// The Seatbelt rules that stop a command from reading a changed toolchain.
pub fn seatbelt_denies(state: &ToolchainState, user_home: &Path) -> String {
    let ToolchainState::Changed { toolchains, .. } = state else { return String::new() };
    let dir = toolchains_dir(user_home);
    toolchains
        .iter()
        .filter(|t| !t.is_empty())
        .map(|t| format!("(deny file-read*{})\n", subpaths(&[dir.join(t)])))
        .collect()
}

fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// The wrapper's flags that take a value in the next word.
fn flag_takes_value(wrapper: &str, flag: &str) -> bool {
    match wrapper {
        "env" => matches!(flag, "-u" | "-C" | "-S" | "--unset" | "--chdir" | "--split-string"),
        "nice" => flag == "-n",
        "xargs" => matches!(flag, "-I" | "-n" | "-P" | "-L" | "-s" | "-d" | "-E" | "-a"),
        "sudo" => matches!(flag, "-u" | "-g" | "-C" | "-h" | "-p" | "-r" | "-t" | "-U" | "-D"),
        "time" => matches!(flag, "-f" | "-o"),
        "exec" => flag == "-a",
        _ => false,
    }
}

const WRAPPERS: [&str; 8] = ["env", "command", "exec", "time", "nice", "nohup", "xargs", "sudo"];

fn part_names_rust_tool(part: &str) -> bool {
    let mut words = part.split_whitespace().peekable();
    let mut wrapper: Option<&str> = None;
    while let Some(word) = words.next() {
        if is_assignment(word) {
            continue;
        }
        if let Some(w) = wrapper {
            if word == "--" {
                wrapper = None;
                continue;
            }
            if word.starts_with('-') {
                if flag_takes_value(w, word) {
                    words.next();
                }
                continue;
            }
        }
        let base = Path::new(word).file_name().and_then(|n| n.to_str()).unwrap_or(word);
        if WRAPPERS.contains(&base) {
            wrapper = WRAPPERS.iter().copied().find(|w| *w == base);
            continue;
        }
        return RUST_TOOLS.contains(&base) || base.starts_with("cargo-");
    }
    false
}

/// Whether a shell command line runs a Rust tool as one of its commands, looking through
/// pipes, lists, substitutions and the usual wrappers. Anything it cannot see, such as an
/// `sh -c` string, is left to the Seatbelt deny.
pub fn names_rust_tool(command: &str) -> bool {
    command.split(['|', '&', ';', '\n', '(', '`']).any(part_names_rust_tool)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};
    use tempfile::TempDir;

    const TC: &str = "stable-aarch64-apple-darwin";

    struct Fixture {
        _dir: TempDir,
        home: PathBuf,
        store: Store,
    }

    impl Fixture {
        fn new() -> Fixture {
            let dir = tempfile::tempdir().unwrap();
            let home = dir.path().join("home");
            let f = Fixture { store: Store::open(&dir.path().join("heron.db")).unwrap(), home, _dir: dir };
            f.write(TC, "bin/cargo", "cargo 1");
            f.write(TC, "bin/rustc", "rustc 1");
            f.write(TC, "lib/librustc_driver.dylib", "driver 1");
            f.write(TC, "libexec/rust-analyzer-proc-macro-srv", "srv 1");
            f.write(TC, "lib/rustlib/src/rust/library/lib.rs", "src 1");
            f.write(TC, "share/doc/rust/index.html", "doc 1");
            f
        }

        fn path(&self, toolchain: &str, rel: &str) -> PathBuf {
            toolchains_dir(&self.home).join(toolchain).join(rel)
        }

        fn write(&self, toolchain: &str, rel: &str, body: &str) {
            let p = self.path(toolchain, rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, body).unwrap();
        }

        fn check(&self) -> ToolchainState {
            check(&self.store, &self.home)
        }

        fn baseline(&self) -> Manifest {
            Manifest::parse(&self.store.kv_get(BASELINE_KEY).unwrap()).unwrap()
        }
    }

    fn changed_paths(state: &ToolchainState) -> Vec<String> {
        match state {
            ToolchainState::Changed { files, .. } => files.iter().map(|f| f.path.clone()).collect(),
            other => panic!("expected Changed, got {other:?}"),
        }
    }

    #[test]
    fn changed_binary_is_reported() {
        let f = Fixture::new();
        assert_eq!(f.check(), ToolchainState::Trusted);
        f.write(TC, "bin/rustc", "rustc 2");
        let state = f.check();
        assert_eq!(changed_paths(&state), vec!["bin/rustc"]);
        let ToolchainState::Changed { toolchains, .. } = state else { unreachable!() };
        assert_eq!(toolchains, vec![TC]);
    }

    #[test]
    fn libexec_change_is_reported() {
        let f = Fixture::new();
        f.check();
        f.write(TC, "libexec/rust-analyzer-proc-macro-srv", "srv 2");
        assert_eq!(changed_paths(&f.check()), vec!["libexec/rust-analyzer-proc-macro-srv"]);
    }

    #[test]
    fn touch_without_content_change_stays_trusted() {
        let f = Fixture::new();
        f.check();
        let before = f.baseline().toolchains[TC]["bin/cargo"].clone();
        let file = fs::OpenOptions::new().write(true).open(f.path(TC, "bin/cargo")).unwrap();
        file.set_modified(SystemTime::now() + Duration::from_secs(3600)).unwrap();
        drop(file);
        assert_eq!(f.check(), ToolchainState::Trusted);
        let after = f.baseline().toolchains[TC]["bin/cargo"].clone();
        assert_ne!(after.mtime, before.mtime);
        assert_eq!(after.sha256, before.sha256);
        assert_eq!(f.check(), ToolchainState::Trusted);
    }

    #[test]
    fn new_toolchain_folder_is_a_change() {
        let f = Fixture::new();
        f.check();
        f.write("nightly-aarch64-apple-darwin", "bin/rustc", "nightly");
        let ToolchainState::Changed { toolchains, .. } = f.check() else { panic!("expected Changed") };
        assert_eq!(toolchains, vec!["nightly-aarch64-apple-darwin"]);
    }

    #[test]
    fn rust_src_and_docs_are_ignored() {
        let f = Fixture::new();
        f.check();
        f.write(TC, "lib/rustlib/src/rust/library/lib.rs", "src 2");
        f.write(TC, "lib/rustlib/src/rust/new.rs", "new");
        f.write(TC, "share/doc/rust/index.html", "doc 2");
        f.write(TC, "share/doc/rust/new.html", "new");
        assert_eq!(f.check(), ToolchainState::Trusted);
        assert!(f.baseline().toolchains[TC].keys().all(|k| !k.starts_with("share/doc") && !k.starts_with("lib/rustlib/src")));
    }

    #[test]
    fn names_rust_tool_finds_cargo_after_a_pipe() {
        assert!(names_rust_tool("ls | cargo build"));
        assert!(names_rust_tool("echo hi && /Users/x/.cargo/bin/cargo test"));
        assert!(names_rust_tool("ls; rustc -V"));
        assert!(names_rust_tool("echo $(rustup show)"));
        assert!(names_rust_tool("echo `rustfmt --version`"));
        assert!(names_rust_tool("(cd x && cargo-nextest run)"));
        assert!(names_rust_tool("true\ncargo check"));
        assert!(!names_rust_tool("echo cargo"));
        assert!(!names_rust_tool("ls -la | grep rustc"));
        assert!(!names_rust_tool("sh -c 'car''go build'"));
    }

    #[test]
    fn names_rust_tool_skips_env_and_assignments() {
        assert!(names_rust_tool("RUSTFLAGS=-Dwarnings cargo build"));
        assert!(names_rust_tool("A=1 B=2 rustc x.rs"));
        assert!(names_rust_tool("env -u HOME FOO=1 cargo test"));
        assert!(names_rust_tool("time cargo build"));
        assert!(names_rust_tool("sudo -u root nice -n 5 cargo build"));
        assert!(names_rust_tool("ls | xargs -n 1 rustc"));
        assert!(names_rust_tool("nohup command exec cargo run"));
        assert!(!names_rust_tool("env FOO=1 ls"));
        assert!(!names_rust_tool("FOO=cargo ls"));
        assert!(!names_rust_tool("time"));
    }

    #[test]
    fn changed_toolchain_is_read_denied() {
        let f = Fixture::new();
        f.write("beta-aarch64-apple-darwin", "bin/rustc", "beta 1");
        f.check();
        f.write(TC, "bin/rustc", "rustc 2");
        let denies = seatbelt_denies(&f.check(), &f.home);
        let dir = toolchains_dir(&f.home);
        assert!(denies.starts_with("(deny file-read*"), "{denies}");
        assert!(denies.contains(&format!("(subpath \"{}\")", dir.join(TC).display())), "{denies}");
        assert!(!denies.contains("beta-aarch64-apple-darwin"), "{denies}");
    }

    #[test]
    fn no_baseline_adds_no_denies() {
        let f = Fixture::new();
        assert_eq!(seatbelt_denies(&ToolchainState::NoBaseline, &f.home), "");
        assert_eq!(seatbelt_denies(&ToolchainState::Trusted, &f.home), "");
        assert_eq!(seatbelt_denies(&f.check(), &f.home), "");
    }

    #[test]
    fn change_is_noticed_once() {
        let f = Fixture::new();
        let before = f.check();
        assert_eq!(before, ToolchainState::Trusted);
        f.write(TC, "bin/rustc", "rustc 2");
        let after = f.check();
        let note = note_after(&f.store, &before, &after, "cargo install --path .").unwrap();
        assert!(note.starts_with("[toolchain] the Rust toolchain stable-aarch64-apple-darwin changed during this command (bin/rustc)."), "{note}");
        assert_eq!(note_after(&f.store, &before, &f.check(), "cargo install --path ."), None);
        let ToolchainState::Changed { during, .. } = f.check() else { panic!("expected Changed") };
        assert_eq!(during.as_deref(), Some("cargo install --path ."));
        assert_eq!(note_after(&f.store, &before, &ToolchainState::Trusted, "ls"), None);
    }

    #[test]
    fn trust_current_makes_state_trusted() {
        let f = Fixture::new();
        f.check();
        f.write(TC, "bin/rustc", "rustc 2");
        let ToolchainState::Changed { since, .. } = f.check() else { panic!("expected Changed") };
        assert_eq!(trust_current(&f.store, &f.home, Some(since)).unwrap(), ToolchainState::Trusted);
        assert_eq!(f.check(), ToolchainState::Trusted);
        assert!(f.store.kv_get(CHANGED_KEY).is_none());
        f.write(TC, "bin/rustc", "rustc 3");
        assert_ne!(f.check(), ToolchainState::Trusted);
    }

    #[test]
    fn exported_baseline_imports_into_another_store() {
        let f = Fixture::new();
        f.check();
        let json = export_baseline(&f.store).unwrap();
        f.write(TC, "bin/rustc", "rustc 2");
        let child = Store::open(&f._dir.path().join("child.db")).unwrap();
        import_baseline(&child, &json).unwrap();
        assert!(child.kv_get(CHANGED_KEY).is_none());
        assert_eq!(changed_paths(&check(&child, &f.home)), vec!["bin/rustc"]);
    }

    #[test]
    fn export_without_a_baseline_fails() {
        let f = Fixture::new();
        assert!(export_baseline(&f.store).is_err());
    }

    #[test]
    fn import_refuses_a_malformed_or_existing_baseline() {
        let f = Fixture::new();
        f.check();
        let good = export_baseline(&f.store).unwrap();
        let child = Store::open(&f._dir.path().join("child.db")).unwrap();
        assert!(import_baseline(&child, "{not json").is_err());
        assert!(import_baseline(&child, &good.replace("\"format\":1", "\"format\":2")).is_err());
        assert!(child.kv_get(BASELINE_KEY).is_none());
        import_baseline(&child, &good).unwrap();
        let before = child.kv_get(BASELINE_KEY);
        let other = Fixture::new();
        other.write("nightly", "bin/rustc", "n");
        other.check();
        assert!(import_baseline(&child, &export_baseline(&other.store).unwrap()).is_err());
        assert_eq!(child.kv_get(BASELINE_KEY), before);
    }

    #[test]
    fn trust_with_a_stale_since_is_refused() {
        let f = Fixture::new();
        f.check();
        f.write(TC, "bin/rustc", "rustc 2");
        let ToolchainState::Changed { since: first, .. } = f.check() else { panic!("expected Changed") };
        f.write(TC, "bin/cargo", "cargo 2");
        let ToolchainState::Changed { since: second, .. } = f.check() else { panic!("expected Changed") };
        assert!(second > first);
        let err = trust_current(&f.store, &f.home, Some(first)).unwrap_err();
        assert!(err.downcast_ref::<StaleSince>().is_some(), "{err}");
        assert!(trust_current(&f.store, &f.home, None).unwrap_err().downcast_ref::<StaleSince>().is_some());
        assert!(matches!(f.check(), ToolchainState::Changed { .. }));
        assert!(trust_current(&f.store, &f.home, Some(second)).is_ok());
    }

    #[test]
    fn stat_refresh_skips_a_file_that_changed_while_hashing() {
        let f = Fixture::new();
        f.check();
        let target = f.path(TC, "bin/cargo");
        let before = f.baseline().toolchains[TC]["bin/cargo"].clone();
        let touch = |secs| {
            let file = fs::OpenOptions::new().write(true).open(&target).unwrap();
            file.set_modified(SystemTime::now() + Duration::from_secs(secs)).unwrap();
        };
        touch(3600);
        let racing = |p: &Path| {
            let sha = hash_file(p)?;
            if p == target {
                touch(7200);
            }
            Ok(sha)
        };
        assert_eq!(check_with(&f.store, &f.home, &racing), ToolchainState::Trusted);
        assert_eq!(f.baseline().toolchains[TC]["bin/cargo"], before);
        assert_eq!(f.check(), ToolchainState::Trusted);
        assert_ne!(f.baseline().toolchains[TC]["bin/cargo"].mtime, before.mtime);
    }

    #[test]
    fn a_reverted_file_stays_changed_until_trusted() {
        let f = Fixture::new();
        f.check();
        f.write(TC, "bin/rustc", "rustc 2");
        assert!(matches!(f.check(), ToolchainState::Changed { .. }));
        f.write(TC, "bin/rustc", "rustc 1");
        assert_eq!(changed_paths(&f.check()), vec!["bin/rustc"]);
    }

    #[test]
    fn an_unreadable_baseline_is_a_change() {
        let f = Fixture::new();
        f.store.kv_set(BASELINE_KEY, "{broken").unwrap();
        assert_eq!(changed_paths(&f.check()), vec![BASELINE_UNREADABLE]);
        assert!(seatbelt_denies(&f.check(), &f.home).contains(TC));
    }

    #[test]
    fn the_refusal_names_the_toolchain_and_files() {
        let f = Fixture::new();
        f.check();
        assert_eq!(refusal(&f.check()), None);
        f.write(TC, "bin/rustc", "rustc 2");
        let text = refusal(&f.check()).unwrap();
        assert_eq!(
            text,
            "Heron won't run the Rust toolchain: stable-aarch64-apple-darwin changed since you last trusted it (bin/rustc). Check it, then trust it in Settings › Sandbox."
        );
    }
}
