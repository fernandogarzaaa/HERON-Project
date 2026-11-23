//! Egress proxy: the only network path out of the sandbox.
//!
//! The sandbox allows outbound connections to `localhost:<port>` only. This
//! proxy listens there, accepts `CONNECT host:443` for allowlisted hosts and
//! refuses everything else, including plain-HTTP requests. Two more checks
//! close the gaps a hostname allowlist leaves open:
//!
//! - the host must resolve to a public address (DNS pointing an allowlisted
//!   name at loopback, the LAN or a cloud metadata endpoint is refused);
//! - the tunnel's TLS ClientHello must name the same host (SNI), so a client
//!   cannot CONNECT to an allowed name and then talk TLS to another.
//!
//! The proxy does not terminate TLS. Every decision is logged for the trace.

use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressDecision {
    pub target: String,
    pub allowed: bool,
    pub reason: String,
}

pub struct EgressProxy {
    pub port: u16,
    /// Linux: the same proxy on a unix socket, for `harness net-bridge`
    /// inside a sandbox whose network namespace has only loopback.
    pub unix_socket: Option<std::path::PathBuf>,
    decisions: Arc<Mutex<Vec<EgressDecision>>>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
    _dir: Option<tempfile::TempDir>,
}

impl EgressProxy {
    pub fn decisions(&self) -> Vec<EgressDecision> {
        self.decisions.lock().unwrap().clone()
    }
}

impl Drop for EgressProxy {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
    }
}

pub fn host_allowed(allow: &[String], host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    allow.iter().any(|p| {
        let p = p.to_ascii_lowercase();
        match p.strip_prefix("*.") {
            Some(suffix) => host.ends_with(&format!(".{suffix}")) && host.len() > suffix.len() + 1,
            None => host == p,
        }
    })
}

/// Parse the request head of a proxy request into (method, target).
pub fn parse_head(head: &str) -> Option<(String, String)> {
    let line = head.lines().next()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();
    let version = parts.next()?;
    if !version.starts_with("HTTP/1.") {
        return None;
    }
    Some((method, target))
}

/// Decide a request: only `CONNECT <allowed-host>:443`. The host must match
/// `allow` and every list in `caps` (the managed requirements' per-file
/// `egress_allow_max`); no caps leaves `allow` alone.
pub fn decide(allow: &[String], caps: &[Vec<String>], method: &str, target: &str) -> EgressDecision {
    let deny = |reason: &str| EgressDecision { target: target.into(), allowed: false, reason: reason.into() };
    if method != "CONNECT" {
        return deny("only CONNECT tunnels are allowed");
    }
    let Some((host, port)) = target.rsplit_once(':') else {
        return deny("target has no port");
    };
    if port != "443" {
        return deny("only port 443 is allowed");
    }
    if host.starts_with('[') || host.parse::<std::net::IpAddr>().is_ok() {
        return deny("IP literals are not allowed; use an allowlisted hostname");
    }
    if !host_allowed(allow, host) {
        return deny("host is not on the allowlist");
    }
    if !caps.iter().all(|cap| host_allowed(cap, host)) {
        return deny("host is outside the managed egress_allow_max");
    }
    EgressDecision { target: target.into(), allowed: true, reason: "allowlisted".into() }
}

/// Whether an address is on the public internet. Loopback, private,
/// link-local (incl. cloud metadata), carrier-grade NAT, unique-local,
/// multicast, unspecified and documentation ranges are not, including their
/// IPv4-mapped IPv6 forms.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_documentation()
                || o[0] == 0
                || (o[0] == 100 && (o[1] & 0xc0) == 64) // 100.64.0.0/10
                || (o[0] == 192 && o[1] == 0 && o[2] == 0) // 192.0.0.0/24
                || (o[0] == 198 && (o[1] & 0xfe) == 18) // 198.18.0.0/15
                || o[0] >= 240)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let s = v6.segments();
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00 // unique local
                || (s[0] & 0xffc0) == 0xfe80 // link local
                || (s[0] == 0x2001 && s[1] == 0x0db8)) // documentation
        }
    }
}

