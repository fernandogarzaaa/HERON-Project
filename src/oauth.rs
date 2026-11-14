//! Optional Google and Microsoft 365 OAuth 2.0 connections.
//! Public desktop clients use authorization-code + PKCE; no client secret is accepted.
//! All API permissions are read-only. Tokens are stored through Config::store_secret,
//! never in config.toml, API responses, audit logs, or the model prompt.
use crate::app::App;
use crate::config::Config;
use crate::util::{now_ts, preview};
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration as ChronoDuration, NaiveDate, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    Google,
    Microsoft,
}
impl Provider {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "google" => Ok(Self::Google),
            "microsoft" => Ok(Self::Microsoft),
            _ => bail!("Unknown OAuth provider."),
        }
    }
    pub fn name(self) -> &'static str {
        match self { Self::Google => "google", Self::Microsoft => "microsoft" }
    }
    fn authorize_url(self) -> &'static str {
        match self {
            Self::Google => "https://accounts.google.com/o/oauth2/v2/auth",
            Self::Microsoft => "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
        }
    }
    fn token_url(self) -> &'static str {
        match self {
            Self::Google => "https://oauth2.googleapis.com/token",
            Self::Microsoft => "https://login.microsoftonline.com/common/oauth2/v2.0/token",
        }
    }
    fn scopes(self) -> &'static str {
        match self {
            Self::Google => "openid email https://www.googleapis.com/auth/gmail.readonly https://www.googleapis.com/auth/calendar.readonly",
            Self::Microsoft => "offline_access User.Read Mail.Read Calendars.Read",
        }
    }
    fn secret_key(self, field: &str) -> String {
        format!("oauth_{}_{}", self.name(), field)
    }
    fn mail_source(self) -> &'static str {
        match self { Self::Google => "google_email", Self::Microsoft => "microsoft_email" }
    }
    fn calendar_source(self) -> &'static str {
        match self { Self::Google => "google_calendar", Self::Microsoft => "microsoft_calendar" }
    }
    pub fn connected(self, cfg: &Config) -> bool {
        cfg.secret(&self.secret_key("refresh_token")).is_some()
    }
}

#[derive(Debug)]
pub struct Pending {
    pub provider: Provider,
    pub client_id: String,
    pub redirect_uri: String,
    pub verifier: String,
    pub issued_at: Instant,
}
impl Pending {
    pub fn expired(&self) -> bool {
        self.issued_at.elapsed() > Duration::from_secs(600)
    }
}

fn oauth_http() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(25))
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}
fn random_secret() -> String {
    format!("{}{}{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}
fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}
pub fn begin(provider: Provider, client_id: &str, redirect_uri: &str) -> Result<(String, Pending, Url)> {
    let client_id = client_id.trim();
    if client_id.is_empty() || client_id.len() > 512 || client_id.contains('\n') || client_id.contains('\r') {
        bail!("Supply a valid public OAuth desktop application client ID.");
    }
    let callback = Url::parse(redirect_uri)?;
    if callback.scheme() != "http" || !matches!(callback.host_str(), Some("127.0.0.1" | "localhost"))
        || callback.port().is_none() || callback.path() != format!("/oauth/callback/{}", provider.name())
        || callback.query().is_some() || callback.fragment().is_some() {
        bail!("OAuth callbacks must use this app's loopback address.");
    }
    let verifier = random_secret();
    let state = random_secret();
    let mut url = Url::parse(provider.authorize_url())?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("client_id", client_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("scope", provider.scopes())
            .append_pair("state", &state)
            .append_pair("code_challenge_method", "S256")
            .append_pair("code_challenge", &challenge(&verifier));
        match provider {
            Provider::Google => { q.append_pair("access_type", "offline").append_pair("prompt", "consent"); }
            Provider::Microsoft => { q.append_pair("response_mode", "query"); }
        }
    }
    Ok((state, Pending {provider, client_id: client_id.to_string(), redirect_uri: redirect_uri.to_string(), verifier, issued_at: Instant::now()}, url))
}

async fn redeem(provider: Provider, fields: &[(&str, &str)]) -> Result<Value> {
    let http = oauth_http()?;
    let reply = http.post(provider.token_url()).form(fields).send().await
        .context("OAuth token exchange could not reach the provider")?;
    if !reply.status().is_success() {
        bail!("OAuth token exchange was rejected by the provider (HTTP {}). Check the app registration, redirect URI and account permissions.", reply.status().as_u16());
    }
    let data: Value = reply.json().await.context("OAuth provider returned an invalid token response")?;
    if data.get("access_token").and_then(Value::as_str).filter(|x| !x.is_empty()).is_none() {
        bail!("OAuth provider did not return an access token.");
    }
    Ok(data)
}

pub async fn finish(cfg: &Config, pending: Pending, code: &str) -> Result<()> {
    if pending.expired() || code.len() > 4096 || code.is_empty() {
        bail!("OAuth authorization expired or returned an invalid code. Please reconnect.");
    }
    let token = redeem(pending.provider, &[
        ("client_id", &pending.client_id), ("code", code), ("code_verifier", &pending.verifier),
        ("redirect_uri", &pending.redirect_uri), ("grant_type", "authorization_code"),
    ]).await?;
    let refresh = token.get("refresh_token").and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("No offline access was granted. Remove the existing app consent and connect again."))?;
    // Store the nonsecret client ID too so token refresh still works after an app restart.
    // The refresh token is not sent to the desktop frontend or an agent tool.
    cfg.store_secret(&pending.provider.secret_key("client_id"), &pending.client_id)?;
    cfg.store_secret(&pending.provider.secret_key("refresh_token"), refresh)?;
    Ok(())
}

