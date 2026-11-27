//! Operating-system sandbox for commands the agent runs and for the CLI providers it spawns.
//!
//! Pattern matching on a command line can be fooled; the operating system cannot. On macOS every shell
//! command, and every CLI provider child, runs inside a Seatbelt profile built by one function
//! ([`seatbelt_profile`]):
//!   - reads are denied by default under the user roots (`$HOME`, `/Users`, `/Volumes`) and Heron's
//!     own home. The working folder (or the CLI's run folder), folders the user granted and the
//!     toolchain folders builds and git need are opened again; Refused paths and secrets are then
//!     denied again, whatever the command line says;
//!   - writes are allowed only in the working folder, temporary folders and a few tool caches (for a
//!     CLI: only its run folder and its own config folders).
//!
//! Linux uses bubblewrap for the shell when a root-owned `/usr/bin/bwrap` or `/usr/local/bin/bwrap` is
//! installed, with today's rules, and says once that commands run unsandboxed when not. Windows has no
//! equivalent here yet. On macOS a missing sandbox is not a fallback: [`required`] is true, and the
//! callers refuse to run commands or CLIs without one.

use crate::cli_llm::{CliSpec, CredentialStore};
use crate::config::Config;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    None,
    Bubblewrap,
    Seatbelt,
}

impl Kind {
    pub fn name(&self) -> &'static str {
        match self {
            Kind::None => "none",
            Kind::Bubblewrap => "bubblewrap",
            Kind::Seatbelt => "seatbelt",
        }
    }
}

/// The sandbox programs by absolute path, for the probe and for every wrapped command. A bare name is
/// looked up on PATH (Heron's own for the probe, the child's for a command), so a program of the same
/// name earlier on it would run instead.
const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";
/// The only places bubblewrap is taken from. A candidate must also pass [`trusted_program`].
#[cfg(any(target_os = "linux", all(test, unix)))]
const BWRAP_CANDIDATES: &[&str] = &["/usr/bin/bwrap", "/usr/local/bin/bwrap"];

/// What the probe found: the sandbox and the exact program it checked, which [`wrap`] then runs.
struct Probed {
    kind: Kind,
    program: PathBuf,
}

/// The program at `candidate` when it can be trusted: its real path is an executable file, and that
/// file and every folder above it have an owner `owner_ok` accepts (root, in use) and are writable by
/// no group or other user. Anyone who could replace the file, or a folder on its path, could run their
/// own program in place of the sandbox.
#[cfg(any(target_os = "linux", all(test, unix)))]
fn trusted_program(candidate: &Path, owner_ok: impl Fn(u32) -> bool) -> Option<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    let real = std::fs::canonicalize(candidate).ok()?;
    let meta = std::fs::metadata(&real).ok()?;
    if !meta.is_file() || meta.mode() & 0o111 == 0 {
        return None;
    }
    let safe = |p: &Path| std::fs::metadata(p).is_ok_and(|m| owner_ok(m.uid()) && m.mode() & 0o022 == 0);
    real.ancestors().all(safe).then_some(real)
}

fn probe() -> Probed {
    let ok = |program: &Path, args: &[&str]| std::process::Command::new(program).args(args).stdin(std::process::Stdio::null()).output().map(|o| o.status.success()).unwrap_or(false);
    #[cfg(target_os = "linux")]
    {
        let found = BWRAP_CANDIDATES.iter().find_map(|c| trusted_program(Path::new(c), |uid| uid == 0));
        if let Some(program) = found.filter(|p| ok(p, &["--ro-bind", "/", "/", "--dev-bind", "/dev", "/dev", "--tmpfs", "/tmp", "true"])) {
            return Probed { kind: Kind::Bubblewrap, program };
        }
        // Linux runs commands without a sandbox when there is none (docs/designs/heron-v1.md), so say so once.
        eprintln!("heron: no usable bubblewrap owned by root at {}, so shell commands run without a sandbox", BWRAP_CANDIDATES.join(" or "));
    }
    if cfg!(target_os = "macos") && ok(Path::new(SANDBOX_EXEC), &["-p", "(version 1)(allow default)", "/usr/bin/true"]) {
        return Probed { kind: Kind::Seatbelt, program: PathBuf::from(SANDBOX_EXEC) };
    }
    Probed { kind: Kind::None, program: PathBuf::new() }
}

fn probed() -> &'static Probed {
    static PROBED: OnceLock<Probed> = OnceLock::new();
    PROBED.get_or_init(probe)
}

/// Which sandbox this computer offers (checked once).
pub fn available() -> Kind {
    probed().kind
}

/// The sandbox in effect for this configuration.
pub fn active(cfg: &Config) -> Kind {
    let off = cfg.agent.sandbox.eq_ignore_ascii_case("off") || std::env::var("HERON_SANDBOX").map(|v| v.eq_ignore_ascii_case("off")).unwrap_or(false);
    if off { Kind::None } else { available() }
}

/// Must commands and CLIs run inside the sandbox? True on macOS: with the sandbox off there, the shell
/// and CLI providers are refused instead of run unsandboxed (Q11). Elsewhere they run as before.
pub fn required() -> bool {
    cfg!(target_os = "macos")
}

/// The network rules of a shell command under the egress allowlist.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EgressRules {
    /// Loopback ports the command may not reach.
    pub deny_ports: Vec<u16>,
}

/// What a sandbox is for.
#[derive(Debug, Clone)]
pub enum Profile<'a> {
    /// A shell command. `grants` are the folders and single paths the user granted for reading.
    /// `protect` is the scan of the working folder, `egress` the network rules (`None`: open), and
    /// `toolchain` the Rust toolchain's state, whose changed toolchains are read-denied.
    Shell { grants: Vec<ReadGrant>, protect: crate::protect::Scan, egress: Option<EgressRules>, toolchain: crate::toolchain::ToolchainState },
    /// A CLI provider child. `binary_dir` is the install folder of its binary. `extra_read` holds the
    /// interpreter's folder when the binary is a script.
    Cli { spec: &'a CliSpec, binary_dir: PathBuf, extra_read: Vec<PathBuf> },
}

/// The mach services of the Keychain (securityd). Denied to shell commands only, so a command cannot
/// ask securityd for a Keychain item without touching any file. No CLI profile denies them: the
/// Security framework loads the system root certificates through securityd, and without them a CLI
/// such as codex-cli cannot verify TLS.
pub const KEYCHAIN_SERVICES: &[&str] = &["com.apple.SecurityServer", "com.apple.securityd.xpc", "com.apple.security.keychaind", "com.apple.securityd"];

/// The last rule of every profile: no asking LaunchServices or Apple Events to start a program, which
/// would run it outside the sandbox (`open -g -j <app the command wrote>`).
pub const LAUNCH_SERVICES: &str = "(deny mach-lookup (global-name \"com.apple.coreservices.launchservicesd\") (global-name-prefix \"com.apple.lsd.\") (global-name \"com.apple.coreservices.appleevents\") (global-name \"com.apple.coreservices.quarantine-resolver\"))";

/// launchd starts a job submitted with `launchctl submit` outside the submitter's sandbox; it asks the
/// sandbox about `job-creation` first, so this rule refuses the submission.
pub const JOB_CREATION: &str = "(deny job-creation)";

/// Folders named inside a multi-part secret pattern. Each one is write-denied itself, so it cannot be
/// renamed and take a secret out of its pattern (`mv .config c2` defeating `.config/gh/hosts.yml`).
/// New files inside them can still be created.
pub const SEATBELT_SECRET_FOLDERS: &[&str] = &[r"^.*/\.config$", r"^.*/\.config/gh$", r"^.*/\.config/git$", r"^.*/\.config/gcloud$", r"^.*/\.heron$", r"^.*/\.local$", r"^.*/\.local/share$", r"^.*/\.local/share/fish$"];

/// The secret check (`permissions::sensitive_path`) as Seatbelt regular expressions, written in lower
/// case: [`seatbelt_regex`] makes each one case-insensitive, as the original is. Seatbelt sees absolute
/// paths, so `(^|/)` becomes `/`. Left out on purpose: `^/(proc|sys|dev)/` (a read deny on `/dev` would
/// block `/dev/null`, `/dev/urandom` and the terminal) and the `^/etc/...` alternatives (every command
/// reads `/etc`). `seatbelt_secret_patterns_match_the_secret_check` checks the list against the original.
pub const SEATBELT_SECRET_PATTERNS: &[&str] = &[
    r"^.*/(\.ssh|\.aws|\.gnupg|\.kube|\.docker|\.config/gcloud)(/.*)?$",
    r"^.*/\.env(\.[-a-z0-9_.]+)?$",
    r"^.*/(id_rsa|id_ed25519|id_ecdsa)(\.pub)?$",
    r"^.*/\.(netrc|npmrc|pypirc|git-credentials)$",
    r"^.*credentials(\.json)?$",
    r"^.*\.pem$",
    r"^.*\.key$",
    r"^.*/heron\.db(-wal|-shm)?$",
    r"^.*/\.heron/secrets\.json$",
    r"^.*/\.config/gh/hosts\.yml$",
    r"^.*/\.config/git/credentials$",
    r"^.*credentials\.toml$",
    r"^.*/\.(bash|zsh|sh|python|node_repl|psql|mysql|sqlite)_history$",
    r"^.*/\.lesshst$",
    r"^.*/\.zsh_sessions(/.*)?$",
    r"^.*/\.local/share/fish/fish_history$",
    r"^.*/(login data|login data for account|cookies|web data)(-journal)?$",
];