/// The server name (SNI) in a TLS ClientHello record, if `buf` holds a
/// complete one. `Err` means the bytes are not a ClientHello; `Ok(None)`
/// means more bytes are needed.
pub fn client_hello_sni(buf: &[u8]) -> Result<Option<Option<String>>, &'static str> {
    if buf.len() < 5 {
        return Ok(None);
    }
    if buf[0] != 0x16 || buf[1] != 0x03 {
        return Err("not a TLS handshake");
    }
    let rec_len = u16::from_be_bytes([buf[3], buf[4]]) as usize;
    if rec_len > 18 * 1024 {
        return Err("TLS record too large");
    }
    if buf.len() < 5 + rec_len {
        return Ok(None);
    }
    let mut p = Parser { b: &buf[5..5 + rec_len], i: 0 };
    if p.u8()? != 0x01 {
        return Err("first handshake message is not a ClientHello");
    }
    let hs_len = p.u24()?;
    let mut p = Parser { b: p.take(hs_len)?, i: 0 };
    p.take(2 + 32)?; // client_version, random
    let n = p.u8()? as usize;
    p.take(n)?; // session_id
    let n = p.u16()? as usize;
    p.take(n)?; // cipher_suites
    let n = p.u8()? as usize;
    p.take(n)?; // compression_methods
    if p.i == p.b.len() {
        return Ok(Some(None)); // no extensions
    }
    let n = p.u16()? as usize;
    let mut ext = Parser { b: p.take(n)?, i: 0 };
    while ext.i < ext.b.len() {
        let kind = ext.u16()?;
        let len = ext.u16()? as usize;
        let body = ext.take(len)?;
        if kind == 0 {
            let mut s = Parser { b: body, i: 0 };
            let list = s.u16()? as usize;
            let mut l = Parser { b: s.take(list)?, i: 0 };
            while l.i < l.b.len() {
                let name_type = l.u8()?;
                let len = l.u16()? as usize;
                let name = l.take(len)?;
                if name_type == 0 {
                    let name = std::str::from_utf8(name).map_err(|_| "SNI is not UTF-8")?;
                    return Ok(Some(Some(name.to_ascii_lowercase())));
                }
            }
        }
    }
    Ok(Some(None))
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], &'static str> {
        let s = self.b.get(self.i..self.i + n).ok_or("truncated ClientHello")?;
        self.i += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, &'static str> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, &'static str> {
        let s = self.take(2)?;
        Ok(u16::from_be_bytes([s[0], s[1]]))
    }
    fn u24(&mut self) -> Result<usize, &'static str> {
        let s = self.take(3)?;
        Ok(((s[0] as usize) << 16) | ((s[1] as usize) << 8) | s[2] as usize)
    }
}

/// Test hooks (reach a local upstream on another port) and whether to
/// also listen on a unix socket.
#[derive(Clone, Copy)]
struct Options {
    allow_internal: bool,
    upstream_port: Option<u16>,
    unix: bool,
}

// Not derivable on Linux, where `unix` defaults to true.
#[allow(clippy::derivable_impls)]
impl Default for Options {
    fn default() -> Self {
        Options { allow_internal: false, upstream_port: None, unix: cfg!(target_os = "linux") }
    }
}

