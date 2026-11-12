//! Operating-system sandbox for commands the agent runs.
//!
//! Pattern matching on a command line can be fooled; the operating system cannot. Where a sandbox is
//! available, every shell command runs inside one that:
//!   - can change files only in the working folder, temporary folders and a few tool caches;
//!   - cannot read Heron's own folder (settings, memory, keys) or the user's key folders (.ssh, .aws, ...).
//! Everything else is readable and the network stays available, so builds, tests and git work as usual.
//!
//! Linux uses bubblewrap (`bwrap`) when installed; macOS uses the built-in Seatbelt (`sandbox-exec`).
//! Windows has no equivalent here yet. The layering (read-only root, private temp, hidden secrets, an
//! environment without credentials) follows the approach of the sealbox project by the same author.

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

fn probe() -> Kind {
    let ok = |program: &str, args: &[&str]| std::process::Command::new(program).args(args).stdin(std::process::Stdio::null()).output().map(|o| o.status.success()).unwrap_or(false);
    if cfg!(target_os = "linux") && ok("bwrap", &["--ro-bind", "/", "/", "--dev-bind", "/dev", "/dev", "--tmpfs", "/tmp", "true"]) {
        Kind::Bubblewrap
    } else if cfg!(target_os = "macos") && ok("sandbox-exec", &["-p", "(version 1)(allow default)", "/usr/bin/true"]) {
        Kind::Seatbelt
    } else {
        Kind::None
    }
}

/// Which sandbox this computer offers (checked once).
pub fn available() -> Kind {
    static KIND: OnceLock<Kind> = OnceLock::new();
    *KIND.get_or_init(probe)
}

/// The sandbox in effect for this configuration.
pub fn active(cfg: &Config) -> Kind {
    let off = cfg.agent.sandbox.eq_ignore_ascii_case("off") || std::env::var("HERON_SANDBOX").map(|v| v.eq_ignore_ascii_case("off")).unwrap_or(false);
    if off { Kind::None } else { available() }
}

fn existing(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    paths.into_iter().filter(|p| p.exists()).map(|p| dunce::canonicalize(&p).unwrap_or(p)).collect()
}

/// Folders a command may write to besides the working folder: caches that builds and package managers need.
fn writable_extras(home: &Path) -> Vec<PathBuf> {
    existing([".cache", ".cargo/registry", ".cargo/git", ".npm", ".rustup/toolchains", "Library/Caches"].iter().map(|d| home.join(d)))
}

/// Folders a command must not even read.
fn hidden(cfg: &Config, home: &Path, workspace: &Path) -> Vec<PathBuf> {
    let mut v = existing([".ssh", ".aws", ".gnupg", ".kube", ".docker", ".config/gcloud", ".netrc", ".git-credentials"].iter().map(|d| home.join(d)).chain([cfg.home.clone()]));
    // Never hide the folder we are working in (or one of its parents).
    v.retain(|p| !workspace.starts_with(p));
    v
}

fn sbpl_quote(p: &Path) -> String {
    format!("\"{}\"", p.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\""))
}

/// The command prefix that runs a program inside the sandbox, or None when there is no sandbox.
/// The caller appends the shell and its arguments.
pub fn wrap(cfg: &Config, workspace: &Path) -> Option<(String, Vec<String>)> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let s = |p: &Path| p.to_string_lossy().to_string();
    match active(cfg) {
        Kind::None => None,
        Kind::Bubblewrap => {
            let mut a: Vec<String> = ["--die-with-parent", "--ro-bind", "/", "/", "--dev-bind", "/dev", "/dev", "--tmpfs", "/tmp"].iter().map(|x| x.to_string()).collect();
            for dir in writable_extras(&home).iter().chain([&workspace.to_path_buf()]) {
                a.extend(["--bind".to_string(), s(dir), s(dir)]);
            }
            for path in hidden(cfg, &home, workspace) {
                if path.is_dir() {
                    a.extend(["--tmpfs".to_string(), s(&path)]);
                } else {
                    a.extend(["--ro-bind".to_string(), "/dev/null".to_string(), s(&path)]);
                }
            }
            a.extend(["--chdir".to_string(), s(workspace), "--".to_string()]);
            Some(("bwrap".to_string(), a))
        }
        Kind::Seatbelt => {
            let mut profile = String::from("(version 1)\n(allow default)\n(deny file-write*)\n(allow file-write*");
            for dir in [PathBuf::from("/private/tmp"), PathBuf::from("/private/var/folders"), PathBuf::from("/dev"), workspace.to_path_buf()].iter().chain(writable_extras(&home).iter()) {
                profile.push_str(&format!(" (subpath {})", sbpl_quote(dir)));
            }
            profile.push_str(")\n");
            for path in hidden(cfg, &home, workspace) {
                profile.push_str(&format!("(deny file-read* file-write* (subpath {}))\n", sbpl_quote(&path)));
            }
            Some(("sandbox-exec".to_string(), vec!["-p".to_string(), profile]))
        }
    }
}

/// Shown to the model when a sandboxed command fails in a way the sandbox explains.
pub const DENIED_HINT: &str = "[This ran inside Heron's sandbox: commands can change files only in the working folder and temporary folders, and cannot read Heron's own data or the user's key folders. If the task legitimately needs more, tell the user; they can turn the sandbox off in the settings file (agent.sandbox = \"off\").]";

pub fn looks_denied(output: &str) -> bool {
    ["Read-only file system", "Operation not permitted", "Permission denied"].iter().any(|m| output.contains(m))
}
