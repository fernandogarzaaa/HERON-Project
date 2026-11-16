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
const MAX_SKILL_BYTES: u64 = 64 * 1024;
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
    (!cleaned.is_empty()
        && cleaned.len() <= max
        && !cleaned.chars().any(char::is_control))
        .then(|| cleaned.to_string())
}

fn parse(name: &str, text: &str) -> Skill {
    let mut description = None;
    let mut version = None;
    let mut source = None;
    let mut declared_tools = Vec::new();
    let mut body = text;
    if let Some(rest) = text.strip_prefix("---\n") {
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
    let mut entries: Vec<_> = entries.filter_map(|entry| entry.ok()).collect();
    entries.sort_by_key(|entry| entry.file_name());
    let mut out = Vec::new();

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
        let Ok(file) = std::fs::File::open(&file) else {
            continue;
        };
        let mut text = String::new();
        if file.take(MAX_SKILL_BYTES + 1).read_to_string(&mut text).is_ok()
            && text.len() as u64 <= MAX_SKILL_BYTES
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
    fn rejects_oversized_and_invalid_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("huge.md"), "z".repeat(MAX_SKILL_BYTES as usize + 1)).unwrap();
        std::fs::write(dir.path().join("normal.md"), "# Local file").unwrap();
        assert_eq!(load_all(dir.path()).len(), 1);
        assert_eq!(load_all(dir.path())[0].name, "normal");
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