pub async fn access_token(cfg: &Config, provider: Provider) -> Result<String> {
    let id = cfg.secret(&provider.secret_key("client_id"))
        .ok_or_else(|| anyhow!("This OAuth connection has no client ID. Please reconnect."))?;
    let refresh = cfg.secret(&provider.secret_key("refresh_token"))
        .ok_or_else(|| anyhow!("This OAuth account is not connected."))?;
    let token = redeem(provider, &[
        ("client_id", &id), ("refresh_token", &refresh), ("grant_type", "refresh_token"),
    ]).await?;
    if let Some(next) = token.get("refresh_token").and_then(Value::as_str).filter(|s| !s.is_empty()) {
        cfg.store_secret(&provider.secret_key("refresh_token"), next)?;
    }
    Ok(token["access_token"].as_str().unwrap_or_default().to_string())
}

pub fn disconnect(app: &App, provider: Provider) -> Result<(usize, usize)> {
    let cfg = app.cfg();
    cfg.store_secret(&provider.secret_key("refresh_token"), "")?;
    cfg.store_secret(&provider.secret_key("client_id"), "")?;
    let mut removed = 0;
    let mut memories = 0;
    for source in [provider.mail_source(), provider.calendar_source()] {
        removed += app.store.clear_knowledge(source)?;
        memories += app.store.delete_memories_by_source(source)?;
    }
    Ok((removed, memories))
}

async fn get_json(http: &reqwest::Client, token: &str, url: &str, params: &[(&str, &str)]) -> Result<Value> {
    let result = http.get(url).bearer_auth(token).query(params).send().await?;
    if !result.status().is_success() {
        bail!("Read-only account sync failed (HTTP {}). The token may be expired or its scopes insufficient.", result.status().as_u16());
    }
    Ok(result.json().await?)
}

fn parse_time(s: &str) -> i64 {
    DateTime::parse_from_rfc3339(s).map(|d| d.timestamp()).or_else(|_| {
        NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp())
    }).unwrap_or_else(|_| now_ts())
}

