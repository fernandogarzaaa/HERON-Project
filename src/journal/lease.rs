//! Process leases: one locked file per process and Heron home, `<heron home>/journal/<proc>.lock`.
//! The operating system drops the lock when the process dies, however it dies, so a lease file that
//! can be locked, or is missing, belongs to a process that has exited.

use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

const ACQUIRE_TRIES: u32 = 20;
const ACQUIRE_PAUSE: Duration = Duration::from_millis(5);

/// This process's id in the journal, made once per process.
pub fn proc_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| uuid::Uuid::new_v4().simple().to_string())
}

pub fn lease_dir(heron_home: &Path) -> PathBuf {
    heron_home.join("journal")
}

fn lease_path(heron_home: &Path, proc: &str) -> PathBuf {
    lease_dir(heron_home).join(format!("{proc}.lock"))
}

fn held() -> MutexGuard<'static, HashMap<PathBuf, File>> {
    static HELD: OnceLock<Mutex<HashMap<PathBuf, File>>> = OnceLock::new();
    HELD.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(unix)]
fn same_file(open: &File, path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (open.metadata(), std::fs::metadata(path)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn same_file(_open: &File, path: &Path) -> bool {
    path.exists()
}

pub struct Lease;

impl Lease {
    /// Hold this process's lease for `heron_home` until the process exits or [`Lease::release_all`].
    pub fn acquire(heron_home: &Path) -> Result<()> {
        let mut map = held();
        if map.contains_key(heron_home) {
            return Ok(());
        }
        let dir = lease_dir(heron_home);
        std::fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
        let path = lease_path(heron_home, proc_id());
        let mut last = anyhow!("the lease {} stayed locked", path.display());
        for _ in 0..ACQUIRE_TRIES {
            let file = OpenOptions::new().create(true).truncate(false).write(true).open(&path).with_context(|| format!("cannot open {}", path.display()))?;
            match file.try_lock() {
                // A sweep in another process may have removed the file between the open and the lock.
                Ok(()) if same_file(&file, &path) => {
                    map.insert(heron_home.to_path_buf(), file);
                    return Ok(());
                }
                Ok(()) => last = anyhow!("the lease {} was removed while it was being taken", path.display()),
                Err(e) => last = anyhow!("cannot lock {}: {e}", path.display()),
            }
            std::thread::sleep(ACQUIRE_PAUSE);
        }
        Err(last)
    }

    /// Whether the process `proc` has exited: its lease file is missing, or can be locked.
    pub fn is_dead(heron_home: &Path, proc: &str) -> bool {
        match OpenOptions::new().read(true).open(lease_path(heron_home, proc)) {
            Err(_) => true,
            Ok(file) => file.try_lock().is_ok(),
        }
    }

    /// On a clean exit: remove this process's lease files while still holding their locks.
    pub fn release_all() {
        let mut map = held();
        for (home, file) in map.drain() {
            let _ = std::fs::remove_file(lease_path(&home, proc_id()));
            drop(file);
        }
    }

    #[cfg(test)]
    pub(crate) fn release(heron_home: &Path) {
        if let Some(file) = held().remove(heron_home) {
            let _ = std::fs::remove_file(lease_path(heron_home, proc_id()));
            drop(file);
        }
    }
}

/// Remove every lease file in `heron_home` that can be locked, holding the lock while removing it.
/// Leases named in `except`, and this process's own, are left. Returns how many were removed.
pub fn sweep(heron_home: &Path, except: &[String]) -> usize {
    let Ok(entries) = std::fs::read_dir(lease_dir(heron_home)) else { return 0 };
    let mut removed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("lock") {
            continue;
        }
        let Some(proc) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        if proc == proc_id() || except.iter().any(|e| e == proc) {
            continue;
        }
        let Ok(file) = OpenOptions::new().read(true).open(&path) else { continue };
        if file.try_lock().is_ok() && std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
        drop(file);
    }
    removed
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A lease file for `proc` held by this test, as another live process would hold it.
    pub(crate) fn hold_lease(heron_home: &Path, proc: &str) -> File {
        std::fs::create_dir_all(lease_dir(heron_home)).unwrap();
        let file = OpenOptions::new().create(true).truncate(false).write(true).open(lease_path(heron_home, proc)).unwrap();
        file.try_lock().unwrap();
        file
    }

    /// A lease file for `proc` that nobody holds, as a crashed process leaves it.
    pub(crate) fn stale_lease(heron_home: &Path, proc: &str) -> PathBuf {
        std::fs::create_dir_all(lease_dir(heron_home)).unwrap();
        let path = lease_path(heron_home, proc);
        std::fs::write(&path, b"").unwrap();
        path
    }

    #[test]
    fn missing_lease_file_counts_as_dead() {
        let home = tempfile::tempdir().unwrap();
        assert!(Lease::is_dead(home.path(), "never-existed"));
        let path = stale_lease(home.path(), "crashed");
        assert!(Lease::is_dead(home.path(), "crashed"));
        assert!(path.exists(), "asking must not remove the file");
        let held = hold_lease(home.path(), "alive");
        assert!(!Lease::is_dead(home.path(), "alive"));
        drop(held);
        assert!(Lease::is_dead(home.path(), "alive"));
    }

    #[test]
    fn own_lease_is_live_and_released_with_its_file() {
        let home = tempfile::tempdir().unwrap();
        Lease::acquire(home.path()).unwrap();
        Lease::acquire(home.path()).unwrap();
        let path = lease_path(home.path(), proc_id());
        assert_eq!(path, home.path().join("journal").join(format!("{}.lock", proc_id())));
        assert!(path.exists());
        assert!(!Lease::is_dead(home.path(), proc_id()));
        Lease::release(home.path());
        assert!(!path.exists());
        assert!(Lease::is_dead(home.path(), proc_id()));
    }

    #[test]
    fn acquire_fails_when_the_home_cannot_hold_a_journal() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("a-file");
        std::fs::write(&home, b"").unwrap();
        assert!(Lease::acquire(&home).is_err());
        assert!(!held().contains_key(&home));
    }

    #[test]
    fn sweep_removes_only_lockable_lease_files() {
        let home = tempfile::tempdir().unwrap();
        let gone = stale_lease(home.path(), "gone");
        let kept = stale_lease(home.path(), "kept");
        let other = lease_dir(home.path()).join("notes.txt");
        std::fs::write(&other, b"").unwrap();
        let live = hold_lease(home.path(), "live");
        Lease::acquire(home.path()).unwrap();
        assert_eq!(sweep(home.path(), &["kept".to_string()]), 1);
        assert!(!gone.exists());
        assert!(kept.exists());
        assert!(other.exists());
        assert!(lease_path(home.path(), "live").exists());
        assert!(lease_path(home.path(), proc_id()).exists());
        assert!(!Lease::is_dead(home.path(), proc_id()));
        drop(live);
        assert_eq!(sweep(home.path(), &[]), 2);
        assert!(!lease_path(home.path(), "live").exists());
        assert!(!kept.exists());
        assert!(lease_path(home.path(), proc_id()).exists());
        Lease::release(home.path());
    }
}
