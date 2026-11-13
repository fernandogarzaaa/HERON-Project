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

fn content_hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Replace a file in one step: the new content is written beside it and renamed over it, so a crash
/// or a full disk can never leave half a file, and a link put in its place is replaced, not followed.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    write_atomic_if(path, bytes, &|| true).map(|_| ())
}

/// As [`write_atomic`], but `still_expected` is asked at the last moment, after the new content is
/// safely on disk and immediately before it takes the file's place. If it says no, nothing is
/// replaced and this returns `false`. This keeps the time between "the file is as expected" and "the
/// file is replaced" as short as it can be made without help from the operating system; it does not
/// make it zero (see T11 in the threat model).
fn write_atomic_if(path: &Path, bytes: &[u8], still_expected: &dyn Fn() -> bool) -> Result<bool> {
    use std::io::Write;
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.heron-{}-{}", std::process::id(), uuid::Uuid::new_v4().simple()));
    // The file being replaced decides who may read the new one, from the first byte: a private file's
    // new content must never sit beside it readable by others, even for a moment.
    let like = std::fs::metadata(path).ok().map(|m| m.permissions());
    let done = (|| -> Result<bool> {
        let mut f = create_new_like(&tmp, like.as_ref())?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        // Keep the file's permissions exactly (an executable script stays executable).
        if let Some(perms) = like.clone() {
            let _ = std::fs::set_permissions(&tmp, perms);
        }
        if !still_expected() {
            let _ = std::fs::remove_file(&tmp);
            return Ok(false);
        }
        std::fs::rename(&tmp, path)?;
        sync_dir(dir);
        Ok(true)
    })();
    if done.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    done.with_context(|| format!("cannot write {}", path.display()))
}

/// Make a change to a folder's list of names (a new file, a rename, a removal) survive a power cut.
/// Writing a file's content to disk does not do this by itself on Linux and macOS. Windows has no
/// equivalent and needs none here. Failure is not reported: the change itself has already happened.
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

/// Create a file that must not exist yet, readable by no more people than `like` allows. Without
/// `like` the system's usual default applies. Fails if something is already there.
fn create_new_like(path: &Path, like: Option<&std::fs::Permissions>) -> std::io::Result<std::fs::File> {
    let mut open = std::fs::OpenOptions::new();
    open.write(true).create_new(true);
    #[cfg(unix)]
    if let Some(perms) = like {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        open.mode(perms.mode() & 0o777);
    }
    #[cfg(not(unix))]
    let _ = like;
    open.open(path)
}

/// Save `bytes` beside `original` under a name nothing else has, with the original's permissions, and
/// return that name. An earlier saved copy is never replaced: if the name is taken, the next is tried.
/// The copy is on disk, name and all, before this returns.
fn save_copy_beside(original: &Path, bytes: &[u8]) -> Result<String> {
    use std::io::Write;
    let like = std::fs::metadata(original).ok().map(|m| m.permissions());
    let stem = format!("{}.before-undo-{}", original.display(), crate::util::now_ts());
    for n in 1..=1000 {
        let name = if n == 1 { stem.clone() } else { format!("{stem}-{n}") };
        let mut f = match create_new_like(Path::new(&name), like.as_ref()) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e).with_context(|| format!("cannot keep a copy as {name}")),
        };
        let written = f.write_all(bytes).and_then(|_| f.sync_all());
        drop(f);
        if let Err(e) = written {
            let _ = std::fs::remove_file(&name);
            return Err(e).with_context(|| format!("cannot keep a copy as {name}"));
        }
        if let Some(perms) = like {
            let _ = std::fs::set_permissions(&name, perms);
        }
        if let Some(dir) = Path::new(&name).parent() {
            sync_dir(dir);
        }
        return Ok(name);
    }
    bail!("cannot keep a copy of {}: too many saved copies with the same name", original.display())
}

