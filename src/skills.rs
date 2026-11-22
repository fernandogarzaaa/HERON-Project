//! Skills are local Markdown instructions in `~/.heron/skills/`.
//! A manifest is descriptive, not a permission grant: agent tool calls still
//! pass through HERON's permissions and approvals.
//!
//! Skills may be copied from untrusted registries. Do not follow symlinks out
//! of the skills directory or load unbounded data into the agent context.

use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

const MAX_SKILLS: usize = 128;
// Bound directory enumeration as well as the number and size of loaded skills.
// Allow unrelated files, but fail closed rather than materializing an unbounded listing.
const MAX_SKILL_ENTRIES: usize = MAX_SKILLS * 8;
pub const MAX_SKILL_BYTES: u64 = 64 * 1024;

/// Reject a save before modifying any existing skill if the loader would
/// refuse the resulting file.
pub fn validate_skill_size(text: &str) -> anyhow::Result<()> {
    anyhow::ensure!(text.len() as u64 <= MAX_SKILL_BYTES,
        "skill is too large (max {} bytes)", MAX_SKILL_BYTES);
    Ok(())
}
const MAX_NAME_BYTES: usize = 80;

#[derive(Debug, Clone, PartialEq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub body: String,
    /// Optional author-declared version; never trusted as a security proof.
    pub version: Option<String>,
    /// Optional author-declared provenance string; never fetched automatically.
    pub source: Option<String>,
    /// Suggested tools only, NOT automatically granted permissions.
    pub declared_tools: Vec<String>,
    /// Hash of the exact raw skill file, for reviewing changes before use.
    pub sha256: String,
}

fn summary(text: &str) -> String {
    text.trim()
        .trim_start_matches('#')
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(140)
        .collect()
}

fn metadata(value: &str, max: usize) -> Option<String> {
    let cleaned = value.trim().trim_matches(['"', '\'']);
    if cleaned.is_empty() || cleaned.chars().any(char::is_control) {
        return None;
    }
    // Truncate by Unicode scalar count; do not silently lose otherwise
    // useful skill descriptions because of long UTF-8 byte sequences.
    Some(cleaned.chars().take(max).collect())
}

