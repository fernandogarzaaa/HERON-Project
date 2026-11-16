//! Skills: reusable instructions stored as Markdown in `~/.heron/skills/`.
//! Either `name.md` or `name/SKILL.md`. Optional front matter: `description: ...`.
//! Only names + descriptions are put in the prompt; the body is loaded on demand.

use std::path::Path;
use std::io::Read;

const MAX_SKILL_BYTES: u64 = 64 * 1024;
const MAX_SKILL_NAME: usize = 80;

/// Local skills can influence a tool-using agent. Never follow a symlink
/// outside the configured skills directory, load unbounded files, or let a
/// nondeterministic duplicate skill name shadow another skill.
fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= MAX_SKILL_NAME
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn read_skill(path: &Path) -> Option<String> {
    // This is a local-only loader. Installing skills from arbitrary remote
    // locations requires a separate signed/hashed manifest and explicit grant.
    let md = std::fs::symlink_metadata(path).ok()?;
    if !md.file_type().is_file() || md.len() > MAX_SKILL_BYTES {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_SKILL_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MAX_SKILL_BYTES {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// Content digest for a future per-skill provenance/approval ledger.
pub fn fingerprint(skill: &Skill) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    for field in [&skill.name, &skill.description, &skill.body] {
        hash.update((field.len() as u64).to_le_bytes());
        hash.update(field.as_bytes());
    }
    hash.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub body: String,
}

fn parse(name: &str, text: &str) -> Skill {
    let mut description = String::new();
    let mut body = text;
    if let Some(rest) = text.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            for line in rest[..end].lines() {
                if let Some(d) = line.trim().strip_prefix("description:") {
                    description = d.trim().trim_matches(['"', '\'']).to_string();
                }
            }
            body = rest[end + 4..].trim_start_matches(['\r', '\n']);
        }
    }
    if description.is_empty() {
        description = body.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim_start_matches('#').trim().chars().take(140).collect();
    }
    Skill { name: name.to_string(), description, body: body.to_string() }
}

pub fn load_all(dir: &Path) -> Vec<Skill> {
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    // Deterministic order, including platforms with different filesystem
    // directory iteration order. Do not follow symlinked skill directories.
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name().to_string_lossy().to_ascii_lowercase());
    let mut found = std::collections::BTreeMap::<String, Skill>::new();
    for entry in entries {
        let p = entry.path();
        let Ok(md) = std::fs::symlink_metadata(&p) else { continue };
        if md.file_type().is_symlink() { continue }
        let (name, file) = if md.is_dir() {
            (p.file_name().unwrap().to_string_lossy().to_string(), p.join("SKILL.md"))
        } else if md.is_file() && p.extension().is_some_and(|ext| ext == "md") {
            (p.file_stem().unwrap().to_string_lossy().to_string(), p.clone())
        } else {
            continue;
        };
        if !valid_name(&name) { continue }
        let Some(content) = read_skill(&file) else { continue };
        let key = name.to_ascii_lowercase();
        found.entry(key).or_insert_with(|| parse(&name, &content));
    }
    found.into_values().collect()
}

pub fn find(dir: &Path, name: &str) -> Option<Skill> {
    load_all(dir).into_iter().find(|s| s.name.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_oversized_and_unsafe_skills_without_skipping_valid_neighbors() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("valid.md"), "# Valid skill").unwrap();
        std::fs::write(dir.path().join("too-big.md"), "x".repeat(MAX_SKILL_BYTES as usize + 1)).unwrap();
        std::fs::write(dir.path().join("bad name.md"), "should be rejected").unwrap();
        let skills = load_all(dir.path());
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "valid");
        assert_eq!(fingerprint(&skills[0]).len(), 64);
        assert_eq!(fingerprint(&skills[0]), fingerprint(&skills[0]));
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlinked_skill_file_or_directory() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.md"), "# Confidential").unwrap();
        std::fs::create_dir(outside.path().join("remote-skill")).unwrap();
        std::fs::write(outside.path().join("remote-skill/SKILL.md"), "# No").unwrap();
        symlink(outside.path().join("secret.md"), dir.path().join("linked.md")).unwrap();
        symlink(outside.path().join("remote-skill"), dir.path().join("linked-folder")).unwrap();
        assert!(load_all(dir.path()).is_empty());
    }

    #[test]
    fn loads_both_layouts_and_front_matter() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("standup.md"), "---\ndescription: Write my daily standup\n---\n# Standup\nSteps...").unwrap();
        std::fs::create_dir(d.path().join("trip")).unwrap();
        std::fs::write(d.path().join("trip/SKILL.md"), "# Plan a trip\nAsk for dates first.").unwrap();
        std::fs::write(d.path().join("ignored.txt"), "x").unwrap();
        let all = load_all(d.path());
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].description, "Write my daily standup");
        assert!(all[0].body.starts_with("# Standup"));
        assert_eq!(all[1].description, "Plan a trip");
        assert!(find(d.path(), "TRIP").is_some());
        assert!(find(d.path(), "nope").is_none());
    }
}
