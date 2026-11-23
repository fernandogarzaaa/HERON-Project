//! Executor-level isolation: macOS Seatbelt (`sandbox-exec`) or, on Linux,
//! bubblewrap (`bwrap`) with fresh user, pid, ipc, uts and network
//! namespaces and `no_new_privs`.
//!
//! The boundary is enforced by the kernel, not by instructions to the model:
//! every process the agent starts (shell commands, child processes, Claude
//! Code and everything it spawns) inherits the profile.
//!
//! - writes are confined to the workspace and a per-run temp/home directory;
//! - network is denied, or allowed only to one localhost port (the harness's
//!   egress proxy, which applies the host allowlist);
//! - credential directories and the harness's own state are unreadable;
//! - paths that run code later outside the sandbox (git hooks and config,
//!   agent, editor and shell config; see `crate::protect`) are readable but
//!   not writable;
//! - common network clients cannot be executed at all (defence in depth;
//!   the network rule is the real control);
//! - unix sockets are reachable only inside the writable paths and on an
//!   explicit allowlist (the system logger, the harness gate); container
//!   daemons and key agents are denied by name as well (a Docker socket
//!   would let a process start an unconfined container).
//!
//! On Linux the same rules are expressed as mounts: the root is read-only,
//! writable paths are bound read-write, protected paths and denied sockets
//! are covered by empty read-only mounts, denied executables by /dev/null,
//! and /tmp, /run and /var/tmp start empty so host sockets are out of reach.
//! So do the mounts a VM host puts into the guest (`host_share_mounts`:
//! shared host folders, OrbStack's guest tools and machine views), which
//! would otherwise expose the host's files and sockets that run commands on
//! the host.
//! The network namespace has only loopback; with a proxy, `harness
//! net-bridge` inside the sandbox forwards one loopback port to the egress
//! proxy's unix socket, which is bind-mounted in.
//!
//! Where neither is available the sandbox reports itself unavailable, and
//! tasks that need write/execute fail closed.

use std::path::{Path, PathBuf};