/// Public CA bundles. The `.pem` secret pattern would deny them, and without them TLS fails before any
/// host matters: curl, git and cargo read `/etc/ssl/cert.pem`, npm Homebrew's, and pip certifi's.
pub const CA_BUNDLES: &[&str] = &[
    "/etc/ssl/cert.pem",
    "/private/etc/ssl/cert.pem",
    "/opt/homebrew/etc/ca-certificates/cert.pem",
    "/opt/homebrew/etc/openssl@3/cert.pem",
    "/usr/local/etc/ca-certificates/cert.pem",
    "/usr/local/etc/openssl@3/cert.pem",
];

/// certifi's bundle, wherever a Python environment keeps it.
pub const CERTIFI_BUNDLE: &str = r"^.*/certifi/cacert\.pem$";

/// The read allow for [`CA_BUNDLES`] and [`CERTIFI_BUNDLE`], placed after the secret rules so it wins
/// over `.pem`. Reads only: the bundles stay write-denied.
fn ca_bundles() -> String {
    let paths: Vec<PathBuf> = CA_BUNDLES.iter().map(PathBuf::from).collect();
    format!("(allow file-read*{} (regex #\"{CERTIFI_BUNDLE}\"))\n", literals(&paths))
}

/// Folders under the home folder that builds, package managers and git need to read. Kept in step with
/// `docs/cli-providers.md`. A folder is added only when the build check
/// (`build_and_git_still_work_under_deny_default_reads`) needs it.
pub const TOOLCHAIN_DIRS: &[&str] = &[
    ".config/git",
    ".cargo",
    ".rustup",
    ".npm",
    ".nvm",
    ".local/bin",
    ".local/share/uv",
    ".cache/uv",
    ".cache/pip",
    "Library/Caches/Homebrew",
    "Library/Caches/pip",
];

/// A Seatbelt pattern made case-insensitive: every ASCII letter outside a bracket expression becomes
/// `[xX]`. Inside brackets, a lower-case range gains its upper-case twin.
pub fn seatbelt_regex(pattern: &str) -> String {
    let mut out = String::new();
    let mut chars = pattern.chars().peekable();
    let mut in_bracket = false;
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                out.push(c);
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            }
            '[' if !in_bracket => {
                in_bracket = true;
                out.push(c);
            }
            ']' if in_bracket => {
                in_bracket = false;
                out.push(c);
            }
            'a'..='z' if in_bracket => {
                out.push(c);
                if chars.peek() == Some(&'-') {
                    // A range such as `a-z`: add `A-Z` after it.
                    chars.next();
                    let end = chars.next().unwrap_or(c);
                    out.push('-');
                    out.push(end);
                    out.push(c.to_ascii_uppercase());
                    out.push('-');
                    out.push(end.to_ascii_uppercase());
                } else {
                    out.push(c.to_ascii_uppercase());
                }
            }
            c if c.is_ascii_alphabetic() && !in_bracket => {
                out.push('[');
                out.push(c.to_ascii_lowercase());
                out.push(c.to_ascii_uppercase());
                out.push(']');
            }
            c => out.push(c),
        }
    }
    out
}

fn existing(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    paths.into_iter().filter(|p| p.exists()).map(|p| dunce::canonicalize(&p).unwrap_or(p)).collect()
}

/// Folders a command may write to besides the working folder: caches that builds and package managers need.
fn writable_extras(home: &Path) -> Vec<PathBuf> {
    existing([".cache", ".cargo/registry", ".cargo/git", ".npm", ".rustup/toolchains", "Library/Caches"].iter().map(|d| home.join(d)))
}

/// Folders a command must not even read (the Linux sandbox).
fn hidden(cfg: &Config, home: &Path, workspace: &Path) -> Vec<PathBuf> {
    let mut v = existing([".ssh", ".aws", ".gnupg", ".kube", ".docker", ".config/gcloud", ".netrc", ".git-credentials"].iter().map(|d| home.join(d)).chain([cfg.home.clone()]));
    // Never hide the folder we are working in (or one of its parents).
    v.retain(|p| !workspace.starts_with(p));
    v
}

fn sbpl_quote(p: &Path) -> String {
    format!("\"{}\"", p.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\""))
}

/// A path as written and its canonical form when that differs (`/var/...` and `/private/var/...`), so
/// a rule matches whichever the kernel reports.
pub(crate) fn both_forms(p: &Path) -> Vec<PathBuf> {
    let mut v = vec![p.to_path_buf()];
    if let Ok(real) = dunce::canonicalize(p) {
        if real != p {
            v.push(real);
        }
    }
    v
}

/// The `subpath` filters of one rule: each path as written and canonical, then each of those under its
/// other macOS name (`/System/Volumes/Data/Users/x` for `/Users/x`, and back), so a rule cannot be
/// stepped around through the firmlink alias.
pub(crate) fn subpaths<'a>(paths: impl IntoIterator<Item = &'a PathBuf>) -> String {
    filters("subpath", paths)
}

/// The `literal` filters of one rule, in the same forms as [`subpaths`]: the path itself, never
/// anything under it.
pub(crate) fn literals<'a>(paths: impl IntoIterator<Item = &'a PathBuf>) -> String {
    filters("literal", paths)
}

fn filters<'a>(kind: &str, paths: impl IntoIterator<Item = &'a PathBuf>) -> String {
    let mut seen: Vec<PathBuf> = vec![];
    for p in paths {
        for form in both_forms(p) {
            if !seen.contains(&form) {
                seen.push(form);
            }
        }
    }
    let aliases: Vec<PathBuf> = seen.iter().filter_map(|p| data_volume_alias(p)).collect();
    for alias in aliases {
        if !seen.contains(&alias) {
            seen.push(alias);
        }
    }
    seen.iter().map(|p| format!(" ({kind} {})", sbpl_quote(p))).collect()
}

/// Every ancestor of a Refused path that lies inside a writable folder, the writable folder included.
/// Each gets a write deny of its own, so a command cannot rename it and carry the Refused path out
/// from under its rule.
fn refused_ancestors(refused: &[PathBuf], writable: &[PathBuf]) -> Vec<PathBuf> {
    let writable: Vec<PathBuf> = writable.iter().flat_map(|w| both_forms(w)).collect();
    let mut out: Vec<PathBuf> = vec![];
    for r in refused.iter().flat_map(|r| both_forms(r)) {
        for w in writable.iter().filter(|w| r.starts_with(w) && r != **w) {
            for ancestor in r.ancestors().skip(1).take_while(|a| a.starts_with(w)) {
                if !out.iter().any(|o| o == ancestor) {
                    out.push(ancestor.to_path_buf());
                }
            }
        }
    }
    out
}

/// The other name of an absolute path on macOS: under `/System/Volumes/Data` or out from under it.
pub(crate) fn data_volume_alias(p: &Path) -> Option<PathBuf> {
    if let Some(stripped) = crate::permissions::without_data_volume(p) {
        return Some(stripped);
    }
    let rest = p.strip_prefix("/").ok()?;
    (!rest.as_os_str().is_empty()).then(|| Path::new(crate::permissions::DATA_VOLUME).join(rest))
}

fn tilde(raw: &str, home: &Path) -> PathBuf {
    if raw == "~" {
        home.to_path_buf()
    } else if let Some(rest) = raw.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(raw)
    }
}

/// A CLI's own config folders: its registry `config_dirs`, plus, for a CLI that takes a config-folder
/// variable, that variable's value, and `llm.cli_config_dir` when this CLI is the configured provider.
/// A too-broad value (`~`, `/`, a parent of the home folder, a relative path) is left out: it would
/// open everything under it.
pub fn own_config_dirs(cfg: &Config, spec: &CliSpec, home: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = spec.config_dirs.iter().map(|d| tilde(d, home)).collect();
    if let Some(var) = spec.config_dir_env {
        let mut overrides: Vec<PathBuf> = vec![];
        if let Ok(value) = std::env::var(var) {
            if !value.trim().is_empty() {
                overrides.push(tilde(value.trim(), home));
            }
        }
        if cfg.llm.provider.trim() == spec.id && !cfg.llm.cli_config_dir.trim().is_empty() {
            overrides.push(PathBuf::from(crate::cli_llm::normalize_config_dir(cfg.llm.cli_config_dir.trim())));
        }
        dirs.extend(overrides.into_iter().filter(|d| !crate::permissions::too_broad(d)));
    }
    dirs
}

