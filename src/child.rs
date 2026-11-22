//! Helpers for the processes Heron starts outside the `shell` tool: the environment allowlist, a
//! private run folder that is removed afterwards, and killing a whole process group. `cli_llm.rs`
//! uses them for the model CLIs; later features use them for their own fixed binaries.

use anyhow::Result;
use std::path::{Path, PathBuf};

/// Variables every CLI child gets from Heron's own environment (when set). Nothing else is passed.
pub const CHILD_ENV_ALLOWED: &[&str] = &["PATH", "HOME", "LANG", "USER", "HTTP_PROXY", "HTTPS_PROXY", "NO_PROXY", "SSL_CERT_FILE"];

/// Heron's own value of `key`: the reader to pass to [`child_env_base`] outside tests.
pub(crate) fn own_var(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

/// The base of a child's whole environment (the child starts from `env_clear`): each variable in
/// [`CHILD_ENV_ALLOWED`] that Heron's environment sets, read through `get`, then `TMPDIR` pointing
/// at `tmpdir`. Heron's own `TMPDIR`, tokens and keys are never in it; a caller adds only what its
/// child needs.
pub(crate) fn child_env_base(tmpdir: &Path, get: &dyn Fn(&str) -> Option<String>) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = CHILD_ENV_ALLOWED.iter().filter_map(|k| get(k).map(|v| (k.to_string(), v))).collect();
    env.push(("TMPDIR".into(), tmpdir.to_string_lossy().into_owned()));
    env
}

/// A new private folder for one child's files (see [`make_run_folder`]), removed with everything in
/// it when the returned guard is dropped, so a cancelled run or an early error leaves nothing behind.
pub(crate) fn private_temp_dir() -> Result<RemoveTempDirOnDrop> {
    Ok(RemoveTempDirOnDrop { dir: make_run_folder()? })
}

/// Kill a process group by id (Unix only). Best effort: the group may
/// already be gone.
#[cfg(unix)]
pub(crate) fn kill_group(pgid: u32) {
    unsafe {
        libc::killpg(pgid as libc::pid_t, libc::SIGKILL);
    }
}

#[cfg(not(unix))]
pub(crate) fn kill_group(_pgid: u32) {}

/// Removes the request's private temp directory when dropped, so
/// cancellation (which skips the explicit cleanup calls) and early
/// errors cannot leave request artifacts behind.
pub(crate) struct RemoveTempDirOnDrop {
    pub(crate) dir: PathBuf,
}

impl Drop for RemoveTempDirOnDrop {
    fn drop(&mut self) {
        let _ = remove_temp_dir(&self.dir);
    }
}

/// The CLI run folder: `<Heron's $TMPDIR>/heron-cli-<run>`, a new UUID per turn, fresh, empty and mode
/// 0700 on Unix, so the CLI's working directory holds nothing it can read or change. The parent is
/// resolved through links (`/tmp` is `/private/tmp` on macOS), because the sandbox profile names it.
pub(crate) fn make_run_folder() -> Result<PathBuf> {
    let base = std::env::temp_dir();
    let base = dunce::canonicalize(&base).unwrap_or(base);
    let dir = base.join(format!("heron-cli-{}", uuid::Uuid::new_v4().simple()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
    }
    #[cfg(not(unix))]
    std::fs::create_dir(&dir)?;
    Ok(dir)
}

pub(crate) fn remove_temp_dir(dir: &PathBuf) -> Result<()> {
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn child_env_base_matches_the_allowlist() {
        let tmp = Path::new("/private/var/folders/heron-cli-test");
        // Heron's environment: every allowed variable, plus secrets and its own TMPDIR.
        let mut parent: BTreeMap<String, String> = CHILD_ENV_ALLOWED.iter().map(|k| (k.to_string(), format!("value-of-{k}"))).collect();
        for (k, v) in [("OPENAI_API_KEY", "sk-secret"), ("ANTHROPIC_API_KEY", "sk-ant"), ("GITHUB_TOKEN", "ghp"), ("TMPDIR", "/tmp/heron-own"), ("PWD", "/Users/me")] {
            parent.insert(k.into(), v.into());
        }
        let env = child_env_base(tmp, &|k| parent.get(k).cloned());
        let got: BTreeMap<String, String> = env.iter().cloned().collect();
        assert_eq!(got.len(), env.len(), "no variable is set twice: {env:?}");
        let mut want: BTreeMap<String, String> = CHILD_ENV_ALLOWED.iter().map(|k| (k.to_string(), format!("value-of-{k}"))).collect();
        want.insert("TMPDIR".into(), tmp.to_string_lossy().into_owned());
        assert_eq!(got, want, "exactly the allowlist, with TMPDIR at the private folder");

        // An allowed variable Heron does not have is left out, not passed empty.
        let env = child_env_base(tmp, &|k| (k == "PATH").then(|| "/usr/bin".to_string()));
        assert_eq!(env, vec![("PATH".to_string(), "/usr/bin".to_string()), ("TMPDIR".to_string(), tmp.to_string_lossy().into_owned())]);
    }

    #[test]
    fn private_temp_dir_is_private_and_removed_on_drop() {
        let guard = private_temp_dir().unwrap();
        let dir = guard.dir.clone();
        assert!(dir.is_dir());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "it starts empty");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
        }
        std::fs::write(dir.join("input.txt"), "x").unwrap();
        let other = private_temp_dir().unwrap();
        assert_ne!(other.dir, dir, "each call gets its own folder");
        drop(guard);
        assert!(!dir.exists(), "removed with its contents on drop");
        assert!(other.dir.exists());
    }
}
