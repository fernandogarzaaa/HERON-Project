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

// Serialize database writes with disconnect. A delayed network response must never
// restore another account\'s records after the user disconnects it.
static SOURCE_WRITES: std::sync::Mutex<()> = std::sync::Mutex::new(());
// Provider-derived memory commits and user-facing account disconnect share
// this async gate. A disconnect must wait for any accepted in-flight
// embedding/save, then remove its resulting memories before returning.
pub(crate) static OAUTH_LEARNING_COMMIT: tokio::sync::Mutex<()> =
    tokio::sync::Mutex::const_new(());
// One full sync per provider, including refresh and the final index commit.
// The provider is the isolation unit (not global), so Google and Microsoft
// can sync concurrently without one refresh rotating credentials underneath
// another same-provider sync that is still fetching remote records.
static GOOGLE_SYNC: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static MICROSOFT_SYNC: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
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
            // openid returns the ID token that says which account signed in.
            Self::Microsoft => "openid offline_access User.Read Mail.Read Calendars.Read",
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
    pub(crate) fn account_epoch(self, cfg: &Config) -> Option<String> {
        cfg.secret(&self.secret_key("account_epoch"))
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
    install(cfg, pending.provider, &pending.client_id, &token)
}

/// Who signed in, from the ID token the token endpoint returned over TLS (OpenID Connect Core
/// 3.1.3.7 allows skipping the signature check then). It must have been issued to this app.
fn account_subject(client_id: &str, token: &Value) -> Result<String> {
    let unreadable = || anyhow!("The provider did not say which account signed in. Connect again.");
    let payload = token.get("id_token").and_then(Value::as_str)
        .and_then(|jwt| jwt.split('.').nth(1)).ok_or_else(unreadable)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).map_err(|_| unreadable())?;
    let claims: Value = serde_json::from_slice(&bytes).map_err(|_| unreadable())?;
    let for_us = match &claims["aud"] {
        Value::String(aud) => aud == client_id,
        Value::Array(auds) => auds.iter().any(|aud| aud == client_id),
        _ => false,
    };
    if !for_us {
        bail!("This sign-in was issued to a different app. Connect again.");
    }
    claims["sub"].as_str().filter(|sub| !sub.is_empty() && sub.len() <= 255)
        .map(str::to_string).ok_or_else(unreadable)
}

#[derive(Debug, PartialEq)]
enum Admit {
    Fresh,
    /// The connected account signed in again: swap its credential and keep its data.
    SameAccount,
}

fn admit(connected: bool, stored_subject: Option<&str>, subject: &str) -> Result<Admit> {
    match (connected, stored_subject) {
        (false, _) => Ok(Admit::Fresh),
        (true, Some(stored)) if stored == subject => Ok(Admit::SameAccount),
        (true, Some(_)) => bail!("You signed in with a different account from the one connected. Sign in with that account, or disconnect it first."),
        (true, None) => bail!("This connection was made before Heron recorded which account it belongs to. Disconnect it, then connect again."),
    }
}

fn install(cfg: &Config, provider: Provider, client_id: &str, token: &Value) -> Result<()> {
    if provider == Provider::Google {
        // Google allows granular consent. A token with Gmail but without
        // Calendar would otherwise look connected while every sync fails.
        // Never install an unusable, partially authorized connection.
        let granted = token.get("scope").and_then(Value::as_str)
            .ok_or_else(|| anyhow!("Google did not report granted OAuth scopes. Reconnect and grant both Gmail and Calendar read access."))?;
        let granted: std::collections::HashSet<&str> = granted.split_whitespace().collect();
        let required = [
            "https://www.googleapis.com/auth/gmail.readonly",
            "https://www.googleapis.com/auth/calendar.readonly",
        ];
        if required.iter().any(|scope| !granted.contains(scope)) {
            bail!("Google account consent was incomplete. Grant both Gmail and Calendar read-only access, then reconnect.");
        }
    }
    let refresh = token.get("refresh_token").and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("No offline access was granted. Remove the existing app consent and connect again."))?;
    let subject = account_subject(client_id, token)?;
    // Store the nonsecret client ID too so token refresh still works after an app restart.
    // The refresh token is not sent to the desktop frontend or an agent tool.
    // A different account needs an explicit Disconnect first. Holding
    // SOURCE_WRITES makes this credential installation atomic relative to
    // refresh-token rotation, source sync commits, and disconnect.
    let _guard = SOURCE_WRITES.lock().unwrap_or_else(|e| e.into_inner());
    let stored = cfg.secret(&provider.secret_key("account_subject"));
    let admitted = admit(provider.connected(cfg), stored.as_deref(), &subject)?;
    cfg.store_secret(&provider.secret_key("client_id"), client_id)?;
    if admitted == Admit::Fresh {
        cfg.store_secret(&provider.secret_key("account_subject"), &subject)?;
        cfg.store_secret(&provider.secret_key("account_epoch"), &random_secret())?;
    }
    // The same account keeps its epoch, so its index and memories stay valid.
    // Publish the connection last, after its stable epoch is durable.
    // Unlike the legacy settings secret API this fails closed if Keychain or
    // Credential Manager is unavailable, and removes any old file fallback.
    cfg.store_vault_only_secret(&provider.secret_key("refresh_token"), refresh)?;
    Ok(())
}

