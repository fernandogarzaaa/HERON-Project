//! File tools: read, write, edit, list, search. Writes are checkpointed so they can be undone.

use super::{arg_str, opt_bool, opt_i64, opt_str, schema, FnTool, ToolCtx};
use crate::permissions::{classify_path, resolve_path, Effect};
use crate::util::preview;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::path::Path;

const MAX_READ_BYTES: u64 = 10 * 1024 * 1024;

fn path_summary(args: &Value) -> String {
    args.get("path").and_then(|p| p.as_str()).unwrap_or(".").to_string()
}

fn read_risk(args: &Value, ctx: &ToolCtx) -> Effect {
    classify_path(&resolve_path(&ctx.workspace, &path_summary(args)), &ctx.workspace, false)
}

fn write_risk(args: &Value, ctx: &ToolCtx) -> Effect {
    let path = resolve_path(&ctx.workspace, &path_summary(args));
    // The agent must never quietly rewrite its own settings, permission rules, skills, memory or keys.
    let home = dunce::canonicalize(&ctx.cfg.home).unwrap_or_else(|_| ctx.cfg.home.clone());
    if path.starts_with(&home) || path.starts_with(&ctx.cfg.home) {
        return Effect::Privileged;
    }
    classify_path(&path, &ctx.workspace, true)
}

fn checkpoint(ctx: &ToolCtx, path: &Path) -> Result<()> {
    let prev = if path.exists() { Some(std::fs::read(path)?) } else { None };
    ctx.store.save_checkpoint(&ctx.session_id, &path.to_string_lossy(), prev.as_deref())
}

/// Undo the most recent file change made in a session. Returns a description, or None if nothing to undo.
pub fn undo_last(ctx: &ToolCtx) -> Result<Option<String>> {
    let Some((path, prev)) = ctx.store.pop_checkpoint(&ctx.session_id)? else { return Ok(None) };
    match prev {
        Some(bytes) => {
            std::fs::write(&path, bytes)?;
            Ok(Some(format!("restored {path}")))
        }
        None => {
            if Path::new(&path).exists() {
                std::fs::remove_file(&path)?;
            }
            Ok(Some(format!("removed {path} (it did not exist before)")))
        }
    }
}

async fn read_file(args: &Value, ctx: &ToolCtx) -> Result<String> {
    let path = resolve_path(&ctx.workspace, arg_str(args, "path")?);
    let meta = std::fs::metadata(&path).with_context(|| format!("cannot read {}", path.display()))?;
    if meta.is_dir() {
        bail!("{} is a directory; use list_dir", path.display());
    }
    if meta.len() > MAX_READ_BYTES {
        bail!("file is {} bytes — too large to read whole; use search_files or shell tools to inspect it", meta.len());
    }
    let bytes = tokio::fs::read(&path).await?;
    if bytes.iter().take(8192).any(|b| *b == 0) {
        bail!("{} appears to be a binary file ({} bytes)", path.display(), bytes.len());
    }
    let text = String::from_utf8_lossy(&bytes);
    let offset = opt_i64(args, "offset").unwrap_or(1).max(1) as usize;
    let limit = opt_i64(args, "limit").unwrap_or(2000).clamp(1, 10_000) as usize;
    let total = text.lines().count();
    let mut out = String::new();
    for (i, line) in text.lines().enumerate().skip(offset - 1).take(limit) {
        out.push_str(&format!("{:>5}\t{}\n", i + 1, line));
    }
    if total == 0 {
        return Ok("(empty file)".into());
    }
    if offset - 1 + limit < total {
        out.push_str(&format!("[showing lines {}-{} of {}; pass offset to read more]\n", offset, offset - 1 + limit, total));
    }
    Ok(out)
}

async fn write_file(args: &Value, ctx: &ToolCtx) -> Result<String> {
    let path = resolve_path(&ctx.workspace, arg_str(args, "path")?);
    let content = arg_str(args, "content")?;
    checkpoint(ctx, &path)?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&path, content).await.with_context(|| format!("cannot write {}", path.display()))?;
    Ok(format!("wrote {} bytes to {}", content.len(), shown(&path, ctx)))
}

async fn edit_file(args: &Value, ctx: &ToolCtx) -> Result<String> {
    let path = resolve_path(&ctx.workspace, arg_str(args, "path")?);
    let old = arg_str(args, "old_string")?;
    let new = arg_str(args, "new_string")?;
    if old.is_empty() {
        bail!("old_string must not be empty (use write_file to create files)");
    }
    if old == new {
        bail!("old_string and new_string are identical");
    }
    let text = tokio::fs::read_to_string(&path).await.with_context(|| format!("cannot read {}", path.display()))?;
    let count = text.matches(old).count();
    if count == 0 {
        bail!("old_string was not found in {}. Read the file again and copy the exact text, including whitespace.", path.display());
    }
    let replace_all = opt_bool(args, "replace_all");
    if count > 1 && !replace_all {
        bail!("old_string occurs {count} times in {}. Add surrounding context to make it unique, or set replace_all=true.", path.display());
    }
    checkpoint(ctx, &path)?;
    let updated = if replace_all { text.replace(old, new) } else { text.replacen(old, new, 1) };
    tokio::fs::write(&path, updated).await?;
    Ok(format!("edited {} ({} replacement{})", path.display(), count.min(if replace_all { count } else { 1 }), if replace_all && count > 1 { "s" } else { "" }))
}

