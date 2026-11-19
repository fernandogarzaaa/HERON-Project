//! File tools: read, write, edit, list, search. Writes are checkpointed so they can be undone.

use super::{arg_str, opt_bool, opt_i64, opt_str, schema, FnTool, ToolCtx};
use crate::config::Config;
use crate::permissions::{classify_path, resolve_path, stricter, unresolvable, without_data_volume, Effect, ReadRoots};
use crate::util::preview;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const MAX_READ_BYTES: u64 = 10 * 1024 * 1024;

fn path_summary(args: &Value) -> String {
    args.get("path").and_then(|p| p.as_str()).unwrap_or(".").to_string()
}

/// The canonical path a file-reading call reads (the `path` argument, default the working folder).
fn read_target(args: &Value, ctx: &ToolCtx) -> PathBuf {
    resolve_path(&ctx.workspace, &path_summary(args))
}

/// The path as written (joined to the working folder, `~` expanded, `..` removed) without following
/// links. It is classified next to the resolved path, and the stricter answer wins.
fn written_path(ctx: &ToolCtx, raw: &str) -> PathBuf {
    let raw = raw.trim();
    let expanded = match raw.strip_prefix("~/") {
        Some(rest) => dirs::home_dir().unwrap_or_default().join(rest),
        None if raw == "~" => dirs::home_dir().unwrap_or_default(),
        None => PathBuf::from(raw),
    };
    let joined = if expanded.is_absolute() { expanded } else { ctx.workspace.join(expanded) };
    let mut out = PathBuf::new();
    for c in joined.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Read tiers: Heron's own home, CLI logins and personal stores are Refused; secrets ask; the
/// user's folders outside the working folder ask unless a folder was granted. Both the name given
/// and the file a link really points at are judged, so a harmless-looking link in a repository
/// cannot lead a read to a secret.
fn read_risk(args: &Value, ctx: &ToolCtx) -> Effect {
    let raw = path_summary(args);
    let resolved = classify_path(&resolve_path(&ctx.workspace, &raw), &ctx.workspace, &ctx.cfg, false);
    stricter(resolved, classify_path(&written_path(ctx, &raw), &ctx.workspace, &ctx.cfg, false))
}

/// The agent must never quietly rewrite its own settings, permission rules, skills, memory or keys:
/// a write into Heron's home is Refused, like a read.
fn write_risk(args: &Value, ctx: &ToolCtx) -> Effect {
    let raw = path_summary(args);
    let resolved = classify_path(&resolve_path(&ctx.workspace, &raw), &ctx.workspace, &ctx.cfg, true);
    stricter(resolved, classify_path(&written_path(ctx, &raw), &ctx.workspace, &ctx.cfg, true))
}

fn content_hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Replace a file in one step: the new content is written beside it and renamed over it, so a crash
/// or a full disk can never leave half a file, and a link put in its place is replaced, not followed.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = stage_new_version(path, bytes)?;
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let done = std::fs::rename(&tmp, path);
    if done.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    done.with_context(|| format!("cannot write {}", path.display()))?;
    sync_dir(dir);
    Ok(())
}

/// Write `bytes` beside `path` under a name nothing else has, with the original's permissions, and
/// return that name. The new version is on disk, fsynced, before this returns; the caller decides
/// how it takes the file's place.
fn stage_new_version(path: &Path, bytes: &[u8]) -> Result<PathBuf> {
    use std::io::Write;
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let tmp = dir.join(format!(
        ".{name}.heron-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ));
    // The file being replaced decides who may read the new one, from the first byte: a private file's
    // new content must never sit beside it readable by others, even for a moment.
    let like = std::fs::metadata(path).ok().map(|m| m.permissions());
    let mut f = create_new_like(&tmp, like.as_ref())?;
    let written = f.write_all(bytes).and_then(|_| f.sync_all());
    drop(f);
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("cannot write {}", path.display()));
    }
    // Keep the file's permissions exactly (an executable script stays executable).
    if let Some(perms) = like {
        let _ = std::fs::set_permissions(&tmp, perms);
    }
    Ok(tmp)
}

/// Replace `path` with `bytes`, but only if the file still holds `expected`: `Some` bytes it must
/// still contain, `None` that it must still be absent. Returns `true` when the file was replaced
/// and `false` when it had changed (or vanished, or appeared) and nothing was touched.
///
/// The replacement is an atomic exchange, not a check followed by a rename: the new version and
/// the old one change places in one operating-system step, and what was displaced is then read
/// back and compared with `expected`. A save that landed before the exchange is caught and the
/// exchange is undone, so it is never overwritten; a save after it replaces the new version
/// normally. When the platform or filesystem cannot exchange, it falls back to reading the file
/// again immediately before a plain rename, which is the narrowed check undo used before (T11).
/// If the verification fails, the rollback puts the displaced content back only while the target
/// still holds the staged version, and restores any save that landed in between, so an
/// intervening save is never deleted. If the verification itself errors after the exchange, the
/// rollback runs and the error is reported; if the rollback also fails, the displaced file is
/// left in place (never deleted) so the version survives on disk. A writer holding the file
/// open and writing in place between the verification and the cleanup can still lose that one
/// write; editors save by renaming, so they are not affected.
fn replace_if_unchanged(path: &Path, bytes: &[u8], expected: &Option<Vec<u8>>) -> Result<bool> {
    let tmp = stage_new_version(path, bytes)?;
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    // After a successful exchange, `tmp` holds the displaced content on Unix (the staged
    // version moved into the target). Deleting it on an error after that point would destroy
    // the user's version, so the exchange paths set `retain_displaced` when the displaced
    // file must survive, and the cleanup below respects it.
    let mut retain_displaced = false;
    let done = (|| -> Result<bool> {
        match atomic_replace(path, &tmp)? {
            Some(displaced) => {
                let was = match read_if_there(&displaced) {
                    Ok(was) => was,
                    Err(e) => {
                        // The new version is installed but the displaced content cannot be
                        // read to verify it. Roll back so the file is as it was, and report
                        // the error. If the rollback fails, the displaced file is left in
                        // place (not deleted) so the version survives on disk.
                        if let Err(restore_err) = atomic_restore(path, &displaced, bytes) {
                            retain_displaced = true;
                            return Err(restore_err).with_context(|| {
                                format!("the displaced version was kept at {}", displaced.display())
                            });
                        }
                        return Err(e);
                    }
                };
                if was == *expected {
                    // The exchange displaced exactly what was expected: the new version is in
                    // place and the displaced copy is no longer needed.
                    let _ = std::fs::remove_file(&displaced);
                    sync_dir(dir);
                    Ok(true)
                } else {
                    // The file changed before the exchange: put it back and report that
                    // nothing was replaced. If the rollback fails, the displaced file is
                    // left in place so the version survives on disk.
                    if let Err(restore_err) = atomic_restore(path, &displaced, bytes) {
                        retain_displaced = true;
                        return Err(restore_err).with_context(|| {
                            format!("the displaced version was kept at {}", displaced.display())
                        });
                    }
                    let _ = std::fs::remove_file(&tmp);
                    Ok(false)
                }
            }
            None => {
                // No exchange here: read the file once more and only rename when it still
                // holds what was expected.
                if read_if_there(path)? == *expected {
                    std::fs::rename(&tmp, path)?;
                    sync_dir(dir);
                    Ok(true)
                } else {
                    let _ = std::fs::remove_file(&tmp);
                    Ok(false)
                }
            }
        }
    })();
    if done.is_err() && !retain_displaced {
        let _ = std::fs::remove_file(&tmp);
    }
    done.with_context(|| format!("cannot write {}", path.display()))
}

/// Atomically install the staged file `tmp` at `path`, and return the path now holding what was
/// displaced, or `None` when this platform or filesystem cannot do the exchange and the caller
/// should fall back to a plain rename after re-checking.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn atomic_replace(path: &Path, tmp: &Path) -> Result<Option<PathBuf>> {
    if swap_files(path, tmp)? {
        return Ok(Some(tmp.to_path_buf()));
    }
    Ok(None)
}

