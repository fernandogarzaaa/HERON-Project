pub mod proxy;

use std::path::{Path, PathBuf};

pub use proxy::{start, Decision, EgressProxy, Refusal};

/// The hosts a sandboxed command may reach until the owner sets a list of their own.
pub const DEFAULT_ALLOW: &[&str] = &[
    "index.crates.io",
    "static.crates.io",
    "static.rust-lang.org",
    "github.com",
    "codeload.github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
    "pypi.org",
    "files.pythonhosted.org",
    "registry.npmjs.org",
];

const SYSTEM_DAEMON_SOCKETS: &[&str] = &["/var/run/docker.sock", "/private/var/run/docker.sock", "/var/run/podman", "/private/var/run/podman"];

const HOME_DAEMON_SOCKETS: &[&str] = &[
    ".orbstack",
    ".docker/run",
    ".colima",
    ".lima",
    ".rd",
    ".local/share/containers",
    "Library/Containers/com.docker.docker",
    ".gnupg",
    "Library/Group Containers/2BUA8C4S2C.com.1password",
];

const LAUNCHD_AGENT_SOCKETS: &str = r"^/private/(tmp|var/run)/com\.apple\.launchd\.[^/]+/Listeners$";

const NO_PROXY: &str = "localhost,127.0.0.1,::1";

pub fn seatbelt_rules(deny_ports: &[u16], user_home: &Path) -> String {
    rules(deny_ports, user_home, std::env::var_os("SSH_AUTH_SOCK").map(PathBuf::from))
}

fn rules(deny_ports: &[u16], user_home: &Path, ssh_agent: Option<PathBuf>) -> String {
    let mut out = String::from("(deny network-outbound (remote ip \"*:*\"))\n(allow network-outbound (remote ip \"localhost:*\"))\n");
    let mut seen: Vec<u16> = vec![];
    for port in deny_ports.iter().copied().filter(|p| *p != 0) {
        if !seen.contains(&port) {
            seen.push(port);
            out.push_str(&format!("(deny network-outbound (remote ip \"localhost:{port}\"))\n"));
        }
    }
    out.push_str("(deny network-outbound (remote unix-socket (path-literal \"/private/var/run/mDNSResponder\")))\n");
    let mut sockets: Vec<PathBuf> = SYSTEM_DAEMON_SOCKETS.iter().map(PathBuf::from).collect();
    sockets.extend(HOME_DAEMON_SOCKETS.iter().map(|p| user_home.join(p)));
    sockets.extend(ssh_agent);
    for path in socket_forms(&sockets) {
        out.push_str(&format!("(deny network-outbound (remote unix-socket (subpath {})))\n", quote(&path)));
    }
    out.push_str(&format!("(deny network-outbound (remote unix-socket (path-regex #\"{LAUNCHD_AGENT_SOCKETS}\")))\n"));
    out
}

fn socket_forms(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen: Vec<PathBuf> = vec![];
    for form in paths.iter().flat_map(|p| crate::sandbox::both_forms(p)) {
        if !seen.contains(&form) {
            seen.push(form);
        }
    }
    let aliases: Vec<PathBuf> = seen.iter().filter_map(|p| crate::sandbox::data_volume_alias(p)).collect();
    for alias in aliases {
        if !seen.contains(&alias) {
            seen.push(alias);
        }
    }
    seen
}

fn quote(p: &Path) -> String {
    format!("\"{}\"", p.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\""))
}

pub fn proxy_env(port: u16) -> Vec<(String, String)> {
    let url = format!("http://127.0.0.1:{port}");
    let mut env: Vec<(String, String)> = ["HTTPS_PROXY", "HTTP_PROXY", "ALL_PROXY", "https_proxy", "http_proxy", "all_proxy"].iter().map(|k| (k.to_string(), url.clone())).collect();
    env.extend(["NO_PROXY", "no_proxy"].iter().map(|k| (k.to_string(), NO_PROXY.to_string())));
    env
}