async fn handle<S>(
    mut client: S,
    allow: Arc<Vec<String>>,
    caps: Arc<Vec<Vec<String>>>,
    log: Arc<Mutex<Vec<EgressDecision>>>,
    opts: Options,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() > 8192 {
            return;
        }
        match client.read(&mut byte).await {
            Ok(1) => head.push(byte[0]),
            _ => return,
        }
    }
    let text = String::from_utf8_lossy(&head).into_owned();
    let decision = match parse_head(&text) {
        Some((m, t)) => decide(&allow, &caps, &m, &t),
        None => EgressDecision { target: String::new(), allowed: false, reason: "malformed request".into() },
    };
    let refuse = |d: &EgressDecision| log.lock().unwrap().push(d.clone());
    if !decision.allowed {
        refuse(&decision);
        let _ = client.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
        return;
    }
    let target = decision.target.clone();
    let deny = |reason: String| EgressDecision { target: target.clone(), allowed: false, reason };
    let host = target.rsplit_once(':').map(|(h, _)| h.to_ascii_lowercase()).unwrap_or_default();
    // Resolve here and connect only to a public address.
    let addrs: Vec<SocketAddr> = match tokio::net::lookup_host(&target).await {
        Ok(a) => a.filter(|a| opts.allow_internal || is_public(a.ip())).collect(),
        Err(_) => vec![],
    };
    if addrs.is_empty() {
        refuse(&deny("host does not resolve to a public address".into()));
        let _ = client.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
        return;
    }
    if client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").await.is_err() {
        return;
    }
    // The client's ClientHello must name the CONNECT host before anything
    // goes upstream.
    let mut hello = Vec::new();
    let mut chunk = [0u8; 4096];
    let sni = loop {
        match client_hello_sni(&hello) {
            Ok(Some(sni)) => break Ok(sni),
            Err(e) => break Err(e.to_string()),
            Ok(None) => {}
        }
        match tokio::time::timeout(Duration::from_secs(10), client.read(&mut chunk)).await {
            Ok(Ok(n)) if n > 0 => hello.extend_from_slice(&chunk[..n]),
            _ => break Err("no TLS ClientHello received".into()),
        }
    };
    match sni {
        Ok(Some(name)) if name == host => {}
        Ok(Some(name)) => return refuse(&deny(format!("TLS server name {name} does not match the CONNECT host"))),
        Ok(None) => return refuse(&deny("TLS ClientHello has no server name".into())),
        Err(e) => return refuse(&deny(e)),
    }
    log.lock().unwrap().push(decision);
    for mut addr in addrs {
        if let Some(port) = opts.upstream_port {
            addr.set_port(port);
        }
        if let Ok(mut upstream) = TcpStream::connect(addr).await {
            if upstream.write_all(&hello).await.is_ok() {
                let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
            }
            return;
        }
    }
}

/// Start a proxy on an ephemeral localhost port.
pub async fn start(allow: Vec<String>, caps: Vec<Vec<String>>) -> std::io::Result<EgressProxy> {
    start_with(allow, caps, Options::default()).await
}

async fn start_with(allow: Vec<String>, caps: Vec<Vec<String>>, opts: Options) -> std::io::Result<EgressProxy> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let decisions = Arc::new(Mutex::new(Vec::new()));
    let log = decisions.clone();
    let allow = Arc::new(allow);
    let caps = Arc::new(caps);
    let (a, c, l) = (allow.clone(), caps.clone(), log.clone());
    let mut tasks = vec![tokio::spawn(async move {
        while let Ok((sock, _)) = listener.accept().await {
            tokio::spawn(handle(sock, a.clone(), c.clone(), l.clone(), opts));
        }
    })];
    let (mut unix_socket, mut dir) = (None, None);
    if opts.unix {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::Builder::new()
            .prefix("ahp-")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in("/tmp")?;
        let path = d.path().join("p.sock");
        let ul = tokio::net::UnixListener::bind(&path)?;
        tasks.push(tokio::spawn(async move {
            while let Ok((sock, _)) = ul.accept().await {
                tokio::spawn(handle(sock, allow.clone(), caps.clone(), log.clone(), opts));
            }
        }));
        unix_socket = Some(path);
        dir = Some(d);
    }
    Ok(EgressProxy { port, unix_socket, decisions, tasks, _dir: dir })
}