pub const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkPolicy {
    Denied,
    /// Only `localhost:<port>` is reachable.
    LocalProxy {
        port: u16,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxPolicy {
    pub writable: Vec<PathBuf>,
    pub network: NetworkPolicy,
    pub deny_read: Vec<PathBuf>,
    /// Readable but not writable, even inside a writable path.
    pub deny_write: Vec<PathBuf>,
    /// macOS only: absolute-path regexes that are not writable (new files
    /// included). bubblewrap cannot match names; it covers `deny_write`.
    pub deny_write_regex: Vec<String>,
    pub deny_exec: Vec<PathBuf>,
    /// Readable, never writable, and visible even inside an emptied scratch
    /// area (Linux). Elsewhere reads are allowed anyway; a denied path still
    /// wins.
    pub read_only: Vec<PathBuf>,
    /// Unix sockets (files or directories) that may be connected to, besides
    /// those inside the writable paths. Everything else is refused.
    pub allow_sockets: Vec<PathBuf>,
    /// Unix sockets (files or whole directories) that may not be connected
    /// to, even inside an allowed path.
    pub deny_sockets: Vec<PathBuf>,
    /// Linux only: how a `LocalProxy` network reaches the proxy from inside
    /// the network namespace.
    pub proxy_bridge: Option<ProxyBridge>,
}

/// The egress proxy's unix socket and the `harness` binary that bridges a
/// loopback port to it inside the Linux sandbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyBridge {
    pub exe: PathBuf,
    pub socket: PathBuf,
}

/// Where the proxy socket appears inside the Linux sandbox.
pub const BRIDGE_SOCKET: &str = "/run/harness-egress.sock";

/// Network clients that are never executable inside the sandbox.
pub fn default_deny_exec() -> Vec<PathBuf> {
    [
        "/usr/bin/curl",
        "/usr/bin/nc",
        "/usr/bin/ncat",
        "/usr/bin/ssh",
        "/usr/bin/scp",
        "/usr/bin/sftp",
        "/usr/bin/ftp",
        "/usr/bin/telnet",
        "/usr/bin/osascript",
        "/opt/homebrew/bin/curl",
        "/opt/homebrew/bin/wget",
        "/usr/local/bin/curl",
        "/usr/local/bin/wget",
    ]
    .iter()
    .map(PathBuf::from)
    .collect()
}

/// Credential stores and similar secrets under the user's home.
pub fn default_deny_read(home: &Path) -> Vec<PathBuf> {
    [
        ".ssh",
        ".aws",
        ".gnupg",
        ".config/gh",
        ".netrc",
        ".docker/config.json",
        ".kube",
        ".config/gcloud",
        "Library/Keychains",
    ]
    .iter()
    .map(|p| home.join(p))
    .collect()
}

/// Unix sockets of container daemons and key agents. Outbound unix sockets
/// are otherwise allowed (the system logger needs them), and talking to a
/// container daemon is a full sandbox escape. Directories cover every socket
/// beneath them.
pub fn default_deny_sockets(home: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = ["/var/run/docker.sock", "/var/run/podman"].iter().map(PathBuf::from).collect();
    v.extend(
        [
            ".orbstack",
            ".docker/run",
            ".colima",
            ".lima",
            ".rd",
            ".local/share/containers",
            "Library/Containers/com.docker.docker",
            ".gnupg",
            "Library/Group Containers/2BUA8C4S2C.com.1password",
        ]
        .iter()
        .map(|p| home.join(p)),
    );
    if let Some(agent) = std::env::var_os("SSH_AUTH_SOCK") {
        v.push(PathBuf::from(agent));
    }
    v
}

/// Unix sockets outside the writable paths that sandboxed processes need
/// (measured 2026-10-25 on macOS: only the system logger). On Linux the
/// sandbox has a fresh /dev and no logger socket.
pub fn default_allow_sockets() -> Vec<PathBuf> {
    if cfg!(target_os = "macos") {
        vec![PathBuf::from("/private/var/run/syslog")]
    } else {
        vec![]
    }
}

/// The bubblewrap binary, if installed.
pub fn bwrap_path() -> Option<PathBuf> {
    ["/usr/bin/bwrap", "/usr/local/bin/bwrap"].iter().map(PathBuf::from).find(|p| p.exists())
}

/// launchd's per-session ssh-agent socket, whose directory name is random.
const LAUNCHD_SSH_AGENT: &str = r#"^/private/tmp/com\.apple\.launchd\.[^/]+/Listeners$"#;

/// Resolve symlinks (e.g. /var -> /private/var) so the profile matches the
/// paths the kernel sees. Non-existent paths are resolved via their parent.
pub fn real_path(p: &Path) -> PathBuf {
    if let Ok(c) = std::fs::canonicalize(p) {
        return c;
    }
    match (p.parent(), p.file_name()) {
        (Some(parent), Some(name)) => real_path(parent).join(name),
        _ => p.to_path_buf(),
    }
}

fn sbpl_string(p: &Path) -> String {
    let s = p.to_string_lossy();
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

impl SandboxPolicy {
    pub fn new(writable: Vec<PathBuf>, network: NetworkPolicy) -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/var/empty"));
        SandboxPolicy {
            writable,
            network,
            deny_read: default_deny_read(&home),
            deny_write: vec![],
            deny_write_regex: vec![],
            deny_exec: default_deny_exec(),
            read_only: vec![],
            allow_sockets: default_allow_sockets(),
            deny_sockets: default_deny_sockets(&home),
            proxy_bridge: None,
        }
    }

    pub fn profile(&self) -> String {
        let mut p = String::from("(version 1)\n(allow default)\n");
        p.push_str("(deny file-write*)\n");
        p.push_str("(allow file-write*\n");
        for w in &self.writable {
            p.push_str(&format!("  (subpath {})\n", sbpl_string(&real_path(w))));
        }
        p.push_str("  (literal \"/dev/null\") (literal \"/dev/zero\") (literal \"/dev/tty\") (literal \"/dev/dtracehelper\")\n");
        p.push_str("  (regex #\"^/dev/fd/\") (regex #\"^/dev/ttys\"))\n");
        p.push_str("(deny network*)\n");
        if let NetworkPolicy::LocalProxy { port } = self.network {
            p.push_str(&format!("(allow network-outbound (remote ip \"localhost:{port}\"))\n"));
        }
        // Unix sockets: a process may create its own (`local` matches every
        // AF_UNIX operation, so outbound connects are denied again right
        // after) and connect only inside the writable paths and to the
        // allowlist. The DNS resolver stays denied: name lookups would be an
        // exfiltration channel.
        p.push_str("(allow network* (local unix-socket))\n(deny network-outbound (remote unix-socket))\n");
        for s in self.writable.iter().chain(&self.allow_sockets) {
            p.push_str(&format!(
                "(allow network-outbound (remote unix-socket (subpath {})))\n",
                sbpl_string(&real_path(s))
            ));
        }
        p.push_str("(deny network-outbound (literal \"/private/var/run/mDNSResponder\"))\n");
        for s in &self.deny_sockets {
            p.push_str(&format!("(deny network-outbound (subpath {}))\n", sbpl_string(&real_path(s))));
        }
        p.push_str(&format!("(deny network-outbound (regex #\"{LAUNCHD_SSH_AGENT}\"))\n"));
        // Protected paths are neither readable nor writable, even inside a
        // writable workspace (these rules come after the write allowances).
        for r in &self.deny_read {
            let r = sbpl_string(&real_path(r));
            p.push_str(&format!("(deny file-read* (subpath {r}))\n(deny file-write* (subpath {r}))\n"));
        }
        for w in &self.deny_write {
            p.push_str(&format!("(deny file-write* (subpath {}))\n", sbpl_string(&real_path(w))));
        }
        for r in &self.deny_write_regex {
            p.push_str(&format!("(deny file-write* (regex #\"{r}\"))\n"));
        }
        for x in &self.deny_exec {
            p.push_str(&format!("(deny process-exec (literal {}))\n", sbpl_string(&real_path(x))));
        }
        p
    }

    /// Program and arguments that run `program args` inside the sandbox.
    pub fn wrap(&self, program: &str, args: &[String]) -> (String, Vec<String>) {
        if cfg!(target_os = "linux") {
            let bwrap = bwrap_path().unwrap_or_else(|| PathBuf::from("/usr/bin/bwrap"));
            return (bwrap.to_string_lossy().into_owned(), self.bwrap_args(program, args));
        }
        let mut a = vec!["-p".to_string(), self.profile(), program.to_string()];
        a.extend(args.iter().cloned());
        (SANDBOX_EXEC.to_string(), a)
    }

    /// bubblewrap arguments expressing this policy (Linux). Later mounts
    /// override earlier ones, so the order below is the policy.
    pub fn bwrap_args(&self, program: &str, args: &[String]) -> Vec<String> {
        let mut a: Vec<String> = ["--die-with-parent", "--new-session", "--unshare-all"].map(String::from).to_vec();
        // Emptied like /tmp: host sockets and temp files, and whatever a VM
        // host mounts into the guest (its files, its control sockets).
        let host_shares =
            std::fs::read_to_string("/proc/self/mountinfo").map(|m| host_share_mounts(&m)).unwrap_or_default();
        let scratch: Vec<PathBuf> = ["/tmp", "/run", "/var/tmp"]
            .iter()
            .map(PathBuf::from)
            .chain(host_shares)
            .filter(|d| std::fs::symlink_metadata(d).map(|m| m.is_dir()).unwrap_or(false))
            .collect();
        let bound: Vec<PathBuf> =
            self.writable.iter().chain(&self.allow_sockets).chain(&self.read_only).map(|p| real_path(p)).collect();
        // Inside an emptied scratch area and not bound back: already invisible.
        // Compare resolved paths on both sides (a scratch dir may sit behind a
        // symlinked component, e.g. /var -> /private/var).
        let scratch_real: Vec<PathBuf> = scratch.iter().map(|d| real_path(d)).collect();
        let hidden_already = |p: &Path| {
            let p = real_path(p);
            scratch_real.iter().any(|d| p.starts_with(d)) && !bound.iter().any(|b| p.starts_with(b))
        };
        let s = |p: &Path| real_path(p).to_string_lossy().into_owned();
        let mut push = |xs: &[&str]| a.extend(xs.iter().map(|x| x.to_string()));
        push(&["--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc"]);
        for d in &scratch {
            push(&["--tmpfs", &d.to_string_lossy()]);
        }
        for w in &self.writable {
            if w.exists() {
                let p = s(w);
                push(&["--bind", &p, &p]);
            }
        }
        for sock in &self.allow_sockets {
            if sock.exists() {
                let p = s(sock);
                push(&["--bind", &p, &p]);
            }
        }
        for r in &self.read_only {
            if r.exists() {
                let p = s(r);
                push(&["--ro-bind", &p, &p]);
            }
        }
        let bridge = match (&self.network, &self.proxy_bridge) {
            (NetworkPolicy::LocalProxy { port }, Some(b)) if b.socket.exists() => {
                push(&["--bind", &s(&b.socket), BRIDGE_SOCKET]);
                Some((b.exe.clone(), *port))
            }
            _ => None, // no network at all
        };
        // The scratch areas become read-only once the allowed paths are bound
        // into them (those binds stay writable): writing elsewhere fails, as
        // on macOS, instead of landing in a private tmpfs.
        for d in &scratch {
            push(&["--remount-ro", &d.to_string_lossy()]);
        }
        // Readable, not writable: bound read-only over the writable binds.
        for w in self.deny_write.iter().filter(|w| w.exists() && !hidden_already(w)) {
            let p = s(w);
            push(&["--ro-bind", &p, &p]);
        }
        // Hidden: protected paths and denied sockets (empty, read-only) and
        // denied executables (/dev/null cannot be executed).
        for p in self.deny_read.iter().chain(&self.deny_sockets).filter(|p| !hidden_already(p)) {
            match std::fs::metadata(p) {
                Ok(m) if m.is_dir() => {
                    let p = s(p);
                    push(&["--tmpfs", &p, "--remount-ro", &p]);
                }
                Ok(_) => push(&["--ro-bind", "/dev/null", &s(p)]),
                Err(_) => {}
            }
        }
        for x in &self.deny_exec {
            if x.exists() && !hidden_already(x) {
                push(&["--ro-bind", "/dev/null", &s(x)]);
            }
        }
        if let Some((exe, port)) = bridge {
            a.push(exe.to_string_lossy().into_owned());
            a.extend(["net-bridge", "--port", &port.to_string(), "--socket", BRIDGE_SOCKET, "--"].map(String::from));
        }
        a.push(program.to_string());
        a.extend(args.iter().cloned());
        a
    }

    /// Whether kernel sandboxing works on this host.
    pub fn available() -> bool {
        if cfg!(target_os = "linux") {
            return bwrap_path().is_some_and(|b| bwrap_works(&b));
        }
        cfg!(target_os = "macos") && seatbelt_works(Path::new(SANDBOX_EXEC))
    }
}

/// Probe bubblewrap with the same mounts a real run uses: some hosts (e.g.
/// an unprivileged container) allow the namespaces but refuse a fresh /proc,
/// and must report unavailable rather than fail every command.
pub fn bwrap_works(bwrap: &Path) -> bool {
    std::process::Command::new(bwrap)
        .args(["--die-with-parent", "--new-session", "--unshare-all", "--ro-bind", "/", "/"])
        .args(["--dev", "/dev", "--proc", "/proc", "--tmpfs", "/tmp", "/bin/true"])
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Mount points a VM host puts into a Linux guest, from
/// `/proc/self/mountinfo`: shared host folders (virtiofs, 9p) and the mounts
/// OrbStack names `orbstack` or `machines` (its guest tools with the sockets
/// that run commands on the Mac, and the other machines' filesystems). No
/// path rule knows them, so the sandbox empties them like /tmp. Only the
/// outermost of nested mount points is returned; `/` never is.
pub fn host_share_mounts(mountinfo: &str) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = mountinfo
        .lines()
        .filter_map(|line| {
            let mut halves = line.splitn(2, " - ");
            let mount_point = halves.next()?.split(' ').nth(4)?;
            let mut fs = halves.next()?.split(' ');
            let (fstype, source) = (fs.next()?, fs.next()?);
            let shared = matches!(fstype, "virtiofs" | "9p") || matches!(source, "orbstack" | "machines");
            (shared && mount_point != "/").then(|| PathBuf::from(unescape_mount_path(mount_point)))
        })
        .collect();
    found.sort();
    found.dedup();
    let outer: Vec<PathBuf> =
        found.iter().filter(|p| !found.iter().any(|q| q != *p && p.starts_with(q))).cloned().collect();
    outer
}

/// mountinfo escapes space, tab, newline and backslash as `\ooo`.
fn unescape_mount_path(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let octal = b.get(i + 1..i + 4).filter(|d| d.iter().all(|c| (b'0'..=b'7').contains(c)));
        match (b[i], octal) {
            (b'\\', Some(d)) => {
                out.push((d[0] - b'0') * 64 + (d[1] - b'0') * 8 + (d[2] - b'0'));
                i += 4;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Probe Seatbelt with an allow-everything profile.
pub fn seatbelt_works(sandbox_exec: &Path) -> bool {
    sandbox_exec.exists()
        && std::process::Command::new(sandbox_exec)
            .args(["-p", "(version 1)(allow default)", "/usr/bin/true"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_contents() {
        let dir = tempfile::tempdir().unwrap();
        let mut pol = SandboxPolicy::new(vec![dir.path().to_path_buf()], NetworkPolicy::Denied);
        pol.deny_read = vec![PathBuf::from("/secret \"x\"")];
        let prof = pol.profile();
        assert!(prof.contains("(deny file-write*)"));
        assert!(prof.contains(&real_path(dir.path()).to_string_lossy().to_string()));
        assert!(prof.contains("(deny network*)"));
        assert!(!prof.contains("localhost:"));
        assert!(prof.contains("/usr/bin/curl"));
        assert!(prof.contains(r#"(subpath "/secret \"x\"")"#), "quotes are escaped: {prof}");
        let proxied = SandboxPolicy::new(vec![], NetworkPolicy::LocalProxy { port: 4242 });
        assert!(proxied.profile().contains(r#"(remote ip "localhost:4242")"#));
    }

    #[test]
    fn bwrap_arguments_express_the_policy_in_order() {
        let ws = tempfile::tempdir().unwrap();
        let sock_dir = tempfile::tempdir().unwrap();
        // Protected paths inside the writable workspace must be covered.
        let hidden = ws.path().join("fixtures");
        std::fs::create_dir(&hidden).unwrap();
        let null_target = ws.path().join("secret.txt");
        std::fs::write(&null_target, "s").unwrap();
        let mut p = SandboxPolicy::new(vec![ws.path().to_path_buf()], NetworkPolicy::Denied);
        p.deny_read = vec![hidden.clone(), null_target.clone(), PathBuf::from("/does/not/exist")];
        p.deny_exec = vec![PathBuf::from("/bin/ls"), PathBuf::from("/no/such/curl")];
        p.allow_sockets = vec![sock_dir.path().to_path_buf()];
        p.deny_sockets = vec![];
        let a = p.bwrap_args("/bin/echo", &["hi".into()]);
        let pos = |xs: &[&str]| {
            a.windows(xs.len())
                .position(|w| w.iter().zip(xs).all(|(x, y)| x == y))
                .unwrap_or_else(|| panic!("{xs:?} not in {a:?}"))
        };
        let r = |p: &Path| real_path(p).to_string_lossy().into_owned();
        assert_eq!(&a[..3], &["--die-with-parent", "--new-session", "--unshare-all"], "every namespace, incl. network");
        let root = pos(&["--ro-bind", "/", "/"]);
        // The first scratch directory that is a real directory on this host
        // (on macOS /tmp is a symlink and /run does not exist).
        let scratch = ["/tmp", "/run", "/var/tmp"]
            .into_iter()
            .find(|d| std::fs::symlink_metadata(d).map(|m| m.is_dir()).unwrap_or(false))
            .unwrap();
        let tmp = pos(&["--tmpfs", scratch]);
        let ws_bind = pos(&["--bind", &r(ws.path()), &r(ws.path())]);
        let sock_bind = pos(&["--bind", &r(sock_dir.path()), &r(sock_dir.path())]);
        let hide_dir = pos(&["--tmpfs", &r(&hidden), "--remount-ro", &r(&hidden)]);
        let hide_file = pos(&["--ro-bind", "/dev/null", &r(&null_target)]);
        let no_exec = pos(&["--ro-bind", "/dev/null", &r(Path::new("/bin/ls"))]);
        assert!(root < tmp && tmp < ws_bind && ws_bind < sock_bind, "empty /tmp first, then rebind what is allowed");
        assert!(ws_bind < hide_dir && hide_dir < hide_file, "protected paths win over writable ones");
        assert!(hide_file < no_exec);
        assert!(!a.iter().any(|x| x.contains("/does/not/exist") || x.contains("/no/such")), "missing paths skipped");
        // A protected path in an emptied scratch area, outside every bind, is
        // already invisible: no mount (it would fail on a read-only scratch
        // dir). Inside a path bound back into the scratch area it is covered.
        let elsewhere = tempfile::Builder::new().tempdir_in(scratch).unwrap();
        let bound = tempfile::Builder::new().tempdir_in(scratch).unwrap();
        let inside = bound.path().join("fixtures");
        std::fs::create_dir(&inside).unwrap();
        let mut q = p.clone();
        q.writable.push(bound.path().to_path_buf());
        q.deny_read = vec![elsewhere.path().to_path_buf(), inside.clone()];
        let qa = q.bwrap_args("/bin/true", &[]);
        assert!(!qa.iter().any(|x| x == &r(elsewhere.path())), "already hidden: {qa:?}");
        assert!(qa.windows(2).any(|w| w[0] == "--tmpfs" && w[1] == r(&inside)), "bound, so covered: {qa:?}");
        // Write-protected paths are bound read-only over the writable binds,
        // unless they are already invisible or missing.
        let hooks = bound.path().join("hooks");
        std::fs::create_dir(&hooks).unwrap();
        let mut w = q.clone();
        w.deny_read = vec![];
        w.deny_write = vec![hooks.clone(), elsewhere.path().to_path_buf(), bound.path().join("missing")];
        let wa = w.bwrap_args("/bin/true", &[]);
        let ro = wa.windows(3).position(|x| x[0] == "--ro-bind" && x[1] == r(&hooks) && x[2] == r(&hooks));
        let rw = wa.windows(3).position(|x| x[0] == "--bind" && x[1] == r(bound.path()));
        assert!(ro.is_some() && rw < ro, "read-only after the writable bind: {wa:?}");
        assert!(!wa.iter().any(|x| x == &r(elsewhere.path()) || x.ends_with("/missing")), "{wa:?}");
        // A bridge whose socket is gone gives no network rather than a broken mount.
        let mut gone = p.clone();
        gone.network = NetworkPolicy::LocalProxy { port: 1 };
        gone.proxy_bridge =
            Some(ProxyBridge { exe: PathBuf::from("/opt/harness"), socket: PathBuf::from("/no/such.sock") });
        assert!(!gone.bwrap_args("/bin/true", &[]).iter().any(|x| x == "net-bridge" || x.contains("/no/such.sock")));
        assert_eq!(&a[a.len() - 2..], &["/bin/echo", "hi"]);
        assert!(!a.contains(&"net-bridge".to_string()), "no network without a bridge");
        // With a proxy bridge the command runs under `harness net-bridge`.
        let bridge_sock = sock_dir.path().join("p.sock");
        std::fs::write(&bridge_sock, "").unwrap();
        p.network = NetworkPolicy::LocalProxy { port: 4321 };
        p.proxy_bridge = Some(ProxyBridge { exe: PathBuf::from("/opt/harness"), socket: bridge_sock.clone() });
        let a = p.bwrap_args("/bin/echo", &["hi".into()]);
        let tail: Vec<&str> = a[a.len() - 9..].iter().map(String::as_str).collect();
        assert_eq!(
            tail,
            ["/opt/harness", "net-bridge", "--port", "4321", "--socket", BRIDGE_SOCKET, "--", "/bin/echo", "hi"]
        );
        assert!(a.windows(3).any(|w| w[0] == "--bind" && w[1] == r(&bridge_sock) && w[2] == BRIDGE_SOCKET));
    }

    /// Local CI states whether this host must have a working sandbox
    /// (`HARNESS_EXPECT_SANDBOX`): a probe that wrongly reports unavailable
    /// would otherwise only make the sandbox tests skip.
    #[test]
    fn the_probe_tells_a_working_backend_from_a_broken_one() {
        let probe = if cfg!(target_os = "linux") { bwrap_works } else { seatbelt_works };
        for p in [bwrap_works, seatbelt_works] {
            assert!(!p(Path::new("/usr/bin/false")), "a binary that fails is not a sandbox");
            assert!(!p(Path::new("/no/such/sandbox")), "a missing binary is not a sandbox");
        }
        match std::env::var("HARNESS_EXPECT_SANDBOX").as_deref() {
            Ok("1") => {
                assert!(SandboxPolicy::available(), "this host must have a working sandbox");
                let real = if cfg!(target_os = "linux") { bwrap_path().unwrap() } else { PathBuf::from(SANDBOX_EXEC) };
                assert!(probe(&real));
            }
            Ok("0") => assert!(!SandboxPolicy::available(), "this host must report the sandbox unavailable"),
            _ => {}
        }
    }

    #[test]
    fn real_path_resolves_missing_leaf() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("a/b");
        assert!(real_path(&missing).ends_with("a/b"));
        assert_eq!(real_path(dir.path()), std::fs::canonicalize(dir.path()).unwrap());
    }

    #[test]
    fn wrap_prefixes_sandbox_exec() {
        let pol = SandboxPolicy::new(vec![], NetworkPolicy::Denied);
        let (prog, args) = pol.wrap("/bin/sh", &["-c".into(), "true".into()]);
        assert_eq!(&args[args.len() - 3..], &["/bin/sh".to_string(), "-c".into(), "true".into()]);
        if cfg!(target_os = "linux") {
            assert!(prog.ends_with("bwrap"), "{prog}");
            assert_eq!(args[0], "--die-with-parent");
        } else {
            assert_eq!(prog, SANDBOX_EXEC);
            assert_eq!(args[0], "-p");
        }
    }

    #[test]
    fn deny_lists_cover_expected_entries() {
        let home = PathBuf::from("/home/x");
        let r = default_deny_read(&home);
        assert!(r.contains(&home.join(".ssh")));
        assert!(r.contains(&home.join(".aws")));
        assert!(default_deny_exec().contains(&PathBuf::from("/usr/bin/curl")));
        let s = default_deny_sockets(&home);
        assert!(s.contains(&PathBuf::from("/var/run/docker.sock")));
        assert!(s.contains(&home.join(".orbstack")));
        assert!(s.contains(&home.join(".docker/run")));
        assert!(s.contains(&home.join(".colima")));
        assert!(s.contains(&home.join(".gnupg")));
        let prof = SandboxPolicy::new(vec![], NetworkPolicy::Denied).profile();
        assert!(prof.contains("com\\.apple\\.launchd"), "{prof}");
    }

    /// Connect to each socket inside `pol`; true where the connect succeeded.
    fn connects(pol: &SandboxPolicy, socks: &[&Path]) -> Vec<bool> {
        let script = "import socket,sys\nfor p in sys.argv[1:]:\n s=socket.socket(socket.AF_UNIX); s.settimeout(3)\n try: s.connect(p); print('Y')\n except OSError: print('N')\n";
        let mut args = vec!["-I".to_string(), "-c".into(), script.into()];
        args.extend(socks.iter().map(|s| s.to_string_lossy().to_string()));
        let (prog, a) = pol.wrap("/usr/bin/python3", &args);
        let out = std::process::Command::new(prog).args(a).output().unwrap();
        String::from_utf8_lossy(&out.stdout).lines().map(|l| l == "Y").collect()
    }

    #[test]
    fn unix_sockets_are_allowlisted() {
        if !SandboxPolicy::available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        for d in ["ws", "allowed", "allowed/daemon", "elsewhere"] {
            std::fs::create_dir(dir.path().join(d)).unwrap();
        }
        let p = |s: &str| dir.path().join(s);
        let socks = [p("ws/own.sock"), p("allowed/gate.sock"), p("allowed/daemon/d.sock"), p("elsewhere/db.sock")];
        let _l = socks.clone().map(|s| std::os::unix::net::UnixListener::bind(s).unwrap());
        std::os::unix::fs::symlink(p("elsewhere/db.sock"), p("ws/link.sock")).unwrap();
        let mut pol = SandboxPolicy::new(vec![p("ws")], NetworkPolicy::Denied);
        pol.allow_sockets.push(p("allowed"));
        pol.deny_sockets.push(p("allowed/daemon"));
        let got = connects(&pol, &[&socks[0], &socks[1], &socks[2], &socks[3], &p("ws/link.sock")]);
        assert_eq!(
            got,
            vec![true, true, false, false, false],
            "writable path and allowlist reachable; deny list wins inside an allowed path; \
             anything else, even through a symlink in the workspace, refused"
        );
    }

    #[test]
    fn the_system_logger_stays_reachable() {
        // A Linux VM may mount the Mac's /private (OrbStack), so test the OS.
        if !cfg!(target_os = "macos") || !SandboxPolicy::available() || !Path::new("/private/var/run/syslog").exists() {
            return;
        }
        let pol = SandboxPolicy::new(vec![], NetworkPolicy::Denied);
        let script = "import socket\ns=socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)\ns.connect('/var/run/syslog')\nprint('Y')\n";
        let (prog, a) = pol.wrap("/usr/bin/python3", &["-I".into(), "-c".into(), script.into()]);
        let out = std::process::Command::new(prog).args(a).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "Y", "{}", String::from_utf8_lossy(&out.stderr));
    }

    #[test]
    fn read_only_paths_are_bound_into_emptied_areas_before_they_close() {
        let Some(scratch) = ["/tmp", "/run", "/var/tmp"]
            .into_iter()
            .find(|d| std::fs::symlink_metadata(d).map(|m| m.is_dir()).unwrap_or(false))
        else {
            return;
        };
        let inside = tempfile::Builder::new().tempdir_in(scratch).unwrap();
        let secret = inside.path().join("secret");
        std::fs::create_dir(&secret).unwrap();
        let mut p = SandboxPolicy::new(vec![], NetworkPolicy::Denied);
        p.read_only = vec![inside.path().to_path_buf(), PathBuf::from("/no/such/fixtures")];
        p.deny_read = vec![secret.clone()];
        let a = p.bwrap_args("/bin/true", &[]);
        let r = |x: &Path| real_path(x).to_string_lossy().into_owned();
        let pos = |xs: &[&str]| a.windows(xs.len()).position(|w| w.iter().zip(xs).all(|(x, y)| x == y));
        let ro = pos(&["--ro-bind", &r(inside.path()), &r(inside.path())]).expect("bound read-only");
        assert!(pos(&["--tmpfs", scratch]).unwrap() < ro && ro < pos(&["--remount-ro", scratch]).unwrap(), "{a:?}");
        assert!(
            pos(&["--tmpfs", &r(&secret), "--remount-ro", &r(&secret)]).unwrap() > ro,
            "a denied path inside still wins"
        );
        assert!(!a.iter().any(|x| x.contains("/no/such")), "missing paths skipped");
    }

    #[test]
    fn host_shares_are_found_in_mountinfo() {
        // Lines from an OrbStack Ubuntu machine and a Docker container with a bind mount.
        let mountinfo = "\
654 441 0:38 /scon/containers/X/rootfs / rw,noatime shared:261 - btrfs /dev/vdb1 rw,subvol=/scon/containers/X
716 654 0:93 / /tmp rw,nosuid,nodev,relatime shared:314 - tmpfs tmpfs rw,size=9843392k
713 654 0:20 /opt/orbstack-guest/lib/modules/current /usr/lib/modules/7.2 ro,noatime shared:313 - overlay orbstack rw,lowerdir=/
717 654 0:20 /opt/orbstack-guest /opt/orbstack-guest ro,noatime shared:315 - overlay orbstack rw,lowerdir=/
719 717 0:34 / /opt/orbstack-guest/run rw,nosuid,nodev,noexec,relatime shared:317 - tmpfs orbstack rw,mode=755
720 654 0:37 / /mnt/mac rw,relatime shared:318 - virtiofs mac rw
723 654 0:37 /Users /Users rw,relatime shared:321 - virtiofs mac rw
726 654 0:40 /for-machines/rw /mnt/machines ro,noatime shared:371 master:2 - tmpfs machines rw,mode=755
773 723 0:40 /for-machines/rw /Users/inan/OrbStack ro,noatime shared:322 master:2 - tmpfs machines rw,mode=755
460 773 0:38 /scon/containers/X/rootfs /Users/inan/OrbStack/m rw,noatime shared:561 - btrfs /dev/vdb1 rw
1039 1030 0:37 /Users/x/My\\040Repo /src\\040dir ro,relatime - virtiofs mac rw
1040 1030 0:41 / /mnt/c rw,relatime - 9p drvfs rw
1043 1030 0:44 / /tab\\011and\\134slash rw - virtiofs mac rw
1041 1030 0:42 / /virt rw - virtiofs root rw
1042 1041 0:43 / / rw - virtiofs root rw
";
        let got = host_share_mounts(mountinfo);
        let mut want: Vec<PathBuf> = [
            "/mnt/c",
            "/mnt/mac",
            "/mnt/machines",
            "/opt/orbstack-guest",
            "/src dir",
            "/tab\tand\\slash",
            "/Users",
            "/usr/lib/modules/7.2",
            "/virt",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        want.sort();
        assert_eq!(got, want, "virtiofs, 9p and OrbStack's own mounts, outermost only, never /");
        assert!(host_share_mounts("garbage\n1 2 3\n").is_empty());
    }

    #[test]
    fn host_shares_are_empty_inside_the_sandbox() {
        if !cfg!(target_os = "linux") || !SandboxPolicy::available() {
            return;
        }
        let shares = host_share_mounts(&std::fs::read_to_string("/proc/self/mountinfo").unwrap());
        let shares: Vec<String> =
            shares.iter().filter(|p| p.is_dir()).map(|p| p.to_string_lossy().into_owned()).collect();
        if shares.is_empty() {
            return;
        }
        let pol = SandboxPolicy::new(vec![], NetworkPolicy::Denied);
        let mut args: Vec<String> = vec!["-c".into(), "for d in \"$@\"; do ls -A \"$d\"; done".into(), "sh".into()];
        args.extend(shares.iter().cloned());
        let (prog, a) = pol.wrap("/bin/sh", &args);
        let out = std::process::Command::new(prog).args(a).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), "", "visible inside {shares:?}");
    }

    #[test]
    fn docker_socket_is_unreachable_when_a_daemon_runs() {
        let sock = Path::new("/var/run/docker.sock");
        if !SandboxPolicy::available() || std::os::unix::net::UnixStream::connect(sock).is_err() {
            return; // no daemon on this host
        }
        let pol = SandboxPolicy::new(vec![], NetworkPolicy::Denied);
        assert_eq!(connects(&pol, &[sock]), vec![false], "the Docker daemon is reachable from inside the sandbox");
    }
}

#[cfg(test)]
mod path_tests {
    use super::*;

    #[test]
    fn missing_paths_resolve_through_their_real_parent() {
        let dir = tempfile::tempdir().unwrap();
        let real = std::fs::canonicalize(dir.path()).unwrap();
        assert_eq!(real_path(&dir.path().join("x/y")), real.join("x/y"));
        assert!(SandboxPolicy::new(vec![], NetworkPolicy::Denied).profile().contains("mDNSResponder"));
    }
}