/// Files the user's git configuration points git at (`core.excludesFile`, `core.attributesFile`, and
/// `include.path` / `includeIf` paths), read from `~/.gitconfig` and `~/.config/git/config`. Git stops
/// with "cannot use ... as an exclude file" when it cannot read the excludes file it was told to use,
/// so these are readable by shell commands; Refused and secret paths are still denied after them. An
/// included file is kept only when `classify_path` finds it neither Refused nor secret.
pub fn git_config_files(cfg: &Config, home: &Path, workdir: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    for config in [home.join(".gitconfig"), home.join(".config/git/config")] {
        let Ok(text) = std::fs::read_to_string(&config) else { continue };
        let mut section = String::new();
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                section = line.trim_start_matches('[').split([']', ' ', '"']).next().unwrap_or("").to_ascii_lowercase();
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let key = key.trim().to_ascii_lowercase();
            let core = section == "core" && (key == "excludesfile" || key == "attributesfile");
            let include = (section == "include" || section == "includeif") && key == "path";
            if !core && !include {
                continue;
            }
            let value = value.split(['#', ';']).next().unwrap_or("").trim().trim_matches('"');
            if value.is_empty() {
                continue;
            }
            let path = tilde(value, home);
            let path = if path.is_absolute() { path } else { config.parent().map(|d| d.join(&path)).unwrap_or(path) };
            if include && matches!(crate::permissions::classify_path(&path, workdir, cfg, false), crate::permissions::Effect::Refused | crate::permissions::Effect::SensitiveRead) {
                continue;
            }
            if !out.contains(&path) {
                out.push(path);
            }
        }
    }
    out
}

/// The temporary folder shell commands get as `TMPDIR`: their own, so the sockets they make there
/// work while other programs' sockets in the user's temporary folder stay out of reach.
pub fn shell_tmpdir() -> PathBuf {
    // On macOS the user's temporary folder is too long for a socket address, so Python's
    // multiprocessing would fall back to `/tmp`, where other programs' sockets are. A short folder
    // of the user's own keeps those sockets in reach of the command and nothing else.
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let uid = unsafe { libc::getuid() };
        let dir = PathBuf::from(format!("/private/tmp/heron-{uid}"));
        let _ = std::fs::create_dir(&dir);
        if let Ok(meta) = std::fs::symlink_metadata(&dir) {
            if meta.is_dir() && meta.uid() == uid && std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).is_ok() {
                return dir;
            }
        }
    }
    let dir = std::env::temp_dir().join("heron-shell");
    let _ = std::fs::create_dir_all(&dir);
    dunce::canonicalize(&dir).unwrap_or(dir)
}

/// Local sockets act with the owner's keys without any key file being read (ssh-agent, gpg-agent,
/// Docker, tmux, password managers). A command may reach only name lookup, the system log, and
/// sockets in the working folder or its own temporary folder (Python's multiprocessing puts its
/// own there too: see `shell_tmpdir`). Never `/tmp/pymp-*`: other programs' multiprocessing servers
/// listen there and run what they are sent.
fn local_sockets(workdir: &Path) -> String {
    let own: String = [workdir.to_path_buf(), shell_tmpdir()].iter().map(|d| format!(" (remote unix-socket (subpath {}))", sbpl_quote(d))).collect();
    format!(
        "(deny network-outbound (remote unix-socket))\n(allow network-outbound (remote unix-socket (path-literal \"/private/var/run/mDNSResponder\")) (remote unix-socket (path-literal \"/private/var/run/syslog\")){own})\n"
    )
}

/// The Seatbelt profile for one command or CLI. Seatbelt applies the last rule that matches, so the
/// order is the design: deny reads under the user roots, allow metadata everywhere, allow the readable
/// set, deny writes except the writable set, deny protected paths inside it (shell only), then deny
/// Refused paths and secrets again (writes too, so nothing is renamed out of its rule), reopen the
/// public CA bundles for reading, the socket and network rules, and last, launching anything outside
/// the sandbox.
pub fn seatbelt_profile(cfg: &Config, home: &Path, workdir: &Path, profile: &Profile) -> String {
    let roots = crate::permissions::ReadRoots::of(cfg);
    let mut p = String::from("(version 1)\n(allow default)\n");
    p.push_str(&format!("(deny file-read*{})\n", subpaths(&[home.to_path_buf(), PathBuf::from("/Users"), PathBuf::from("/Volumes"), cfg.home.clone()])));
    // Path resolution, `realpath` and git need to `stat` parent folders. This shows that a file exists,
    // its size and its time, never its contents.
    p.push_str("(allow file-read-metadata)\n");
    let regexes = |patterns: &[&str]| -> String { patterns.iter().map(|r| format!(" (regex #\"{}\")", seatbelt_regex(r))).collect() };
    let secrets = || format!("(deny file-read*{})\n(deny file-write*{}{})\n", regexes(SEATBELT_SECRET_PATTERNS), regexes(SEATBELT_SECRET_PATTERNS), regexes(SEATBELT_SECRET_FOLDERS));
    let ancestors = |refused: &[PathBuf], writable: &[PathBuf]| {
        let a = refused_ancestors(refused, writable);
        if a.is_empty() { String::new() } else { format!("(deny file-write*{})\n", literals(&a)) }
    };
    let launch = format!("{JOB_CREATION}\n{LAUNCH_SERVICES}\n");
    match profile {
        Profile::Shell { grants, protect, egress, toolchain } => {
            let mut readable = vec![workdir.to_path_buf()];
            readable.extend(grants.iter().filter_map(|g| match g {
                ReadGrant::Folder(f) => Some(f.clone()),
                ReadGrant::Exact(_) => None,
            }));
            readable.extend(TOOLCHAIN_DIRS.iter().map(|d| home.join(d)));
            let mut single = vec![home.join(".gitconfig")];
            single.extend(git_config_files(cfg, home, workdir));
            // A `read=:` grant opens that one path, never anything under it, even when it names a
            // folder (`read=:/`).
            single.extend(grants.iter().filter_map(|g| match g {
                ReadGrant::Exact(e) => Some(e.clone()),
                ReadGrant::Folder(_) => None,
            }));
            p.push_str(&format!("(allow file-read*{}{})\n", subpaths(&readable), literals(&single)));
            p.push_str("(deny file-write*)\n");
            let mut writable = vec![PathBuf::from("/private/tmp"), PathBuf::from("/private/var/folders"), PathBuf::from("/dev"), workdir.to_path_buf()];
            writable.extend(writable_extras(home));
            p.push_str(&format!("(allow file-write*{})\n", subpaths(&writable)));
            let (protect_rules, protected) = crate::protect::seatbelt_rules(workdir, home, &crate::permissions::heron_homes(cfg), protect, &writable);
            p.push_str(&protect_rules);
            p.push_str(&format!("(deny file-read* file-write*{})\n", subpaths(roots.refused_paths())));
            p.push_str(&ancestors(roots.refused_paths(), &writable));
            p.push_str(&secrets());
            p.push_str(&ancestors(&protected, &writable));
            p.push_str(&ca_bundles());
            let names: Vec<String> = KEYCHAIN_SERVICES.iter().map(|n| format!(" (global-name \"{n}\")")).collect();
            p.push_str(&format!("(deny mach-lookup{})\n", names.concat()));
            p.push_str(&local_sockets(workdir));
            if let Some(egress) = egress {
                p.push_str(&crate::egress::seatbelt_rules(&egress.deny_ports, home));
            }
            p.push_str(&crate::toolchain::seatbelt_denies(toolchain, home));
            p.push_str(&launch);
        }
        Profile::Cli { spec, binary_dir, extra_read } => {
            let canonical = |d: &PathBuf| dunce::canonicalize(d).unwrap_or_else(|_| d.clone());
            let mut readable = vec![workdir.to_path_buf(), canonical(binary_dir)];
            readable.extend(extra_read.iter().map(canonical));
            p.push_str(&format!("(allow file-read*{})\n", subpaths(&readable)));
            p.push_str("(deny file-write*)\n");
            let writable = [PathBuf::from("/dev"), workdir.to_path_buf()];
            p.push_str(&format!("(allow file-write*{})\n", subpaths(&writable)));
            let own = own_config_dirs(cfg, spec, home);
            let own_forms: Vec<PathBuf> = own.iter().flat_map(|d| both_forms(d)).collect();
            let refused: Vec<PathBuf> = roots.refused_paths().iter().filter(|r| !own_forms.iter().any(|o| r.starts_with(o))).cloned().collect();
            if !refused.is_empty() {
                p.push_str(&format!("(deny file-read* file-write*{})\n", subpaths(&refused)));
                p.push_str(&ancestors(&refused, &writable));
            }
            p.push_str(&secrets());
            // Only a CLI that keeps its login in the Keychain reaches the Keychain folder. No CLI's
            // securityd lookups are denied: they also load the system root certificates.
            if spec.credential_store == CredentialStore::MacosKeychain {
                p.push_str(&format!("(allow file-read* file-write*{})\n", subpaths(&[home.join("Library/Keychains")])));
            }
            // After the Refused and secret rules, so they win over them: a CLI's own folders are the
            // one exception to Refused, and only inside its own sandbox. A CLI with no folders of its
            // own (custom-cli) gets no rule: a bare `(allow file-read* file-write*)` would open
            // everything.
            if !own.is_empty() {
                p.push_str(&format!("(allow file-read* file-write*{})\n", subpaths(&own)));
            }
            p.push_str(&launch);
        }
    }
    p
}