/// Atomically install the staged file `tmp` at `path` on Windows: `ReplaceFileW` replaces the
/// target and moves what was displaced to a backup path beside it, which is returned. `None`
/// when the target is missing or the replacement cannot be done here.
#[cfg(windows)]
fn atomic_replace(path: &Path, tmp: &Path) -> Result<Option<PathBuf>> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let backup = dir.join(format!(
        ".{name}.heron-backup-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let wide = |p: &Path| {
        p.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>()
    };
    let replaced = wide(path);
    let replacement = wide(tmp);
    let backup_wide = wide(&backup);
    // SAFETY: the three pointers are null-terminated UTF-16 strings that live for the call, and
    // the last two arguments are null. ReplaceFileW only reads them.
    let ok = unsafe {
        ReplaceFileW(
            replaced.as_ptr(),
            replacement.as_ptr(),
            backup_wide.as_ptr(),
            0,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if ok != 0 {
        return Ok(Some(backup));
    }
    // Any failure falls back to a plain rename after re-checking: the rename reports a real
    // error (permissions, a missing target) itself, and an unsupported filesystem simply gets
    // the narrowed check instead of the exchange.
    Ok(None)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn atomic_replace(path: &Path, tmp: &Path) -> Result<Option<PathBuf>> {
    let _ = (path, tmp);
    Ok(None)
}

/// Put the displaced content back at `path` after a failed verification, so the file is exactly
/// as it was before the exchange. The rollback only happens while the target still holds what
/// the first exchange installed: a save that landed since is never moved into the displaced
/// path and deleted. On Unix the two files change places again and the staged file is removed,
/// and a save that landed between the check and the swap is put back at the target instead of
/// being deleted; on Windows the displaced backup is moved back over the target, unless the
/// target no longer holds what was installed, in which case a writer has saved over it and the
/// writer's version is left standing.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn atomic_restore(path: &Path, displaced: &Path, installed: &[u8]) -> Result<()> {
    // Roll back only while the target still holds what the first exchange installed. A save
    // that landed since would be moved into the displaced path by the swap and deleted with
    // it, so the writer's version stands instead and the displaced copy is discarded.
    if read_if_there(path)? != Some(installed.to_vec()) {
        let _ = std::fs::remove_file(displaced);
        if let Some(dir) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            sync_dir(dir);
        }
        return Ok(());
    }
    if !swap_files(path, displaced)? {
        bail!(
            "could not put back {} after a changed restore",
            path.display()
        );
    }
    // The swap put what was at the target into the displaced path. It should be the staged
    // version, which is no longer needed. If a save landed between the check above and the
    // swap, it is there instead: put it back at the target rather than deleting it.
    if read_if_there(displaced)? != Some(installed.to_vec()) {
        std::fs::rename(displaced, path)
            .with_context(|| format!("could not put back {}", path.display()))?;
    } else {
        let _ = std::fs::remove_file(displaced);
    }
    if let Some(dir) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        sync_dir(dir);
    }
    Ok(())
}

#[cfg(windows)]
fn atomic_restore(path: &Path, displaced: &Path, installed: &[u8]) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;
    if read_if_there(path)? == Some(installed.to_vec()) {
        let wide = |p: &Path| {
            p.as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect::<Vec<u16>>()
        };
        let replaced = wide(path);
        let replacement = wide(displaced);
        // SAFETY: the pointers are null-terminated UTF-16 strings that live for the call, and
        // the last three arguments are null. ReplaceFileW only reads them.
        let ok = unsafe {
            ReplaceFileW(
                replaced.as_ptr(),
                replacement.as_ptr(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error())
                .with_context(|| format!("could not put back {}", path.display()));
        }
    } else {
        // A writer saved over the target after the exchange: restoring would destroy their
        // save, so their version stands and the displaced copy is discarded.
        let _ = std::fs::remove_file(displaced);
    }
    if let Some(dir) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        sync_dir(dir);
    }
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn atomic_restore(path: &Path, displaced: &Path, _installed: &[u8]) -> Result<()> {
    let _ = std::fs::remove_file(displaced);
    let _ = path;
    Ok(())
}

/// Atomically exchange the files at `a` and `b`. Returns `Ok(true)` when the exchange happened,
/// `Ok(false)` when the target is missing or the filesystem cannot exchange (the caller falls
/// back to a plain rename after re-checking), and `Err` for anything else. The displaced content
/// ends up at the other path, so the caller can verify it is what was expected.
#[cfg(target_os = "linux")]
fn swap_files(a: &Path, b: &Path) -> Result<bool> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let a = CString::new(a.as_os_str().as_bytes())?;
    let b = CString::new(b.as_os_str().as_bytes())?;
    // SAFETY: both pointers are valid C strings that live for the call, and the call only
    // reads them.
    let rc = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            a.as_ptr(),
            libc::AT_FDCWD,
            b.as_ptr(),
            libc::RENAME_EXCHANGE,
        )
    };
    if rc == 0 {
        return Ok(true);
    }
    let err = std::io::Error::last_os_error();
    match err.raw_os_error() {
        // The target is missing, or the filesystem does not support the exchange: fall back.
        Some(libc::ENOENT) | Some(libc::ENOSYS) | Some(libc::EINVAL) => Ok(false),
        _ => Err(err.into()),
    }
}

#[cfg(target_os = "macos")]
fn swap_files(a: &Path, b: &Path) -> Result<bool> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let a = CString::new(a.as_os_str().as_bytes())?;
    let b = CString::new(b.as_os_str().as_bytes())?;
    // SAFETY: both pointers are valid C strings that live for the call, and the call only
    // reads them.
    let rc = unsafe { libc::renamex_np(a.as_ptr(), b.as_ptr(), libc::RENAME_SWAP) };
    if rc == 0 {
        return Ok(true);
    }
    let err = std::io::Error::last_os_error();
    match err.raw_os_error() {
        // The target is missing, or the filesystem does not support the exchange: fall back.
        Some(libc::ENOENT) | Some(libc::EINVAL) | Some(libc::ENOTSUP) => Ok(false),
        _ => Err(err.into()),
    }
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
    /// `id` names the change that was asked about, so a later "restore anyway" can be tied to exactly
    /// this one. It is derived from the change itself (see [`confirmation`]), not a row number.
    Conflict { id: String, why: String },
    /// "Restore anyway" was asked for a checkpoint that is no longer the latest (Heron has changed
    /// something else since the question was put). Nothing was touched.
    Stale(String),
}

