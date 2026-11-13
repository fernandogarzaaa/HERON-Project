//! Heron's character: one plain Markdown file the user owns (`~/.heron/persona.md`).
//! It is read at the start of every conversation. The user can edit it in Settings, or simply
//! ask in conversation ("be more casual") — Heron then proposes the change and the user approves it.
//! Tone is defined by observable behaviour rather than adjectives, so it actually changes what is written.

use crate::config::Config;
use anyhow::{bail, Result};

pub const MAX_CHARS: usize = 4000;

pub const DEFAULT: &str = "\
# Who you are
- You are warm, direct and unhurried: a capable colleague, not a servant and not a salesperson.
- You are on the user's side. You say so when you think they are about to make a mistake, and explain why.

# How you speak
- Lead with the answer or the result. Context comes after, and only if it helps.
- Give one recommendation unless asked for options.
- Say plainly when you are unsure or when something failed. Never dress up a guess as a fact.
- No openers (\"Great question\"), no recaps of what was asked, no closing offers (\"Let me know if...\").
- Match the user's length and register: a short question gets a short answer.
- When asking permission, state the action, what it costs or changes, and what happens if they say no — together.
- Be kind about hard days without flattering. Never guilt, never nag.

# What you never do
- Pretend to have done something you have not.
- Raise something again that the user has said is settled.
";

fn path(cfg: &Config) -> std::path::PathBuf {
    cfg.home.join("persona.md")
}

pub fn load(cfg: &Config) -> String {
    std::fs::read_to_string(path(cfg)).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| DEFAULT.trim().to_string())
}

pub fn is_default(cfg: &Config) -> bool {
    load(cfg) == DEFAULT.trim()
}

pub fn save(cfg: &Config, text: &str) -> Result<()> {
    let text = text.trim();
    if text.len() < 20 {
        bail!("That is too short to describe a character. Write a few lines, or reset to the default.");
    }
    if text.chars().count() > MAX_CHARS {
        bail!("Keep it under {MAX_CHARS} characters: a long character sheet crowds out the actual work.");
    }
    cfg.ensure_home()?;
    std::fs::write(path(cfg), crate::util::redact_secrets(text))?;
    Ok(())
}

pub fn reset(cfg: &Config) -> Result<()> {
    let p = path(cfg);
    if p.exists() {
        std::fs::remove_file(p)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persona_round_trips_and_resets() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.home = dir.path().join("home");
        assert!(is_default(&cfg));
        assert!(load(&cfg).contains("Lead with the answer"));
        save(&cfg, "# Who you are\n- Dry, funny, a little blunt.\n- Call me Dana.").unwrap();
        assert!(load(&cfg).contains("Dry, funny"));
        assert!(!is_default(&cfg));
        assert!(save(&cfg, "hi").is_err());
        assert!(save(&cfg, &"x".repeat(MAX_CHARS + 1)).is_err());
        reset(&cfg).unwrap();
        assert!(is_default(&cfg));
    }
}
