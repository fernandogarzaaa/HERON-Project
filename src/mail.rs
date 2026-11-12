//! Read-only email access over IMAP (TLS). Heron never sends, deletes or marks anything:
//! the mailbox is opened with EXAMINE and messages are fetched with BODY.PEEK.

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

#[derive(Debug, Clone, PartialEq)]
pub struct Email {
    pub id: String,
    pub from: String,
    pub to: String,
    pub subject: String,
    pub date: String,
    pub ts: i64,
    /// Plain-text excerpt of the body.
    pub text: String,
}

const HEADER_FIELDS: &str = "FROM TO SUBJECT DATE MESSAGE-ID CONTENT-TYPE CONTENT-TRANSFER-ENCODING";
const BODY_BYTES: usize = 6000;
const EXCERPT_CHARS: usize = 1500;

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

struct Conn<S> {
    stream: BufReader<S>,
    tag: u32,
}

/// One untagged response line, with the literals that were attached to it.
struct Untagged {
    text: String,
    literals: Vec<(String, Vec<u8>)>,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Conn<S> {
    async fn line(&mut self) -> Result<String> {
        let mut buf = Vec::new();
        let n = self.stream.read_until(b'\n', &mut buf).await?;
        if n == 0 {
            bail!("the mail server closed the connection");
        }
        Ok(String::from_utf8_lossy(&buf).trim_end_matches(['\r', '\n']).to_string())
    }

    /// Send a command and collect untagged responses until the tagged completion.
    async fn command(&mut self, cmd: &str) -> Result<Vec<Untagged>> {
        self.tag += 1;
        let tag = format!("k{}", self.tag);
        self.stream.get_mut().write_all(format!("{tag} {cmd}\r\n").as_bytes()).await?;
        self.stream.get_mut().flush().await?;
        let mut out: Vec<Untagged> = vec![];
        loop {
            let mut line = self.line().await?;
            if let Some(rest) = line.strip_prefix(&format!("{tag} ")) {
                if rest.to_uppercase().starts_with("OK") {
                    return Ok(out);
                }
                bail!("{}", rest.splitn(2, ' ').nth(1).unwrap_or(rest));
            }
            let mut item = Untagged { text: String::new(), literals: vec![] };
            // A line ending in {n} is followed by n raw bytes, then the response continues.
            loop {
                let literal_len = line.strip_suffix('}').and_then(|l| l.rfind('{').map(|i| (i, l[i + 1..].parse::<usize>())));
                match literal_len {
                    Some((i, Ok(n))) if n <= 5_000_000 => {
                        let label = line[..i].to_string();
                        let mut data = vec![0u8; n];
                        self.stream.read_exact(&mut data).await?;
                        item.text.push_str(&label);
                        item.literals.push((label, data));
                        line = self.line().await?;
                    }
                    _ => {
                        item.text.push_str(&line);
                        break;
                    }
                }
            }
            out.push(item);
        }
    }
}

async fn session<S: AsyncRead + AsyncWrite + Unpin>(stream: S, username: &str, password: &str, limit: usize) -> Result<Vec<Email>> {
    let mut c = Conn { stream: BufReader::new(stream), tag: 0 };
    let greeting = c.line().await?;
    if !greeting.starts_with("* OK") && !greeting.starts_with("* PREAUTH") {
        bail!("unexpected greeting from the mail server");
    }
    c.command(&format!("LOGIN {} {}", quote(username), quote(password)))
        .await
        .map_err(|e| anyhow!("The mail server refused the sign-in ({e}). Check the address, and use an app password rather than your normal password."))?;
    let examined = c.command("EXAMINE INBOX").await.context("could not open the inbox")?;
    let exists: usize = examined.iter().find_map(|u| u.text.strip_suffix(" EXISTS").and_then(|t| t.trim_start_matches("* ").parse().ok())).unwrap_or(0);
    let mut emails = vec![];
    if exists > 0 {
        let first = exists.saturating_sub(limit.max(1) - 1).max(1);
        let fetched = c.command(&format!("FETCH {first}:{exists} (BODY.PEEK[HEADER.FIELDS ({HEADER_FIELDS})] BODY.PEEK[TEXT]<0.{BODY_BYTES}>)")).await.context("could not read messages")?;
        for item in fetched {
            if !item.text.to_uppercase().contains(" FETCH ") {
                continue;
            }
            let header = item.literals.iter().find(|(l, _)| l.to_uppercase().contains("HEADER")).map(|(_, d)| String::from_utf8_lossy(d).to_string()).unwrap_or_default();
            let body = item.literals.iter().find(|(l, _)| l.to_uppercase().contains("BODY[TEXT]")).map(|(_, d)| d.clone()).unwrap_or_default();
            if header.is_empty() {
                continue;
            }
            emails.push(parse_email(&header, &body));
        }
    }
    let _ = c.command("LOGOUT").await;
    emails.sort_by_key(|e| -e.ts);
    Ok(emails)
}

/// Fetch the most recent `limit` messages from the inbox, newest first. Read-only.
pub async fn fetch_recent(host: &str, port: u16, tls: bool, username: &str, password: &str, limit: usize) -> Result<Vec<Email>> {
    let work = async {
        let tcp = tokio::net::TcpStream::connect((host, port)).await.with_context(|| format!("could not reach {host}:{port}"))?;
        if !tls {
            // Plain connections would expose the password; allowed only to this computer (used by tests).
            if !tcp.peer_addr()?.ip().is_loopback() {
                bail!("refusing to sign in to a remote mail server without encryption");
            }
            return session(tcp, username, password, limit).await;
        }
        use tokio_rustls::rustls;
        let mut roots = rustls::RootCertStore::empty();
        for cert in rustls_native_certs::load_native_certs().certs {
            let _ = roots.add(cert);
        }
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()?
            .with_root_certificates(roots)
            .with_no_client_auth();
        let name = rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|_| anyhow!("'{host}' is not a valid server name"))?;
        let stream = tokio_rustls::TlsConnector::from(Arc::new(config)).connect(name, tcp).await.with_context(|| format!("could not establish a secure connection to {host}"))?;
        session(stream, username, password, limit).await
    };
    tokio::time::timeout(Duration::from_secs(60), work).await.map_err(|_| anyhow!("the mail server took too long to answer"))?
}