/// The name a "restore anyway" has to quote back. It is made from what the change is (which file,
/// what was there before, what Heron left, when), not only from the checkpoint's row number: the
/// database hands a deleted row's number to the next row, so a number alone could come to mean a
/// different change while an old question is still on someone's screen.
fn confirmation(id: i64, made_at: i64, path: &str, prev: &Option<Vec<u8>>, after: &Option<String>) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    let before = prev.as_ref().map(|bytes| content_hash(bytes)).unwrap_or_else(|| "absent".into());
    for field in [id.to_string(), made_at.to_string(), path.to_string(), before, after.clone().unwrap_or_else(|| "none".into())] {
        h.update((field.len() as u64).to_le_bytes());
        h.update(field.as_bytes());
    }
    h.finalize().iter().take(16).map(|b| format!("{b:02x}")).collect()
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
/// checkpoint stays. The answer names the change. Passing that name back as `restore_anyway` means
/// "I have seen that, restore it regardless": the current content is first saved beside the file and
/// then the old version is restored. If the latest checkpoint is by then a different one, nothing is
/// done, because that is not what was agreed to. The checkpoint is removed only after the restore has
/// succeeded.
pub fn undo(ctx: &ToolCtx, restore_anyway: Option<&str>) -> Result<Undo> {
    let Some((id, path, prev, after)) = ctx.store.last_checkpoint(&ctx.session_id)? else {
        return Ok(match restore_anyway {
            Some(_) => Undo::Stale("That change has already been undone. Nothing was restored.".into()),
            None => Undo::Nothing,
        });
    };
    let made_at = ctx.store.checkpoint_made_at(id)?.unwrap_or(0);
    let name = confirmation(id, made_at, &path, &prev, &after);
    if restore_anyway.is_some_and(|confirmed| confirmed != name) {
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
                    id: name,
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
    // The replacement is an atomic exchange that verifies what it displaced (see
    // replace_if_unchanged), so a save that lands before it is caught and undone. Removing a
    // file has no such primitive: it is still asked at the last moment whether the file is
    // still what was just looked at, and stops if it changed.
    let still_as_seen = || read_if_there(file).map(|now| now == current).unwrap_or(false);
    let changed_meanwhile = || Undo::Conflict { id: name.clone(), why: format!("{path} changed while it was being restored. Nothing was restored.{kept}") };
    let said = match prev {
        Some(bytes) => {
            if !replace_if_unchanged(file, &bytes, &current)? {
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
    let path = walk_root(args, ctx)?;
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

/// What one `search_files` or `find_files` call may open. Refused locations and secrets are never
/// opened. A file in the user's own folders outside the working folder (`OutsideRead`) is opened only
/// when the call's own read target was itself such a read (and so was asked about or granted) and the
/// file really lies under it, or when a grant covers it: a standing one, or one of this conversation's
/// while it is not tainted. A `read:` grant covers its folder; a `read=:` grant covers its one path and
/// nothing under it, even when it is the walk's own root. A link in the working folder that points
/// into the Ask tier, or a walk rooted at `/` (which is `LocalRead`), reaches nothing the user did not
/// open.
struct WalkScope {
    workspace: PathBuf,
    cfg: std::sync::Arc<Config>,
    roots: std::sync::Arc<ReadRoots>,
    /// Folders under which an `OutsideRead` entry may be opened.
    folders: Vec<PathBuf>,
    /// Single paths that may be opened, and nothing under them.
    exact: Vec<PathBuf>,
}

/// A path as written and canonical, each also without the `/System/Volumes/Data` prefix.
fn path_forms(p: PathBuf) -> Vec<PathBuf> {
    let real = dunce::canonicalize(&p).unwrap_or_else(|_| p.clone());
    [p, real].into_iter().flat_map(|p| [without_data_volume(&p), Some(p)]).flatten().collect()
}

impl WalkScope {
    fn new(root: &Path, root_effect: Effect, ctx: &ToolCtx) -> WalkScope {
        use crate::sandbox::ReadGrant;
        // A conversation grant stops applying once outside content is present, as it does for a read.
        // Standing rules come from the run's live policy: one revoked mid-run no longer opens files.
        let grants = ctx.read_grants();
        let (mut folders, mut exact) = (vec![], vec![]);
        for grant in grants {
            match grant {
                ReadGrant::Folder(p) => folders.extend(path_forms(p)),
                ReadGrant::Exact(p) => exact.extend(path_forms(p)),
            }
        }
        // The call's own root opens its folder when the call was itself an outside read, so was asked
        // about or covered by a grant. A `read=:` grant on the root is the one exception: it covers
        // that one path, so a walk it let through opens nothing under it.
        let root_forms = path_forms(root.to_path_buf());
        let exact_on_root = root_forms.iter().any(|r| exact.contains(r));
        if root_effect == Effect::OutsideRead && !exact_on_root {
            folders.extend(root_forms);
        }
        WalkScope { workspace: ctx.workspace.clone(), cfg: ctx.cfg.clone(), roots: ReadRoots::of(&ctx.cfg), folders, exact }
    }

    /// The place this path really is (links followed), also without the data-volume prefix.
    fn real_forms(path: &Path) -> Vec<PathBuf> {
        let real = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        [without_data_volume(&real), Some(real)].into_iter().flatten().collect()
    }

    fn under_folder(&self, path: &Path) -> bool {
        Self::real_forms(path).iter().any(|p| self.folders.iter().any(|o| p.starts_with(o)))
    }

    /// Does the place this path really is lie under an opened folder, or is it an opened path?
    fn covers(&self, path: &Path) -> bool {
        self.under_folder(path) || Self::real_forms(path).iter().any(|p| self.exact.contains(p))
    }

    /// Files the search tools pass over.
    fn skips(&self, path: &Path) -> bool {
        match classify_path(path, &self.workspace, &self.cfg, false) {
            Effect::Refused | Effect::SensitiveRead => true,
            Effect::OutsideRead => !self.covers(path),
            _ => false,
        }
    }

    /// Folders the walk does not enter: Refused ones (by location, or because where they lead can't be
    /// determined), and Ask-tier ones that no folder grant opens and that hold neither the working
    /// folder nor anything opened (every file in them would be skipped).
    fn prunes(&self, dir: &Path) -> bool {
        if self.roots.refused(dir).is_some() {
            return true;
        }
        let effect = classify_path(dir, &self.workspace, &self.cfg, false);
        if effect == Effect::Refused {
            return true;
        }
        let leads_somewhere = self.workspace.starts_with(dir) || self.folders.iter().any(|o| o.starts_with(dir)) || self.exact.iter().any(|e| e.starts_with(dir) && e != dir);
        !leads_somewhere && effect == Effect::OutsideRead && !self.under_folder(dir)
    }
}

/// The folder `list_dir`, `search_files` or `find_files` starts from. A root whose real location
/// can't be determined (a `/.vol` path, say) is refused before anything is opened.
fn walk_root(args: &Value, ctx: &ToolCtx) -> Result<PathBuf> {
    let raw = opt_str(args, "path").unwrap_or(".");
    let root = resolve_path(&ctx.workspace, raw);
    if unresolvable(&root) {
        bail!("{raw} can't be resolved to where it really is, so Heron won't read it");
    }
    Ok(root)
}

/// Walk a folder for the search tools. Build output is skipped, and a folder [`WalkScope::prunes`] is
/// never entered.
fn walker(root: &Path, scope: std::sync::Arc<WalkScope>) -> ignore::Walk {
    ignore::WalkBuilder::new(root)
        .hidden(false)
        .filter_entry(move |e| {
            if matches!(e.file_name().to_str(), Some(".git" | "node_modules" | "target" | ".venv" | "__pycache__")) {
                return false;
            }
            let is_dir = e.file_type().is_some_and(|t| t.is_dir());
            scope.roots.refused(e.path()).is_none() && !(is_dir && e.depth() > 0 && scope.prunes(e.path()))
        })
        .build()
}

async fn search_files(args: &Value, ctx: &ToolCtx) -> Result<String> {
    let pattern = arg_str(args, "pattern")?.to_string();
    let root = walk_root(args, ctx)?;
    let glob = match opt_str(args, "glob") {
        Some(g) => Some(globset::Glob::new(g).map_err(|e| anyhow!("bad glob: {e}"))?.compile_matcher()),
        None => None,
    };
    let re = regex::RegexBuilder::new(&pattern).case_insensitive(opt_bool(args, "ignore_case")).build().map_err(|e| anyhow!("bad regex: {e}"))?;
    let max = 200usize;
    let scope = std::sync::Arc::new(WalkScope::new(&root, read_risk(args, ctx), ctx));
    tokio::task::spawn_blocking(move || {
        let mut out = String::new();
        let mut hits = 0usize;
        'files: for entry in walker(&root, scope.clone()).flatten() {
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
            if scope.skips(p) {
                continue; // never grep through credential files, Heron's own data or unopened folders
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
    let root = walk_root(args, ctx)?;
    let g = globset::Glob::new(pattern).map_err(|e| anyhow!("bad glob: {e}"))?.compile_matcher();
    let scope = std::sync::Arc::new(WalkScope::new(&root, read_risk(args, ctx), ctx));
    tokio::task::spawn_blocking(move || {
        // A name is listed only where its file could be read.
        let mut found: Vec<String> = walker(&root, scope.clone())
            .flatten()
            .filter(|e| e.path().is_file() && !scope.skips(e.path()))
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
            target: Some(read_target),
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
            target: None,
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
            target: None,
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
            target: Some(read_target),
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
            target: Some(read_target),
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
            target: Some(read_target),
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
        let Undo::Done(said) = undo(&c, Some(&asked_about)).unwrap() else { panic!("expected a restore") };
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "the user's original");
        let kept: Vec<_> = std::fs::read_dir(&c.workspace).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().starts_with("notes.md.before-undo-")).collect();
        assert_eq!(kept.len(), 1, "{said}");
        assert_eq!(std::fs::read_to_string(kept[0].path()).unwrap(), "the user's later edit");
        assert_eq!(undo(&c, None).unwrap(), Undo::Nothing);
        assert!(matches!(undo(&c, Some(&asked_about)).unwrap(), Undo::Stale(_)), "saying yes twice does nothing the second time");

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
            undo(c, Some(&id)).unwrap()
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
        assert!(matches!(undo(&c, Some(&about_a)).unwrap(), Undo::Stale(why) if why.contains("no longer the change")));
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "b: Heron");
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "a: the user's edit");

        // Asked afresh, b.md is undone cleanly, and then a.md's question comes round again.
        assert!(matches!(undo(&c, None).unwrap(), Undo::Done(_)));
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "b: original");
        let Undo::Conflict { id: again, .. } = undo(&c, None).unwrap() else { panic!("expected to be asked") };
        assert_eq!(again, about_a);
        assert!(matches!(undo(&c, Some(&again)).unwrap(), Undo::Done(_)));
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "a: original");
    }

    #[tokio::test]
    async fn an_old_question_cannot_authorise_a_later_change_that_got_the_same_row_number() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(dir.path());
        let (a, b) = (c.workspace.join("a.md"), c.workspace.join("b.md"));
        std::fs::write(&a, "a: original").unwrap();
        std::fs::write(&b, "b: original").unwrap();

        // A question about a.md is left open on one screen...
        write_file(&json!({"path": "a.md", "content": "a: Heron"}), &c).await.unwrap();
        std::fs::write(&a, "a: the user's edit").unwrap();
        let Undo::Conflict { id: old_question, .. } = undo(&c, None).unwrap() else { panic!("expected to be asked") };
        let row_then = c.store.last_checkpoint("s").unwrap().unwrap().0;

        // ...while it is answered from somewhere else (another window, the terminal).
        let Undo::Conflict { id, .. } = undo(&c, None).unwrap() else { panic!() };
        assert!(matches!(undo(&c, Some(&id)).unwrap(), Undo::Done(_)));

        // Heron then changes a different file. The database gives that change the row number the
        // old one had, and the user edits the file afterwards.
        write_file(&json!({"path": "b.md", "content": "b: Heron"}), &c).await.unwrap();
        assert_eq!(c.store.last_checkpoint("s").unwrap().unwrap().0, row_then, "the row number really is handed out again");
        std::fs::write(&b, "b: the user's edit").unwrap();

        // Pressing the old "Restore anyway" must not touch b.md: nobody was asked about it.
        assert!(matches!(undo(&c, Some(&old_question)).unwrap(), Undo::Stale(_)));
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "b: the user's edit");
        // Asked properly, it gets its own question with its own name.
        let Undo::Conflict { id: new_question, .. } = undo(&c, None).unwrap() else { panic!("expected to be asked") };
        assert_ne!(new_question, old_question);
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

        // The exchange check: the file is as expected when undo looks, and changes before the
        // old version takes its place. Nothing is replaced.
        std::fs::write(&file, "saved from an editor just now").unwrap();
        let replaced =
            replace_if_unchanged(&file, b"old version", &Some(b"original".to_vec())).unwrap();
        assert!(!replaced);
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "saved from an editor just now"
        );
        let stray: Vec<_> = std::fs::read_dir(&c.workspace)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".heron-"))
            .collect();
        assert!(stray.is_empty(), "and the unused new version is cleaned up");
    }

    #[tokio::test]
    async fn replace_if_unchanged_only_replaces_what_it_was_shown() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes.md");
        let stray = || {
            std::fs::read_dir(dir.path())
                .unwrap()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_name().to_string_lossy().contains(".heron-"))
                .count()
        };

        // The file holds what was expected: the new version takes its place.
        std::fs::write(&file, "as Heron left it").unwrap();
        assert!(
            replace_if_unchanged(&file, b"old version", &Some(b"as Heron left it".to_vec()))
                .unwrap()
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "old version");
        assert_eq!(stray(), 0, "the displaced copy is cleaned up");

        // The file changed since it was looked at: nothing is replaced, and the newer version
        // is left exactly as it is.
        std::fs::write(&file, "saved from an editor just now").unwrap();
        assert!(
            !replace_if_unchanged(&file, b"old version", &Some(b"as Heron left it".to_vec()))
                .unwrap()
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "saved from an editor just now"
        );
        assert_eq!(stray(), 0, "and the unused new version is cleaned up");

        // The file is gone although it was expected to be there: nothing is installed.
        std::fs::remove_file(&file).unwrap();
        assert!(
            !replace_if_unchanged(&file, b"old version", &Some(b"as Heron left it".to_vec()))
                .unwrap()
        );
        assert!(!file.exists());
        assert_eq!(stray(), 0);

        // The file is gone and was expected to be gone: the old version is installed.
        assert!(replace_if_unchanged(&file, b"old version", &None).unwrap());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "old version");
        assert_eq!(stray(), 0);
    }

    #[tokio::test]
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    async fn atomic_restore_keeps_a_save_that_landed_after_the_exchange() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes.md");
        let displaced = dir.path().join("notes.md.displaced");

        // The target still holds what the exchange installed: the rollback puts the old
        // version back and the staged file is gone.
        std::fs::write(&file, "staged").unwrap();
        std::fs::write(&displaced, "old").unwrap();
        atomic_restore(&file, &displaced, b"staged").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "old");
        assert!(!displaced.exists());

        // The target was saved over after the exchange: the writer's version stands, and
        // the displaced copy is discarded, not the writer's save.
        std::fs::write(&file, "the editor's newest save").unwrap();
        std::fs::write(&displaced, "old").unwrap();
        atomic_restore(&file, &displaced, b"staged").unwrap();
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "the editor's newest save"
        );
        assert!(!displaced.exists());
    }

    #[tokio::test]
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    async fn atomic_restore_failure_keeps_the_displaced_version() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes.md");
        let displaced = dir.path().join("notes.md.displaced");

        // The target cannot be read (it is a folder, not a file), so the rollback
        // reports an error and the displaced content is left in place, not deleted,
        // and the target is untouched.
        std::fs::create_dir(&file).unwrap();
        std::fs::write(&displaced, "old").unwrap();
        assert!(atomic_restore(&file, &displaced, b"staged").is_err());
        assert!(file.is_dir());
        assert_eq!(std::fs::read_to_string(&displaced).unwrap(), "old");
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