struct Record {
    id: String,
    title: String,
    body: String,
    timestamp: i64,
}
fn string(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}
async fn google_mail(http: &reqwest::Client, token: &str) -> Result<Vec<Record>> {
    let list = get_json(http, token, "https://gmail.googleapis.com/gmail/v1/users/me/messages",
        &[("maxResults", "25"), ("labelIds", "INBOX")]).await?;
    let mut out = vec![];
    for item in list["messages"].as_array().into_iter().flatten().take(25) {
        let id = string(item, "id");
        if id.is_empty() || !id.bytes().all(|c| c.is_ascii_alphanumeric()) { continue; }
        let msg = get_json(http, token, &format!("https://gmail.googleapis.com/gmail/v1/users/me/messages/{id}"),
            &[("format", "metadata"), ("metadataHeaders", "From"), ("metadataHeaders", "Subject")]).await?;
        let header = |name: &str| {
            msg["payload"]["headers"].as_array().into_iter().flatten()
                .find(|v| v["name"].as_str().map(|n| n.eq_ignore_ascii_case(name)).unwrap_or(false))
                .and_then(|v| v["value"].as_str()).unwrap_or_default().to_string()
        };
        let title = header("Subject");
        let from = header("From");
        let ts = msg["internalDate"].as_str().and_then(|v| v.parse::<i64>().ok()).map(|v| v / 1000).unwrap_or_else(now_ts);
        out.push(Record {id, title: preview(&title, 250), body: format!("From: {}\nPreview: {}", preview(&from, 300), preview(msg["snippet"].as_str().unwrap_or_default(), 700)), timestamp: ts});
    }
    Ok(out)
}
async fn google_calendar(http: &reqwest::Client, token: &str) -> Result<Vec<Record>> {
    let since = Utc::now().to_rfc3339();
    let list = get_json(http, token, "https://www.googleapis.com/calendar/v3/calendars/primary/events",
        &[("timeMin", &since), ("maxResults", "50"), ("singleEvents", "true"), ("orderBy", "startTime")]).await?;
    let mut out = vec![];
    for item in list["items"].as_array().into_iter().flatten().take(50) {
        let id = string(item, "id"); if id.is_empty() { continue; }
        let start = item["start"]["dateTime"].as_str().or_else(|| item["start"]["date"].as_str()).unwrap_or_default();
        out.push(Record {id, title: preview(&string(item, "summary"), 250),
            body: format!("When: {}\nWhere: {}\nNotes: {}", start, preview(&string(item, "location"), 300), preview(&string(item, "description"), 1000)),
            timestamp: parse_time(start)});
    }
    Ok(out)
}
async fn microsoft_mail(http: &reqwest::Client, token: &str) -> Result<Vec<Record>> {
    let list = get_json(http, token, "https://graph.microsoft.com/v1.0/me/mailFolders/inbox/messages",
        &[("$top", "25"), ("$select", "id,subject,from,receivedDateTime,bodyPreview"), ("$orderby", "receivedDateTime desc")]).await?;
    let mut out = vec![];
    for item in list["value"].as_array().into_iter().flatten().take(25) {
        let id = string(item, "id"); if id.is_empty() {continue;}
        out.push(Record {id, title: preview(&string(item, "subject"), 250),
            body: format!("From: {}\nPreview: {}", preview(item["from"]["emailAddress"]["address"].as_str().unwrap_or_default(), 300), preview(&string(item,"bodyPreview"), 700)),
            timestamp: parse_time(&string(item, "receivedDateTime"))});
    }
    Ok(out)
}
async fn microsoft_calendar(http: &reqwest::Client, token: &str) -> Result<Vec<Record>> {
    let from = Utc::now();
    let until = from + ChronoDuration::days(30);
    let start = from.to_rfc3339();
    let end = until.to_rfc3339();
    let list = get_json(http, token, "https://graph.microsoft.com/v1.0/me/calendarView",
        &[("startDateTime", &start), ("endDateTime", &end), ("$top", "50"), ("$select", "id,subject,start,end,location")]).await?;
    let mut out = vec![];
    for item in list["value"].as_array().into_iter().flatten().take(50) {
        let id = string(item, "id"); if id.is_empty() { continue; }
        let when = item["start"]["dateTime"].as_str().unwrap_or_default();
        out.push(Record {id, title: preview(&string(item, "subject"), 250),
            body: format!("When: {} ({})\nWhere: {}", when, item["start"]["timeZone"].as_str().unwrap_or("UTC"), preview(item["location"]["displayName"].as_str().unwrap_or_default(), 300)),
            timestamp: parse_time(when)});
    }
    Ok(out)
}

/// Refresh both read-only sources. Read everything before replacing saved results,
/// so a failed API request does not erase the previous offline index.
pub async fn sync(app: &App, provider: Provider) -> Result<(usize, usize)> {
    let token = access_token(&app.cfg(), provider).await?;
    let http = oauth_http()?;
    let (email, calendar) = match provider {
        Provider::Google => (google_mail(&http, &token).await?, google_calendar(&http, &token).await?),
        Provider::Microsoft => (microsoft_mail(&http, &token).await?, microsoft_calendar(&http, &token).await?),
    };
    app.store.clear_knowledge(provider.mail_source())?;
    for rec in &email {
        app.store.upsert_knowledge(provider.mail_source(), &rec.id, &rec.title, &rec.body, rec.timestamp)?;
    }
    app.store.clear_knowledge(provider.calendar_source())?;
    for rec in &calendar {
        app.store.upsert_knowledge(provider.calendar_source(), &rec.id, &rec.title, &rec.body, rec.timestamp)?;
    }
    Ok((email.len(), calendar.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pkce_is_sha256_base64url_without_padding() {
        assert_eq!(challenge("hello"), "LPJNul-wow4m6DsqxbninhsWHlwfp0JecwQzYpOLmCQ");
    }
    #[test]
    fn callbacks_are_only_local_and_bound_to_the_provider() {
        assert!(begin(Provider::Google, "client", "https://my.site/oauth/callback/google").is_err());
        assert!(begin(Provider::Google, "client", "http://127.0.0.1:8080/oauth/callback/microsoft").is_err());
        assert!(begin(Provider::Google, "client", "http://127.0.0.1:8080/oauth/callback/google").is_ok());
    }
    #[test]
    fn read_only_authorization_scopes_and_unique_state() {
        let (a, pending, url) = begin(Provider::Microsoft, "desktop-client", "http://localhost:8999/oauth/callback/microsoft").unwrap();
        let (b, _, _) = begin(Provider::Microsoft, "desktop-client", "http://localhost:8999/oauth/callback/microsoft").unwrap();
        assert_ne!(a, b);
        assert_eq!(pending.verifier.len(), 128);
        let pairs: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(pairs.get("code_challenge_method").map(String::as_str), Some("S256"));
        assert!(pairs.get("scope").unwrap().contains("Calendars.Read"));
        assert!(!pairs.get("scope").unwrap().contains("Mail.Send"));
    }
}