fn parse(name: &str, text: &str) -> Skill {
    let mut description = None;
    let mut version = None;
    let mut source = None;
    let mut declared_tools = Vec::new();
    // Windows-authored files may have CRLF front matter; parsing normalizes
    // delimiters without changing the SHA-256 of the original source.
    let normalized = text.replace("\r\n", "\n");
    let mut body = normalized.as_str();
    if let Some(rest) = normalized.strip_prefix("---\n") {
        if let Some((front, after)) = rest.split_once("\n---") {
            for line in front.lines() {
                if let Some((key, raw)) = line.split_once(':') {
                    match key.trim() {
                        "description" => description = metadata(raw, 140),
                        "version" => version = metadata(raw, 48),
                        "source" => source = metadata(raw, 2048),
                        "tools" => {
                            // Metadata is not executable. Never feed tool
                            // declarations into HERON's standing grant list.
                            declared_tools = raw.split(',')
                                .filter_map(|s| metadata(s, 80))
                                .take(32)
                                .collect();
                        }
                        _ => {}
                    }
                }
            }
            body = after.trim_start_matches(['\r', '\n']);
        }
    }
    let description = description.unwrap_or_else(|| {
        summary(body.lines().find(|line| !line.trim().is_empty()).unwrap_or(""))
    });
    let sha256 = Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Skill {
        name: name.to_string(),
        description,
        body: body.to_string(),
        version,
        source,
        declared_tools,
        sha256,
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_BYTES
        && name != "."
        && name != ".."
        && !name.chars().any(char::is_control)
}

fn within_skills_directory(root: &Path, path: &Path) -> bool {
    std::fs::canonicalize(path)
        .map(|resolved| resolved.starts_with(root))
        .unwrap_or(false)
}

#[cfg(unix)]
fn secure_open(root: &Path, folder: Option<&std::ffi::OsStr>, filename: &std::ffi::OsStr) -> std::io::Result<std::fs::File> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;

    let croot = CString::new(root.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let cfile = CString::new(filename.as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    // SAFETY: CString pointers remain valid for the duration of each call;
    // returned owned File closes its descriptor on every path.
    let root_fd = unsafe {
        libc::open(croot.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
    };
    if root_fd < 0 { return Err(std::io::Error::last_os_error()); }
    let root_file = unsafe { std::fs::File::from_raw_fd(root_fd) };

    let nested = if let Some(folder) = folder {
        let cname = CString::new(folder.as_bytes())
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
        let fd = unsafe {
            libc::openat(root_file.as_raw_fd(), cname.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        };
        if fd < 0 { return Err(std::io::Error::last_os_error()); }
        Some(unsafe { std::fs::File::from_raw_fd(fd) })
    } else {
        None
    };
    let parent = nested.as_ref().unwrap_or(&root_file);
    let fd = unsafe {
        libc::openat(parent.as_raw_fd(), cfile.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
    };
    if fd < 0 { return Err(std::io::Error::last_os_error()); }
    Ok(unsafe { std::fs::File::from_raw_fd(fd) })
}

#[cfg(not(unix))]
fn secure_open(root: &Path, folder: Option<&std::ffi::OsStr>, filename: &std::ffi::OsStr) -> std::io::Result<std::fs::File> {
    let path = match folder {
        Some(dir) => root.join(dir).join(filename),
        None => root.join(filename),
    };
    std::fs::File::open(path)
}

/// Read skills deterministically, imposing a limit on each file and the
/// number of skills. Direct symlinks and nested SKILL.md symlinks are ignored.
/// This deliberately does NOT install skills from URLs or trust their content.
pub fn load_all(dir: &Path) -> Vec<Skill> {
    let Ok(root) = std::fs::canonicalize(dir) else {
        return vec![];
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut bounded_entries = Vec::with_capacity(MAX_SKILL_ENTRIES);
    for entry in entries.filter_map(|entry| entry.ok()) {
        if bounded_entries.len() == MAX_SKILL_ENTRIES {
            // More entries than the bounded scan can safely inspect. Fail closed
            // instead of allocating memory proportional to an untrusted directory.
            return vec![];
        }
        bounded_entries.push(entry);
    }
    bounded_entries.sort_by_key(|entry| entry.file_name());
    let mut out = Vec::new();
    let mut names = std::collections::HashSet::new();

    for entry in entries {
        if out.len() >= MAX_SKILLS {
            break;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        let path = entry.path();
        let (name, file) = if kind.is_dir() {
            (entry.file_name().to_string_lossy().to_string(), path.join("SKILL.md"))
        } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "md") {
            (
                path.file_stem().map(|stem| stem.to_string_lossy().to_string()).unwrap_or_default(),
                path.clone(),
            )
        } else {
            continue;
        };
        if !valid_name(&name) || !within_skills_directory(&root, &file) {
            continue;
        }
        let Ok(info) = std::fs::symlink_metadata(&file) else {
            continue;
        };
        if info.file_type().is_symlink() || !info.is_file() || info.len() > MAX_SKILL_BYTES {
            continue;
        }
        // Open relative to a verified directory descriptor on Unix.
        // O_NOFOLLOW on a final file alone is insufficient when an attacker
        // can replace the *parent* directory between checks and open.
        let folder = kind.is_dir().then(|| entry.file_name());
        let top_level_name = entry.file_name();
        let filename = if kind.is_dir() {
            std::ffi::OsStr::new("SKILL.md")
        } else {
            top_level_name.as_os_str()
        };
        let handle = secure_open(&root, folder.as_deref(), filename);
        let Ok(handle) = handle else { continue };
        let Ok(opened) = handle.metadata() else { continue };
        if !opened.is_file() || opened.len() > MAX_SKILL_BYTES {
            continue;
        }
        let mut text = String::new();
        if handle.take(MAX_SKILL_BYTES + 1).read_to_string(&mut text).is_ok()
            && text.len() as u64 <= MAX_SKILL_BYTES
            && names.insert(name.to_ascii_lowercase())
        {
            out.push(parse(&name, &text));
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn find(dir: &Path, name: &str) -> Option<Skill> {
    load_all(dir)
        .into_iter()
        .find(|skill| skill.name.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_directories_with_more_entries_than_the_bounded_scan() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..=MAX_SKILL_ENTRIES {
            std::fs::write(
                dir.path().join(format!("noise-{index:04}.txt")),
                "not a skill",
            ).unwrap();
        }
        std::fs::write(dir.path().join("valid.md"), "# Valid skill").unwrap();
        assert!(load_all(dir.path()).is_empty());
    }

    #[test]
    fn loads_local_skills_with_immutable_review_fingerprints() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("standup.md"),
            "---\ndescription: Write my daily standup\nversion: 1.2.0\nsource: local\ntools: shell, file_read\n---\n# Standup\nSteps...",
        ).unwrap();
        std::fs::create_dir(dir.path().join("trip")).unwrap();
        std::fs::write(dir.path().join("trip/SKILL.md"), "# Plan a trip\nAsk for dates first.").unwrap();
        std::fs::write(dir.path().join("ignored.txt"), "ignored").unwrap();
        let all = load_all(dir.path());
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].description, "Write my daily standup");
        assert_eq!(all[0].version.as_deref(), Some("1.2.0"));
        assert_eq!(all[0].source.as_deref(), Some("local"));
        assert_eq!(all[0].declared_tools, ["shell", "file_read"]);
        assert_eq!(all[0].sha256.len(), 64);
        assert!(all[0].body.starts_with("# Standup"));
        assert_eq!(all[1].description, "Plan a trip");
        assert!(find(dir.path(), "TRIP").is_some());
        assert!(find(dir.path(), "nope").is_none());
    }

    #[test]
    fn crlf_preserves_metadata_but_keeps_original_content_digest() {
        let d = tempfile::tempdir().unwrap();
        let content = "---\r\ndescription: Windows task\r\nversion: 1.0\r\n---\r\n# Body";
        std::fs::write(d.path().join("windows.md"), content).unwrap();
        let result = load_all(d.path());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].description, "Windows task");
        assert_eq!(result[0].version.as_deref(), Some("1.0"));
        let raw_hash: String = Sha256::digest(content.as_bytes()).iter()
            .map(|b| format!("{b:02x}")).collect();
        assert_eq!(result[0].sha256, raw_hash);
    }

    #[test]
    fn unicode_long_descriptions_are_truncated_not_discarded() {
        let d = tempfile::tempdir().unwrap();
        let description = "é".repeat(160);
        std::fs::write(d.path().join("unicode.md"),
            format!("---\ndescription: {description}\n---\n# Body")).unwrap();
        let skills = load_all(d.path());
        assert_eq!(skills[0].description.chars().count(), 140);
        assert_eq!(skills[0].description, "é".repeat(140));
    }

    #[test]
    fn rejects_oversized_and_invalid_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("huge.md"), "z".repeat(MAX_SKILL_BYTES as usize + 1)).unwrap();
        std::fs::write(dir.path().join("normal.md"), "# Local file").unwrap();
        assert_eq!(load_all(dir.path()).len(), 1);
        assert_eq!(load_all(dir.path())[0].name, "normal");
    }

    #[test]
    fn case_folded_duplicates_do_not_shadow_other_skills() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Alpha.md"), "# First").unwrap();
        std::fs::write(dir.path().join("alpha.md"), "# Second").unwrap();
        std::fs::write(dir.path().join("beta.md"), "# Third").unwrap();
        let all = load_all(dir.path());
        assert_eq!(all.len(), 2);
        assert!(find(dir.path(), "ALPHA").is_some());
        assert!(find(dir.path(), "beta").is_some());
    }

    #[cfg(unix)]
    #[test]
    fn ignores_symlinks_to_outside_skills_folder() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        std::fs::write(other.path().join("secret.md"), "should not be read").unwrap();
        symlink(other.path().join("secret.md"), dir.path().join("escape.md")).unwrap();
        std::fs::create_dir(dir.path().join("nested")).unwrap();
        symlink(
            other.path().join("secret.md"),
            dir.path().join("nested/SKILL.md"),
        ).unwrap();
        assert!(load_all(dir.path()).is_empty());
    }
}