/// Inside the Linux sandbox (network namespace with only loopback): listen
/// on `127.0.0.1:port`, forward every connection to the egress proxy's unix
/// socket, and run `cmd`. Returns `cmd`'s exit code.
pub async fn net_bridge(port: u16, socket: std::path::PathBuf, cmd: Vec<String>) -> i32 {
    let listener = match TcpListener::bind(("127.0.0.1", port)).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("harness net-bridge: cannot listen on 127.0.0.1:{port}: {e}");
            return 125;
        }
    };
    let Some((prog, rest)) = cmd.split_first() else {
        eprintln!("harness net-bridge: no command");
        return 125;
    };
    let mut child = match tokio::process::Command::new(prog).args(rest).spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("harness net-bridge: cannot start {prog}: {e}");
            return 127;
        }
    };
    loop {
        tokio::select! {
            status = child.wait() => return status.ok().and_then(|s| s.code()).unwrap_or(1),
            accepted = listener.accept() => {
                if let Ok((mut tcp, _)) = accepted {
                    let socket = socket.clone();
                    tokio::spawn(async move {
                        if let Ok(mut up) = tokio::net::UnixStream::connect(&socket).await {
                            let _ = tokio::io::copy_bidirectional(&mut tcp, &mut up).await;
                        }
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allow() -> Vec<String> {
        vec!["api.anthropic.com".into(), "*.example.org".into()]
    }

    #[test]
    fn decisions() {
        assert!(decide(&allow(), &[], "CONNECT", "api.anthropic.com:443").allowed);
        assert!(decide(&allow(), &[], "CONNECT", "a.example.org:443").allowed);
        for (m, t) in [
            ("GET", "http://api.anthropic.com/"),
            ("CONNECT", "api.anthropic.com:80"),
            ("CONNECT", "evil.com:443"),
            ("CONNECT", "api.anthropic.com.evil.com:443"),
            ("CONNECT", "example.org:443"),
            ("CONNECT", "1.2.3.4:443"),
            ("CONNECT", "[::1]:443"),
            ("CONNECT", "noport"),
        ] {
            assert!(!decide(&allow(), &[], m, t).allowed, "{m} {t}");
        }
    }

    #[test]
    fn every_cap_must_match_as_well_as_the_allowlist() {
        let allow = vec!["a.com".to_string(), "b.com".to_string(), "c.com".to_string()];
        let system = vec!["a.com".to_string()];
        let home = vec!["a.com".to_string(), "b.com".to_string()];
        let caps = [system.clone(), home];
        assert!(decide(&allow, &caps, "CONNECT", "a.com:443").allowed);
        let b = decide(&allow, &caps, "CONNECT", "b.com:443");
        assert!(!b.allowed, "b is outside the system cap");
        assert!(b.reason.contains("egress_allow_max"), "{}", b.reason);
        assert!(!decide(&allow, &[system], "CONNECT", "b.com:443").allowed);
        assert!(!decide(&allow, &caps, "CONNECT", "c.com:443").allowed, "outside every cap");
        assert!(!decide(&["a.com".to_string()], &caps, "CONNECT", "b.com:443").allowed, "caps never widen allow");
        assert!(decide(&allow, &[], "CONNECT", "c.com:443").allowed, "no caps: allowlist alone decides");
        assert!(!decide(&allow, &[vec![]], "CONNECT", "a.com:443").allowed, "an empty cap allows nothing");
    }

    #[tokio::test]
    async fn capped_host_is_refused_over_the_wire() {
        let proxy = start(allow(), vec![vec!["api.anthropic.com".into()]]).await.unwrap();
        let mut s = TcpStream::connect(("127.0.0.1", proxy.port)).await.unwrap();
        s.write_all(b"CONNECT a.example.org:443 HTTP/1.1\r\nHost: a.example.org\r\n\r\n").await.unwrap();
        let mut buf = vec![0u8; 64];
        let n = s.read(&mut buf).await.unwrap();
        assert!(String::from_utf8_lossy(&buf[..n]).starts_with("HTTP/1.1 403"));
        let d = proxy.decisions();
        assert_eq!(d.len(), 1);
        assert!(!d[0].allowed);
        assert!(d[0].reason.contains("egress_allow_max"));
    }

    #[test]
    fn head_parsing() {
        assert_eq!(
            parse_head("CONNECT a.com:443 HTTP/1.1\r\nHost: a.com\r\n\r\n"),
            Some(("CONNECT".into(), "a.com:443".into()))
        );
        assert_eq!(parse_head("garbage"), None);
        assert_eq!(parse_head("CONNECT a.com:443 SPDY/3\r\n\r\n"), None);
    }

    #[tokio::test]
    async fn refuses_disallowed_tunnels_over_the_wire() {
        let proxy = start(allow(), vec![]).await.unwrap();
        let mut s = TcpStream::connect(("127.0.0.1", proxy.port)).await.unwrap();
        s.write_all(b"CONNECT evil.com:443 HTTP/1.1\r\nHost: evil.com\r\n\r\n").await.unwrap();
        let mut buf = vec![0u8; 64];
        let n = s.read(&mut buf).await.unwrap();
        assert!(String::from_utf8_lossy(&buf[..n]).starts_with("HTTP/1.1 403"));
        let d = proxy.decisions();
        assert_eq!(d.len(), 1);
        assert!(!d[0].allowed);
        assert_eq!(d[0].target, "evil.com:443");
    }
}

#[cfg(test)]
mod edge_tests {
    use super::*;

    #[test]
    fn wildcard_needs_a_real_label_and_ips_are_refused_even_if_listed() {
        let allow = vec!["*.example.org".to_string(), "1.2.3.4".to_string()];
        assert!(!host_allowed(&allow, ".example.org"), "empty label");
        assert!(!host_allowed(&allow, "example.org"));
        assert!(host_allowed(&allow, "a.example.org"));
        assert!(
            !decide(&allow, &[], "CONNECT", "1.2.3.4:443").allowed,
            "IP literals are refused even when allowlisted"
        );
        assert!(!decide(&allow, &[], "CONNECT", ".example.org:443").allowed);
    }

    #[tokio::test]
    async fn dropping_the_proxy_closes_its_port() {
        let p = start(vec!["a.com".into()], vec![]).await.unwrap();
        let port = p.port;
        drop(p);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());
    }

    #[tokio::test]
    async fn oversized_request_head_is_dropped() {
        let p = start(vec!["a.com".into()], vec![]).await.unwrap();
        let mut s = TcpStream::connect(("127.0.0.1", p.port)).await.unwrap();
        let junk = format!("CONNECT a.com:443 HTTP/1.1\r\nX: {}\r\n", "y".repeat(9000));
        let _ = s.write_all(junk.as_bytes()).await;
        let mut buf = vec![0u8; 64];
        let n = tokio::time::timeout(std::time::Duration::from_secs(2), s.read(&mut buf)).await.unwrap().unwrap_or(0);
        assert!(!String::from_utf8_lossy(&buf[..n]).contains("200"), "no tunnel for an oversized head");
        assert!(p.decisions().is_empty());
    }
}

#[cfg(test)]
mod tls_tests {
    use super::*;

    /// A minimal TLS 1.2-style ClientHello record with an optional SNI.
    pub fn client_hello(sni: Option<&str>) -> Vec<u8> {
        let mut ext = Vec::new();
        ext.extend_from_slice(&[0x00, 0x0b, 0x00, 0x02, 0x01, 0x00]); // ec_point_formats
        if let Some(name) = sni {
            let n = name.len() as u16;
            let mut body = Vec::new();
            body.extend_from_slice(&(n + 3).to_be_bytes());
            body.push(0);
            body.extend_from_slice(&n.to_be_bytes());
            body.extend_from_slice(name.as_bytes());
            ext.extend_from_slice(&[0x00, 0x00]);
            ext.extend_from_slice(&(body.len() as u16).to_be_bytes());
            ext.extend_from_slice(&body);
        }
        let mut hs = vec![0x03, 0x03];
        hs.extend_from_slice(&[7u8; 32]);
        hs.push(0);
        hs.extend_from_slice(&[0x00, 0x02, 0x13, 0x01]);
        hs.extend_from_slice(&[0x01, 0x00]);
        hs.extend_from_slice(&(ext.len() as u16).to_be_bytes());
        hs.extend_from_slice(&ext);
        let mut msg = vec![0x01];
        msg.extend_from_slice(&(hs.len() as u32).to_be_bytes()[1..]);
        msg.extend_from_slice(&hs);
        let mut rec = vec![0x16, 0x03, 0x01];
        rec.extend_from_slice(&(msg.len() as u16).to_be_bytes());
        rec.extend_from_slice(&msg);
        rec
    }

    #[test]
    fn sni_is_read_from_a_client_hello() {
        let h = client_hello(Some("API.Anthropic.com"));
        assert_eq!(client_hello_sni(&h), Ok(Some(Some("api.anthropic.com".into()))));
        assert_eq!(client_hello_sni(&client_hello(None)), Ok(Some(None)));
        assert_eq!(client_hello_sni(&h[..h.len() - 3]), Ok(None), "needs the whole record");
        assert_eq!(client_hello_sni(&h[..4]), Ok(None));
        assert!(client_hello_sni(b"GET / HTTP/1.1\r\n\r\n").is_err(), "plain HTTP inside the tunnel");
        let mut bad = h.clone();
        bad[5] = 0x02; // ServerHello, not ClientHello
        assert!(client_hello_sni(&bad).is_err());
        let mut lying = h.clone();
        lying[4] = lying[4].wrapping_add(40); // record claims more than the message holds
        lying.extend_from_slice(&[0u8; 40]);
        assert!(client_hello_sni(&lying).is_ok() || client_hello_sni(&lying).is_err());
        let mut huge = h;
        huge[3] = 0xff;
        assert!(client_hello_sni(&huge).is_err(), "oversized record");
    }

    /// A ClientHello padded to about `size` bytes (real ones with
    /// post-quantum key shares exceed 1 KB).
    fn padded_hello(sni: &str, size: usize) -> Vec<u8> {
        let mut h = client_hello(Some(sni));
        let pad = size.saturating_sub(h.len());
        // Append a padding extension (type 21) and fix the three lengths.
        let mut ext = vec![0x00, 0x15];
        ext.extend_from_slice(&(pad as u16).to_be_bytes());
        ext.extend(std::iter::repeat_n(0u8, pad));
        let ext_len_at = h.len() - (h.len() - 5 - 4 - 2 - 32 - 1 - 4 - 2) - 2;
        let _ = ext_len_at;
        // Rebuild: record header (5) + handshake header (4) + body.
        let body_start = 9;
        let mut body = h[body_start..].to_vec();
        // extensions length sits after version(2)+random(32)+sid(1)+suites(4)+comp(2)
        let at = 2 + 32 + 1 + 4 + 2;
        let old = u16::from_be_bytes([body[at], body[at + 1]]) as usize;
        let new = old + ext.len();
        body[at..at + 2].copy_from_slice(&(new as u16).to_be_bytes());
        body.extend_from_slice(&ext);
        let mut msg = vec![0x01];
        msg.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
        msg.extend_from_slice(&body);
        h = vec![0x16, 0x03, 0x01];
        h.extend_from_slice(&(msg.len() as u16).to_be_bytes());
        h.extend_from_slice(&msg);
        h
    }

    #[test]
    fn client_hello_parser_edges() {
        // Large ClientHellos (post-quantum key shares) are normal.
        for size in [1500, 4000, 16000] {
            let h = padded_hello("api.anthropic.com", size);
            assert_eq!(client_hello_sni(&h), Ok(Some(Some("api.anthropic.com".into()))), "{size} bytes");
        }
        // The size limit is inclusive at 18 KiB.
        let mut at_limit = vec![0x16, 0x03, 0x01];
        at_limit.extend_from_slice(&((18 * 1024) as u16).to_be_bytes());
        assert_eq!(client_hello_sni(&at_limit), Ok(None), "18 KiB is allowed; wait for the rest");
        let mut over = vec![0x16, 0x03, 0x01];
        over.extend_from_slice(&((18 * 1024 + 1) as u16).to_be_bytes());
        assert!(client_hello_sni(&over).is_err());
        // A record header with an empty body is not a ClientHello.
        assert!(client_hello_sni(&[0x16, 0x03, 0x01, 0x00, 0x00]).is_err());
        // Wrong record version byte.
        let mut v = client_hello(Some("a.com"));
        v[1] = 0x05;
        assert!(client_hello_sni(&v).is_err());
        // An SNI list with only a non-hostname entry has no server name.
        let mut odd = client_hello(Some("a.com"));
        let pos = odd.windows(5).position(|w| w == b"a.com").unwrap();
        odd[pos - 3] = 1; // name_type 1 instead of host_name
        assert_eq!(client_hello_sni(&odd), Ok(Some(None)));
    }

    #[test]
    fn only_public_addresses_are_reachable() {
        for ip in [
            "93.184.216.34",
            "160.79.104.10",
            "8.8.8.8",
            "100.0.0.1",   // outside 100.64.0.0/10
            "100.128.0.1", // outside 100.64.0.0/10
            "8.64.1.1",
            "192.0.1.1",  // outside 192.0.0.0/24
            "198.20.0.1", // outside 198.18.0.0/15
            "11.0.0.1",
            "2606:4700::1111",
            "2001:4860:4860::8888", // 2001:: but not documentation
            "2a00:1450::1",
        ] {
            assert!(is_public(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["100.127.255.255", "198.19.255.255", "192.0.0.255", "127.255.255.254"] {
            assert!(!is_public(ip.parse().unwrap()), "{ip}");
        }
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "255.255.255.255",
            "224.0.0.1",
            "198.18.0.1",
            "192.0.0.8",
            "240.0.0.1",
            "::1",
            "::",
            "fe80::1",
            "fd00::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "::ffff:169.254.169.254",
            "2001:db8::1",
        ] {
            assert!(!is_public(ip.parse().unwrap()), "{ip}");
        }
    }

    async fn connect_through(proxy: &EgressProxy, host: &str) -> TcpStream {
        let mut s = TcpStream::connect(("127.0.0.1", proxy.port)).await.unwrap();
        s.write_all(format!("CONNECT {host}:443 HTTP/1.1\r\nHost: {host}\r\n\r\n").as_bytes()).await.unwrap();
        s
    }

    async fn read_some(s: &mut TcpStream) -> String {
        let mut buf = vec![0u8; 256];
        let n = tokio::time::timeout(Duration::from_secs(5), s.read(&mut buf)).await.unwrap().unwrap_or(0);
        String::from_utf8_lossy(&buf[..n]).into_owned()
    }

    #[tokio::test]
    async fn allowlisted_names_resolving_to_internal_addresses_are_refused() {
        let proxy = start(vec!["localhost".into()], vec![]).await.unwrap();
        let mut s = connect_through(&proxy, "localhost").await;
        assert!(read_some(&mut s).await.starts_with("HTTP/1.1 403"));
        let d = proxy.decisions();
        assert!(d.iter().all(|d| !d.allowed) && d[0].reason.contains("public"), "{d:?}");
    }

    #[tokio::test]
    async fn the_tls_server_name_must_match_the_connect_host() {
        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = upstream.local_addr().unwrap().port();
        let got = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
        let sink = got.clone();
        tokio::spawn(async move {
            while let Ok((mut c, _)) = upstream.accept().await {
                let sink = sink.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = c.read(&mut buf).await.unwrap_or(0);
                    sink.lock().unwrap().push(buf[..n].to_vec());
                });
            }
        });
        let opts = Options { allow_internal: true, upstream_port: Some(port), unix: true };
        let proxy = start_with(vec!["localhost".into()], vec![], opts).await.unwrap();
        for (sni, ok) in [(Some("evil.com"), false), (None, false), (Some("localhost"), true)] {
            let mut s = connect_through(&proxy, "localhost").await;
            assert!(read_some(&mut s).await.starts_with("HTTP/1.1 200"));
            // Sent in two pieces: the proxy must wait for the whole record.
            let hello = client_hello(sni);
            s.write_all(&hello[..10]).await.unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
            s.write_all(&hello[10..]).await.unwrap();
            tokio::time::sleep(Duration::from_millis(200)).await;
            let reached = got.lock().unwrap().iter().any(|b| b == &hello);
            assert_eq!(reached, ok, "sni {sni:?}");
        }
        // A client that closes before sending a ClientHello is refused at
        // once (no waiting, no spinning on end-of-file).
        let mut s = connect_through(&proxy, "localhost").await;
        read_some(&mut s).await;
        let before = proxy.decisions().len();
        s.shutdown().await.unwrap();
        let t0 = std::time::Instant::now();
        while proxy.decisions().len() == before && t0.elapsed() < Duration::from_secs(5) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(t0.elapsed() < Duration::from_secs(2), "refused promptly after the client closed");
        assert!(proxy.decisions().last().unwrap().reason.contains("no TLS ClientHello"));
        let mut s = connect_through(&proxy, "localhost").await;
        read_some(&mut s).await;
        s.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n").await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(got.lock().unwrap().len(), 1, "plain HTTP in the tunnel never reaches upstream");
        let reasons: Vec<String> = proxy.decisions().into_iter().map(|d| d.reason).collect();
        assert!(reasons.iter().any(|r| r.contains("does not match")), "{reasons:?}");
        assert!(reasons.iter().any(|r| r.contains("no server name")), "{reasons:?}");
        assert!(reasons.iter().any(|r| r.contains("not a TLS handshake")), "{reasons:?}");
    }
}

#[cfg(test)]
mod unix_tests {
    use super::*;

    #[tokio::test]
    async fn the_unix_listener_applies_the_same_rules() {
        let opts = Options { unix: true, ..Options::default() };
        let proxy = start_with(vec!["api.anthropic.com".into()], vec![], opts).await.unwrap();
        let path = proxy.unix_socket.clone().unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path.parent().unwrap()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        let mut s = tokio::net::UnixStream::connect(&path).await.unwrap();
        s.write_all(b"CONNECT evil.com:443 HTTP/1.1\r\nHost: evil.com\r\n\r\n").await.unwrap();
        let mut buf = vec![0u8; 64];
        let n = s.read(&mut buf).await.unwrap();
        assert!(String::from_utf8_lossy(&buf[..n]).starts_with("HTTP/1.1 403"));
        assert_eq!(proxy.decisions()[0].reason, "host is not on the allowlist");
        let dir = path.parent().unwrap().to_path_buf();
        drop(proxy);
        assert!(!dir.exists(), "socket directory removed with the proxy");
    }

    #[tokio::test]
    async fn the_bridge_forwards_a_loopback_port_and_returns_the_exit_code() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("u.sock");
        let ul = tokio::net::UnixListener::bind(&sock).unwrap();
        tokio::spawn(async move {
            if let Ok((mut c, _)) = ul.accept().await {
                let mut buf = [0u8; 5];
                let _ = c.read_exact(&mut buf).await;
                let _ = c.write_all(b"pong:").await;
                let _ = c.write_all(&buf).await;
            }
        });
        let port = TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port();
        let script = format!(
            "import socket,time\nfor _ in range(50):\n  try:\n    s=socket.create_connection(('127.0.0.1',{port}),1); break\n  except OSError: time.sleep(0.05)\ns.sendall(b'hello'); d=b''\nwhile len(d)<10: d+=s.recv(10)\nprint(d.decode()); raise SystemExit(7)\n"
        );
        let code = net_bridge(port, sock, vec!["/usr/bin/python3".into(), "-c".into(), script]).await;
        assert_eq!(code, 7, "the bridged command's exit code");
    }
}
