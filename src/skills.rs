//! Skills: reusable instructions stored as Markdown in `~/.heron/skills/`.
//! Either `name.md` or `name/SKILL.md`. Optional front matter: `description: ...`.
//! Only names + descriptions are put in the prompt; the body is loaded on demand.

use std::path::Path;

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
    let mut out = vec![];
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        let (name, file) = if p.is_dir() {
            (p.file_name().unwrap().to_string_lossy().to_string(), p.join("SKILL.md"))
        } else if p.extension().map(|x| x == "md").unwrap_or(false) {
            (p.file_stem().unwrap().to_string_lossy().to_string(), p.clone())
        } else {
            continue;
        };
        if let Ok(text) = std::fs::read_to_string(&file) {
            out.push(parse(&name, &text));
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn find(dir: &Path, name: &str) -> Option<Skill> {
    load_all(dir).into_iter().find(|s| s.name.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

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
