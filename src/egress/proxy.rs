//! The egress proxy: the only network path out of a sandboxed shell command.
//!
//! The sandbox allows outbound connections to `localhost` only. This proxy listens there and
//! accepts `CONNECT host:443`, nothing else. Plain-HTTP requests are refused. Two checks close the
//! gaps a host-name allowlist leaves open:
//!
//! - the host must resolve to a public address, so an allowed name that points at loopback, the
//!   LAN or a cloud metadata endpoint is refused;
//! - the tunnel's TLS ClientHello must name the same host (SNI), so a client cannot CONNECT to an
//!   allowed name and then talk TLS to another.
//!
//! It never decrypts TLS. Every decision is recorded.

use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const FORBIDDEN: &[u8] = b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    NotListed,
    NotPublic,
    NameMismatch(Option<String>),
    NotHttps,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub target: String,
    pub refusal: Option<Refusal>,
}

impl Decision {
    pub fn allowed(&self) -> bool {
        self.refusal.is_none()
    }

    fn refused(target: impl Into<String>, why: Refusal) -> Self {
        Decision { target: target.into(), refusal: Some(why) }
    }
}

pub struct EgressProxy {
    pub port: u16,
    decisions: Arc<Mutex<Vec<Decision>>>,
    task: tokio::task::JoinHandle<()>,
}

impl EgressProxy {
    pub fn decisions(&self) -> Vec<Decision> {
        self.decisions.lock().unwrap().clone()
    }
}

impl Drop for EgressProxy {
    fn drop(&mut self) {
        self.task.abort();
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

/// The method and target of a proxy request's first line.
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

/// Only `CONNECT <listed host>:443` passes. IP literals are refused even when listed.
pub fn decide(allow: &[String], method: &str, target: &str) -> Decision {
    let not_https = || Decision::refused(format!("{method} {target}"), Refusal::NotHttps);
    if method != "CONNECT" {
        return not_https();
    }
    let Some((host, port)) = target.rsplit_once(':') else {
        return not_https();
    };
    if port != "443" {
        return not_https();
    }
    if host.starts_with('[') || host.parse::<IpAddr>().is_ok() || !host_allowed(allow, host) {
        return Decision::refused(target, Refusal::NotListed);
    }
    Decision { target: target.into(), refusal: None }
}

/// Whether an address is on the public internet. Loopback, private, link-local (cloud metadata
/// included), carrier-grade NAT, unique-local, multicast, unspecified and documentation ranges are
/// not, nor are their IPv4-mapped IPv6 forms.
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
                || (o[0] == 100 && (o[1] & 0xc0) == 64)
                || (o[0] == 192 && o[1] == 0 && o[2] == 0)
                || (o[0] == 198 && (o[1] & 0xfe) == 18)
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
                || (s[0] & 0xfe00) == 0xfc00
                || (s[0] & 0xffc0) == 0xfe80
                || (s[0] == 0x2001 && s[1] == 0x0db8))
        }
    }
}