#[cfg(all(test, unix))]
mod read_gate_tests {
    use super::*;
    use crate::config::Config;
    use crate::memory::Memory;
    use crate::store::Store;
    use std::sync::Arc;

    fn ctx(ws: &Path, home: &Path) -> ToolCtx {
        let store = Store::in_memory().unwrap();
        let cfg = Config { home: home.to_path_buf(), ..Default::default() };
        ToolCtx { workspace: dunce::canonicalize(ws).unwrap(), memory: Memory::new(store.clone(), None), store, session_id: "s".into(), cfg: Arc::new(cfg), tainted: Default::default(), seen_urls: Default::default() }
    }

    fn dirs() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let (ws, home) = (dir.path().join("ws"), dir.path().join("heron-home"));
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        (dir, ws, home)
    }

    #[tokio::test]
    async fn search_never_reads_a_secret_through_a_harmless_looking_link() {
        let (dir, ws, home) = dirs();
        let ssh = dir.path().join("elsewhere/.ssh");
        std::fs::create_dir_all(&ssh).unwrap();
        std::fs::write(ssh.join("id_ed25519"), "-----BEGIN OPENSSH PRIVATE KEY-----\nFAKEKEY\n").unwrap();
        std::os::unix::fs::symlink(ssh.join("id_ed25519"), ws.join("notes.txt")).unwrap();
        std::fs::write(ws.join("plain.txt"), "FAKEKEY is only a word here\n").unwrap();
        let c = ctx(&ws, &home);

        let out = search_files(&json!({"pattern": "BEGIN|FAKEKEY"}), &c).await.unwrap();
        assert!(!out.contains("PRIVATE KEY"), "{out}");
        assert!(out.contains("plain.txt:1:"), "ordinary files are still searched: {out}");
        assert_eq!(read_risk(&json!({"path": "notes.txt"}), &c), Effect::SensitiveRead);
    }

    #[tokio::test]
    async fn heron_own_home_is_a_secret_to_read_and_to_search() {
        let (_dir, ws, home) = dirs();
        std::fs::write(home.join("secrets.json"), r#"{"api_key": "sk-FAKE"}"#).unwrap();
        std::fs::write(home.join("config.toml"), "[mcp.env]\nTOKEN = \"FAKE\"\n").unwrap();
        std::os::unix::fs::symlink(&home, ws.join("settings")).unwrap();
        let c = ctx(&ws, &home);

        // Heron's own home is Refused: no mode or approval opens it.
        for path in [home.join("secrets.json"), home.join("config.toml")] {
            assert_eq!(read_risk(&json!({"path": path.to_string_lossy()}), &c), Effect::Refused, "{}", path.display());
        }
        assert_eq!(read_risk(&json!({"path": "settings/config.toml"}), &c), Effect::Refused);
        let out = search_files(&json!({"pattern": "FAKE", "path": home.to_string_lossy()}), &c).await.unwrap();
        assert_eq!(out, "no matches");
    }

    #[tokio::test]
    async fn ordinary_reads_inside_and_outside_the_workspace_do_not_ask() {
        let (dir, ws, home) = dirs();
        std::fs::write(ws.join("README.md"), "hello").unwrap();
        std::fs::write(dir.path().join("report.txt"), "hello").unwrap();
        let c = ctx(&ws, &home);
        assert_eq!(read_risk(&json!({"path": "README.md"}), &c), Effect::LocalRead);
        assert_eq!(read_risk(&json!({"path": dir.path().join("report.txt").to_string_lossy()}), &c), Effect::LocalRead);
    }
}

