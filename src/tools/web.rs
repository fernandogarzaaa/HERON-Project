//! Web tools: fetch a page as readable text, and keyless web search.
//! All web content is returned wrapped as untrusted data.

use super::{arg_str, opt_i64, schema, FnTool, ToolCtx};
use crate::permissions::{check_url, url_host_obviously_local, Risk};
use anyhow::{anyhow, bail, Result};
use futures_util::StreamExt;
use regex::Regex;
use serde_json::{json, Value};
use std::sync::OnceLock;
use std::time::Duration;

const MAX_BODY: usize = 3 * 1024 * 1024;

fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        let allow_local = std::env::var("HERON_ALLOW_LOCAL_FETCH").is_ok();
        reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .user_agent("Mozilla/5.0 (compatible; HeronAgent/0.1)")
            // A public page must not be able to redirect the agent into the local network.
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() >= 5 {
                    attempt.error("too many redirects")
                } else if !allow_local && url_host_obviously_local(attempt.url()) {
                    attempt.error("redirect to a local/internal address blocked")
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .expect("http client")
    })
}

pub(crate) async fn get_limited(url: url::Url) -> Result<(String, Vec<u8>)> {
    let resp = client().get(url).send().await.map_err(|e| anyhow!("request failed: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        bail!("HTTP {status}");
    }
    let ctype = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_lowercase();
    let mut body = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        body.extend_from_slice(&chunk?);
        if body.len() > MAX_BODY {
            body.truncate(MAX_BODY);
            break;
        }
    }
    Ok((ctype, body))
}

pub fn html_to_text(html: &[u8]) -> String {
    html2text::from_read(html, 100).unwrap_or_else(|_| String::from_utf8_lossy(html).to_string())
}

async fn web_fetch(args: &Value, _ctx: &ToolCtx) -> Result<String> {
    let url = check_url(arg_str(args, "url")?).await?;
    let max_chars = opt_i64(args, "max_chars").unwrap_or(20_000).clamp(500, 100_000) as usize;
    let (ctype, body) = get_limited(url).await?;
    let text = if ctype.contains("html") || (ctype.is_empty() && body.starts_with(b"<")) {
        html_to_text(&body)
    } else if ctype.starts_with("text/") || ctype.contains("json") || ctype.contains("xml") || ctype.contains("javascript") || ctype.is_empty() {
        String::from_utf8_lossy(&body).to_string()
    } else {
        bail!("unsupported content type '{ctype}' ({} bytes) — only text content can be read", body.len());
    };
    Ok(crate::util::truncate_middle(text.trim(), max_chars))
}

#[derive(Debug, PartialEq)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

fn strip_tags(s: &str) -> String {
    static T: OnceLock<Regex> = OnceLock::new();
    let no_tags = T.get_or_init(|| Regex::new(r"<[^>]+>").unwrap()).replace_all(s, "");
    no_tags.replace("&amp;", "&").replace("&quot;", "\"").replace("&#x27;", "'").replace("&#39;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&nbsp;", " ").trim().to_string()
}

/// Parse DuckDuckGo's HTML results page.
pub fn parse_ddg(html: &str) -> Vec<SearchResult> {
    static A: OnceLock<Regex> = OnceLock::new();
    static S: OnceLock<Regex> = OnceLock::new();
    let a = A.get_or_init(|| Regex::new(r#"(?s)<a[^>]*class="result__a"[^>]*href="([^"]+)"[^>]*>(.*?)</a>"#).unwrap());
    let s = S.get_or_init(|| Regex::new(r#"(?s)class="result__snippet"[^>]*>(.*?)</(?:a|td|div)>"#).unwrap());
    let snippets: Vec<String> = s.captures_iter(html).map(|c| strip_tags(&c[1])).collect();
    a.captures_iter(html)
        .enumerate()
        .filter_map(|(i, c)| {
            let mut href = c[1].replace("&amp;", "&");
            if href.starts_with("//") {
                href = format!("https:{href}");
            }
            // DDG wraps targets as /l/?uddg=<encoded url>
            if let Ok(u) = url::Url::parse(&href) {
                if let Some((_, target)) = u.query_pairs().find(|(k, _)| k == "uddg") {
                    href = target.to_string();
                }
            }
            if href.contains("duckduckgo.com/y.js") {
                return None; // ad
            }
            Some(SearchResult { title: strip_tags(&c[2]), url: href, snippet: snippets.get(i).cloned().unwrap_or_default() })
        })
        .collect()
}

async fn web_search(args: &Value, _ctx: &ToolCtx) -> Result<String> {
    let query = arg_str(args, "query")?;
    let limit = opt_i64(args, "limit").unwrap_or(8).clamp(1, 20) as usize;
    let url = url::Url::parse_with_params("https://html.duckduckgo.com/html/", &[("q", query)])?;
    let (_, body) = get_limited(url).await.map_err(|e| anyhow!("search failed: {e}"))?;
    let results = parse_ddg(&String::from_utf8_lossy(&body));
    if results.is_empty() {
        return Ok("no results (the search provider returned nothing parseable; try rephrasing or fetch a known URL directly)".into());
    }
    Ok(results.iter().take(limit).enumerate().map(|(i, r)| format!("{}. {}\n   {}\n   {}", i + 1, r.title, r.url, r.snippet)).collect::<Vec<_>>().join("\n"))
}

pub fn tools() -> Vec<FnTool> {
    vec![
        FnTool {
            name: "web_fetch",
            description: "Fetch a public web page or text resource and return it as readable text. Cannot access local/internal addresses.",
            parameters: schema(json!({"url": {"type": "string"}, "max_chars": {"type": "integer", "description": "Default 20000"}}), &["url"]),
            summary: |a| a.get("url").and_then(|u| u.as_str()).unwrap_or("").to_string(),
            risk: |_, _| Risk::Network,
            detail: None,
            untrusted: true,
            run: |a, c| Box::pin(web_fetch(a, c)),
        },
        FnTool {
            name: "web_search",
            description: "Search the web. Returns titles, URLs and snippets. Follow up with web_fetch to read a result.",
            parameters: schema(json!({"query": {"type": "string"}, "limit": {"type": "integer", "description": "Default 8"}}), &["query"]),
            summary: |a| a.get("query").and_then(|u| u.as_str()).unwrap_or("").to_string(),
            risk: |_, _| Risk::Network,
            detail: None,
            untrusted: true,
            run: |a, c| Box::pin(web_search(a, c)),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ddg_results() {
        let html = r#"
        <div class="result"><h2 class="result__title"><a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.rust%2Dlang.org%2F&amp;rut=abc">Rust <b>Programming</b> Language</a></h2>
        <a class="result__snippet" href="x">A language empowering <b>everyone</b> &amp; more.</a></div>
        <div class="result"><a class="result__a" href="https://example.com/page">Example</a>
        <a class="result__snippet" href="x">Second snippet</a></div>"#;
        let r = parse_ddg(html);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0], SearchResult { title: "Rust Programming Language".into(), url: "https://www.rust-lang.org/".into(), snippet: "A language empowering everyone & more.".into() });
        assert_eq!(r[1].url, "https://example.com/page");
    }

    #[test]
    fn html_becomes_readable_text() {
        let t = html_to_text(b"<html><body><h1>Title</h1><p>Hello <b>world</b></p><script>evil()</script></body></html>");
        assert!(t.contains("Title") && t.contains("Hello"));
        assert!(!t.contains("evil"));
    }
}