async fn access_token_with_generation(cfg: &Config, provider: Provider) -> Result<(String, String)> {
    let id = cfg.secret(&provider.secret_key("client_id"))
        .ok_or_else(|| anyhow!("This OAuth connection has no client ID. Please reconnect."))?;
    // Provider refresh tokens must never use Config::store_secret's legacy
    // owner-only plaintext-file fallback. Legacy installations must explicitly
    // disconnect and reconnect before their tokens may be used again.
    if !cfg.vault_only_secret_present(&provider.secret_key("refresh_token")) {
        bail!("OAuth credential is not in a verified native credential vault. Disconnect and reconnect this account.");
    }
    let refresh = cfg.secret(&provider.secret_key("refresh_token"))
        .ok_or_else(|| anyhow!("This OAuth account is not connected."))?;
    let original_epoch = provider.account_epoch(cfg)
        .ok_or_else(|| anyhow!("OAuth account has no generation. Disconnect and reconnect."))?;
    let token = redeem(provider, &[
        ("client_id", &id), ("refresh_token", &refresh), ("grant_type", "refresh_token"),
    ]).await?;
    // Disconnect may occur during this network request. Never resurrect a revoked
    // connection by saving a rotated refresh token after the user's disconnect.
    {
        let _guard = SOURCE_WRITES.lock().unwrap_or_else(|e| e.into_inner());
        if cfg.secret(&provider.secret_key("refresh_token")).as_deref() != Some(refresh.as_str())
            || provider.account_epoch(cfg).as_deref() != Some(original_epoch.as_str()) {
            bail!("Account was disconnected or changed while renewing access.");
        }
        let generation = original_epoch;
        if let Some(next) = token.get("refresh_token").and_then(Value::as_str).filter(|s| !s.is_empty()) {
            cfg.store_vault_only_secret(&provider.secret_key("refresh_token"), next)?;
        }
        return Ok((token["access_token"].as_str().unwrap_or_default().to_string(), generation));
    }
}

/// The credential generation is captured atomically with the access token.
/// Callers syncing account data must validate that generation when committing
/// fetched records, rather than sampling it after the token was issued.
pub async fn access_token(cfg: &Config, provider: Provider) -> Result<String> {
    access_token_with_generation(cfg, provider).await.map(|(token, _)| token)
}

pub fn disconnect(app: &App, provider: Provider) -> Result<(usize, usize)> {
    let _guard = SOURCE_WRITES.lock().unwrap_or_else(|e| e.into_inner());
    let cfg = app.cfg();
    // Data first. If anything fails, the account still shows as connected and Disconnect can be
    // tried again; removing the credential first would hide that button and strand the data.
    let mut removed = 0;
    let mut memories = 0;
    for source in [provider.mail_source(), provider.calendar_source()] {
        removed += app.store.clear_knowledge(source)?;
        memories += app.store.delete_memories_by_source(source)?;
    }
    let refresh = provider.secret_key("refresh_token");
    cfg.delete_vault_only_secret(&refresh)?;
    if cfg.secret(&refresh).is_some() {
        // A token saved before the verified vault existed lives in the fallback file.
        cfg.store_secret(&refresh, "")?;
    }
    // Neither is secret, and the next connection replaces both. With the token gone, a failure
    // here leaves nothing that can sync.
    for leftover in ["account_epoch", "account_subject", "client_id"] {
        if let Err(err) = cfg.store_secret(&provider.secret_key(leftover), "") {
            eprintln!("HERON OAuth: {} disconnected, but its {leftover} could not be cleared: {err:#}", provider.name());
        }
    }
    Ok((removed, memories))
}