async fn list_dir(args: &Value, ctx: &ToolCtx) -> Result<String> {
    let path = resolve_path(&ctx.workspace, opt_str(args, "path").unwrap_or("."));
    let mut entries: Vec<(bool, String, u64)> = vec![];
    let mut rd = tokio::fs::read_dir(&path).await.with_context(|| format!("cannot list {}", path.display()))?;
    while let Some(e) = rd.next_entry().await? {
        let meta = e.metadata().await.ok();
        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        entries.push((is_dir, e.file_name().to_string_lossy().to_string(), meta.map(|m| m.len()).unwrap_or(0)));
    }
    entries.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let total = entries.len();
    let name = match shown(&path, ctx) {
        n if n.is_empty() => "the working folder".to_string(),
        n => n,
    };
    let mut out = format!("{name} ({total} entries)\n");
    for (is_dir, name, size) in entries.into_iter().take(500) {
        if is_dir {
            out.push_str(&format!("  {name}/\n"));
        } else {
            out.push_str(&format!("  {name}  ({size} bytes)\n"));
        }
    }
    if total > 500 {
        out.push_str("  [... more entries not shown ...]\n");
    }
    Ok(out)
}

fn walker(root: &Path) -> ignore::Walk {
    ignore::WalkBuilder::new(root).hidden(false).filter_entry(|e| !matches!(e.file_name().to_str(), Some(".git" | "node_modules" | "target" | ".venv" | "__pycache__"))).build()
}

async fn search_files(args: &Value, ctx: &ToolCtx) -> Result<String> {
    let pattern = arg_str(args, "pattern")?.to_string();
    let root = resolve_path(&ctx.workspace, opt_str(args, "path").unwrap_or("."));
    let glob = match opt_str(args, "glob") {
        Some(g) => Some(globset::Glob::new(g).map_err(|e| anyhow!("bad glob: {e}"))?.compile_matcher()),
        None => None,
    };
    let re = regex::RegexBuilder::new(&pattern).case_insensitive(opt_bool(args, "ignore_case")).build().map_err(|e| anyhow!("bad regex: {e}"))?;
    let max = 200usize;
    tokio::task::spawn_blocking(move || {
        let mut out = String::new();
        let mut hits = 0usize;
        'files: for entry in walker(&root).flatten() {
            let p = entry.path();
            if !p.is_file() {
                continue;
            }
            let rel = p.strip_prefix(&root).unwrap_or(p);
            if let Some(g) = &glob {
                if !g.is_match(rel) && !p.file_name().map(|f| g.is_match(f)).unwrap_or(false) {
                    continue;
                }
            }
            if classify_path(p, &root, false) == Effect::SensitiveRead {
                continue; // never grep through credential files
            }
            let Ok(bytes) = std::fs::read(p) else { continue };
            if bytes.len() as u64 > MAX_READ_BYTES || bytes.iter().take(4096).any(|b| *b == 0) {
                continue;
            }
            let text = String::from_utf8_lossy(&bytes);
            for (i, line) in text.lines().enumerate() {
                if re.is_match(line) {
                    out.push_str(&format!("{}:{}: {}\n", rel.display(), i + 1, preview(line, 240)));
                    hits += 1;
                    if hits >= max {
                        out.push_str(&format!("[stopped at {max} matches; narrow the pattern or path]\n"));
                        break 'files;
                    }
                }
            }
        }
        if hits == 0 { "no matches".to_string() } else { out }
    })
    .await
    .map_err(|e| anyhow!("search failed: {e}"))
}

async fn find_files(args: &Value, ctx: &ToolCtx) -> Result<String> {
    let pattern = arg_str(args, "glob")?;
    let root = resolve_path(&ctx.workspace, opt_str(args, "path").unwrap_or("."));
    let g = globset::Glob::new(pattern).map_err(|e| anyhow!("bad glob: {e}"))?.compile_matcher();
    tokio::task::spawn_blocking(move || {
        let mut found: Vec<String> = walker(&root)
            .flatten()
            .filter(|e| e.path().is_file())
            .filter_map(|e| {
                let rel = e.path().strip_prefix(&root).unwrap_or(e.path()).to_path_buf();
                (g.is_match(&rel) || e.path().file_name().map(|f| g.is_match(f)).unwrap_or(false)).then(|| rel.display().to_string())
            })
            .take(501)
            .collect();
        found.sort();
        if found.is_empty() {
            "no files match".to_string()
        } else if found.len() > 500 {
            format!("{}\n[more than 500 matches; narrow the glob]", found[..500].join("\n"))
        } else {
            found.join("\n")
        }
    })
    .await
    .map_err(|e| anyhow!("find failed: {e}"))
}