// ---------------- message parsing ----------------

fn header_value(headers: &str, name: &str) -> String {
    let mut value: Option<String> = None;
    for line in headers.lines() {
        if let Some(v) = &mut value {
            if line.starts_with([' ', '\t']) {
                v.push(' ');
                v.push_str(line.trim());
                continue;
            }
            break;
        }
        if line.len() > name.len() && line[..name.len()].eq_ignore_ascii_case(name) && line.as_bytes()[name.len()] == b':' {
            value = Some(line[name.len() + 1..].trim().to_string());
        }
    }
    value.unwrap_or_default()
}

fn decode_qp(input: &[u8], header_mode: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        match input[i] {
            b'=' => {
                let hex = input.get(i + 1..i + 3);
                if let Some(v) = hex.and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    out.push(v);
                    i += 3;
                } else if input.get(i + 1) == Some(&b'\r') || input.get(i + 1) == Some(&b'\n') {
                    i += 1; // soft line break
                    while i < input.len() && (input[i] == b'\r' || input[i] == b'\n') {
                        i += 1;
                    }
                } else {
                    out.push(b'=');
                    i += 1;
                }
            }
            b'_' if header_mode => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

fn decode_b64(input: &[u8]) -> Vec<u8> {
    let clean: Vec<u8> = input.iter().copied().filter(|b| b.is_ascii_alphanumeric() || *b == b'+' || *b == b'/').collect();
    // A truncated excerpt may end mid-group; a single leftover character cannot be decoded.
    let usable = if clean.len() % 4 == 1 { clean.len() - 1 } else { clean.len() };
    base64::engine::general_purpose::STANDARD_NO_PAD.decode(&clean[..usable]).unwrap_or_default()
}

fn to_text(bytes: &[u8], charset: &str) -> String {
    match String::from_utf8(bytes.to_vec()) {
        Ok(s) => s,
        Err(_) if charset.to_lowercase().contains("utf-8") => String::from_utf8_lossy(bytes).to_string(),
        Err(_) => bytes.iter().map(|b| *b as char).collect(), // Latin-1 and close relatives
    }
}

/// Decode RFC 2047 encoded words: =?charset?B|Q?data?=
pub fn decode_header(raw: &str) -> String {
    let mut out = String::new();
    let mut rest = raw;
    let mut last_was_encoded = false;
    while let Some(start) = rest.find("=?") {
        let before = &rest[..start];
        let after = &rest[start + 2..];
        let parts: Vec<&str> = after.splitn(3, '?').collect();
        let end = parts.get(2).and_then(|p| p.find("?="));
        match (parts.len() == 3, end) {
            (true, Some(end)) => {
                if !(last_was_encoded && before.trim().is_empty()) {
                    out.push_str(before);
                }
                let data = &parts[2][..end];
                let bytes = if parts[1].eq_ignore_ascii_case("b") { decode_b64(data.as_bytes()) } else { decode_qp(data.as_bytes(), true) };
                out.push_str(&to_text(&bytes, parts[0]));
                rest = &parts[2][end + 2..];
                last_was_encoded = true;
            }
            _ => {
                out.push_str(&rest[..start + 2]);
                rest = after;
                last_was_encoded = false;
            }
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn param(content_type: &str, name: &str) -> String {
    content_type
        .split(';')
        .filter_map(|p| p.trim().split_once('='))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case(name))
        .map(|(_, v)| v.trim().trim_matches('"').to_string())
        .unwrap_or_default()
}

fn decode_part(content_type: &str, encoding: &str, body: &[u8]) -> String {
    let bytes = match encoding.trim().to_lowercase().as_str() {
        "base64" => decode_b64(body),
        "quoted-printable" => decode_qp(body, false),
        _ => body.to_vec(),
    };
    let text = to_text(&bytes, &param(content_type, "charset"));
    if content_type.to_lowercase().contains("html") { crate::tools::web::html_to_text(text.as_bytes()) } else { text }
}

/// Best-effort readable text from a (possibly truncated) MIME body.
pub fn body_text(content_type: &str, encoding: &str, body: &[u8]) -> String {
    let boundary = param(content_type, "boundary");
    let text = if content_type.to_lowercase().starts_with("multipart/") && !boundary.is_empty() {
        let raw = String::from_utf8_lossy(body);
        let mut best: Option<(bool, String)> = None;
        for part in raw.split(&format!("--{boundary}")).skip(1) {
            let part = part.trim_start_matches(['\r', '\n']);
            let Some((head, content)) = part.split_once("\r\n\r\n").or_else(|| part.split_once("\n\n")) else { continue };
            let ct = header_value(head, "Content-Type");
            let lower = ct.to_lowercase();
            if lower.starts_with("multipart/") {
                let inner = body_text(&ct, "", content.as_bytes());
                if !inner.trim().is_empty() && best.is_none() {
                    best = Some((false, inner));
                }
                continue;
            }
            if !(lower.starts_with("text/") || lower.is_empty()) {
                continue;
            }
            let decoded = decode_part(&ct, &header_value(head, "Content-Transfer-Encoding"), content.as_bytes());
            let plain = !lower.contains("html");
            // Prefer the plain-text alternative over HTML.
            if best.as_ref().map(|(was_plain, _)| plain && !was_plain).unwrap_or(true) {
                best = Some((plain, decoded));
            }
        }
        best.map(|(_, t)| t).unwrap_or_default()
    } else if content_type.is_empty() || content_type.to_lowercase().starts_with("text/") {
        decode_part(content_type, encoding, body)
    } else {
        String::new()
    };
    let cleaned = text.lines().map(str::trim_end).filter(|l| !l.starts_with('>')).collect::<Vec<_>>().join("\n");
    let collapsed = cleaned.split("\n\n").map(str::trim).filter(|p| !p.is_empty()).collect::<Vec<_>>().join("\n\n");
    collapsed.chars().take(EXCERPT_CHARS).collect()
}

pub fn parse_email(headers: &str, body: &[u8]) -> Email {
    let date = header_value(headers, "Date");
    let ts = chrono::DateTime::parse_from_rfc2822(date.split(" (").next().unwrap_or(&date).trim()).map(|d| d.timestamp()).unwrap_or(0);
    let subject = decode_header(&header_value(headers, "Subject"));
    let from = decode_header(&header_value(headers, "From"));
    let mut id = header_value(headers, "Message-ID");
    if id.is_empty() {
        id = format!("{date}|{from}|{subject}");
    }
    Email {
        id,
        to: decode_header(&header_value(headers, "To")),
        text: body_text(&header_value(headers, "Content-Type"), &header_value(headers, "Content-Transfer-Encoding"), body),
        from,
        subject,
        date,
        ts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_headers() {
        assert_eq!(decode_header("=?UTF-8?B?SGVsbG8gV8O2cmxk?="), "Hello Wörld");
        assert_eq!(decode_header("=?utf-8?Q?Caf=C3=A9_menu?= for Friday"), "Café menu for Friday");
        assert_eq!(decode_header("=?UTF-8?B?SGVsbG8=?= =?UTF-8?B?IHRoZXJl?="), "Hello there");
        assert_eq!(decode_header("=?ISO-8859-1?Q?Jos=E9?= <jose@example.com>"), "José <jose@example.com>");
        assert_eq!(decode_header("Plain subject =? not encoded"), "Plain subject =? not encoded");
    }

    #[test]
    fn extracts_text_from_mime_bodies() {
        assert_eq!(body_text("text/plain; charset=utf-8", "quoted-printable", b"Caf=C3=A9 at 5?=\r\n Yes.\r\n> quoted reply"), "Café at 5? Yes.");
        assert_eq!(body_text("text/plain", "base64", b"SGVsbG8gdGhlcmUsIHRoaXMgaXMgYmFzZTY0Lg==\r\n"), "Hello there, this is base64.");
        let multi = b"--b1\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<p>HTML <b>version</b></p>\r\n--b1\r\nContent-Type: text/plain\r\n\r\nPlain version\r\n--b1\r\nContent-Type: image/png\r\nContent-Transfer-Encoding: base64\r\n\r\niVBORw0KG\r\n--b1--";
        assert_eq!(body_text("multipart/alternative; boundary=\"b1\"", "", multi), "Plain version");
        let html_only = b"--b2\r\nContent-Type: text/html\r\n\r\n<p>Only <b>HTML</b> here</p><script>x()</script>\r\n--b2--";
        let t = body_text("multipart/mixed; boundary=b2", "", html_only);
        assert!(t.contains("Only") && t.contains("HTML") && !t.contains("x()"));
        assert_eq!(body_text("application/pdf", "base64", b"JVBERi0="), "");
    }

    /// A scripted IMAP server on a local socket: checks the commands Heron sends and replies like a real one.
    async fn mock_server(accept_login: bool) -> (u16, tokio::task::JoinHandle<Vec<String>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            let mut s = BufReader::new(sock);
            let mut seen = vec![];
            s.get_mut().write_all(b"* OK IMAP4rev1 ready\r\n").await.unwrap();
            loop {
                let mut line = String::new();
                if s.read_line(&mut line).await.unwrap_or(0) == 0 {
                    break;
                }
                let line = line.trim_end().to_string();
                let (tag, cmd) = line.split_once(' ').unwrap();
                seen.push(cmd.to_string());
                let reply = if cmd.starts_with("LOGIN") {
                    if accept_login { format!("{tag} OK logged in\r\n") } else { format!("{tag} NO [AUTHENTICATIONFAILED] Invalid credentials\r\n") }
                } else if cmd.starts_with("EXAMINE") {
                    format!("* FLAGS (\\Seen)\r\n* 2 EXISTS\r\n* 0 RECENT\r\n{tag} OK [READ-ONLY] done\r\n")
                } else if cmd.starts_with("FETCH") {
                    let h1 = "From: =?UTF-8?B?TWFyaWEgTMOzcGV6?= <maria@example.com>\r\nTo: me@example.com\r\nSubject: Dinner on Friday?\r\nDate: Tue, 10 Nov 2026 18:30:00 +0000\r\nMessage-ID: <a1@example.com>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n";
                    let b1 = "Hi! Are you free for dinner on Friday at 7?\r\nMaria\r\n";
                    let h2 = "From: Billing <billing@example.org>\r\nSubject: Your invoice {draft}\r\nDate: Wed, 11 Nov 2026 09:00:00 +0000\r\nMessage-ID: <a2@example.org>\r\nContent-Type: text/html\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\n";
                    let b2 = "<p>Invoice #42 is due on <b>Nov 30</b>=2E</p>";
                    format!(
                        "* 1 FETCH (BODY[HEADER.FIELDS (FROM TO SUBJECT)] {{{}}}\r\n{h1} BODY[TEXT]<0> {{{}}}\r\n{b1})\r\n* 2 FETCH (BODY[TEXT]<0> {{{}}}\r\n{b2} BODY[HEADER.FIELDS (FROM TO SUBJECT)] {{{}}}\r\n{h2})\r\n{tag} OK fetch done\r\n",
                        h1.len(), b1.len(), b2.len(), h2.len()
                    )
                } else {
                    format!("* BYE\r\n{tag} OK bye\r\n")
                };
                s.get_mut().write_all(reply.as_bytes()).await.unwrap();
                if cmd.starts_with("LOGOUT") {
                    break;
                }
            }
            seen
        });
        (port, handle)
    }

    #[tokio::test]
    async fn fetches_recent_mail_read_only() {
        let (port, server) = mock_server(true).await;
        let mail = fetch_recent("127.0.0.1", port, false, "me@example.com", "app \"pass\"", 50).await.unwrap();
        assert_eq!(mail.len(), 2);
        // newest first; literals may arrive in either order; braces in a subject are not mistaken for a literal
        assert_eq!(mail[0].subject, "Your invoice {draft}");
        assert!(mail[0].text.contains("Invoice #42 is due on Nov 30."));
        assert_eq!(mail[1].from, "Maria López <maria@example.com>");
        assert!(mail[1].text.starts_with("Hi! Are you free for dinner"));
        assert_eq!(mail[1].ts, 1794335400);
        let seen = server.await.unwrap();
        assert_eq!(seen[0], r#"LOGIN "me@example.com" "app \"pass\"""#);
        assert_eq!(seen[1], "EXAMINE INBOX", "the inbox is opened read-only");
        assert!(seen[2].starts_with("FETCH 1:2 (BODY.PEEK["), "messages are peeked, never marked as read");
        assert!(!seen.iter().any(|c| c.starts_with("STORE") || c.starts_with("SELECT") || c.starts_with("APPEND") || c.starts_with("EXPUNGE")));
    }

    #[tokio::test]
    async fn bad_password_gives_a_useful_error_and_remote_plaintext_is_refused() {
        let (port, _server) = mock_server(false).await;
        let err = fetch_recent("127.0.0.1", port, false, "me@example.com", "wrong", 10).await.unwrap_err().to_string();
        assert!(err.contains("refused the sign-in") && err.contains("app password"), "{err}");
    }
}
