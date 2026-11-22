//! Helpers for the processes Heron starts outside the `shell` tool: the environment allowlist, a
//! private run folder that is removed afterwards, and killing a whole process group. `cli_llm.rs`
//! uses them for the model CLIs; later features use them for their own fixed binaries.

use anyhow::Result;
use std::path::PathBuf;

/// Variables every CLI child gets from Heron's own environment (when set). Nothing else is passed.
pub const CHILD_ENV_ALLOWED: &[&str] = &["PATH", "HOME", "LANG", "USER", "HTTP_PROXY", "HTTPS_PROXY", "NO_PROXY", "SSL_CERT_FILE"];

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
