//! Bounded, non-replaying MCP HTTP binding. No system proxies, redirects, or
//! environment-controlled local-network overrides apply to connector traffic.
use super::{fail, Error, RemoteTool, Result};
use base64::Engine;
use futures_util::StreamExt;
use reqwest::{
    header::{HeaderMap, HeaderValue},
    Client, StatusCode,
};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use url::Url;

pub const MODERN: &str = "2026-07-28";
const LEGACY: &str = "2025-11-25";
const LEGACY_VERSIONS: &[&str] = &[LEGACY, "2025-06-18", "2025-03-26"];
const MAX_BODY: usize = 1024 * 1024;
const MAX_EVENT: usize = 128 * 1024;

// Connector credentials never travel to special-purpose/documentation/tunnel
// networks, even when those addresses are not RFC1918 private addresses.
pub(super) fn public_addresses(
    addresses: Vec<std::net::SocketAddr>,
) -> Result<Vec<std::net::SocketAddr>> {
    let public = |ip: std::net::IpAddr| {
        if crate::permissions::is_private_ip(&ip) {
            return false;
        }
        match ip {
            std::net::IpAddr::V4(ip) => {
                let o = ip.octets();
                !matches!(
                    [o[0], o[1], o[2]],
                    [192, 0, 2] | [198, 51, 100] | [203, 0, 113] | [192, 88, 99]
                )
            }
            std::net::IpAddr::V6(ip) => {
                let s = ip.segments();
                (s[0] & 0xe000) == 0x2000
                    && !(s[0] == 0x2001 && (s[1] < 0x200 || s[1] == 0xdb8))
                    && s[0] != 0x2002
                    && !(s[0] == 0x3fff && s[1] < 0x1000)
            }
        }
    };
    if addresses.is_empty() || addresses.iter().any(|a| !public(a.ip())) {
        return Err(fail(
            "address",
            "Connector DNS returned a non-public address.",
        ));
    }
    Ok(addresses)
}
struct PublicConnectorResolver;
impl reqwest::dns::Resolve for PublicConnectorResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(async move {
            let addresses = tokio::net::lookup_host((name.as_str(), 0)).await?.collect();
            let addresses = public_addresses(addresses)?;
            Ok(Box::new(addresses.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

pub struct Peer {
    client: Client,
    url: Url,
    pub protocol: String,
    session: Option<HeaderValue>,
    next: AtomicU64,
}

pub fn client() -> Result<Client> {
    Client::builder()
        .no_proxy()
        .dns_resolver(Arc::new(PublicConnectorResolver))
        .http1_only()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(25))
        .pool_max_idle_per_host(0) // validate DNS again on each new connection
        .build()
        .map_err(|_| fail("transport", "Cannot create connector transport."))
}

// Header values cannot select arbitrary headers: schema annotations are always
// prefixed with Mcp-Param-. Encode whitespace, controls, Unicode and sentinels.
fn encoded(value: &str) -> Result<HeaderValue> {
    let safe = value.trim() == value
        && value.bytes().all(|b| (32..=126).contains(&b))
        && !value.starts_with("=?base64?");
    let value = if safe {
        value.to_owned()
    } else {
        format!(
            "=?base64?{}?=",
            base64::engine::general_purpose::STANDARD.encode(value)
        )
    };
    HeaderValue::from_str(&value).map_err(|_| fail("header", "Invalid MCP metadata header."))
}

impl Peer {
    pub async fn connect(url: Url, credential: Option<&str>, client: Client) -> Result<Self> {
        let mut peer = Self {
            client,
            url,
            protocol: MODERN.into(),
            session: None,
            next: AtomicU64::new(1),
        };
        let (status, reply, _) = peer
            .exchange("server/discover", json!({}), credential, None, false)
            .await?;
        let mut legacy_version = LEGACY;
        if reply.get("error").is_some() && reply["error"]["code"] == -32022 {
            let supported = reply["error"]["data"]["supported"]
                .as_array()
                .ok_or_else(|| fail("protocol", "Invalid protocol negotiation response."))?;
            if supported.iter().any(|v| v == MODERN) {
                return Err(fail("protocol", "Server rejected a version it advertises."));
            }
            legacy_version = LEGACY_VERSIONS
                .iter()
                .copied()
                .find(|v| supported.iter().any(|s| s == *v))
                .ok_or_else(|| fail("protocol", "No supported MCP protocol version."))?;
        } else if status.is_success() && reply.get("result").is_some() {
            let versions = reply["result"]["supportedVersions"]
                .as_array()
                .ok_or_else(|| fail("protocol", "Server discovery omitted supported versions."))?;
            if versions.iter().any(|v| v == MODERN) {
                if reply["result"]["resultType"] != "complete" {
                    return Err(fail("protocol", "Invalid modern discovery result."));
                }
                return Ok(peer);
            }
            legacy_version = LEGACY_VERSIONS
                .iter()
                .copied()
                .find(|v| versions.iter().any(|s| s == *v))
                .ok_or_else(|| fail("protocol", "No supported MCP protocol version."))?;
        } else {
            // Never reinterpret authentication, throttling, or a recognized
            // modern validation error as permission to try a different protocol.
            let code = reply["error"]["code"].as_i64();
            if code.is_some_and(|c| c != -32601)
                || !(matches!(status.as_u16(), 400 | 404 | 405)
                    || (status.is_success() && code == Some(-32601)))
            {
                return Err(fail(
                    "protocol",
                    "MCP discovery failed; no safe legacy fallback.",
                ));
            }
        }
        peer.protocol = legacy_version.into();
        let (status, reply, session) = peer
            .exchange(
                "initialize",
                json!({
                    "protocolVersion": legacy_version, "capabilities": {},
                    "clientInfo": {"name":"heron", "version":env!("CARGO_PKG_VERSION")}
                }),
                credential,
                None,
                false,
            )
            .await?;
        checked(status, &reply)?;
        let version = reply["result"]["protocolVersion"].as_str().unwrap_or("");
        if ![LEGACY, "2025-06-18", "2025-03-26"].contains(&version) {
            return Err(fail(
                "protocol",
                "Unsupported legacy MCP version; HTTP+SSE is not enabled.",
            ));
        }
        peer.protocol = version.into();
        peer.session = session;
        peer.exchange(
            "notifications/initialized",
            json!({}),
            credential,
            None,
            true,
        )
        .await?;
        Ok(peer)
    }

    pub async fn request(
        &self,
        method: &str,
        params: Value,
        credential: Option<&str>,
        tool: Option<&RemoteTool>,
    ) -> Result<Value> {
        let (status, reply, _) = self
            .exchange(method, params, credential, tool, false)
            .await?;
        checked(status, &reply)?;
        if self.protocol == MODERN && reply["result"]["resultType"] != "complete" {
            return Err(fail(
                "capability",
                "Unsupported modern result type or client input request.",
            ));
        }
        Ok(reply["result"].clone())
    }

    async fn exchange(
        &self,
        method: &str,
        mut params: Value,
        credential: Option<&str>,
        tool: Option<&RemoteTool>,
        notification: bool,
    ) -> Result<(StatusCode, Value, Option<HeaderValue>)> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let mut headers = HeaderMap::new();
        headers.insert(
            "accept",
            HeaderValue::from_static("application/json, text/event-stream"),
        );
        headers.insert("mcp-protocol-version", encoded(&self.protocol)?);
        if self.protocol == MODERN {
            params["_meta"] = json!({
                "io.modelcontextprotocol/protocolVersion":MODERN,
                "io.modelcontextprotocol/clientInfo":{"name":"heron","version":env!("CARGO_PKG_VERSION")},
                "io.modelcontextprotocol/clientCapabilities":{}
            });
            headers.insert("mcp-method", encoded(method)?);
            if let Some(name) = params["name"].as_str() {
                headers.insert("mcp-name", encoded(name)?);
            }
            if let Some(tool) = tool {
                for (path, header) in &tool.headers {
                    let mut value = &params["arguments"];
                    for key in path {
                        value = &value[key];
                    }
                    if !value.is_null() {
                        let text = value
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| value.to_string());
                        let name = reqwest::header::HeaderName::from_bytes(
                            format!("Mcp-Param-{header}").as_bytes(),
                        )
                        .map_err(|_| fail("header", "Invalid tool header annotation."))?;
                        headers.insert(name, encoded(&text)?);
                    }
                }
            }
        } else if let Some(session) = &self.session {
            headers.insert("mcp-session-id", session.clone());
        }
        if let Some(key) = credential {
            let mut auth = HeaderValue::from_str(&format!("Bearer {key}")).map_err(|_| {
                fail(
                    "credential",
                    "Credential cannot be used as an HTTP bearer token.",
                )
            })?;
            auth.set_sensitive(true);
            headers.insert("authorization", auth);
        }
        let mut message = json!({"jsonrpc":"2.0","method":method,"params":params});
        if !notification {
            message["id"] = json!(id);
        }
        if message.to_string().len() > MAX_EVENT {
            return Err(fail("limit", "MCP request exceeds its size limit."));
        }
        let response = self
            .client
            .post(self.url.clone())
            .headers(headers)
            .json(&message)
            .send()
            .await
            .map_err(|_| fail("transport", "Connector connection failed or timed out."))?;
        let status = response.status();
        if status.is_redirection() {
            return Err(fail(
                "redirect",
                "Connector redirects are refused; register the final HTTPS endpoint.",
            ));
        }
        if matches!(status.as_u16(), 401 | 403 | 429) {
            return Err(fail(
                if status == 429 {
                    "rate_limit"
                } else {
                    "authentication"
                },
                "Connector denied access or is rate limited. The request was not retried.",
            ));
        }
        let session = response
            .headers()
            .get("mcp-session-id")
            .cloned()
            .map(|mut value| {
                value.set_sensitive(true);
                value
            });
        if session
            .as_ref()
            .is_some_and(|s| s.len() > 256 || !s.as_bytes().iter().all(|b| (33..=126).contains(b)))
        {
            return Err(fail("protocol", "Invalid MCP session identifier."));
        }
        if notification {
            return if status == StatusCode::ACCEPTED {
                Ok((status, Value::Null, session))
            } else {
                Err(fail("protocol", "MCP notification was not accepted."))
            };
        }
        let ctype = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        let mut stream = response.bytes_stream();
        let mut body = Vec::new();
        let mut total = 0;
        let mut events = 0;
        let mut frames = 0;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| {
                fail(
                    "transport",
                    "MCP response stream was interrupted; invocation was not retried.",
                )
            })?;
            total += chunk.len();
            if total > MAX_BODY {
                return Err(fail("limit", "MCP response exceeds its size limit."));
            }
            body.extend_from_slice(&chunk);
            if ctype == "text/event-stream" {
                // Normalize CRLF without interpreting partial UTF-8 chunks.
                while let Some(frame) = take_frame(&mut body) {
                    frames += 1;
                    if frames > 512 {
                        return Err(fail(
                            "limit",
                            "Too many MCP response frames, including keepalives.",
                        ));
                    }
                    if frame.len() > MAX_EVENT {
                        return Err(fail("limit", "MCP event exceeds its size limit."));
                    }
                    let text = std::str::from_utf8(&frame)
                        .map_err(|_| fail("protocol", "Invalid UTF-8 in MCP stream."))?;
                    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
                    let data = normalized
                        .lines()
                        .filter_map(|l| {
                            l.strip_prefix("data:")
                                .map(|s| s.strip_prefix(' ').unwrap_or(s))
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    if data.is_empty() {
                        continue;
                    }
                    events += 1;
                    if events > 128 {
                        return Err(fail("limit", "Too many MCP response events."));
                    }
                    let value: Value = serde_json::from_str(&data)
                        .map_err(|_| fail("protocol", "Invalid MCP event JSON."))?;
                    if value["jsonrpc"] != "2.0" {
                        return Err(fail("protocol", "Invalid MCP event envelope."));
                    }
                    if value["method"]
                        .as_str()
                        .is_some_and(|s| s.ends_with("list_changed"))
                    {
                        return Err(fail(
                            "manifest_changed",
                            "Server capabilities changed during the request.",
                        ));
                    }
                    if value.get("method").is_some() {
                        if value.get("id").is_some()
                            || !value["method"]
                                .as_str()
                                .unwrap_or("")
                                .starts_with("notifications/")
                        {
                            return Err(fail(
                                "capability",
                                "Unsolicited server requests are not supported.",
                            ));
                        }
                        continue;
                    }
                    validate_envelope(&value, id)?;
                    return Ok((status, value, session));
                }
                if body.len() > MAX_EVENT {
                    return Err(fail("limit", "MCP event exceeds its size limit."));
                }
            }
        }
        if ctype == "text/event-stream" {
            return Err(fail("protocol", "MCP stream ended without a response."));
        }
        if ctype != "application/json" {
            if matches!(status.as_u16(), 400 | 404 | 405) {
                return Ok((status, Value::Null, session));
            }
            return Err(fail("protocol", "MCP response must be JSON or SSE."));
        }
        let value: Value = serde_json::from_slice(&body)
            .map_err(|_| fail("protocol", "Invalid MCP JSON response."))?;
        validate_envelope(&value, id)?;
        Ok((status, value, session))
    }
}

/// SSE accepts LF, CRLF and CR, including mixed endings and split CRLF chunks.
fn take_frame(body: &mut Vec<u8>) -> Option<Vec<u8>> {
    let mut start = 0;
    let mut i = 0;
    while i < body.len() {
        let width = match body[i] {
            b'\r' if i + 1 == body.len() => {
                if i == start {
                    return Some(std::mem::take(body));
                }
                return None;
            }
            b'\r' if body[i + 1] == b'\n' => 2,
            b'\r' | b'\n' => 1,
            _ => {
                i += 1;
                continue;
            }
        };
        if i == start {
            return Some(body.drain(..i + width).collect());
        }
        i += width;
        start = i;
    }
    None
}

fn validate_envelope(value: &Value, id: u64) -> Result<()> {
    if value["jsonrpc"] != "2.0"
        || value.get("method").is_some()
        || (value.get("result").is_some() == value.get("error").is_some())
        || (value["id"] != id && !(value.get("id").is_none() && value.get("error").is_some()))
    {
        return Err(fail(
            "protocol",
            "Malformed MCP response or mismatched request ID.",
        ));
    }
    Ok(())
}
fn checked(status: StatusCode, reply: &Value) -> Result<()> {
    if !status.is_success() || reply.get("error").is_some() || !reply["result"].is_object() {
        return Err(Error {
            code: "remote",
            message: "MCP server rejected the request; remote error text is withheld.",
        });
    }
    if reply["result"].get("inputRequests").is_some() {
        return Err(fail(
            "capability",
            "Server requested an unapproved client capability.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn headers_encode_untrusted_values_and_sentinels() {
        for value in ["line\r\nbreak", "α", " padded ", "=?base64?abc?="] {
            assert!(encoded(value)
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("=?base64?"));
        }
        assert_eq!(encoded("plain").unwrap(), "plain");
    }
}

#[cfg(test)]
mod frame_tests {
    use super::*;
    #[test]
    fn frame_boundaries_accept_mixed_newlines_and_split_crlf() {
        for ending in ["\n\n", "\r\n\r\n", "\r\r", "\n\r\n"] {
            let frame = format!("data: {{}}{ending}");
            let mut body = frame.as_bytes().to_vec();
            assert!(take_frame(&mut body).is_some(), "{ending:?}");
        }
        let mut split = b"data: {}\r\n\r".to_vec();
        assert!(take_frame(&mut split).is_some());
        split.push(b'\n');
        assert!(take_frame(&mut split).is_some());
        assert!(split.is_empty());
    }
}