/// The server name (SNI) in a TLS ClientHello record, if `buf` holds a complete one. `Err` means
/// the bytes are not a ClientHello. `Ok(None)` means more bytes are needed.
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
    p.take(2 + 32)?;
    let n = p.u8()? as usize;
    p.take(n)?;
    let n = p.u16()? as usize;
    p.take(n)?;
    let n = p.u8()? as usize;
    p.take(n)?;
    if p.i == p.b.len() {
        return Ok(Some(None));
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

/// Test hooks: reach a local upstream on another port.
#[derive(Clone, Copy, Default)]
struct Options {
    allow_internal: bool,
    upstream_port: Option<u16>,
}

async fn handle(mut client: TcpStream, allow: Arc<Vec<String>>, log: Arc<Mutex<Vec<Decision>>>, opts: Options) {
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
        Some((method, target)) => decide(&allow, &method, &target),
        None => {
            let line: String = text.lines().next().unwrap_or_default().chars().take(100).collect();
            Decision::refused(line, Refusal::NotHttps)
        }
    };
    let refuse = |d: Decision| log.lock().unwrap().push(d);
    if !decision.allowed() {
        refuse(decision);
        let _ = client.write_all(FORBIDDEN).await;
        return;
    }
    let target = decision.target.clone();
    let host = target.rsplit_once(':').map(|(h, _)| h.to_ascii_lowercase()).unwrap_or_default();
    let addrs: Vec<SocketAddr> = match tokio::net::lookup_host(&target).await {
        Ok(a) => a.filter(|a| opts.allow_internal || is_public(a.ip())).collect(),
        Err(_) => vec![],
    };
    if addrs.is_empty() {
        refuse(Decision::refused(&target, Refusal::NotPublic));
        let _ = client.write_all(FORBIDDEN).await;
        return;
    }
    if client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").await.is_err() {
        return;
    }
    let mut hello = Vec::new();
    let mut chunk = [0u8; 4096];
    let sni = loop {
        match client_hello_sni(&hello) {
            Ok(Some(sni)) => break Some(sni),
            Err(_) => break None,
            Ok(None) => {}
        }
        match tokio::time::timeout(Duration::from_secs(10), client.read(&mut chunk)).await {
            Ok(Ok(n)) if n > 0 => hello.extend_from_slice(&chunk[..n]),
            _ => break None,
        }
    };
    match sni {
        Some(Some(name)) if name == host => {}
        Some(Some(name)) => return refuse(Decision::refused(&target, Refusal::NameMismatch(Some(name)))),
        _ => return refuse(Decision::refused(&target, Refusal::NameMismatch(None))),
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

/// Start a proxy on an ephemeral loopback port. It stops when the returned value is dropped.
pub async fn start(allow: Vec<String>) -> std::io::Result<EgressProxy> {
    start_with(allow, Options::default()).await
}

async fn start_with(allow: Vec<String>, opts: Options) -> std::io::Result<EgressProxy> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let decisions = Arc::new(Mutex::new(Vec::new()));
    let log = decisions.clone();
    let allow = Arc::new(allow);
    let task = tokio::spawn(async move {
        while let Ok((sock, _)) = listener.accept().await {
            tokio::spawn(handle(sock, allow.clone(), log.clone(), opts));
        }
    });
    Ok(EgressProxy { port, decisions, task })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allow() -> Vec<String> {
        vec!["api.anthropic.com".into(), "*.example.org".into()]
    }

    #[test]
    fn decisions() {
        assert!(decide(&allow(), "CONNECT", "api.anthropic.com:443").allowed());
        assert!(decide(&allow(), "CONNECT", "a.example.org:443").allowed());
        for (method, target, why) in [
            ("GET", "http://api.anthropic.com/", Refusal::NotHttps),
            ("CONNECT", "api.anthropic.com:80", Refusal::NotHttps),
            ("CONNECT", "noport", Refusal::NotHttps),
            ("CONNECT", "evil.com:443", Refusal::NotListed),
            ("CONNECT", "api.anthropic.com.evil.com:443", Refusal::NotListed),
            ("CONNECT", "example.org:443", Refusal::NotListed),
            ("CONNECT", "1.2.3.4:443", Refusal::NotListed),
            ("CONNECT", "[::1]:443", Refusal::NotListed),
        ] {
            let d = decide(&allow(), method, target);
            assert_eq!(d.refusal, Some(why), "{method} {target}");
        }
        let d = decide(&allow(), "GET", "http://api.anthropic.com/");
        assert_eq!(d.target, "GET http://api.anthropic.com/");
        assert_eq!(decide(&allow(), "CONNECT", "evil.com:443").target, "evil.com:443");
    }

    #[test]
    fn head_parsing() {
        assert_eq!(parse_head("CONNECT a.com:443 HTTP/1.1\r\nHost: a.com\r\n\r\n"), Some(("CONNECT".into(), "a.com:443".into())));
        assert_eq!(parse_head("garbage"), None);
        assert_eq!(parse_head("CONNECT a.com:443 SPDY/3\r\n\r\n"), None);
        assert_eq!(parse_head(""), None);
    }

    #[test]
    fn wildcard_needs_a_real_label_and_ips_are_refused_even_if_listed() {
        let allow = vec!["*.example.org".to_string(), "1.2.3.4".to_string()];
        assert!(!host_allowed(&allow, ".example.org"), "empty label");
        assert!(!host_allowed(&allow, "example.org"));
        assert!(host_allowed(&allow, "a.example.org"));
        assert!(host_allowed(&allow, "A.Example.ORG"));
        assert!(!decide(&allow, "CONNECT", "1.2.3.4:443").allowed(), "IP literals are refused even when allowlisted");
        assert!(!decide(&allow, "CONNECT", ".example.org:443").allowed());
    }

    pub(super) fn client_hello(sni: Option<&str>) -> Vec<u8> {
        let mut ext = Vec::new();
        ext.extend_from_slice(&[0x00, 0x0b, 0x00, 0x02, 0x01, 0x00]);
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
        bad[5] = 0x02;
        assert!(client_hello_sni(&bad).is_err());
        let mut huge = h;
        huge[3] = 0xff;
        assert!(client_hello_sni(&huge).is_err(), "oversized record");
    }

    fn padded_hello(sni: &str, size: usize) -> Vec<u8> {
        let h = client_hello(Some(sni));
        let pad = size.saturating_sub(h.len());
        let mut ext = vec![0x00, 0x15];
        ext.extend_from_slice(&(pad as u16).to_be_bytes());
        ext.extend(std::iter::repeat_n(0u8, pad));
        let mut body = h[9..].to_vec();
        let at = 2 + 32 + 1 + 4 + 2;
        let old = u16::from_be_bytes([body[at], body[at + 1]]) as usize;
        body[at..at + 2].copy_from_slice(&((old + ext.len()) as u16).to_be_bytes());
        body.extend_from_slice(&ext);
        let mut msg = vec![0x01];
        msg.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
        msg.extend_from_slice(&body);
        let mut rec = vec![0x16, 0x03, 0x01];
        rec.extend_from_slice(&(msg.len() as u16).to_be_bytes());
        rec.extend_from_slice(&msg);
        rec
    }

    #[test]
    fn client_hello_parser_edges() {
        for size in [1500, 4000, 16000] {
            let h = padded_hello("api.anthropic.com", size);
            assert_eq!(client_hello_sni(&h), Ok(Some(Some("api.anthropic.com".into()))), "{size} bytes");
        }
        let mut at_limit = vec![0x16, 0x03, 0x01];
        at_limit.extend_from_slice(&((18 * 1024) as u16).to_be_bytes());
        assert_eq!(client_hello_sni(&at_limit), Ok(None), "18 KiB is allowed; wait for the rest");
        let mut over = vec![0x16, 0x03, 0x01];
        over.extend_from_slice(&((18 * 1024 + 1) as u16).to_be_bytes());
        assert!(client_hello_sni(&over).is_err());
        assert!(client_hello_sni(&[0x16, 0x03, 0x01, 0x00, 0x00]).is_err());
        let mut v = client_hello(Some("a.com"));
        v[1] = 0x05;
        assert!(client_hello_sni(&v).is_err());
        let mut odd = client_hello(Some("a.com"));
        let pos = odd.windows(5).position(|w| w == b"a.com").unwrap();
        odd[pos - 3] = 1;
        assert_eq!(client_hello_sni(&odd), Ok(Some(None)));
    }

    #[test]
    fn only_public_addresses_are_reachable() {
        for ip in [
            "93.184.216.34",
            "8.8.8.8",
            "100.0.0.1",
            "100.128.0.1",
            "8.64.1.1",
            "192.0.1.1",
            "198.20.0.1",
            "11.0.0.1",
            "2606:4700::1111",
            "2001:4860:4860::8888",
            "2a00:1450::1",
        ] {
            assert!(is_public(ip.parse().unwrap()), "{ip}");
        }
        for ip in [
            "127.0.0.1",
            "127.255.255.254",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "100.127.255.255",
            "0.0.0.0",
            "255.255.255.255",
            "224.0.0.1",
            "198.18.0.1",
            "198.19.255.255",
            "192.0.0.8",
            "192.0.0.255",
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

}

#[cfg(test)]
mod net;