/// One line for each distinct refusal, to append to a command's output. `None` when nothing was refused.
pub fn blocked_note(decisions: &[Decision]) -> Option<String> {
    let mut lines: Vec<String> = vec![];
    for d in decisions {
        let Some(why) = &d.refusal else { continue };
        let target = plain(&d.target);
        let line = match why {
            Refusal::NotListed => format!("[network] blocked {target}: not on the sandbox's egress allowlist. Don't work around this; tell the user, who can add the host in Settings › Sandbox."),
            Refusal::NotPublic => format!("[network] blocked {target}: it resolves to a private or local address, which the sandbox never reaches."),
            Refusal::NameMismatch(name) => {
                let name = name.as_deref().map(plain).unwrap_or_else(|| "none".to_string());
                format!("[network] blocked {target}: the TLS connection named a different server ({name}).")
            }
            Refusal::NotHttps => format!("[network] blocked {target}: the sandbox allows only HTTPS on port 443, through its proxy."),
        };
        if !lines.contains(&line) {
            lines.push(line);
        }
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// Text from the network, made safe to show: printable ASCII only, and short.
fn plain(s: &str) -> String {
    s.chars().take(120).map(|c| if c.is_ascii_graphic() || c == ' ' { c } else { '?' }).collect()
}

/// An exact host name, or `*.` and at least two labels. IP addresses, ports, URLs and empty entries are not.
pub fn validate_host(entry: &str) -> anyhow::Result<()> {
    let name = entry.strip_prefix("*.").unwrap_or(entry);
    let wildcard = name.len() != entry.len();
    let labels: Vec<&str> = name.split('.').collect();
    let label_ok = |l: &&str| {
        !l.is_empty() && l.len() <= 63 && !l.starts_with('-') && !l.ends_with('-') && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    };
    let valid = name.len() <= 253
        && labels.iter().all(label_ok)
        && !labels.last().is_some_and(|l| l.bytes().all(|b| b.is_ascii_digit()))
        && (!wildcard || labels.len() >= 2);
    if valid {
        Ok(())
    } else {
        anyhow::bail!("Not a host name: {entry}.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        PathBuf::from("/Users/someone")
    }

    fn lines(rules: &str) -> Vec<&str> {
        rules.lines().collect()
    }

    fn at(rules: &str, line: &str) -> usize {
        lines(rules).iter().position(|l| *l == line).unwrap_or_else(|| panic!("missing {line}\n{rules}"))
    }

    #[test]
    fn rules_deny_outbound_ip_except_loopback() {
        let r = seatbelt_rules(&[], &home());
        let deny = at(&r, "(deny network-outbound (remote ip \"*:*\"))");
        let allow = at(&r, "(allow network-outbound (remote ip \"localhost:*\"))");
        assert!(deny < allow, "the loopback allowance comes after the blanket deny");
        for broader in ["(deny network*", "network-bind", "network-inbound", "(deny default"] {
            assert!(!r.contains(broader), "{broader} would break local servers\n{r}");
        }
    }

    #[test]
    fn deny_local_port_overrides_loopback() {
        let r = seatbelt_rules(&[7878, 8080, 7878, 0], &home());
        let allow = at(&r, "(allow network-outbound (remote ip \"localhost:*\"))");
        assert!(at(&r, "(deny network-outbound (remote ip \"localhost:7878\"))") > allow);
        assert!(at(&r, "(deny network-outbound (remote ip \"localhost:8080\"))") > allow);
        assert_eq!(r.matches("localhost:7878").count(), 1, "a repeated port is denied once");
        assert!(!r.contains("localhost:0\""), "port 0 is not a port");
        assert!(!seatbelt_rules(&[], &home()).contains("localhost:7878"));
    }

    #[test]
    fn rules_deny_the_resolver_socket() {
        let r = seatbelt_rules(&[], &home());
        assert!(r.contains("(deny network-outbound (remote unix-socket (path-literal \"/private/var/run/mDNSResponder\")))"), "{r}");
    }

    #[test]
    #[cfg_attr(not(unix), ignore = "Seatbelt rules are written for unix paths")]
    fn daemon_sockets_are_denied() {
        let r = rules(&[], &home(), Some(PathBuf::from("/private/var/run/com.apple.launchd.AbC/Listeners")));
        for path in [
            "/var/run/docker.sock",
            "/private/var/run/docker.sock",
            "/var/run/podman",
            "/Users/someone/.orbstack",
            "/Users/someone/.docker/run",
            "/Users/someone/.colima",
            "/Users/someone/.lima",
            "/Users/someone/.rd",
            "/Users/someone/.local/share/containers",
            "/Users/someone/Library/Containers/com.docker.docker",
            "/Users/someone/.gnupg",
            "/Users/someone/Library/Group Containers/2BUA8C4S2C.com.1password",
            "/private/var/run/com.apple.launchd.AbC/Listeners",
        ] {
            let rule = format!("(deny network-outbound (remote unix-socket (subpath \"{path}\")))");
            assert!(r.contains(&rule), "missing {rule}\n{r}");
        }
        assert!(r.contains(r##"(path-regex #"^/private/(tmp|var/run)/com\.apple\.launchd\.[^/]+/Listeners$")"##), "{r}");
        assert!(!rules(&[], &home(), None).contains("AbC"));
    }

    #[test]
    fn blocked_hosts_are_named_once_with_their_reason() {
        let refused = |target: &str, why| Decision { target: target.into(), refusal: Some(why) };
        let decisions = vec![
            refused("evil.com:443", Refusal::NotListed),
            Decision { target: "static.crates.io:443".into(), refusal: None },
            refused("evil.com:443", Refusal::NotListed),
            refused("localhost:443", Refusal::NotPublic),
            refused("github.com:443", Refusal::NameMismatch(Some("evil.com".into()))),
            refused("github.com:443", Refusal::NameMismatch(None)),
            refused("GET http://x.test/", Refusal::NotHttps),
        ];
        let note = blocked_note(&decisions).unwrap();
        assert_eq!(
            note,
            "[network] blocked evil.com:443: not on the sandbox's egress allowlist. Don't work around this; tell the user, who can add the host in Settings › Sandbox.\n\
             [network] blocked localhost:443: it resolves to a private or local address, which the sandbox never reaches.\n\
             [network] blocked github.com:443: the TLS connection named a different server (evil.com).\n\
             [network] blocked github.com:443: the TLS connection named a different server (none).\n\
             [network] blocked GET http://x.test/: the sandbox allows only HTTPS on port 443, through its proxy."
        );
        assert_eq!(blocked_note(&decisions[1..2]), None);
        assert_eq!(blocked_note(&[]), None);
        let hostile = refused("a\u{1b}[2J\nb:443", Refusal::NotListed);
        assert!(!blocked_note(&[hostile]).unwrap().contains(['\u{1b}', '\r']), "control bytes from the network never reach the model");
    }

    #[test]
    fn default_list_excludes_crates_io() {
        assert!(!DEFAULT_ALLOW.contains(&"crates.io"));
        assert!(DEFAULT_ALLOW.contains(&"index.crates.io") && DEFAULT_ALLOW.contains(&"static.crates.io"));
        assert_eq!(DEFAULT_ALLOW.len(), 10);
        let owned: Vec<String> = DEFAULT_ALLOW.iter().map(|h| h.to_string()).collect();
        assert!(!proxy::host_allowed(&owned, "crates.io"));
        for h in DEFAULT_ALLOW {
            assert!(validate_host(h).is_ok(), "{h}");
        }
    }

    #[test]
    fn host_entries_are_validated() {
        for ok in ["github.com", "*.github.com", "localhost", "a-b.example.org", "EXAMPLE.org", "x1.y2.z3.example.co.uk"] {
            assert!(validate_host(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            " ",
            "*",
            "*.",
            "*.com",
            "*.*.example.org",
            "a*.example.org",
            "example.org:443",
            "https://example.org",
            "example.org/path",
            "1.2.3.4",
            "::1",
            "[::1]",
            "a..b.com",
            ".example.org",
            "example.org.",
            "-a.com",
            "a-.com",
            "ex ample.com",
            "exämple.com",
        ] {
            let err = validate_host(bad).unwrap_err().to_string();
            assert_eq!(err, format!("Not a host name: {bad}."), "{bad:?}");
        }
        assert!(validate_host(&format!("{}.com", "a".repeat(64))).is_err());
        assert!(validate_host(&format!("{}.com", "a".repeat(63))).is_ok());
    }

    #[test]
    fn proxy_env_points_every_tool_at_the_proxy() {
        let env = proxy_env(4321);
        for key in ["HTTPS_PROXY", "HTTP_PROXY", "ALL_PROXY", "https_proxy", "http_proxy", "all_proxy"] {
            assert!(env.contains(&(key.to_string(), "http://127.0.0.1:4321".to_string())), "{key}");
        }
        for key in ["NO_PROXY", "no_proxy"] {
            assert!(env.contains(&(key.to_string(), "localhost,127.0.0.1,::1".to_string())), "{key}");
        }
        assert_eq!(env.len(), 8);
    }
}