/// Change a file so that the change can be undone: keep what was there together with what the file
/// will look like afterwards, then write the new content in one step. If the write fails nothing is
/// kept. If Heron stops between the two, the record says what the file should have become, so undo
/// can see that it never did (or that someone has changed it since) instead of guessing.
fn write_with_checkpoint(ctx: &ToolCtx, path: &Path, bytes: &[u8]) -> Result<()> {
    let prev = if path.exists() { Some(std::fs::read(path)?) } else { None };
    let id = ctx.store.save_checkpoint(&ctx.session_id, &path.to_string_lossy(), prev.as_deref(), Some(&content_hash(bytes)))?;
    if let Err(e) = write_atomic(path, bytes) {
        let _ = ctx.store.drop_checkpoint(id);
        return Err(e);
    }
    Ok(())
}

/// What undoing would do, or why it will not.
#[derive(Debug, PartialEq)]
pub enum Undo {
    Nothing,
    Done(String),
    /// The file is no longer as Heron left it. Nothing was touched; the checkpoint is kept.
    /// `id` names the checkpoint, so a later "restore anyway" can be tied to exactly this one.
    Conflict { id: i64, why: String },
    /// "Restore anyway" was asked for a checkpoint that is no longer the latest (Heron has changed
    /// something else since the question was put). Nothing was touched.
    Stale(String),
}

fn read_if_there(file: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(file) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("cannot read {}", file.display())),
    }
}

/// Undo the most recent file change Heron made in a conversation.
///
/// It first checks that the file is still exactly as Heron left it. If someone has edited, replaced or
/// removed it since, restoring the old version would destroy their work, so nothing is touched and the
/// checkpoint stays. The answer names the checkpoint. Passing that name back as `restore_anyway` means
/// "I have seen that, restore it regardless": the current content is first saved beside the file and
/// then the old version is restored. If the latest checkpoint is by then a different one, nothing is
/// done, because that is not what was agreed to. The checkpoint is removed only after the restore has
/// succeeded.
pub fn undo(ctx: &ToolCtx, restore_anyway: Option<i64>) -> Result<Undo> {
    let Some((id, path, prev, after)) = ctx.store.last_checkpoint(&ctx.session_id)? else {
        return Ok(match restore_anyway {
            Some(_) => Undo::Stale("That change has already been undone. Nothing was restored.".into()),
            None => Undo::Nothing,
        });
    };
    if restore_anyway.is_some_and(|confirmed| confirmed != id) {
        return Ok(Undo::Stale("Heron has changed another file since you were asked, so this is no longer the change you agreed to restore. Nothing was restored; choose Undo again to see what would be undone now.".into()));
    }
    let force = restore_anyway.is_some();
    let file = Path::new(&path);
    let current = read_if_there(file)?;
    // The file is already what it was before Heron's change (the change never landed, or someone put
    // it back by hand): there is nothing to restore.
    if current == prev && after.is_some() {
        ctx.store.drop_checkpoint(id)?;
        return Ok(Undo::Done(match current {
            Some(_) => format!("{path} is already as it was before Heron's change"),
            None => format!("{path} was already removed"),
        }));
    }
    // Checkpoints from before this check existed have no hash and are restored as they always were.
    let untouched = match (&after, &current) {
        (None, _) => true,
        (Some(hash), Some(bytes)) => *hash == content_hash(bytes),
        (Some(_), None) => false,
    };
    let mut kept = String::new();
    if !untouched {
        match (&current, force) {
            (_, false) => {
                return Ok(Undo::Conflict {
                    id,
                    why: match current {
                        Some(_) => format!("{path} has been changed since Heron edited it, so undoing would overwrite those changes. Nothing was restored."),
                        None => format!("{path} has been removed or moved since Heron edited it. Nothing was restored."),
                    },
                });
            }
            (Some(bytes), true) => {
                let copy = save_copy_beside(file, bytes)?;
                kept = format!(" The version that was there is kept as {copy}.");
            }
            (None, true) => {}
        }
    }
    // Asked again at the last moment: is the file still what was just looked at (and, if a copy was
    // saved, still what was saved)? If it changed in between, stop and say so.
    let still_as_seen = || read_if_there(file).map(|now| now == current).unwrap_or(false);
    let changed_meanwhile = || Undo::Conflict { id, why: format!("{path} changed while it was being restored. Nothing was restored.{kept}") };
    let said = match prev {
        Some(bytes) => {
            if !write_atomic_if(file, &bytes, &still_as_seen)? {
                return Ok(changed_meanwhile());
            }
            format!("restored {path}.{kept}")
        }
        None => {
            if current.is_some() {
                if !still_as_seen() {
                    return Ok(changed_meanwhile());
                }
                std::fs::remove_file(file).with_context(|| format!("cannot remove {path}"))?;
                if let Some(dir) = file.parent() {
                    sync_dir(dir);
                }
            }
            format!("removed {path} (it did not exist before).{kept}")
        }
    };
    ctx.store.drop_checkpoint(id)?;
    Ok(Undo::Done(said))
}