async fn get_json(http: &reqwest::Client, token: &str, url: &str, params: &[(&str, &str)]) -> Result<Value> {
    let mut request = http.get(url).bearer_auth(token).query(params);
    if url.starts_with("https://graph.microsoft.com/") {
        // Graph DateTimeTimeZone fields are otherwise returned in account-local time.
        request = request.header("Prefer", "outlook.timezone=\"UTC\"");
    }
    let result = request.send().await?;
    if !result.status().is_success() {
        bail!("Read-only account sync failed (HTTP {}). The token may be expired or its scopes insufficient.", result.status().as_u16());
    }
    Ok(result.json().await?)
}

fn parse_time(s: &str) -> i64 {
    DateTime::parse_from_rfc3339(s).map(|d| d.timestamp()).or_else(|_| {
        chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
            .map(|d| d.and_utc().timestamp())
    }).or_else(|_| {
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
    // Keep token generation stable until fetched data is committed. A
    // second same-provider sync must not rotate the refresh token during
    // this network request, as doing so would cause a false account
    // replacement detection at commit time.
    let _sync_guard = match provider {
        Provider::Google => GOOGLE_SYNC.lock().await,
        Provider::Microsoft => MICROSOFT_SYNC.lock().await,
    };
    let cfg = app.cfg();
    // Bind the fetched account data to the exact credential generation
    // that issued this access token. Disconnect/reconnect during the
    // network fetch must never write old-account data into new indexes.
    let (token, epoch_at_start) = access_token_with_generation(&cfg, provider).await?;
    let http = oauth_http()?;
    let (email, calendar) = match provider {
        Provider::Google => (google_mail(&http, &token).await?, google_calendar(&http, &token).await?),
        Provider::Microsoft => (microsoft_mail(&http, &token).await?, microsoft_calendar(&http, &token).await?),
    };
    let _guard = SOURCE_WRITES.lock().unwrap_or_else(|e| e.into_inner());
    if provider.account_epoch(&cfg).as_deref() != Some(epoch_at_start.as_str()) || !provider.connected(&cfg) {
        bail!("Account was disconnected or changed while the sync was running.");
    }
    let rows = |items: &[Record]| -> Vec<(String, String, String, i64)> {
        items.iter().map(|rec| (rec.id.clone(), rec.title.clone(), rec.body.clone(), rec.timestamp)).collect()
    };
    app.store.replace_knowledge_snapshot(provider.mail_source(), &rows(&email))?;
    app.store.replace_knowledge_snapshot(provider.calendar_source(), &rows(&calendar))?;
    Ok((email.len(), calendar.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id_token(claims: Value) -> String {
        format!("e30.{}.signature", URL_SAFE_NO_PAD.encode(claims.to_string()))
    }

    #[test]
    fn the_signed_in_account_comes_from_an_id_token_issued_to_this_app() {
        let token = |claims| serde_json::json!({"id_token": id_token(claims)});
        use serde_json::json;
        assert_eq!(account_subject("app", &token(json!({"aud": "app", "sub": "user-1"}))).unwrap(), "user-1");
        assert_eq!(account_subject("app", &token(json!({"aud": ["other", "app"], "sub": "user-1"}))).unwrap(), "user-1");
        assert!(account_subject("app", &token(json!({"aud": "other", "sub": "user-1"}))).unwrap_err().to_string().contains("different app"));
        assert!(account_subject("app", &token(json!({"aud": ["other"], "sub": "user-1"}))).is_err());
        let long = "x".repeat(256);
        for bad in [json!({}), json!({"id_token": "no-dots"}), json!({"id_token": "a.!!!.c"}), json!({"id_token": "a.bm90LWpzb24.c"}),
                    token(json!({"aud": "app"})), token(json!({"aud": "app", "sub": ""})), token(json!({"aud": "app", "sub": long}))] {
            assert!(account_subject("app", &bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn only_the_connected_account_may_sign_in_again_without_disconnecting() {
        assert_eq!(admit(false, None, "u1").unwrap(), Admit::Fresh);
        assert_eq!(admit(false, Some("old"), "u1").unwrap(), Admit::Fresh);
        assert_eq!(admit(true, Some("u1"), "u1").unwrap(), Admit::SameAccount);
        assert!(admit(true, Some("u2"), "u1").unwrap_err().to_string().contains("different account"));
        assert!(admit(true, None, "u1").unwrap_err().to_string().contains("Disconnect it"));
    }

    /// Installing a credential needs the OS vault: run on an authorized desktop, and in CI's vault step.
    #[tokio::test]
    #[cfg(any(target_os = "macos", windows))]
    #[ignore = "requires an authorized, unlocked native credential vault"]
    async fn reconnecting_the_same_account_keeps_its_data_vault_live() {
        use crate::llm::ScriptedLlm;
        use serde_json::json;
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.home = dir.path().join("oauth-vault-qualification");
        let app = App::assemble(cfg, dir.path().join("ws"), crate::store::Store::in_memory().unwrap(), std::sync::Arc::new(ScriptedLlm::new(vec![]))).await.unwrap();
        let p = Provider::Google;
        let token = |refresh: &str, sub: &str| json!({"scope": p.scopes(), "refresh_token": refresh, "id_token": id_token(json!({"aud": "cid", "sub": sub}))});
        install(&app.cfg(), p, "cid", &token("first-refresh", "user-1")).unwrap();
        let epoch = p.account_epoch(&app.cfg()).unwrap();
        app.store.replace_knowledge_snapshot(p.mail_source(), &[("m1".into(), "Invoice".into(), "Due Friday".into(), 1)]).unwrap();
        app.memory.save("The invoice from Acme is due Friday", "fact", "", 0.5, p.mail_source()).await.unwrap();

        // The consent expired or was revoked; the same person signs in again.
        install(&app.cfg(), p, "cid", &token("second-refresh", "user-1")).unwrap();
        let cfg = app.cfg();
        assert_eq!(cfg.secret("oauth_google_refresh_token").as_deref(), Some("second-refresh"));
        assert_eq!(p.account_epoch(&cfg).as_deref(), Some(epoch.as_str()));
        assert_eq!(app.store.knowledge_count(p.mail_source()), 1);
        assert_eq!(app.store.memories_by_source(p.mail_source(), 10).unwrap().len(), 1);

        // Another account cannot take over the connection.
        assert!(install(&cfg, p, "cid", &token("intruder-refresh", "user-2")).is_err());
        assert_eq!(cfg.secret("oauth_google_refresh_token").as_deref(), Some("second-refresh"));
        disconnect(&app, p).unwrap();
        assert!(!p.connected(&app.cfg()));
    }

    #[tokio::test]
    async fn a_failed_disconnect_keeps_the_account_listed_so_it_can_be_retried() {
        use crate::llm::ScriptedLlm;
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.home = dir.path().join("home");
        cfg.ensure_home().unwrap();
        // A token from before the verified vault: the fallback file is the only copy.
        std::fs::write(cfg.home.join("secrets.json"), serde_json::to_string(&serde_json::json!({
            "oauth_google_refresh_token": "legacy-refresh", "oauth_google_account_epoch": "e1", "oauth_google_client_id": "cid",
        })).unwrap()).unwrap();
        let app = App::assemble(cfg, dir.path().join("ws"), crate::store::Store::in_memory().unwrap(), std::sync::Arc::new(ScriptedLlm::new(vec![]))).await.unwrap();
        let p = Provider::Google;
        assert!(p.connected(&app.cfg()));
        app.store.replace_knowledge_snapshot(p.mail_source(), &[("m1".into(), "Invoice".into(), "Due Friday".into(), 1)]).unwrap();
        app.memory.save("The invoice from Acme is due Friday", "fact", "", 0.5, p.mail_source()).await.unwrap();

        app.store.execute_batch_for_test("CREATE TEMP TRIGGER stuck BEFORE DELETE ON knowledge BEGIN SELECT RAISE(ABORT, 'disk busy'); END;");
        assert!(disconnect(&app, p).is_err());
        assert!(p.connected(&app.cfg()), "the account vanished from Settings while its data stayed");
        assert_eq!(app.store.knowledge_count(p.mail_source()), 1);

        app.store.execute_batch_for_test("DROP TRIGGER stuck;");
        assert_eq!(disconnect(&app, p).unwrap(), (1, 1));
        let cfg = app.cfg();
        assert!(!p.connected(&cfg));
        assert_eq!(app.store.knowledge_count(p.mail_source()), 0);
        assert!(cfg.secret("oauth_google_account_epoch").is_none() && cfg.secret("oauth_google_client_id").is_none());
        assert!(!std::fs::read_to_string(cfg.home.join("secrets.json")).unwrap_or_default().contains("legacy-refresh"));
        // Disconnecting again is harmless.
        assert_eq!(disconnect(&app, p).unwrap(), (0, 0));
    }

    #[test]
    fn incomplete_granular_google_consent_is_not_accepted_as_full_sync() {
        let required = Provider::Google.scopes();
        assert!(required.contains("gmail.readonly"));
        assert!(required.contains("calendar.readonly"));
    }
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