/// The command prefix that runs a program inside the sandbox, or None when there is none. `workdir` is
/// the working folder for the shell and the run folder for a CLI. The caller appends the program and
/// its arguments. A CLI is wrapped only by Seatbelt: on Linux and Windows it runs as before.
pub fn wrap(cfg: &Config, workdir: &Path, profile: &Profile) -> Option<(String, Vec<String>)> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let home = dunce::canonicalize(&home).unwrap_or(home);
    #[cfg(test)]
    let home = crate::toolchain::TEST_HOME.try_with(|h| h.clone()).unwrap_or(home);
    let s = |p: &Path| p.to_string_lossy().to_string();
    match (active(cfg), profile) {
        (Kind::None, _) | (Kind::Bubblewrap, Profile::Cli { .. }) => None,
        (Kind::Bubblewrap, Profile::Shell { .. }) => {
            let mut a: Vec<String> = ["--die-with-parent", "--ro-bind", "/", "/", "--dev-bind", "/dev", "/dev", "--tmpfs", "/tmp"].iter().map(|x| x.to_string()).collect();
            for dir in writable_extras(&home).iter().chain([&workdir.to_path_buf()]) {
                a.extend(["--bind".to_string(), s(dir), s(dir)]);
            }
            for path in hidden(cfg, &home, workdir) {
                if path.is_dir() {
                    a.extend(["--tmpfs".to_string(), s(&path)]);
                } else {
                    a.extend(["--ro-bind".to_string(), "/dev/null".to_string(), s(&path)]);
                }
            }
            a.extend(["--chdir".to_string(), s(workdir), "--".to_string()]);
            Some((s(&probed().program), a))
        }
        (Kind::Seatbelt, profile) => Some((s(&probed().program), vec!["-p".to_string(), seatbelt_profile(cfg, &home, workdir, profile)])),
    }
}

/// What a shell command may read besides the working folder: the folders of the standing `read:`
/// rules (in the settings file and those saved from approvals) and, while the conversation is not
/// tainted, the `read:` folders and `read=:` single paths of this conversation. Once outside content is
/// present a conversation grant no longer applies, as it no longer applies to a read. A grant made
/// earlier in the same turn that is not saved yet is not seen, so the command fails closed.
pub fn shell_grants(cfg: &Config, store: &crate::store::Store, session_id: &str, tainted: bool) -> Vec<ReadGrant> {
    read_grants(cfg, store, (!tainted).then_some(session_id))
}

/// One thing the user granted for reading.
#[derive(Debug, Clone, PartialEq)]
pub enum ReadGrant {
    /// A `read:` folder rule: the folder and everything under it.
    Folder(PathBuf),
    /// A `read=:` grant: that one path, never anything under it.
    Exact(PathBuf),
}

impl ReadGrant {
    pub fn path(&self) -> &Path {
        match self {
            ReadGrant::Folder(p) | ReadGrant::Exact(p) => p,
        }
    }
}

/// What the user granted for reading: the standing `read:` rules, plus the `read:` and `read=:` grants
/// of one conversation when `session_id` is given. Too-broad folders and relative paths are left out.
pub fn read_grants(cfg: &Config, store: &crate::store::Store, session_id: Option<&str>) -> Vec<ReadGrant> {
    read_grants_from(standing_rules(cfg, store), store, session_id)
}

/// The standing rules: those in the settings, and the saved "always" answers as they are now.
pub fn standing_rules(cfg: &Config, store: &crate::store::Store) -> Vec<String> {
    let mut rules: Vec<String> = cfg.permissions.allow.clone();
    if let Some(saved) = store.kv_get("allow_rules") {
        rules.extend(serde_json::from_str::<Vec<String>>(&saved).unwrap_or_default());
    }
    rules
}

/// [`read_grants`], with the standing rules given.
pub fn read_grants_from(standing: Vec<String>, store: &crate::store::Store, session_id: Option<&str>) -> Vec<ReadGrant> {
    let mut rules = standing;
    if let Some(saved) = session_id.and_then(|s| store.kv_get(&format!("grants:{s}"))) {
        rules.extend(serde_json::from_str::<Vec<String>>(&saved).unwrap_or_default());
    }
    let mut out: Vec<ReadGrant> = vec![];
    for rule in rules {
        let grant = if let Some(file) = rule.strip_prefix("read=:") {
            Some(ReadGrant::Exact(PathBuf::from(file)))
        } else {
            crate::permissions::read_rule_folder(&rule).filter(|f| !crate::permissions::too_broad(f)).map(ReadGrant::Folder)
        };
        if let Some(grant) = grant.filter(|g| g.path().is_absolute()) {
            if !out.contains(&grant) {
                out.push(grant);
            }
        }
    }
    out
}

/// A Seatbelt test that cannot run here: it prints `<why>; skipping`, or panics when
/// `HERON_REQUIRE_SEATBELT=1`, so a skipped Seatbelt test cannot pass in the gate.
#[cfg(test)]
pub(crate) fn skip_seatbelt_test(why: &str) {
    if std::env::var("HERON_REQUIRE_SEATBELT").as_deref() == Ok("1") {
        panic!("HERON_REQUIRE_SEATBELT=1, but {why}");
    }
    eprintln!("{why}; skipping");
}

/// Shown to the model when a sandboxed command fails in a way the sandbox explains.
pub const DENIED_HINT: &str = "[This ran inside Heron's sandbox. Commands can change files only in the working folder and temporary folders. They can read the working folder, system and toolchain folders, and folders the user granted, but not the rest of the home folder, secrets or Heron's own data. To read another file, use read_file, which can ask the user.]";