/// Path as shown to the user: relative when inside the workspace, absolute otherwise.
fn shown(path: &Path, ctx: &ToolCtx) -> String {
    path.strip_prefix(&ctx.workspace).map(|p| p.display().to_string()).unwrap_or_else(|_| path.display().to_string())
}

fn write_detail(args: &Value, ctx: &ToolCtx) -> String {
    let path = resolve_path(&ctx.workspace, &path_summary(args));
    let content = args.get("content").and_then(|c| c.as_str()).unwrap_or("");
    let head: String = content.lines().take(12).collect::<Vec<_>>().join("\n");
    format!("{} {} ({} bytes)\n{}{}", if path.exists() { "Replace the contents of" } else { "Create" }, shown(&path, ctx), content.len(), head, if content.lines().count() > 12 { "\n…" } else { "" })
}

fn edit_detail(args: &Value, ctx: &ToolCtx) -> String {
    let path = resolve_path(&ctx.workspace, &path_summary(args));
    let fmt = |key: &str, sign: char| {
        args.get(key).and_then(|c| c.as_str()).unwrap_or("").lines().take(10).map(|l| format!("{sign} {l}")).collect::<Vec<_>>().join("\n")
    };
    format!("Edit {}\n{}\n{}", shown(&path, ctx), fmt("old_string", '-'), fmt("new_string", '+'))
}

pub fn tools() -> Vec<FnTool> {
    vec![
        FnTool {
            name: "read_file",
            description: "Read a text file. Returns numbered lines. Use offset/limit for large files.",
            parameters: schema(json!({"path": {"type": "string", "description": "Path (relative to the workspace, absolute, or ~/...)"}, "offset": {"type": "integer", "description": "First line to read (1-based)"}, "limit": {"type": "integer", "description": "Max lines (default 2000)"}}), &["path"]),
            summary: path_summary,
            risk: read_risk,
            detail: None,
            untrusted: false,
            run: |a, c| Box::pin(read_file(a, c)),
        },
        FnTool {
            name: "write_file",
            description: "Create or overwrite a file with the given content. Parent directories are created. The previous version is checkpointed and can be undone.",
            parameters: schema(json!({"path": {"type": "string"}, "content": {"type": "string"}}), &["path", "content"]),
            summary: path_summary,
            risk: write_risk,
            detail: Some(write_detail),
            untrusted: false,
            run: |a, c| Box::pin(write_file(a, c)),
        },
        FnTool {
            name: "edit_file",
            description: "Replace an exact string in a file. old_string must match exactly and be unique unless replace_all is true. Prefer this over write_file for changes to existing files.",
            parameters: schema(json!({"path": {"type": "string"}, "old_string": {"type": "string"}, "new_string": {"type": "string"}, "replace_all": {"type": "boolean"}}), &["path", "old_string", "new_string"]),
            summary: path_summary,
            risk: write_risk,
            detail: Some(edit_detail),
            untrusted: false,
            run: |a, c| Box::pin(edit_file(a, c)),
        },
        FnTool {
            name: "list_dir",
            description: "List a directory's entries (directories first).",
            parameters: schema(json!({"path": {"type": "string", "description": "Directory (default: workspace root)"}}), &[]),
            summary: path_summary,
            risk: read_risk,
            detail: None,
            untrusted: false,
            run: |a, c| Box::pin(list_dir(a, c)),
        },
        FnTool {
            name: "search_files",
            description: "Search file contents with a regular expression (respects .gitignore). Returns path:line: text.",
            parameters: schema(json!({"pattern": {"type": "string", "description": "Regex"}, "path": {"type": "string", "description": "Directory to search (default: workspace)"}, "glob": {"type": "string", "description": "Only files matching this glob, e.g. *.rs"}, "ignore_case": {"type": "boolean"}}), &["pattern"]),
            summary: |a| format!("/{}/ in {}", a.get("pattern").and_then(|p| p.as_str()).unwrap_or(""), path_summary(a)),
            risk: read_risk,
            detail: None,
            untrusted: false,
            run: |a, c| Box::pin(search_files(a, c)),
        },
        FnTool {
            name: "find_files",
            description: "Find files by glob pattern (e.g. **/*.md, Cargo.toml). Respects .gitignore.",
            parameters: schema(json!({"glob": {"type": "string"}, "path": {"type": "string", "description": "Directory to search (default: workspace)"}}), &["glob"]),
            summary: |a| format!("{} in {}", a.get("glob").and_then(|p| p.as_str()).unwrap_or(""), path_summary(a)),
            risk: read_risk,
            detail: None,
            untrusted: false,
            run: |a, c| Box::pin(find_files(a, c)),
        },
    ]
}