/// Undo the most recent file change made in a session. Returns a description, or None if nothing to undo.
/// A file changed since is reported as an error and left alone.
pub fn undo_last(ctx: &ToolCtx) -> Result<Option<String>> {
    match undo(ctx, None)? {
        Undo::Nothing => Ok(None),
        Undo::Done(said) => Ok(Some(said)),
        Undo::Conflict { why, .. } | Undo::Stale(why) => bail!("{why}"),
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
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    write_with_checkpoint(ctx, &path, content.as_bytes())?;
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
    let updated = if replace_all { text.replace(old, new) } else { text.replacen(old, new, 1) };
    write_with_checkpoint(ctx, &path, updated.as_bytes())?;
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

#[cfg(test)]
mod undo_tests {
    use super::*;
    use crate::config::Config;
    use crate::memory::Memory;
    use crate::store::Store;
    use std::sync::Arc;

    fn ctx(ws: &Path) -> ToolCtx {
        let store = Store::in_memory().unwrap();
        ToolCtx { workspace: dunce::canonicalize(ws).unwrap(), memory: Memory::new(store.clone(), None), store, session_id: "s".into(), cfg: Arc::new(Config::default()), tainted: Default::default(), seen_urls: Default::default() }
    }

    #[tokio::test]
    async fn undo_restores_only_what_is_still_as_heron_left_it() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(dir.path());
        let file = c.workspace.join("notes.md");
        std::fs::write(&file, "the user's original").unwrap();

        // The ordinary case: Heron edits, nobody touches it, undo puts the original back.
        write_file(&json!({"path": "notes.md", "content": "Heron's version"}), &c).await.unwrap();
        assert_eq!(undo(&c, None).unwrap(), Undo::Done(format!("restored {}.", file.display())));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "the user's original");
        assert_eq!(undo(&c, None).unwrap(), Undo::Nothing, "the checkpoint is used up");

        // Heron edits, then the user edits. Undo must not throw the user's edit away.
        write_file(&json!({"path": "notes.md", "content": "Heron's version"}), &c).await.unwrap();
        std::fs::write(&file, "the user's later edit").unwrap();
        let Undo::Conflict { id: asked_about, why } = undo(&c, None).unwrap() else { panic!("expected a refusal") };
        assert!(why.contains("changed since"), "{why}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "the user's later edit", "nothing was touched");
        assert!(undo_last(&c).is_err(), "the plain form reports it as a failure");
        assert!(c.store.last_checkpoint("s").unwrap().is_some(), "and the checkpoint is still there to use");

        // If they insist, their version is kept beside the file before the old one comes back.
        let Undo::Done(said) = undo(&c, Some(asked_about)).unwrap() else { panic!("expected a restore") };
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "the user's original");
        let kept: Vec<_> = std::fs::read_dir(&c.workspace).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().starts_with("notes.md.before-undo-")).collect();
        assert_eq!(kept.len(), 1, "{said}");
        assert_eq!(std::fs::read_to_string(kept[0].path()).unwrap(), "the user's later edit");
        assert_eq!(undo(&c, None).unwrap(), Undo::Nothing);
        assert!(matches!(undo(&c, Some(asked_about)).unwrap(), Undo::Stale(_)), "saying yes twice does nothing the second time");

        // A file Heron created and the user then filled in is not deleted by undo.
        write_file(&json!({"path": "new.md", "content": "draft"}), &c).await.unwrap();
        std::fs::write(c.workspace.join("new.md"), "the user wrote a lot here").unwrap();
        assert!(matches!(undo(&c, None).unwrap(), Undo::Conflict { .. }));
        assert!(c.workspace.join("new.md").exists());
        // ...and one the user already deleted needs no undoing.
        std::fs::remove_file(c.workspace.join("new.md")).unwrap();
        assert!(matches!(undo(&c, None).unwrap(), Undo::Done(said) if said.contains("already removed")));

        // A file the user removed after Heron edited it is reported, not silently recreated.
        std::fs::write(&file, "v1").unwrap();
        edit_file(&json!({"path": "notes.md", "old_string": "v1", "new_string": "v2"}), &c).await.unwrap();
        std::fs::remove_file(&file).unwrap();
        assert!(matches!(undo(&c, None).unwrap(), Undo::Conflict { why, .. } if why.contains("removed or moved")));
        assert!(!file.exists());
    }

    #[tokio::test]
    async fn forced_undo_never_replaces_an_earlier_saved_copy_or_loosens_who_can_read_it() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(dir.path());
        let file = c.workspace.join("plan.md");
        std::fs::write(&file, "original").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        }

        // Two of Heron's edits, each followed by one of the user's, then two forced restores straight
        // after each other (well inside one second, so both copies would want the same name).
        write_file(&json!({"path": "plan.md", "content": "Heron 1"}), &c).await.unwrap();
        std::fs::write(&file, "the user's first edit").unwrap();
        write_file(&json!({"path": "plan.md", "content": "Heron 2"}), &c).await.unwrap();
        std::fs::write(&file, "the user's second edit").unwrap();
        let restore_anyway = |c: &ToolCtx| {
            let Undo::Conflict { id, .. } = undo(c, None).unwrap() else { panic!("expected to be asked first") };
            undo(c, Some(id)).unwrap()
        };
        assert!(matches!(restore_anyway(&c), Undo::Done(_)));
        // Restoring put "the user's first edit" back, which is what the earlier checkpoint expects to
        // find changed: Heron left "Heron 1" there.
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "the user's first edit");
        assert!(matches!(restore_anyway(&c), Undo::Done(_)));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "original");

        let mut kept: Vec<String> = std::fs::read_dir(&c.workspace)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("plan.md.before-undo-"))
            .map(|e| std::fs::read_to_string(e.path()).unwrap())
            .collect();
        kept.sort();
        assert_eq!(kept, ["the user's first edit", "the user's second edit"], "both versions survive");

        // A private file stays private, and so do the copies saved from it.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for entry in std::fs::read_dir(&c.workspace).unwrap().filter_map(|e| e.ok()) {
                let mode = entry.metadata().unwrap().permissions().mode() & 0o777;
                assert_eq!(mode, 0o600, "{} is readable by others", entry.path().display());
            }
        }

        // The helper itself refuses to write over anything.
        std::fs::write(c.workspace.join("taken"), "do not replace").unwrap();
        assert!(create_new_like(&c.workspace.join("taken"), None).is_err());
        assert_eq!(std::fs::read_to_string(c.workspace.join("taken")).unwrap(), "do not replace");
    }

    #[tokio::test]
    async fn restoring_anyway_applies_only_to_the_change_that_was_asked_about() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(dir.path());
        let (a, b) = (c.workspace.join("a.md"), c.workspace.join("b.md"));
        std::fs::write(&a, "a: original").unwrap();
        std::fs::write(&b, "b: original").unwrap();

        // Heron edits a.md, the user edits it too, and undo asks.
        write_file(&json!({"path": "a.md", "content": "a: Heron"}), &c).await.unwrap();
        std::fs::write(&a, "a: the user's edit").unwrap();
        let Undo::Conflict { id: about_a, .. } = undo(&c, None).unwrap() else { panic!("expected to be asked") };

        // Before the answer arrives Heron changes b.md. "Yes, restore anyway" was said about a.md:
        // it must not undo b.md, which nobody was asked about.
        write_file(&json!({"path": "b.md", "content": "b: Heron"}), &c).await.unwrap();
        assert!(matches!(undo(&c, Some(about_a)).unwrap(), Undo::Stale(why) if why.contains("no longer the change")));
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "b: Heron");
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "a: the user's edit");

        // Asked afresh, b.md is undone cleanly, and then a.md's question comes round again.
        assert!(matches!(undo(&c, None).unwrap(), Undo::Done(_)));
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "b: original");
        let Undo::Conflict { id: again, .. } = undo(&c, None).unwrap() else { panic!("expected to be asked") };
        assert_eq!(again, about_a);
        assert!(matches!(undo(&c, Some(again)).unwrap(), Undo::Done(_)));
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "a: original");
    }

    #[tokio::test]
    async fn a_change_that_never_landed_or_was_put_back_by_hand_is_not_restored_over_newer_work() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(dir.path());
        let file = c.workspace.join("notes.md");
        std::fs::write(&file, "original").unwrap();

        // Heron stopped after noting the change it was about to make and before making it. The note
        // says what the file should have become, so a later edit by the user is seen as a conflict
        // (a note without that would have been restored over the user's edit without asking).
        c.store.save_checkpoint("s", &file.to_string_lossy(), Some(b"original"), Some(&content_hash(b"Heron's version"))).unwrap();
        std::fs::write(&file, "the user's edit after the crash").unwrap();
        assert!(matches!(undo(&c, None).unwrap(), Undo::Conflict { .. }));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "the user's edit after the crash");

        // If the file is simply still what it was before (the change never landed, or the user put
        // it back), there is nothing to restore and the note is cleared.
        std::fs::write(&file, "original").unwrap();
        assert!(matches!(undo(&c, None).unwrap(), Undo::Done(said) if said.contains("already as it was")));
        assert_eq!(undo(&c, None).unwrap(), Undo::Nothing);

        // The last-moment check: the file is as expected when undo looks, and changes before the
        // old version takes its place. Nothing is replaced.
        let changed = std::cell::Cell::new(false);
        let replaced = write_atomic_if(&file, b"old version", &|| {
            std::fs::write(&file, "saved from an editor just now").unwrap();
            changed.set(true);
            false
        })
        .unwrap();
        assert!(changed.get() && !replaced);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "saved from an editor just now");
        let stray: Vec<_> = std::fs::read_dir(&c.workspace).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().contains(".heron-")).collect();
        assert!(stray.is_empty(), "and the unused new version is cleaned up");
    }

    #[tokio::test]
    async fn a_write_that_fails_leaves_nothing_to_undo_and_no_stray_files() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(dir.path());
        // The target is a folder, so the write cannot succeed.
        std::fs::create_dir(c.workspace.join("taken")).unwrap();
        assert!(write_file(&json!({"path": "taken", "content": "x"}), &c).await.is_err());
        assert_eq!(undo(&c, None).unwrap(), Undo::Nothing, "a change that did not happen is not offered for undo");
        let stray: Vec<_> = std::fs::read_dir(&c.workspace).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().contains(".heron-")).collect();
        assert!(stray.is_empty(), "no temporary file is left behind");

        // Writing keeps a script executable, and replaces a link instead of writing through it.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let script = c.workspace.join("run.sh");
            std::fs::write(&script, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
            write_file(&json!({"path": "run.sh", "content": "#!/bin/sh\necho hi\n"}), &c).await.unwrap();
            assert_eq!(std::fs::metadata(&script).unwrap().permissions().mode() & 0o777, 0o755);
        }
    }
}