pub fn looks_denied(output: &str) -> bool {
    ["Read-only file system", "Operation not permitted", "Permission denied"].iter().any(|m| output.contains(m))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seatbelt_secret_patterns_match_the_secret_check() {
        let compiled: Vec<regex::Regex> = SEATBELT_SECRET_PATTERNS.iter().map(|r| regex::Regex::new(&seatbelt_regex(r)).unwrap()).collect();
        let seatbelt = |p: &str| compiled.iter().any(|r| r.is_match(p));
        let cases = [
            "/Users/u/.ssh/id_rsa", "/Users/u/.ssh", "/Users/u/.SSH/config", "/Users/u/.aws/credentials", "/Users/u/.gnupg/x", "/Users/u/.kube/config", "/Users/u/.docker/config.json",
            "/Users/u/.config/gcloud/a", "/Users/u/p/.env", "/Users/u/p/.env.local", "/Users/u/p/.ENV", "/Users/u/p/.envrc", "/Users/u/p/.environment", "/Users/u/id_ed25519.pub", "/Users/u/id_ecdsa",
            "/Users/u/.netrc", "/Users/u/.npmrc", "/Users/u/.pypirc", "/Users/u/.git-credentials", "/Users/u/x/credentials", "/Users/u/x/credentials.json", "/Users/u/x/server.pem", "/Users/u/x/server.KEY",
            "/Users/u/.heron/heron.db", "/Users/u/.heron/heron.db-wal", "/Users/u/.heron/secrets.json", "/Users/u/.config/gh/hosts.yml", "/Users/u/.config/git/credentials", "/Users/u/.cargo/credentials.toml",
            "/Users/u/.bash_history", "/Users/u/.zsh_history", "/Users/u/.python_history", "/Users/u/.lesshst", "/Users/u/.zsh_sessions/abc.history", "/Users/u/.local/share/fish/fish_history",
            "/Users/u/Library/Application Support/Google/Chrome/Default/Login Data", "/Users/u/Library/x/Cookies", "/Users/u/Library/x/Web Data-journal",
            "/Users/u/p/notes.md", "/Users/u/p/src/main.rs", "/Users/u/p/keys.txt", "/Users/u/p/monkey", "/Users/u/p/history.md", "/Users/u/.cargo/config.toml", "/Users/u/p/.github/workflows/ci.yml",
            "/Users/u/p/environment.ts", "/Users/u/p/.envy/x", "/opt/homebrew/bin/git", "/Users/u/p/my.pem.txt", "/Users/u/p/sshd/x",
        ];
        for path in cases {
            assert_eq!(seatbelt(path), crate::permissions::is_secret_path(Path::new(path)), "{path}");
        }
        // The two left out on purpose.
        assert!(crate::permissions::is_secret_path(Path::new("/etc/shadow")) && !seatbelt("/etc/shadow"));
        assert!(!seatbelt("/dev/null") && !seatbelt("/dev/urandom"));
    }

    /// SBPL text with every quoted string removed (honouring `\\` and `\"` escapes), so what is left
    /// is the rules themselves.
    fn sbpl_outside_strings(text: &str) -> String {
        let (mut out, mut chars, mut quoted) = (String::new(), text.chars(), false);
        while let Some(c) = chars.next() {
            match (quoted, c) {
                (true, '\\') => {
                    chars.next();
                }
                (_, '"') => quoted = !quoted,
                (false, c) => out.push(c),
                (true, _) => {}
            }
        }
        out
    }

    #[test]
    fn a_quote_in_the_working_folder_cannot_add_rules_to_the_socket_deny() {
        let workdir = Path::new("/tmp/w\") (allow default) (x \"");
        let text = local_sockets(workdir);
        assert!(text.contains(&sbpl_quote(workdir)), "{text}");
        assert!(!sbpl_outside_strings(&text).contains("allow default"), "the folder name became a rule:\n{text}");
        assert!(sbpl_outside_strings(&format!("(subpath \"{}\")", workdir.display())).contains("allow default"), "the check sees an injection");
    }

    #[test]
    fn seatbelt_regex_is_case_insensitive_outside_brackets() {
        assert_eq!(seatbelt_regex(r"^.*/\.env$"), r"^.*/\.[eE][nN][vV]$");
        assert_eq!(seatbelt_regex(r"[-a-z0-9_.]+"), r"[-a-zA-Z0-9_.]+");
        assert_eq!(seatbelt_regex(r"[^/]"), r"[^/]");
    }

    fn cli_spec(id: &str) -> CliSpec {
        crate::cli_llm::spec_for(id).unwrap()
    }

    #[test]
    fn git_config_files_are_the_excludes_attributes_and_include_paths() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        // `/srv/attrs` has no drive on Windows, so a git config there names it with one.
        let attrs = if cfg!(windows) { "C:/srv/attrs" } else { "/srv/attrs" };
        std::fs::write(
            h.join(".gitconfig"),
            format!("[user]\n\tname = x\n[core]\n\texcludesfile = ~/.gitignore_global\n\tattributesFile = \"{attrs}\" # comment\n\teditor = vim\n[include]\n\tpath = extra.inc\n[includeIf \"gitdir:~/work/\"]\n\tpath = ~/.gitconfig-work\n[credential]\n\thelper = osxkeychain\n"),
        )
        .unwrap();
        let cfg = Config { home: h.join(".heron-test-home"), ..Default::default() };
        let ws = h.join("project");
        assert_eq!(git_config_files(&cfg, h, &ws), vec![h.join(".gitignore_global"), PathBuf::from(attrs), h.join("extra.inc"), h.join(".gitconfig-work")]);
        assert!(git_config_files(&cfg, &h.join("nowhere"), &ws).is_empty());
        // An include that names a secret or a Refused file is not opened.
        let real = real_home();
        std::fs::write(
            h.join(".gitconfig"),
            format!("[include]\n\tpath = ~/.ssh/config\n\tpath = {}/.claude/settings.json\n\tpath = {}/hosts.inc\n\tpath = plain.inc\n", real.display(), cfg.home.display()),
        )
        .unwrap();
        assert_eq!(git_config_files(&cfg, h, &ws), vec![h.join("plain.inc")]);
    }

    /// The real home folder: the Refused list is built from it.
    fn real_home() -> PathBuf {
        let h = dirs::home_dir().unwrap();
        dunce::canonicalize(&h).unwrap_or(h)
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn shell_profile_has_the_designed_order() {
        let home = real_home();
        let h = home.display().to_string();
        let cfg = Config { home: home.join(".heron-profile-test"), ..Default::default() };
        let profile = seatbelt_profile(&cfg, &home, &home.join("project-profile-test"), &Profile::Shell { grants: vec![ReadGrant::Folder(home.join("Documents/notes-profile-test"))], protect: Default::default(), egress: None, toolchain: Default::default() });
        let at = |needle: &str| profile.find(needle).unwrap_or_else(|| panic!("{needle} missing from:\n{profile}"));
        let deny_roots = at(&format!("(deny file-read* (subpath \"{h}\") (subpath \"/Users\") (subpath \"/Volumes\") (subpath \"{h}/.heron-profile-test\") (subpath \"/System/Volumes/Data{h}\") (subpath \"/System/Volumes/Data/Users\") (subpath \"/System/Volumes/Data/Volumes\")"));
        let metadata = at("(allow file-read-metadata)");
        let readable = at(&format!("(allow file-read* (subpath \"{h}/project-profile-test\") (subpath \"{h}/Documents/notes-profile-test\") (subpath \"{h}/.config/git\")"));
        assert!(profile[readable..].contains(&format!("(literal \"{h}/.gitconfig\")")));
        let deny_write = at("(deny file-write*)");
        let write = at(&format!("(allow file-write* (subpath \"/private/tmp\") (subpath \"/private/var/folders\") (subpath \"/dev\") (subpath \"{h}/project-profile-test\")"));
        let refused = at(&format!("(deny file-read* file-write* (subpath \"{h}/.heron-profile-test\")"));
        let secrets = at("(deny file-read* (regex #\"");
        let secret_writes = secrets + profile[secrets..].find("(deny file-write* (regex #\"").expect("secret writes");
        let keychain = at("(deny mach-lookup (global-name \"com.apple.SecurityServer\")");
        let launch = at(LAUNCH_SERVICES);
        assert!(
            deny_roots < metadata && metadata < readable && readable < deny_write && deny_write < write && write < refused && refused < secrets && secrets < secret_writes && secret_writes < keychain && keychain < launch,
            "{profile}"
        );
        assert!(profile.ends_with(&format!("{JOB_CREATION}\n{LAUNCH_SERVICES}\n")), "the launch rules come last: {profile}");
        assert!(profile[refused..secrets].contains(&format!("{h}/.claude\"")) && profile[refused..secrets].contains(&format!("{h}/Library/Keychains\"")), "every Refused path is denied: {profile}");
        // ...by its firmlink alias too (`/System/Volumes/Data/Users/<me>/.claude` is `~/.claude`).
        for refused_alias in [format!("(subpath \"/System/Volumes/Data{h}/.claude\")"), format!("(subpath \"/System/Volumes/Data{h}/.heron-profile-test\")")] {
            assert!(profile[refused..secrets].contains(&refused_alias), "{refused_alias} missing from the Refused block: {profile}");
        }
        assert!(profile.starts_with("(version 1)\n(allow default)\n"));
    }

    #[test]
    fn data_volume_alias_goes_both_ways() {
        assert_eq!(data_volume_alias(Path::new("/Users/me/.codex")), Some(PathBuf::from("/System/Volumes/Data/Users/me/.codex")));
        assert_eq!(data_volume_alias(Path::new("/System/Volumes/Data/Users/me/.codex")), Some(PathBuf::from("/Users/me/.codex")));
        assert_eq!(data_volume_alias(Path::new("/")), None);
        assert_eq!(data_volume_alias(Path::new("relative/path")), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn cli_profile_is_narrower_and_opens_only_its_own_config() {
        let home = real_home();
        let h = home.display().to_string();
        let cfg = Config { home: home.join(".heron-profile-test"), ..Default::default() };
        let run = PathBuf::from("/private/var/folders/xy/T/heron-cli-1");
        let codex = cli_spec("codex-cli");
        let profile = seatbelt_profile(&cfg, &home, &run, &Profile::Cli { spec: &codex, binary_dir: PathBuf::from("/opt/homebrew/lib/node_modules/@openai/codex"), extra_read: vec![] });
        let at = |needle: &str| profile.find(needle).unwrap_or_else(|| panic!("{needle} missing from:\n{profile}"));
        at("(allow file-read* (subpath \"/private/var/folders/xy/T/heron-cli-1\") (subpath \"/opt/homebrew/lib/node_modules/@openai/codex\") (subpath \"/System/Volumes/Data/private/var/folders/xy/T/heron-cli-1\")");
        let write = at("(allow file-write* (subpath \"/dev\") (subpath \"/private/var/folders/xy/T/heron-cli-1\") (subpath \"/System/Volumes/Data/dev\")");
        let refused = at(&format!("(deny file-read* file-write* (subpath \"{h}/.heron-profile-test\")"));
        let secrets = at("(deny file-read* (regex #\"");
        let secret_writes = secrets + profile[secrets..].find("(deny file-write* (regex #\"").expect("secret writes");
        let own = at(&format!("(allow file-read* file-write* (subpath \"{h}/.codex\")"));
        let launch = at(LAUNCH_SERVICES);
        assert!(write < refused && refused < secrets && secrets < secret_writes && secret_writes < own && own < launch, "{profile}");
        assert!(profile.ends_with(&format!("{JOB_CREATION}\n{LAUNCH_SERVICES}\n")), "the launch rules come last: {profile}");
        // A CLI that keeps its login in its own folder: no Keychain folder, and no securityd deny.
        assert!(!profile.lines().any(|l| l.starts_with("(allow") && l.contains("Library/Keychains")), "{profile}");
        for name in KEYCHAIN_SERVICES {
            assert!(!profile.contains(name), "{name} denied to a CLI:\n{profile}");
        }
        // Its own folder is not in the Refused block; another CLI's is.
        assert!(!profile[refused..secrets].contains(&format!("{h}/.codex\"")) && profile[refused..secrets].contains(&format!("{h}/.claude\"")), "{profile}");
        // No grants, toolchain folders, .gitconfig or shared temp folders.
        for absent in [".gitconfig", "/private/tmp\"", "/.cargo", "/.rustup"] {
            assert!(!profile.contains(absent), "{absent} in:\n{profile}");
        }
        // A Keychain CLI gets the Keychain folder, and no securityd deny either.
        let claude = cli_spec("claude-cli");
        let profile = seatbelt_profile(&cfg, &home, &run, &Profile::Cli { spec: &claude, binary_dir: home.join(".local/share/claude/versions"), extra_read: vec![] });
        for name in KEYCHAIN_SERVICES {
            assert!(!profile.contains(name), "{name} denied to a CLI:\n{profile}");
        }
        let keychain = profile.find(&format!("(allow file-read* file-write* (subpath \"{h}/Library/Keychains\")")).expect("keychain allow");
        let own = profile.find(&format!("(allow file-read* file-write* (subpath \"{h}/.claude\") (subpath \"{h}/.claude.json\")")).expect("own config");
        assert!(profile.find("(deny file-read* file-write*").unwrap() < keychain && keychain < own, "{profile}");
        assert!(profile.ends_with(&format!("{JOB_CREATION}\n{LAUNCH_SERVICES}\n")), "{profile}");
    }

    /// Every allow or deny in a profile names at least one path: Seatbelt reads a rule with no filter
    /// as "everything", so an empty list must drop the rule, never leave it bare.
    fn no_bare_file_rules(profile: &str) {
        for line in profile.lines() {
            let bare = ["(allow file-read*)", "(allow file-write*)", "(allow file-read* file-write*)", "(deny file-read* file-write*)"];
            assert!(!bare.contains(&line.trim()), "bare rule `{line}` in:\n{profile}");
        }
    }

    #[test]
    fn a_cli_with_no_config_folders_gets_no_open_rule() {
        let home = real_home();
        let cfg = Config { home: home.join(".heron-profile-test"), ..Default::default() };
        let custom = cli_spec("custom-cli");
        assert!(own_config_dirs(&cfg, &custom, &home).is_empty(), "custom-cli has no folders of its own");
        let profile = seatbelt_profile(&cfg, &home, Path::new("/private/var/folders/xy/T/heron-cli-2"), &Profile::Cli { spec: &custom, binary_dir: PathBuf::from("/usr/local/bin"), extra_read: vec![] });
        no_bare_file_rules(&profile);
        // The last file rule is still a deny: nothing after the secrets reopens the home folder.
        let last_allow = profile.rfind("(allow file-").unwrap();
        assert!(last_allow < profile.find("(deny file-read* (regex #\"").unwrap(), "{profile}");
        // The other profiles have no bare rule either.
        for id in ["claude-cli", "codex-cli", "gemini-cli", "opencode-cli"] {
            let spec = cli_spec(id);
            no_bare_file_rules(&seatbelt_profile(&cfg, &home, Path::new("/tmp/run"), &Profile::Cli { spec: &spec, binary_dir: PathBuf::from("/usr/local/bin"), extra_read: vec![] }));
        }
        no_bare_file_rules(&seatbelt_profile(&cfg, &home, Path::new("/tmp/ws"), &Profile::Shell { grants: vec![], protect: Default::default(), egress: None, toolchain: Default::default() }));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn shell_profile_refuses_every_registry_config_folder() {
        // The text half of `every_registry_config_dir_is_refused_in_the_sandbox`, which runs only
        // under Seatbelt: every CLI's config folder is in the Refused block, present on disk or not.
        let home = real_home();
        let cfg = Config { home: home.join(".heron-profile-test"), ..Default::default() };
        let profile = seatbelt_profile(&cfg, &home, &home.join("project-profile-test"), &Profile::Shell { grants: vec![], protect: Default::default(), egress: None, toolchain: Default::default() });
        let refused = &profile[profile.find("(deny file-read* file-write*").unwrap()..profile.find("(deny file-read* (regex #\"").unwrap()];
        for spec in crate::cli_llm::providers() {
            for d in spec.config_dirs {
                let path = tilde(d, &home);
                assert!(refused.contains(&format!("(subpath {})", sbpl_quote(&path))), "{} ({}) missing from the Refused block:\n{refused}", path.display(), spec.id);
            }
        }
    }

    #[test]
    fn a_cli_is_wrapped_only_by_seatbelt() {
        let cfg = Config { agent: crate::config::AgentConfig { sandbox: "off".into(), ..Default::default() }, ..Default::default() };
        let spec = cli_spec("codex-cli");
        assert!(wrap(&cfg, Path::new("/tmp"), &Profile::Cli { spec: &spec, binary_dir: PathBuf::from("/usr/bin"), extra_read: vec![] }).is_none());
        assert!(wrap(&cfg, Path::new("/tmp"), &Profile::Shell { grants: vec![], protect: Default::default(), egress: None, toolchain: Default::default() }).is_none());
        let on = Config::default();
        let wrapped = wrap(&on, Path::new("/tmp"), &Profile::Cli { spec: &spec, binary_dir: PathBuf::from("/usr/bin"), extra_read: vec![] });
        match available() {
            Kind::Seatbelt => assert_eq!(wrapped.unwrap().0, "/usr/bin/sandbox-exec"),
            Kind::Bubblewrap | Kind::None => assert!(wrapped.is_none(), "a CLI is never wrapped by bubblewrap"),
        }
        assert_eq!(required(), cfg!(target_os = "macos"));
    }

    #[test]
    #[cfg(unix)]
    fn a_sandbox_program_is_trusted_only_when_no_one_else_can_replace_it() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let me = unsafe { libc::geteuid() };
        let mine_or_root = |uid: u32| uid == 0 || uid == me;
        let chmod = |p: &Path, mode: u32| std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
        // The system temporary folder is writable by everyone on Linux, so the fixture lives in target/.
        let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
        std::fs::create_dir_all(&target).unwrap();
        let dir = tempfile::tempdir_in(&target).unwrap();
        let bin = dir.path().join("bwrap");
        std::fs::write(&bin, "#!/bin/sh\n").unwrap();
        chmod(&bin, 0o755);
        let real = std::fs::canonicalize(&bin).unwrap();
        assert_eq!(trusted_program(&bin, mine_or_root), Some(real.clone()), "a file only its owner can change is trusted");
        assert_eq!(trusted_program(&bin, |uid| uid == 0), None, "a file the user owns is not root's");
        let link = dir.path().join("link");
        symlink(&bin, &link).unwrap();
        assert_eq!(trusted_program(&link, mine_or_root), Some(real.clone()), "a link is judged by its target");
        chmod(&bin, 0o757);
        assert_eq!(trusted_program(&bin, mine_or_root), None, "other users can write the file");
        chmod(&bin, 0o775);
        assert_eq!(trusted_program(&bin, mine_or_root), None, "the group can write the file");
        chmod(&bin, 0o644);
        assert_eq!(trusted_program(&bin, mine_or_root), None, "the file is not executable");
        chmod(&bin, 0o755);
        chmod(dir.path(), 0o777);
        assert_eq!(trusted_program(&bin, mine_or_root), None, "other users can replace the file through its folder");
        chmod(dir.path(), 0o700);
        assert_eq!(trusted_program(dir.path(), mine_or_root), None, "a folder is not a program");
        assert_eq!(trusted_program(&dir.path().join("missing"), mine_or_root), None);
        assert!(BWRAP_CANDIDATES.iter().all(|c| Path::new(c).is_absolute()));
    }

    #[test]
    #[cfg(unix)]
    fn wrap_runs_the_program_the_probe_checked() {
        let wrapped = wrap(&Config::default(), Path::new("/tmp"), &Profile::Shell { grants: vec![], protect: Default::default(), egress: None, toolchain: Default::default() });
        match available() {
            Kind::None => assert!(wrapped.is_none()),
            kind => {
                let program = wrapped.expect("a sandbox is available").0;
                assert_eq!(Path::new(&program), probed().program.as_path());
                if kind == Kind::Bubblewrap {
                    let trusted: Vec<PathBuf> = BWRAP_CANDIDATES.iter().filter_map(|c| trusted_program(Path::new(c), |uid| uid == 0)).collect();
                    assert!(trusted.contains(&PathBuf::from(&program)), "{program} is not a trusted bubblewrap ({trusted:?})");
                } else {
                    assert_eq!(program, "/usr/bin/sandbox-exec");
                }
            }
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn a_sandbox_exec_earlier_on_the_childs_path_never_runs() {
        use std::os::unix::fs::PermissionsExt;
        let cfg = Config::default();
        if active(&cfg) != Kind::Seatbelt {
            return skip_seatbelt_test(&format!("Seatbelt is not in effect here ({})", active(&cfg).name()));
        }
        let fake_dir = tempfile::tempdir().unwrap();
        let ran = fake_dir.path().join("fake-ran");
        let fake = fake_dir.path().join("sandbox-exec");
        std::fs::write(&fake, format!("#!/bin/sh\ntouch '{}'\necho fake\n", ran.display())).unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let ws = tempfile::tempdir().unwrap();
        let ws = dunce::canonicalize(ws.path()).unwrap();
        let (program, prefix) = wrap(&cfg, &ws, &Profile::Shell { grants: vec![], protect: Default::default(), egress: None, toolchain: Default::default() }).expect("Seatbelt wraps a shell command");
        // The callers clear the environment and hand the child an allowlisted PATH, and the program
        // is looked up on that PATH.
        let out = std::process::Command::new(&program)
            .args(&prefix)
            .args(["/bin/echo", "real"])
            .current_dir(&ws)
            .env_clear()
            .env("PATH", format!("{}:/usr/bin:/bin", fake_dir.path().display()))
            .output()
            .unwrap();
        assert!(!ran.exists(), "the sandbox-exec on PATH ran instead of the system one (program {program:?})");
        assert_eq!(String::from_utf8_lossy(&out.stdout), "real\n", "{out:?}");
    }

    #[test]
    fn shell_grants_come_from_standing_rules_and_this_conversation() {
        let store = crate::store::Store::in_memory().unwrap();
        // A grant must be an absolute path, and `/srv/x` has no drive on Windows, so a Windows fixture
        // names one.
        let abs = |p: &str| if cfg!(windows) { format!("C:{p}") } else { p.to_string() };
        let mut cfg = Config::default();
        cfg.permissions.allow = vec![format!("read:{}/**", abs("/srv/docs")), "shell:git *".into(), "read:/**".into()];
        store.kv_set("allow_rules", &serde_json::to_string(&[format!("read:{}/**", abs("/srv/saved"))]).unwrap()).unwrap();
        store.kv_set("grants:s1", &serde_json::to_string(&[format!("read:{}/**", abs("/srv/talk")), format!("read=:{}", abs("/srv/one/file.txt")), "write_file:*".to_string()]).unwrap()).unwrap();
        store.kv_set("grants:s2", &serde_json::to_string(&[format!("read:{}/**", abs("/srv/other"))]).unwrap()).unwrap();
        let grants = shell_grants(&cfg, &store, "s1", false);
        assert_eq!(
            grants,
            vec![
                ReadGrant::Folder(PathBuf::from(abs("/srv/docs"))),
                ReadGrant::Folder(PathBuf::from(abs("/srv/saved"))),
                ReadGrant::Folder(PathBuf::from(abs("/srv/talk"))),
                ReadGrant::Exact(PathBuf::from(abs("/srv/one/file.txt"))),
            ],
            "a too-broad rule and another conversation's grant are left out"
        );
    }

    #[test]
    fn own_config_dirs_take_the_configured_folder_only_for_its_provider_and_never_a_broad_one() {
        let home = real_home();
        let codex = cli_spec("codex-cli");
        let acct = home.join("heron-own-dirs-test/acct");
        let mut cfg = Config { home: home.join(".heron-profile-test"), ..Default::default() };
        cfg.llm.cli_config_dir = acct.display().to_string();
        cfg.llm.provider = "claude-cli".into();
        assert!(!own_config_dirs(&cfg, &codex, &home).contains(&acct), "claude-cli's account folder is not codex-cli's");
        assert!(own_config_dirs(&cfg, &cli_spec("claude-cli"), &home).contains(&acct));
        cfg.llm.provider = "codex-cli".into();
        assert!(own_config_dirs(&cfg, &codex, &home).contains(&acct));
        // `/Users` is the folder a macOS home folder sits in. On Windows it has no drive, so it becomes
        // a folder of the current drive, which is not the profile's parent: `home.parent()` is that case.
        let mut broad_folders = vec!["~", "/", home.parent().unwrap().to_str().unwrap()];
        if !cfg!(windows) {
            broad_folders.push("/Users");
        }
        for broad in broad_folders {
            cfg.llm.cli_config_dir = broad.into();
            let normalized = PathBuf::from(crate::cli_llm::normalize_config_dir(broad));
            assert!(!own_config_dirs(&cfg, &codex, &home).contains(&normalized), "{broad} must not become a CLI's own folder");
            let profile = seatbelt_profile(&cfg, &home, Path::new("/private/var/folders/xy/T/heron-cli-3"), &Profile::Cli { spec: &codex, binary_dir: PathBuf::from("/usr/local/bin"), extra_read: vec![] });
            let opened = profile.lines().find(|l| l.starts_with("(allow file-read* file-write*")).unwrap();
            assert!(!opened.contains(&format!("(subpath {})", sbpl_quote(&normalized))), "{broad}: {opened}");
        }
    }

    #[test]
    #[cfg_attr(not(unix), ignore = "the toolchain check runs only on unix")]
    fn changed_toolchain_is_denied_in_the_profile() {
        let home = PathBuf::from("/Users/someone");
        let ws = home.join("project-toolchain-test");
        let cfg = Config { home: home.join(".heron"), ..Default::default() };
        let changed = crate::toolchain::ToolchainState::Changed { toolchains: vec!["stable-aarch64-apple-darwin".into()], files: vec![], since: 1, during: None };
        let shell = |toolchain| seatbelt_profile(&cfg, &home, &ws, &Profile::Shell { grants: vec![], protect: Default::default(), egress: None, toolchain });
        let profile = shell(changed);
        let deny = format!("(deny file-read* (subpath {})", sbpl_quote(&home.join(".rustup/toolchains/stable-aarch64-apple-darwin")));
        let at = profile.find(&deny).unwrap_or_else(|| panic!("no toolchain deny in:\n{profile}"));
        let allow = profile.find(&format!("(subpath {})", sbpl_quote(&home.join(".rustup")))).unwrap();
        assert!(allow < at, "the deny must come after the toolchain folders are opened");
        assert!(at < profile.find(JOB_CREATION).unwrap(), "the deny comes before the launch rules");
        assert!(!shell(crate::toolchain::ToolchainState::Trusted).contains(".rustup/toolchains/stable"));
        assert!(!shell(crate::toolchain::ToolchainState::NoBaseline).contains(".rustup/toolchains/stable"));
    }

    #[test]
    fn cli_profile_has_no_egress_rules() {
        let home = PathBuf::from("/Users/someone");
        let cfg = Config { home: home.join(".heron"), ..Default::default() };
        let codex = cli_spec("codex-cli");
        let cli = seatbelt_profile(&cfg, &home, Path::new("/private/var/folders/xy/T/heron-cli-4"), &Profile::Cli { spec: &codex, binary_dir: PathBuf::from("/usr/local/bin"), extra_read: vec![] });
        assert!(!cli.contains("network-outbound") && !cli.contains("remote ip"), "{cli}");
        assert!(!cli.contains(".rustup/toolchains/"), "{cli}");
        let shell = seatbelt_profile(&cfg, &home, &home.join("p"), &Profile::Shell { grants: vec![], protect: Default::default(), egress: None, toolchain: Default::default() });
        assert!(!shell.contains("(remote ip"), "egress None leaves the shell's network open: {shell}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn secrets_and_refused_ancestors_are_write_denied_in_both_profiles() {
        // The text half of `secrets_and_refused_paths_cannot_be_renamed_out_of_their_rules`.
        let home = real_home();
        let ws = home.join("project-rename-test");
        let mut cfg = Config { home: ws.join("data/heron"), ..Default::default() };
        cfg.llm.cli_config_dir = ws.join("a/b/acct").display().to_string();
        let shell = seatbelt_profile(&cfg, &home, &ws, &Profile::Shell { grants: vec![], protect: Default::default(), egress: None, toolchain: Default::default() });
        let run = ws.clone();
        let codex = cli_spec("codex-cli");
        let cli = seatbelt_profile(&cfg, &home, &run, &Profile::Cli { spec: &codex, binary_dir: PathBuf::from("/usr/local/bin"), extra_read: vec![] });
        let w = ws.display().to_string();
        for profile in [&shell, &cli] {
            let line = |start: &str, has: &str| profile.lines().find(|l| l.starts_with(start) && l.contains(has)).unwrap_or_else(|| panic!("{start} with {has} missing from:\n{profile}")).to_string();
            let secret_writes = line("(deny file-write* (regex #\"", &seatbelt_regex(SEATBELT_SECRET_PATTERNS[0]));
            for pattern in SEATBELT_SECRET_PATTERNS.iter().chain(SEATBELT_SECRET_FOLDERS) {
                assert!(secret_writes.contains(&format!("(regex #\"{}\")", seatbelt_regex(pattern))), "{pattern} is not write-denied: {secret_writes}");
            }
            let ancestors = line("(deny file-write* (literal", &format!("(literal \"{w}/a/b\")"));
            for a in [format!("{w}/a/b"), format!("{w}/a"), w.clone(), format!("{w}/data")] {
                assert!(ancestors.contains(&format!("(literal \"{a}\")")), "{a} can be renamed: {ancestors}");
            }
            // Only ancestors: never the Refused folder's contents, nor the home folder above the writable one.
            assert!(!ancestors.contains(&format!("(literal \"{w}/a/b/acct\")")) && !ancestors.contains(&format!("(literal \"{}\")", home.display())), "{ancestors}");
        }
        let compiled: Vec<regex::Regex> = SEATBELT_SECRET_FOLDERS.iter().map(|r| regex::Regex::new(&seatbelt_regex(r)).unwrap()).collect();
        let folder = |p: &str| compiled.iter().any(|r| r.is_match(p));
        for p in ["/u/p/.config", "/u/p/.CONFIG/gh", "/u/p/.config/git", "/u/p/.config/gcloud", "/u/.heron", "/u/.local", "/u/.local/share", "/u/.local/share/fish"] {
            assert!(folder(p), "{p}");
        }
        for p in ["/u/p/.config/new.txt", "/u/p/.config/gh/new.txt", "/u/p/.configs", "/u/p/x.config", "/u/.local/bin"] {
            assert!(!folder(p), "{p}: new files inside the folders stay writable");
        }
    }

    fn shell_with(cfg: &Config, home: &Path, ws: &Path, protect: crate::protect::Scan, egress: Option<EgressRules>) -> String {
        seatbelt_profile(cfg, home, ws, &Profile::Shell { grants: vec![], protect, egress, toolchain: Default::default() })
    }

    #[cfg(unix)]
    #[test]
    fn shell_profile_denies_protected_writes_after_the_writable_allowance() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dunce::canonicalize(dir.path()).unwrap().join("project");
        std::fs::create_dir_all(ws.join("app/.git/hooks")).unwrap();
        std::fs::write(ws.join("app/.git/config"), "[core]\n").unwrap();
        let home = PathBuf::from("/Users/someone");
        let cfg = Config { home: home.join(".heron"), ..Default::default() };
        let scan = crate::protect::scan(&ws);
        assert_eq!(scan.repos.len(), 1);
        let profile = shell_with(&cfg, &home, &ws, scan, None);
        let at = |needle: &str| profile.find(needle).unwrap_or_else(|| panic!("{needle} missing from:\n{profile}"));
        let w = ws.display().to_string();
        let write = at(&format!("(allow file-write* (subpath \"/private/tmp\") (subpath \"/private/var/folders\") (subpath \"/dev\") (subpath \"{w}\")"));
        let names = at(&format!(" (regex #\"{}\")", seatbelt_regex(&format!("^{}/(.*/)?\\.envrc$", crate::protect::regex_escape(&w)))));
        let hooks = at(&seatbelt_regex(r"(.*/)?\.git/(.*/)?hooks/.+$"));
        let samples = at(&seatbelt_regex(r"(.*/)?\.git/(.*/)?hooks/[^/]+\.sample$"));
        let config = at(&format!("(literal \"{w}/app/.git/config\")"));
        let refused = at("(deny file-read* file-write* (subpath");
        let secrets = at("(deny file-read* (regex #\"");
        let after_secrets = &profile[secrets..];
        let repo_ancestors = secrets + after_secrets.find(&format!("(literal \"{w}/app\")")).unwrap_or_else(|| panic!("no rename deny over the repository:\n{profile}"));
        let launch = at(JOB_CREATION);
        assert!(write < names && names < hooks && hooks < samples && samples < config && config < refused && refused < secrets && secrets < repo_ancestors && repo_ancestors < launch, "{profile}");
        let line_of = |i: usize| profile[..i].rfind('\n').map_or(0, |n| n + 1);
        assert!(profile[line_of(repo_ancestors)..].starts_with("(deny file-write* (literal"), "{profile}");
        assert!(profile[line_of(samples)..].starts_with("(allow file-write*"), "only git's samples are allowed back: {profile}");
        let no_repo = shell_with(&cfg, &home, &ws, Default::default(), None);
        assert!(!no_repo.contains(&format!("(literal \"{w}/app/.git/config\")")), "the config literal comes from the scan: {no_repo}");
    }

    #[test]
    fn egress_off_leaves_network_rules_out() {
        let home = PathBuf::from("/Users/someone");
        let ws = home.join("project-egress-test");
        let cfg = Config { home: home.join(".heron"), ..Default::default() };
        let open = shell_with(&cfg, &home, &ws, Default::default(), None);
        for rule in ["(remote ip", "/private/var/run/mDNSResponder\")))", ".orbstack"] {
            assert!(!open.contains(rule), "{rule} with egress off:\n{open}");
        }
        assert!(open.contains("(allow network-outbound (remote unix-socket (path-literal \"/private/var/run/mDNSResponder\"))"), "#79's socket rules stay: {open}");
        let filtered = shell_with(&cfg, &home, &ws, Default::default(), Some(EgressRules { deny_ports: vec![7878, 9000] }));
        for rule in [
            "(deny network-outbound (remote ip \"*:*\"))",
            "(allow network-outbound (remote ip \"localhost:*\"))",
            "(deny network-outbound (remote ip \"localhost:7878\"))",
            "(deny network-outbound (remote ip \"localhost:9000\"))",
            "(deny network-outbound (remote unix-socket (path-literal \"/private/var/run/mDNSResponder\")))",
        ] {
            assert!(filtered.contains(rule), "{rule} missing:\n{filtered}");
        }
    }

    #[test]
    fn egress_rules_come_after_local_socket_rules() {
        let home = PathBuf::from("/Users/someone");
        let cfg = Config { home: home.join(".heron"), ..Default::default() };
        let profile = shell_with(&cfg, &home, &home.join("project-egress-test"), Default::default(), Some(EgressRules { deny_ports: vec![7878] }));
        let at = |needle: &str| profile.find(needle).unwrap_or_else(|| panic!("{needle} missing from:\n{profile}"));
        let own_sockets = at("(deny network-outbound (remote unix-socket))\n");
        let resolver_allowed = at("(allow network-outbound (remote unix-socket (path-literal \"/private/var/run/mDNSResponder\"))");
        let ip_deny = at("(deny network-outbound (remote ip \"*:*\"))");
        let loopback = at("(allow network-outbound (remote ip \"localhost:*\"))");
        let api_port = at("(deny network-outbound (remote ip \"localhost:7878\"))");
        let resolver_denied = at("(deny network-outbound (remote unix-socket (path-literal \"/private/var/run/mDNSResponder\")))");
        let docker = at("(deny network-outbound (remote unix-socket (subpath \"/var/run/docker.sock\")))");
        let launch = at(JOB_CREATION);
        assert!(own_sockets < resolver_allowed && resolver_allowed < ip_deny && ip_deny < loopback && loopback < api_port && api_port < resolver_denied && resolver_denied < docker && docker < launch, "{profile}");
        assert!(profile.ends_with(&format!("{JOB_CREATION}\n{LAUNCH_SERVICES}\n")), "{profile}");
    }
}