#[cfg(test)]
mod tier_tests {
    use super::*;
    use crate::memory::Memory;
    use crate::permissions::{Call, Mode, Policy, Verdict};
    use crate::store::Store;
    use std::sync::Arc;

    fn ctx(ws: &Path, home: &Path) -> ToolCtx {
        let store = Store::in_memory().unwrap();
        let cfg = Config { home: home.to_path_buf(), ..Default::default() };
        ToolCtx { workspace: dunce::canonicalize(ws).unwrap(), memory: Memory::new(store.clone(), None), store, session_id: "s".into(), cfg: Arc::new(cfg), tainted: Default::default(), seen_urls: Default::default() }
    }

    fn dirs_for_test() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let (ws, home) = (dir.path().join("ws"), dir.path().join("heron-home"));
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        (dir, ws, home)
    }

    /// The verdict a read gets, through the tool's own risk and read target.
    fn verdict(c: &ToolCtx, tool: &str, args: &Value, mode: Mode, rules: &[String]) -> (Effect, Verdict) {
        let t = tools().into_iter().find(|t| t.name == tool).unwrap();
        let (effect, summary, target) = ((t.risk)(args, c), (t.summary)(args), t.target.map(|f| f(args, c)));
        let p = Policy::new(mode, rules, &[]);
        (effect, p.decide(&Call { tool, summary: &summary, effect, read_target: target.as_deref() }, c))
    }

    #[test]
    fn every_registry_config_dir_is_refused() {
        let (_dir, ws, home) = dirs_for_test();
        let c = ctx(&ws, &home);
        let user_home = dirs::home_dir().unwrap();
        let mut checked = 0;
        for spec in crate::cli_llm::providers() {
            for dir in spec.config_dirs {
                let folder = dir.strip_prefix("~/").map(|rest| user_home.join(rest)).unwrap_or_else(|| PathBuf::from(dir));
                for path in [folder.clone(), folder.join("auth.json")] {
                    let args = json!({"path": path.to_string_lossy()});
                    for mode in [Mode::ReadOnly, Mode::Ask, Mode::Auto, Mode::Yolo] {
                        let (effect, v) = verdict(&c, "read_file", &args, mode, &["*".into(), "read_file:*".into()]);
                        assert_eq!(effect, Effect::Refused, "{} ({})", path.display(), spec.id);
                        assert!(matches!(v, Verdict::Deny(_)), "{} in {mode:?}: {v:?}", path.display());
                    }
                    // The same through the `~/` form the model would write.
                    let (effect, _) = verdict(&c, "read_file", &json!({"path": format!("{dir}/x")}), Mode::Yolo, &[]);
                    assert_eq!(effect, Effect::Refused, "{dir}/x");
                    checked += 1;
                }
            }
            // The listing tools are refused there too.
            for dir in spec.config_dirs.iter().filter(|d| !d.ends_with(".json")) {
                for tool in ["list_dir", "search_files", "find_files"] {
                    let (effect, _) = verdict(&c, tool, &json!({"path": dir, "pattern": "x", "glob": "*"}), Mode::Yolo, &[]);
                    assert_eq!(effect, Effect::Refused, "{tool} {dir}");
                }
            }
        }
        assert!(checked >= 2 * 20, "every registry entry's folders were checked ({checked})");
        // A CLI config folder chosen in the settings is Refused too.
        let mut custom = (*c.cfg).clone();
        custom.llm.cli_config_dir = ws.join("claude-work").to_string_lossy().to_string();
        let c2 = ToolCtx { cfg: Arc::new(custom), ..c.clone() };
        assert_eq!(read_risk(&json!({"path": "claude-work/.credentials.json"}), &c2), Effect::Refused);
        assert_eq!(read_risk(&json!({"path": "notes.md"}), &c2), Effect::LocalRead);
    }

    #[test]
    fn workspace_env_and_key_files_still_ask() {
        let (_dir, ws, home) = dirs_for_test();
        for f in [".env", ".env.local", "server.pem", "deploy/credentials.json", "id_ed25519", "notes.md"] {
            let p = ws.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, "x").unwrap();
        }
        let c = ctx(&ws, &home);
        for f in [".env", ".env.local", "server.pem", "deploy/credentials.json", "id_ed25519"] {
            for mode in [Mode::ReadOnly, Mode::Ask, Mode::Auto, Mode::Yolo] {
                for tainted in [false, true] {
                    c.tainted.store(tainted, std::sync::atomic::Ordering::SeqCst);
                    let ws_rule = format!("read:{}/**", c.workspace.display());
                    let (effect, v) = verdict(&c, "read_file", &json!({"path": f}), mode, &["*".into(), "read_file:*".into(), ws_rule]);
                    assert_eq!(effect, Effect::SensitiveRead, "{f}");
                    assert_eq!(v, Verdict::Ask, "{f} in {mode:?}, tainted={tainted}");
                }
            }
        }
        c.tainted.store(false, std::sync::atomic::Ordering::SeqCst);
        // An ordinary working-folder file is read without a question, and stays trusted output.
        assert_eq!(verdict(&c, "read_file", &json!({"path": "notes.md"}), Mode::Ask, &[]), (Effect::LocalRead, Verdict::Allow));
        assert!(tools().iter().filter(|t| matches!(t.name, "read_file" | "list_dir" | "search_files" | "find_files")).all(|t| !t.untrusted));
    }

    #[tokio::test]
    async fn walkers_skip_refused_and_secret_paths() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        // Heron's home inside the working folder, as when the working folder is the home folder.
        let home = ws.join(".heron-home");
        for (f, text) in [("notes.md", "TOKEN plain"), (".env", "TOKEN=1"), ("keys/server.pem", "TOKEN pem"), ("deploy/credentials.json", "TOKEN"), (".heron-home/config.toml", "TOKEN home"), (".heron-home/skills/a.md", "TOKEN skill"), ("docs/guide.md", "TOKEN guide")] {
            let p = ws.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, text).unwrap();
        }
        let c = ctx(&ws, &home);
        let found = search_files(&json!({"pattern": "TOKEN"}), &c).await.unwrap();
        assert!(found.contains("notes.md:1:") && found.contains("guide.md:1:"), "{found}");
        for hidden in [".env", "server.pem", "credentials.json", "config.toml", "a.md"] {
            assert!(!found.contains(hidden), "search_files went into {hidden}: {found}");
        }
        let listed = find_files(&json!({"glob": "**/*"}), &c).await.unwrap();
        assert!(listed.contains("notes.md") && listed.contains("guide.md"), "{listed}");
        for hidden in [".env", "server.pem", "credentials.json", "config.toml", "skills"] {
            assert!(!listed.contains(hidden), "find_files listed {hidden}: {listed}");
        }
        // Searching Heron's home directly finds nothing either.
        let inside = search_files(&json!({"pattern": "TOKEN", "path": home.to_string_lossy()}), &c).await.unwrap();
        assert_eq!(inside, "no matches");
        assert_eq!(find_files(&json!({"glob": "*", "path": home.to_string_lossy()}), &c).await.unwrap(), "no files match");

        // A file link in the working folder that points into the Ask tier is passed over: reading it
        // would ask.
        let outside = ask_tier_dir();
        let diary = outside.path().join("diary.txt");
        std::fs::write(&diary, "TOKEN ask-tier").unwrap();
        assert_eq!(classify_path(&diary, &c.workspace, &c.cfg, false), Effect::OutsideRead, "the fixture must be in the Ask tier");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&diary, ws.join("docs/linked.txt")).unwrap();
            let found = search_files(&json!({"pattern": "TOKEN"}), &c).await.unwrap();
            assert!(found.contains("guide.md:1:"), "{found}");
            assert!(!found.contains("linked.txt") && !found.contains("ask-tier"), "search_files followed a link into the Ask tier: {found}");
            let listed = find_files(&json!({"glob": "**/*"}), &c).await.unwrap();
            assert!(listed.contains("guide.md") && !listed.contains("linked.txt"), "{listed}");
        }

        // A walk rooted at `/` is `LocalRead`, yet it reaches every user's home: the same filter both
        // walkers use passes over the Ask tier there, and does not enter the home folder at all. (A real
        // walk of the whole disk is too slow for a test.)
        let slash = json!({"path": "/"});
        assert_eq!(read_risk(&slash, &c), Effect::LocalRead);
        let scope = WalkScope::new(Path::new("/"), read_risk(&slash, &c), &c);
        assert!(scope.skips(&diary), "a /-rooted walk would read {}", diary.display());
        assert!(scope.skips(&c.workspace.join(".env")) && scope.skips(&c.workspace.join(".heron-home/config.toml")));
        assert!(!scope.skips(&c.workspace.join("notes.md")), "the working folder is still searched");
        let user_home = dirs::home_dir().unwrap();
        if !c.workspace.starts_with(&user_home) {
            assert!(scope.prunes(&user_home), "a /-rooted walk enters the home folder");
        }
    }

    /// A folder under the home folder, outside the working folder and not Refused: the Ask tier. It sits
    /// in the build folder, so it is removed with the other build output if a run is cut short.
    fn ask_tier_dir() -> tempfile::TempDir {
        let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
        std::fs::create_dir_all(&target).unwrap();
        tempfile::Builder::new().prefix("heron-ask-tier-test").tempdir_in(target).unwrap()
    }

    /// The Ask-tier entries a walk may open: those under a call that was itself an outside read (and
    /// so was asked about or granted), and those a `read:` grant covers. A conversation grant stops
    /// counting once outside content is present; a standing grant does not.
    #[tokio::test]
    async fn walkers_never_read_uncovered_ask_tier_entries() {
        let (_dir, ws, home) = dirs_for_test();
        std::fs::write(ws.join("notes.md"), "TOKEN plain").unwrap();
        let outside = ask_tier_dir();
        let base = dunce::canonicalize(outside.path()).unwrap();
        let (asked, other) = (base.join("asked"), base.join("other"));
        std::fs::create_dir_all(&asked).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(asked.join("diary.txt"), "TOKEN asked-folder").unwrap();
        std::fs::write(other.join("letter.txt"), "TOKEN other-folder").unwrap();
        let c = ctx(&ws, &home);
        assert_eq!(classify_path(&asked.join("diary.txt"), &c.workspace, &c.cfg, false), Effect::OutsideRead, "the fixture must be in the Ask tier");

        // A search of a folder the user was asked about reads that folder...
        let args = json!({"pattern": "TOKEN", "path": asked.to_string_lossy()});
        assert_eq!(read_risk(&args, &c), Effect::OutsideRead);
        let found = search_files(&args, &c).await.unwrap();
        assert!(found.contains("diary.txt:1:"), "{found}");
        assert!(find_files(&json!({"glob": "*.txt", "path": asked.to_string_lossy()}), &c).await.unwrap().contains("diary.txt"));
        // ...but a link in it that leads elsewhere in the Ask tier is passed over.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(other.join("letter.txt"), asked.join("letter-link.txt")).unwrap();
            let found = search_files(&args, &c).await.unwrap();
            assert!(found.contains("diary.txt:1:") && !found.contains("other-folder"), "{found}");
            let listed = find_files(&json!({"glob": "*", "path": asked.to_string_lossy()}), &c).await.unwrap();
            assert!(listed.contains("diary.txt") && !listed.contains("letter-link"), "{listed}");
        }

        // A walk of the working folder or of `/` opens what a grant covers, and only that.
        let scope = |c: &ToolCtx| WalkScope::new(Path::new("/"), Effect::LocalRead, c);
        assert!(scope(&c).skips(&asked.join("diary.txt")) && scope(&c).skips(&other.join("letter.txt")));
        // A single-file grant (`read=:`, given where a folder was too broad) opens that file only,
        // never what is under it.
        let exact = |p: &Path| serde_json::to_string(&[format!("read=:{}", p.display())]).unwrap();
        c.store.kv_set("grants:s", &exact(&base)).unwrap();
        assert!(scope(&c).skips(&asked.join("diary.txt")), "a single-file grant on {} opened a file under it", base.display());
        c.store.kv_set("grants:s", &exact(&asked.join("diary.txt"))).unwrap();
        assert!(!scope(&c).skips(&asked.join("diary.txt")), "a single-file grant opens its file");
        assert!(scope(&c).skips(&other.join("letter.txt")));
        assert!(scope(&c).prunes(&other), "a single-file grant leads into no other folder");
        let grant = format!("read:{}/**", asked.display());
        c.store.kv_set("grants:s", &serde_json::to_string(&[&grant]).unwrap()).unwrap();
        assert!(!scope(&c).skips(&asked.join("diary.txt")), "a conversation grant opens its folder");
        assert!(scope(&c).skips(&other.join("letter.txt")), "and nothing else");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(asked.join("diary.txt"), ws.join("granted-link.txt")).unwrap();
            assert!(search_files(&json!({"pattern": "TOKEN"}), &c).await.unwrap().contains("granted-link.txt:1:"));
        }
        // Once outside content is present, the conversation grant no longer applies.
        c.tainted.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(scope(&c).skips(&asked.join("diary.txt")), "a conversation grant held after outside content");
        #[cfg(unix)]
        assert!(!search_files(&json!({"pattern": "TOKEN"}), &c).await.unwrap().contains("granted-link.txt"));
        // A standing grant does.
        let mut cfg = (*c.cfg).clone();
        cfg.permissions.allow = vec![grant.clone()];
        let standing = ToolCtx { cfg: Arc::new(cfg), ..ctx(&ws, &home) };
        standing.tainted.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(!scope(&standing).skips(&asked.join("diary.txt")), "a standing grant opens its folder");
        assert!(scope(&standing).skips(&other.join("letter.txt")));
        // A folder nothing opens is not entered, unless the working folder or a grant lies inside it.
        assert!(scope(&standing).prunes(&other) && !scope(&standing).prunes(&base) && !scope(&standing).prunes(&asked));
    }

    /// One "this conversation" answer on `list_dir` of a too-broad folder stores `read=:<folder>`. A
    /// search of that folder is let through by the same grant, but the grant is one path: nothing under
    /// it is opened. A `read:` grant does open the folder's contents.
    #[tokio::test]
    async fn exact_grant_on_walk_root_opens_no_subtree() {
        let (_dir, ws, home) = dirs_for_test();
        let outside = ask_tier_dir();
        let root = dunce::canonicalize(outside.path()).unwrap();
        std::fs::create_dir_all(root.join("inner")).unwrap();
        std::fs::write(root.join("diary.txt"), "TOKEN top-level").unwrap();
        std::fs::write(root.join("inner/letter.txt"), "TOKEN inner").unwrap();
        let c = ctx(&ws, &home);
        let search = json!({"pattern": "TOKEN", "path": root.to_string_lossy()});
        let find = json!({"glob": "**/*.txt", "path": root.to_string_lossy()});
        assert_eq!(read_risk(&search, &c), Effect::OutsideRead, "the fixture must be in the Ask tier");
        let grant = |rule: String| c.store.kv_set("grants:s", &serde_json::to_string(&[rule]).unwrap()).unwrap();

        grant(format!("read=:{}", read_target(&search, &c).display()));
        let found = search_files(&search, &c).await.unwrap();
        assert_eq!(found, "no matches", "a single-path grant on the walk's root opened what is under it");
        let listed = find_files(&find, &c).await.unwrap();
        assert_eq!(listed, "no files match", "a single-path grant on the walk's root listed what is under it");
        let scope = WalkScope::new(&root, Effect::OutsideRead, &c);
        assert!(scope.skips(&root.join("diary.txt")) && scope.skips(&root.join("inner/letter.txt")));
        assert!(scope.prunes(&root.join("inner")));

        // A folder grant opens the folder's contents.
        grant(format!("read:{}/**", root.display()));
        let found = search_files(&search, &c).await.unwrap();
        assert!(found.contains("diary.txt:1:") && found.contains("letter.txt:1:"), "{found}");
        let listed = find_files(&find, &c).await.unwrap();
        assert!(listed.contains("diary.txt") && listed.contains("letter.txt"), "{listed}");

        // Once outside content is present the single-path grant no longer applies, so the walk was
        // asked about on its own, and opens its root.
        grant(format!("read=:{}", root.display()));
        c.tainted.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(search_files(&search, &c).await.unwrap().contains("diary.txt:1:"));
    }

    /// `/.vol/<device>/<inode>/...` opens a file by volume id, and `realpath` fails on it, so its
    /// written form says nothing about where it leads. Such a path is Refused for reads and writes in
    /// every mode, and the listing and search tools refuse it as a root.
    #[tokio::test]
    async fn volume_id_paths_are_refused() {
        let (_dir, ws, home) = dirs_for_test();
        let c = ctx(&ws, &home);
        let mut paths: Vec<PathBuf> = ["/.vol/16777232/2/Users/me/.codex/config.toml", "/.vol/1/2", "/.VOL/1/2/notes.md", "/System/Volumes/Data/.vol/1/2/notes.md", "/.vol"].iter().map(PathBuf::from).collect();
        std::fs::write(c.workspace.join("notes.md"), "VOLUME-CONTENT").unwrap();
        // The real thing where it exists: the working folder, and a Refused folder, by volume id.
        #[cfg(target_os = "macos")]
        {
            use std::os::unix::fs::MetadataExt;
            let meta = std::fs::metadata(&c.workspace).unwrap();
            let by_id = PathBuf::from(format!("/.vol/{}/{}", meta.dev(), meta.ino()));
            if by_id.join("notes.md").exists() {
                paths.extend([by_id.join("notes.md"), by_id.join("new.md"), by_id.clone()]);
            } else {
                eprintln!("{} is not reachable here; checking written forms only", by_id.display());
            }
            let home_meta = std::fs::metadata(dirs::home_dir().unwrap()).unwrap();
            paths.push(PathBuf::from(format!("/.vol/{}/{}/.codex/config.toml", home_meta.dev(), home_meta.ino())));
            // A link in the working folder into Heron's home by volume id: Refused however `realpath`
            // treats it (failing, returning the `/.vol` form, or returning the real path), and a search
            // of the working folder does not read through it.
            std::fs::write(home.join("config.toml"), "REFUSED-BY-VOLUME").unwrap();
            let home_meta = std::fs::metadata(&home).unwrap();
            let link = c.workspace.join("by-volume.toml");
            std::os::unix::fs::symlink(format!("/.vol/{}/{}/config.toml", home_meta.dev(), home_meta.ino()), &link).unwrap();
            eprintln!("canonical form of a link into /.vol: {:?}", dunce::canonicalize(&link));
            for write in [false, true] {
                assert_eq!(classify_path(&link, &c.workspace, &c.cfg, write), Effect::Refused, "link (write: {write})");
            }
            let found = search_files(&json!({"pattern": "BY-VOLUME|VOLUME-CONTENT"}), &c).await.unwrap();
            assert!(found.contains("notes.md:1:") && !found.contains("REFUSED-BY-VOLUME"), "{found}");
            std::fs::remove_file(&link).unwrap();
        }
        // A walk of `/` does not enter `/.vol`.
        assert!(WalkScope::new(Path::new("/"), Effect::LocalRead, &c).prunes(Path::new("/.vol")));
        for path in &paths {
            assert!(crate::permissions::unresolvable(path), "{}", path.display());
            for write in [false, true] {
                assert_eq!(classify_path(path, &c.workspace, &c.cfg, write), Effect::Refused, "{} (write: {write})", path.display());
            }
            let args = json!({"path": path.to_string_lossy(), "pattern": "CONTENT", "glob": "*"});
            for tool in ["read_file", "list_dir", "search_files", "find_files"] {
                for mode in [Mode::ReadOnly, Mode::Ask, Mode::Auto, Mode::Yolo] {
                    let (effect, v) = verdict(&c, tool, &args, mode, &["*".into(), format!("{tool}:*")]);
                    assert_eq!(effect, Effect::Refused, "{tool} {} in {mode:?}", path.display());
                    assert!(matches!(v, Verdict::Deny(_)), "{tool} {} in {mode:?}: {v:?}", path.display());
                }
            }
            for write_tool in ["write_file", "edit_file"] {
                let (effect, v) = verdict(&c, write_tool, &json!({"path": path.to_string_lossy()}), Mode::Yolo, &["*".into()]);
                assert_eq!(effect, Effect::Refused, "{write_tool} {}", path.display());
                assert!(matches!(v, Verdict::Deny(_)), "{write_tool} {}: {v:?}", path.display());
            }
            // The tools refuse such a root even when called directly.
            for result in [list_dir(&args, &c).await, search_files(&args, &c).await, find_files(&args, &c).await] {
                let err = result.expect_err(&format!("{} was walked", path.display()));
                assert!(err.to_string().contains("can't be resolved"), "{err}");
            }
        }
        // An ordinary path that resolves is judged as before.
        assert!(!crate::permissions::unresolvable(&c.workspace.join("notes.md")));
        assert!(!crate::permissions::unresolvable(&c.workspace.join("not-yet/new.md")));
        assert_eq!(classify_path(&c.workspace.join("notes.md"), &c.workspace, &c.cfg, false), Effect::LocalRead);
        assert!(list_dir(&json!({}), &c).await.is_ok());
    }

    /// `/.nofollow/<path>` and `/.resolve/<flags>/<path>` name the file at `<path>`, and `realpath`
    /// returns them unchanged, so like `/.vol` they are Refused for reads and writes in every mode:
    /// in any letter case, after `..` and `//`, behind the data and boot volume aliases, and through a
    /// link in the working folder. The listing and search tools refuse them as a root.
    #[tokio::test]
    async fn kernel_prefix_paths_are_refused() {
        use crate::permissions::DATA_VOLUME;
        let (_dir, ws, home) = dirs_for_test();
        let c = ctx(&ws, &home);
        let home = dunce::canonicalize(&home).unwrap();
        let secret = home.join("config.toml");
        std::fs::write(&secret, "REFUSED-KERNEL-CONTENT").unwrap();
        std::fs::write(c.workspace.join("notes.md"), "WORKSPACE-CONTENT").unwrap();
        let (s, h, w) = (secret.display().to_string(), home.display().to_string(), c.workspace.display().to_string());
        let up = "../".repeat(c.workspace.components().count() - 1);
        let mut written: Vec<String> = vec![
            format!("/.nofollow{s}"),
            format!("/.NoFollow{s}"),
            format!("/.resolve/1{s}"),
            format!("/.RESOLVE/1{s}"),
            format!("/.vol/1/2{s}"),
            format!("//.nofollow/{s}"),
            format!("/private/../.nofollow{s}"),
            format!("/tmp/x/../../.resolve/1{s}"),
            format!("{DATA_VOLUME}/.nofollow{s}"),
            format!("/.nofollow{h}"),
            format!("/.nofollow{w}/notes.md"),
            format!("/.nofollow{w}/not-yet/new.md"),
            "/.nofollow".into(),
            "/.resolve".into(),
            // Relative to the working folder: `..` up to `/`, then the prefix.
            format!("{up}.nofollow{s}"),
        ];
        // The boot volume's entry in `/Volumes` is a link to `/`.
        for entry in std::fs::read_dir("/Volumes").into_iter().flatten().flatten() {
            if dunce::canonicalize(entry.path()).is_ok_and(|r| r == Path::new("/")) {
                let boot = entry.path().display().to_string();
                written.extend([format!("{boot}/.nofollow{s}"), format!("{boot}{DATA_VOLUME}/.nofollow{s}"), format!("/volumes/{}/.Nofollow{s}", entry.file_name().to_string_lossy())]);
            }
        }
        let rules = |tool: &str| ["*".to_string(), format!("{tool}:*"), format!("read:{h}/**"), format!("read:/.nofollow{h}/**")];
        for raw in &written {
            let path = c.workspace.join(raw);
            assert!(crate::permissions::unresolvable(&path), "{raw}");
            for write in [false, true] {
                assert_eq!(classify_path(&path, &c.workspace, &c.cfg, write), Effect::Refused, "{raw} (write: {write})");
            }
            let args = json!({"path": raw, "pattern": "CONTENT", "glob": "*"});
            for tool in ["read_file", "list_dir", "search_files", "find_files"] {
                for mode in [Mode::ReadOnly, Mode::Ask, Mode::Auto, Mode::Yolo] {
                    let (effect, v) = verdict(&c, tool, &args, mode, &rules(tool));
                    assert_eq!(effect, Effect::Refused, "{tool} {raw} in {mode:?}");
                    assert!(matches!(v, Verdict::Deny(_)), "{tool} {raw} in {mode:?}: {v:?}");
                }
            }
            for write_tool in ["write_file", "edit_file"] {
                let (effect, v) = verdict(&c, write_tool, &json!({"path": raw}), Mode::Yolo, &["*".into()]);
                assert_eq!(effect, Effect::Refused, "{write_tool} {raw}");
                assert!(matches!(v, Verdict::Deny(_)), "{write_tool} {raw}: {v:?}");
            }
            for result in [list_dir(&args, &c).await, search_files(&args, &c).await, find_files(&args, &c).await] {
                let err = result.expect_err(&format!("{raw} was walked"));
                assert!(err.to_string().contains("can't be resolved"), "{err}");
            }
        }
        // A link in the working folder to Heron's home through `/.nofollow`: Refused however `realpath`
        // treats it, and a search or listing of the working folder does not read through it. A link to
        // `/` followed by the prefix is the prefix once the link is followed.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(format!("/.nofollow{s}"), c.workspace.join("by-prefix.toml")).unwrap();
            std::os::unix::fs::symlink(format!("/.nofollow{h}"), c.workspace.join("home-by-prefix")).unwrap();
            std::os::unix::fs::symlink("/", c.workspace.join("root")).unwrap();
            eprintln!("canonical form of a link into /.nofollow: {:?}", dunce::canonicalize(c.workspace.join("by-prefix.toml")));
            for raw in ["by-prefix.toml", "home-by-prefix/config.toml", "home-by-prefix", &format!("root/.nofollow{s}"), &format!("root/.nofollow{w}/not-yet/new.md"), "root/.nofollow", "root/.resolve"] {
                let path = c.workspace.join(raw);
                for write in [false, true] {
                    assert_eq!(classify_path(&path, &c.workspace, &c.cfg, write), Effect::Refused, "link {raw} (write: {write})");
                }
                for tool in ["read_file", "list_dir", "search_files", "find_files"] {
                    let (effect, v) = verdict(&c, tool, &json!({"path": raw, "pattern": "CONTENT", "glob": "*"}), Mode::Yolo, &rules(tool));
                    assert_eq!(effect, Effect::Refused, "{tool} link {raw}");
                    assert!(matches!(v, Verdict::Deny(_)), "{tool} link {raw}: {v:?}");
                }
            }
            let found = search_files(&json!({"pattern": "KERNEL-CONTENT|WORKSPACE-CONTENT"}), &c).await.unwrap();
            assert!(found.contains("notes.md:1:") && !found.contains("REFUSED-KERNEL-CONTENT"), "{found}");
            let listed = find_files(&json!({"glob": "*"}), &c).await.unwrap();
            assert!(listed.contains("notes.md") && !listed.contains("config.toml") && !listed.contains("by-prefix.toml"), "{listed}");
            // Called directly (past the policy), the walkers still open nothing through the link.
            for args in [json!({"path": "home-by-prefix", "pattern": "CONTENT"}), json!({"path": "home-by-prefix", "glob": "*"})] {
                for out in [search_files(&args, &c).await, find_files(&args, &c).await].into_iter().flatten() {
                    assert!(!out.contains("REFUSED-KERNEL-CONTENT") && !out.contains("config.toml"), "{out}");
                }
            }
        }
        // An ordinary path that resolves is judged as before.
        assert!(!crate::permissions::unresolvable(&c.workspace.join("notes.md")));
        assert!(!crate::permissions::unresolvable(&c.workspace.join(".nofollow-not-a-prefix/new.md")));
        assert_eq!(classify_path(&c.workspace.join("notes.md"), &c.workspace, &c.cfg, false), Effect::LocalRead);
    }

    /// M-4, the `read_file` half: credential files the old list did not name now ask. The glob and
    /// quote half belongs to the shell (`read_file` never expanded globs, before or after M-4), and is
    /// checked by `m4_sandbox_denies_globs_quotes_and_unlisted_secrets` in `shell.rs`.
    #[tokio::test]
    async fn m4_globs_quotes_and_unlisted_secrets_ask() {
        let (_dir, ws, home) = dirs_for_test();
        let c = ctx(&ws, &home);
        let user_home = dirs::home_dir().unwrap();
        let unlisted = [
            "~/.cargo/credentials.toml",
            "~/.cargo/credentials",
            "~/.config/gh/hosts.yml",
            "~/.config/git/credentials",
            "~/.zsh_history",
            "~/.bash_history",
            "~/.python_history",
            "~/.psql_history",
            "~/.lesshst",
            "~/.zsh_sessions/ABC.history",
            "~/.local/share/fish/fish_history",
            "~/Library/Application Support/Vivaldi/Default/Login Data",
            "~/Library/Application Support/Arc/User Data/Default/Cookies",
            "~/Library/Application Support/Vivaldi/Default/Web Data-journal",
        ];
        for path in unlisted {
            for mode in [Mode::ReadOnly, Mode::Ask, Mode::Auto, Mode::Yolo] {
                let (effect, v) = verdict(&c, "read_file", &json!({"path": path}), mode, &["*".into(), "read_file:*".into(), format!("read:{}/**", user_home.join(".config").display())]);
                assert_eq!(effect, Effect::SensitiveRead, "{path}");
                assert_eq!(v, Verdict::Ask, "{path} in {mode:?}");
            }
        }
        // The browser profiles Heron knows are Refused outright.
        let (effect, _) = verdict(&c, "read_file", &json!({"path": "~/Library/Application Support/Google/Chrome/Default/Login Data"}), Mode::Yolo, &[]);
        assert_eq!(effect, Effect::Refused);
    }
}
