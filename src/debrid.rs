//! Explicit opt-in cloud providers. Tokens stay in memory and are never
//! forwarded to generated download URLs or persisted with queue entries.
//!
//! The panel in `app.rs` is the only caller: it validates the account,
//! hands the magnet or hosting link to the provider, lets the user pick
//! files and puts the direct links into the normal queue.
//!
//! The provider surface is complete — cache checks, link expiry, error
//! codes — but the panel uses only part of it, so the rest would read as
//! dead code even though the tests cover it.
#![allow(dead_code)]

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

/// The cloud provider a job belongs to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provider {
    #[default]
    RealDebrid,
    TorBox,
    AllDebrid,
    Premiumize,
    DebridLink,
}

impl Provider {
    /// Every provider the panel can talk to, in menu order.
    pub const ALL: [Provider; 5] = [
        Provider::RealDebrid,
        Provider::TorBox,
        Provider::AllDebrid,
        Provider::Premiumize,
        Provider::DebridLink,
    ];

    /// Human-readable provider name.
    pub fn label(self) -> &'static str {
        match self {
            Self::RealDebrid => "Real-Debrid",
            Self::TorBox => "TorBox",
            Self::AllDebrid => "AllDebrid",
            Self::Premiumize => "Premiumize",
            Self::DebridLink => "Debrid-Link",
        }
    }

    /// Provider order used by the (later) settings UI.
    pub fn index(self) -> usize {
        match self {
            Self::RealDebrid => 0,
            Self::TorBox => 1,
            Self::AllDebrid => 2,
            Self::Premiumize => 3,
            Self::DebridLink => 4,
        }
    }

    /// The provider at `index`, wrapping around.
    pub fn from_index(index: usize) -> Provider {
        Provider::ALL[index % Provider::ALL.len()]
    }

    /// Whether the provider holds a torrent back until its files are
    /// named.
    ///
    /// Debrid-Link's seedbox takes a torrent in a waiting state and
    /// starts it when the files it must not fetch are named.
    pub fn waits_for_file_selection(self) -> bool {
        matches!(self, Provider::DebridLink)
    }

    /// What this provider's own API documents that it can do.
    ///
    /// Every flag is read from the provider's published API surface. A
    /// workflow an API does not offer is reported as missing so the panel
    /// can say why, instead of pretending the provider did the work.
    pub fn capabilities(self) -> Capabilities {
        match self {
            // Real-Debrid: instant availability, per-file selection, .torrent
            // upload, and a hosting link that resolves to a direct download.
            Self::RealDebrid => Capabilities {
                cache_query: true,
                provider_file_selection: true,
                torrent_metadata: true,
                direct_hosting: true,
                hosting_password: true,
                stated_link_lifetime: true,
            },
            // TorBox: a cache query and .torrent upload, but a hosting link
            // becomes a task and every file of a torrent is fetched.
            Self::TorBox => Capabilities {
                cache_query: true,
                provider_file_selection: false,
                torrent_metadata: true,
                direct_hosting: false,
                hosting_password: true,
                stated_link_lifetime: false,
            },
            // AllDebrid: a hosting link resolves straight away and a .torrent
            // can be uploaded, but there is no instant-availability query and
            // no per-file selection.
            Self::AllDebrid => Capabilities {
                cache_query: false,
                provider_file_selection: false,
                torrent_metadata: true,
                direct_hosting: true,
                hosting_password: true,
                stated_link_lifetime: false,
            },
            // Premiumize: a hosting link can be resolved into a direct link
            // and a magnet can be queued, but there is no .torrent upload and
            // no hash cache query.
            Self::Premiumize => Capabilities {
                cache_query: false,
                provider_file_selection: false,
                torrent_metadata: false,
                direct_hosting: true,
                hosting_password: true,
                stated_link_lifetime: false,
            },
            // Debrid-Link: a seedbox with per-file selection and .torrent
            // upload, but a host link becomes a tracked record.
            Self::DebridLink => Capabilities {
                cache_query: false,
                provider_file_selection: true,
                torrent_metadata: true,
                direct_hosting: false,
                hosting_password: true,
                stated_link_lifetime: false,
            },
        }
    }

    /// Why a workflow is not available for this provider, when it is not.
    ///
    /// `None` means the provider's API supports it.
    pub fn unsupported_reason(self, workflow: Workflow) -> Option<&'static str> {
        let caps = self.capabilities();
        let supported = match workflow {
            Workflow::CacheQuery => caps.cache_query,
            Workflow::ProviderFileSelection => caps.provider_file_selection,
            Workflow::TorrentMetadata => caps.torrent_metadata,
            Workflow::DirectHosting => caps.direct_hosting,
            Workflow::HostingPassword => caps.hosting_password,
            Workflow::StatedLinkLifetime => caps.stated_link_lifetime,
        };
        if supported {
            return None;
        }
        Some(match (self, workflow) {
            (
                Provider::AllDebrid,
                Workflow::CacheQuery,
            ) => "AllDebrid has no instant-availability query. It says whether a torrent is already available when the torrent is submitted.",
            (
                Provider::Premiumize,
                Workflow::CacheQuery,
            ) => "Premiumize's cache query takes download links, not torrent hashes, so it cannot say whether a magnet is already held.",
            (
                Provider::DebridLink,
                Workflow::CacheQuery,
            ) => "Debrid-Link removed its cached-torrent query endpoint, so DownloadCraft cannot say whether a magnet is already held.",
            (
                Provider::TorBox,
                Workflow::ProviderFileSelection,
            ) => "TorBox downloads every file of a torrent; the provider offers no per-file selection.",
            (
                Provider::AllDebrid,
                Workflow::ProviderFileSelection,
            ) => "AllDebrid fetches every file of a torrent; the provider offers no per-file selection.",
            (
                Provider::Premiumize,
                Workflow::ProviderFileSelection,
            ) => "Premiumize fetches every file of a transfer; the provider offers no per-file selection.",
            (
                Provider::Premiumize,
                Workflow::TorrentMetadata,
            ) => "Premiumize's API takes magnet links and host URLs, not uploaded .torrent metadata.",
            (
                Provider::TorBox,
                Workflow::DirectHosting,
            ) => "TorBox turns a hosting link into a task that is polled like a torrent, so there is no instant direct link.",
            (
                Provider::DebridLink,
                Workflow::DirectHosting,
            ) => "Debrid-Link keeps a downloader record for every host link, so the link is tracked and can expire.",
            (
                Provider::TorBox,
                Workflow::StatedLinkLifetime,
            ) => "TorBox does not state how long a link stays valid; the app assumes one day.",
            (
                Provider::AllDebrid,
                Workflow::StatedLinkLifetime,
            ) => "AllDebrid does not state how long a link stays valid; the app assumes one day.",
            (
                Provider::Premiumize,
                Workflow::StatedLinkLifetime,
            ) => "Premiumize does not state how long a link stays valid; the app assumes one day.",
            (
                Provider::DebridLink,
                Workflow::StatedLinkLifetime,
            ) => "Debrid-Link only says whether a link has expired, not when it will; the app assumes one day.",
            _ => "This provider's API does not offer that.",
        })
    }
}

/// What a provider's own API documents that it can do.
///
/// A missing flag means the workflow is switched off in the panel and
/// the reason is shown, rather than the workflow being simulated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    /// A query that says whether the provider already holds a torrent.
    pub cache_query: bool,
    /// The provider can be told which files of a torrent to fetch.
    pub provider_file_selection: bool,
    /// The provider accepts uploaded `.torrent` metadata.
    pub torrent_metadata: bool,
    /// A hosting link resolves to a direct download without a cloud task.
    pub direct_hosting: bool,
    /// A hosting link may carry a password.
    pub hosting_password: bool,
    /// A generated link comes with a stated lifetime.
    pub stated_link_lifetime: bool,
}

/// A workflow the panel can offer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Workflow {
    /// Asking whether the provider already holds a torrent.
    CacheQuery,
    /// Telling the provider which files of a torrent to fetch.
    ProviderFileSelection,
    /// Uploading `.torrent` metadata.
    TorrentMetadata,
    /// A hosting link that resolves without a cloud task.
    DirectHosting,
    /// A password on a hosting link.
    HostingPassword,
    /// A stated lifetime for a generated link.
    StatedLinkLifetime,
}

/// What the provider was asked to do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    #[default]
    Torrent,
    Hosting,
}

/// A cloud task the provider runs on our behalf.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CloudJob {
    pub provider: Provider,
    pub id: String,
    #[serde(default)]
    pub kind: Kind,
    /// The source the provider was given, when it needs it again.
    ///
    /// Premiumize resolves a transfer's files from its source, so the
    /// magnet or host URL has to travel with the task.
    #[serde(default)]
    pub source: Option<String>,
    /// Whether the provider said the torrent was already available.
    #[serde(default)]
    pub cached: Option<bool>,
}

/// One file inside a cloud task.
#[derive(Clone, Debug)]
pub struct CloudFile {
    pub id: u64,
    pub name: String,
    pub bytes: u64,
    /// The provider's own "this file is picked" flag.
    pub selected: bool,
    pub link: String,
    /// The provider says this file's address has run out.
    pub expired: bool,
}

/// The files of a cloud task plus its provider status.
#[derive(Clone, Debug)]
pub struct Listing {
    pub files: Vec<CloudFile>,
    /// The provider's own status word, kept verbatim.
    pub status: String,
    /// The same status normalized across providers.
    pub state: CloudStatus,
}

/// A hosting link the provider resolved to a direct download.
///
/// This is a local DTO: the provider module must not depend on the
/// static mirror subsystem, so the link shape is declared here instead
/// of borrowing the mirror's discovery record.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedFile {
    pub url: String,
    pub name: String,
    #[serde(default)]
    pub connections: Option<usize>,
}

/// Whether the provider already holds a torrent.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum CacheState {
    /// The provider holds every file it reported: an instant download.
    Cached,
    /// The provider holds only part of the torrent.
    Partial,
    /// The provider does not hold it and has to fetch it.
    Uncached,
    /// The provider did not answer the question.
    #[default]
    Unknown,
}

impl CacheState {
    /// Short label for the (later) UI.
    pub fn label(self) -> &'static str {
        match self {
            Self::Cached => "cached",
            Self::Partial => "partial",
            Self::Uncached => "uncached",
            Self::Unknown => "unknown",
        }
    }
}

/// A file the provider reports inside a cached torrent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CachedFile {
    pub id: u64,
    pub name: String,
    pub bytes: u64,
}

/// What the provider says about a torrent hash.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CacheStatus {
    pub hash: String,
    pub state: CacheState,
    /// The files the provider reported as available.
    pub files: Vec<CachedFile>,
    /// The summed size of the reported files.
    pub bytes: u64,
}

/// The provider's job state, normalized across providers.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum CloudStatus {
    /// Accepted by the provider, nothing fetched yet.
    Queued,
    /// The provider is fetching it.
    Downloading,
    /// The provider holds it; files can be selected and downloaded.
    Ready,
    /// Available in part, or waiting for the user to select files.
    Partial,
    /// The provider gave up on it.
    Failed,
    /// The provider could not be asked.
    #[default]
    Unknown,
}

impl CloudStatus {
    /// Short label for the (later) UI.
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Downloading => "downloading",
            Self::Ready => "ready",
            Self::Partial => "partial",
            Self::Failed => "failed",
            Self::Unknown => "unknown",
        }
    }
}

/// The provider account behind a token.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Account {
    pub provider: Provider,
    /// The account name or address the provider reports.
    pub username: String,
    /// The plan the provider reports ("premium", "Pro", …).
    pub plan: String,
    /// Does the account have a paid plan?
    pub premium: bool,
    /// Unix seconds when the account expires, when the provider says.
    pub expires_at: Option<u64>,
}

impl Account {
    /// One line for the user: who the account is and what it may do.
    pub fn summary(&self) -> String {
        let who = if self.username.is_empty() {
            self.provider.label().to_string()
        } else {
            self.username.clone()
        };
        let plan = if self.plan.is_empty() {
            "plan unknown".to_string()
        } else {
            self.plan.clone()
        };
        format!("{who} — {plan}")
    }
}

/// A provider refusal, carrying the provider's own code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderError {
    pub provider: Provider,
    /// The HTTP status, when the failure was a status.
    pub status: Option<u16>,
    /// The provider's code: a number for Real-Debrid, a word for TorBox,
    /// AllDebrid, Premiumize and Debrid-Link.
    pub code: Option<String>,
    /// The provider's own message, when it sent one.
    pub message: String,
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} request failed", self.provider.label())?;
        if let Some(status) = self.status {
            write!(f, " (HTTP {status})")?;
        }
        if let Some(code) = &self.code {
            write!(f, " — {code}")?;
            // The documented meanings belong to Real-Debrid's own codes;
            // another provider's code is never read through that table.
            if self.provider == Provider::RealDebrid {
                if let Some(text) = realdebrid_error_text(code) {
                    write!(f, " ({text})")?;
                }
            }
        }
        if !self.message.is_empty() {
            write!(f, ": {}", self.message)?;
        }
        Ok(())
    }
}

impl std::error::Error for ProviderError {}

/// The provider's error code, when the failure carried one.
pub fn provider_code(error: &anyhow::Error) -> Option<&str> {
    error
        .downcast_ref::<ProviderError>()
        .and_then(|e| e.code.as_deref())
}

/// Real-Debrid's numeric error codes, with their documented meaning.
///
/// Source: the Real-Debrid API documentation ("List of numeric error
/// codes"). The number is what an application should branch on; the text
/// is only for the message shown to the user.
fn realdebrid_error_text(code: &str) -> Option<&'static str> {
    Some(match code {
        "-1" => "internal error",
        "1" => "missing parameter",
        "2" => "bad parameter value",
        "3" => "unknown method",
        "4" => "method not allowed",
        "5" => "slow down",
        "6" => "resource unreachable",
        "7" => "resource not found",
        "8" => "bad token",
        "9" => "permission denied",
        "10" => "two-factor authentication needed",
        "11" => "two-factor authentication pending",
        "12" => "invalid login",
        "13" => "invalid password",
        "14" => "account locked",
        "15" => "account not activated",
        "16" => "unsupported hoster",
        "17" => "hoster in maintenance",
        "18" => "hoster limit reached",
        "19" => "hoster temporarily unavailable",
        "20" => "hoster not available for free users",
        "21" => "too many active downloads",
        "22" => "IP address not allowed",
        "23" => "traffic exhausted",
        "24" => "file unavailable",
        "25" => "service unavailable",
        "26" => "upload too big",
        "27" => "upload error",
        "28" => "file not allowed",
        "29" => "torrent too big",
        "30" => "torrent file invalid",
        "31" => "action already done",
        "32" => "image resolution error",
        "33" => "torrent already active",
        "34" => "too many requests",
        "35" => "infringing file",
        "36" => "fair usage limit",
        "37" => "disabled endpoint",
        _ => return None,
    })
}

/// A provider-generated direct link and the lifetime the provider gave it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectLink {
    /// The direct HTTPS address. Never contains the API token.
    pub url: String,
    /// The provider's connection limit for this file, when it sent one.
    pub connections: Option<usize>,
    /// Unix seconds when the provider says the link stops working.
    pub expires_at: Option<u64>,
    /// Unix seconds when the link was requested.
    pub generated_at: u64,
    /// The lifetime used when the provider did not state one.
    pub lifetime_s: u64,
}

impl DirectLink {
    /// When the link stops working: the provider's own expiry when it
    /// sent one, otherwise the provider's default lifetime.
    pub fn effective_expiry(&self) -> u64 {
        self.expires_at
            .unwrap_or_else(|| self.generated_at.saturating_add(self.lifetime_s))
    }

    /// Did the provider state an expiry of its own?
    pub fn expiry_known(&self) -> bool {
        self.expires_at.is_some()
    }

    /// Is the link past its lifetime?
    pub fn is_expired(&self, now: u64) -> bool {
        now >= self.effective_expiry()
    }

    /// Seconds left before the link expires.
    pub fn expires_in(&self, now: u64) -> u64 {
        self.effective_expiry().saturating_sub(now)
    }

    /// Should the link be replaced? A refresh is always safe: it asks the
    /// provider for a new address instead of reusing a stale one.
    pub fn needs_refresh(&self, now: u64) -> bool {
        self.is_expired(now)
    }
}

/// TorBox opens a download link for three hours (TorBox API help:
/// "This endpoint opens the link for 3 hours").
const TORBOX_LINK_LIFETIME_S: u64 = 3 * 60 * 60;

/// Real-Debrid publishes no lifetime for a generated link. Community
/// measurements put one at about a week; the value only decides when a
/// link is refreshed, and refreshing is always safe.
const REALDEBRID_LINK_LIFETIME_S: u64 = 7 * 24 * 60 * 60;

/// AllDebrid, Premiumize and Debrid-Link state no lifetime for a
/// generated link either, so the app assumes a day. The value only
/// decides when a link is refreshed, and refreshing is always safe.
const DEFAULT_LINK_LIFETIME_S: u64 = 24 * 60 * 60;

/// Bounded retries for a rate-limited request.
const RATE_RETRIES: u32 = 3;
/// First backoff step; every retry doubles it.
const RATE_BACKOFF: Duration = Duration::from_millis(500);
/// No provider may hold the app longer than this.
const RATE_BACKOFF_MAX: Duration = Duration::from_secs(30);
/// Largest provider answer we read.
const RESPONSE_LIMIT: usize = 4 * 1024 * 1024;
/// Largest error answer we read.
const ERROR_BODY_LIMIT: usize = 64 * 1024;

/// A provider client. The token lives in memory only.
#[derive(Clone)]
pub struct Api {
    provider: Provider,
    token: String,
    client: reqwest::Client,
    /// First backoff step for a rate-limited request.
    backoff: Duration,
    #[cfg(test)]
    test_base: Option<String>,
}

impl Api {
    /// Build a client for `provider`. The token is trimmed and kept in
    /// memory; it is never written to disk or to a generated URL.
    pub fn new(provider: Provider, token: String) -> Result<Self> {
        if token.trim().is_empty() {
            return Err(anyhow!("Enter your provider API token first"));
        }
        Ok(Self {
            #[cfg(test)]
            test_base: None,
            provider,
            token: token.trim().into(),
            backoff: RATE_BACKOFF,
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
        })
    }

    /// The provider endpoint for `path`.
    fn url(&self, path: &str) -> String {
        self.url_at(self.base_url(), path)
    }

    /// The provider endpoint for `path` on another documented API
    /// version of the same provider.
    fn url_at(&self, base: &str, path: &str) -> String {
        #[cfg(test)]
        if let Some(base) = &self.test_base {
            return format!("{base}/{path}");
        }
        format!("{base}{path}")
    }

    /// The API root the provider documents.
    fn base_url(&self) -> &'static str {
        match self.provider {
            Provider::RealDebrid => "https://api.real-debrid.com/rest/1.0/",
            Provider::TorBox => "https://api.torbox.app/v1/api/",
            Provider::AllDebrid => "https://api.alldebrid.com/v4/",
            Provider::Premiumize => "https://www.premiumize.me/api/",
            Provider::DebridLink => "https://debrid-link.com/api/v2/",
        }
    }

    /// Send a request and decode the JSON answer.
    ///
    /// `build` is called again for every attempt, so a retried request
    /// always carries a fresh body. A rate-limited answer (HTTP 429) is
    /// retried a bounded number of times with an exponential backoff.
    ///
    /// Errors never echo the request URL: the token is part of it.
    async fn json(&self, build: impl Fn() -> reqwest::RequestBuilder) -> Result<Value> {
        let mut attempt = 0u32;
        loop {
            let mut response = build()
                .send()
                .await
                .map_err(|_| anyhow!("Provider request failed or timed out"))?;
            let status = response.status();
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS && attempt < RATE_RETRIES {
                let wait = self.backoff_for(attempt, response.headers());
                attempt += 1;
                tokio::time::sleep(wait).await;
                continue;
            }
            if !status.is_success() {
                let bytes = read_body(&mut response, ERROR_BODY_LIMIT)
                    .await
                    .unwrap_or_default();
                let value: Option<Value> = serde_json::from_slice(&bytes).ok();
                return Err(anyhow::Error::new(self.provider_error(status, value.as_ref())));
            }
            if status == reqwest::StatusCode::NO_CONTENT {
                return Ok(Value::Null);
            }
            let bytes = read_body(&mut response, RESPONSE_LIMIT).await?;
            let value: Value =
                serde_json::from_slice(&bytes).map_err(|_| anyhow!("Invalid provider response"))?;
            if self.envelope_failed(&value) {
                return Err(anyhow::Error::new(self.provider_error(status, Some(&value))));
            }
            return Ok(value);
        }
    }

    /// Whether the provider's own answer says the request failed.
    ///
    /// Every provider wraps its answer in its own envelope: Real-Debrid
    /// and TorBox use `success`/`error_code`, AllDebrid and Premiumize
    /// use `status`, and Debrid-Link uses `success` with an `error`
    /// object. A provider that answers `200 OK` with a refusal in the
    /// body is a refusal, not a success.
    fn envelope_failed(&self, value: &Value) -> bool {
        match self.provider {
            Provider::RealDebrid | Provider::TorBox => {
                value.get("success").and_then(Value::as_bool) == Some(false)
                    || value.get("error_code").is_some()
            }
            Provider::AllDebrid | Provider::Premiumize => {
                value.get("status").and_then(Value::as_str) == Some("error")
            }
            Provider::DebridLink => {
                value.get("success").and_then(Value::as_bool) == Some(false)
                    || value.get("error").is_some()
            }
        }
    }

    /// How long to wait before the next attempt after a 429.
    ///
    /// A provider that says when to come back wins, but only up to the
    /// cap: a broken header must not stall the app.
    fn backoff_for(&self, attempt: u32, headers: &reqwest::header::HeaderMap) -> Duration {
        if let Some(after) = headers
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok())
        {
            if after > 0 {
                return Duration::from_secs(after).min(RATE_BACKOFF_MAX);
            }
        }
        self.backoff
            .saturating_mul(1u32 << attempt.min(5))
            .min(RATE_BACKOFF_MAX)
    }

    /// Build the error for a provider refusal, keeping its own code.
    fn provider_error(&self, status: reqwest::StatusCode, value: Option<&Value>) -> ProviderError {
        let (code, message) = match self.provider {
            // Real-Debrid and TorBox: a numeric code and a word.
            Provider::RealDebrid | Provider::TorBox => {
                let code = value
                    .and_then(|v| v.get("error_code"))
                    .and_then(Value::as_i64)
                    .map(|n| n.to_string())
                    .or_else(|| {
                        value
                            .and_then(|v| v.get("error"))
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    });
                let message = value
                    .and_then(|v| v.get("detail").and_then(Value::as_str))
                    .or_else(|| value.and_then(|v| v.get("error").and_then(Value::as_str)))
                    .unwrap_or("")
                    .to_string();
                (code, message)
            }
            // AllDebrid: the code and the sentence sit in `error`.
            Provider::AllDebrid => {
                let error = value.and_then(|v| v.get("error"));
                let code = error
                    .and_then(|e| e.get("code"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let message = error
                    .and_then(|e| e.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                (code, message)
            }
            // Premiumize: a code and a message at the top level.
            Provider::Premiumize => {
                let code = value
                    .and_then(|v| v.get("code"))
                    .map(|c| match c {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    });
                let message = value
                    .and_then(|v| v.get("message").and_then(Value::as_str))
                    .unwrap_or("")
                    .to_string();
                (code, message)
            }
            // Debrid-Link: the error name and its explanation.
            Provider::DebridLink => {
                let code = value
                    .and_then(|v| v.get("error"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let message = value
                    .and_then(|v| v.get("error_description").and_then(Value::as_str))
                    .or_else(|| value.and_then(|v| v.get("error").and_then(Value::as_str)))
                    .unwrap_or("")
                    .to_string();
                (code, message)
            }
        };
        ProviderError {
            provider: self.provider,
            status: Some(status.as_u16()),
            code,
            message,
        }
    }

    /// Attach the bearer token.
    fn auth(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        request.bearer_auth(&self.token)
    }

    /// Hand a magnet link to the provider. The provider runs the swarm;
    /// this app has no local torrent engine.
    pub async fn add(&self, magnet: &str) -> Result<CloudJob> {
        let magnet = magnet.trim();
        let u = url::Url::parse(magnet)?;
        if u.scheme() != "magnet"
            || !u
                .query_pairs()
                .any(|(k, v)| k == "xt" && v.starts_with("urn:bt"))
        {
            return Err(anyhow!("Enter a magnet link"));
        }
        let (value, cached) = match self.provider {
            Provider::RealDebrid => {
                let value = self
                    .json(|| {
                        self.auth(self.client.post(self.url("torrents/addMagnet")))
                            .form(&[("magnet", magnet)])
                    })
                    .await?;
                (value, None)
            }
            Provider::TorBox => {
                let value = self
                    .json(|| {
                        self.auth(self.client.post(self.url("torrents/createtorrent")))
                            .multipart(
                                reqwest::multipart::Form::new()
                                    .text("magnet", magnet.to_string())
                                    .text("seed", "1"),
                            )
                    })
                    .await?;
                (value, None)
            }
            // AllDebrid takes the magnet in `magnets[]` and answers with
            // the magnet it stored. Its `ready` flag says whether the
            // torrent is already available: that is this provider's cache
            // answer, and it comes with the submission.
            Provider::AllDebrid => {
                let value = self
                    .json(|| {
                        self.auth(self.client.post(self.url("magnet/upload")))
                            .form(&[("magnets[]", magnet)])
                    })
                    .await?;
                let ready = value
                    .pointer("/data/magnets/0/ready")
                    .and_then(Value::as_bool);
                (value, ready)
            }
            // Premiumize queues the magnet as a transfer in its cloud.
            Provider::Premiumize => {
                let value = self
                    .json(|| {
                        self.auth(self.client.post(self.url("transfer/create")))
                            .form(&[("src", magnet)])
                    })
                    .await?;
                (value, None)
            }
            // Debrid-Link's seedbox only takes a torrent it already holds.
            // `wait` holds the torrent back so its file list can be
            // narrowed before the copy starts.
            Provider::DebridLink => {
                let value = self
                    .json(|| {
                        self.auth(self.client.post(self.url("seedbox/add")))
                            .form(&[("url", magnet), ("wait", "true")])
                    })
                    .await?;
                (value, None)
            }
        };
        let id = match self.provider {
            Provider::RealDebrid => value.get("id"),
            Provider::TorBox => value.pointer("/data/torrent_id"),
            Provider::AllDebrid => value.pointer("/data/magnets/0/id"),
            Provider::Premiumize => value.get("id"),
            Provider::DebridLink => value.pointer("/value/id"),
        }
        .ok_or_else(|| anyhow!("Provider did not return a cloud task ID"))?;
        let id = id
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| id.to_string());
        if !safe_id(&id) {
            return Err(anyhow!("Invalid cloud task ID"));
        }
        Ok(CloudJob {
            provider: self.provider,
            id,
            kind: Kind::Torrent,
            source: Some(magnet.to_string()),
            cached,
        })
    }

    /// Upload `.torrent` metadata for the provider to fetch.
    pub async fn torrent_file(&self, name: String, bytes: Vec<u8>) -> Result<CloudJob> {
        let name = name.trim().to_string();
        if bytes.is_empty() || bytes.len() > 16 * 1024 * 1024 {
            return Err(anyhow!("Choose a nonempty .torrent file under 16 MiB"));
        }
        let value = match self.provider {
            Provider::RealDebrid => {
                self.json(|| {
                    self.auth(self.client.put(self.url("torrents/addTorrent")))
                        .header("Content-Type", "application/x-bittorrent")
                        .body(bytes.clone())
                })
                .await?
            }
            Provider::TorBox => {
                self.json(|| {
                    self.auth(self.client.post(self.url("torrents/createtorrent")))
                        .multipart(
                            reqwest::multipart::Form::new()
                                .part(
                                    "file",
                                    reqwest::multipart::Part::bytes(bytes.clone())
                                        .file_name(name.clone())
                                        // A constant MIME type cannot fail,
                                        // and its error type cannot cross
                                        // the retry closure.
                                        .mime_str("application/x-bittorrent")
                                        .expect("constant MIME type"),
                                )
                                .text("seed", "1"),
                        )
                })
                .await?
            }
            // AllDebrid takes the metadata as a multipart file.
            Provider::AllDebrid => {
                self.json(|| {
                    self.auth(self.client.post(self.url("magnet/upload/file")))
                        .multipart(
                            reqwest::multipart::Form::new().part(
                                "files",
                                reqwest::multipart::Part::bytes(bytes.clone())
                                    .file_name(name.clone())
                                    .mime_str("application/x-bittorrent")
                                    .expect("constant MIME type"),
                            ),
                        )
                })
                .await?
            }
            // Debrid-Link takes the metadata as a multipart file too.
            Provider::DebridLink => {
                self.json(|| {
                    self.auth(self.client.post(self.url("seedbox/add")))
                        .multipart(reqwest::multipart::Form::new().part(
                            "file",
                            reqwest::multipart::Part::bytes(bytes.clone())
                                .file_name(name.clone())
                                .mime_str("application/x-bittorrent")
                                .expect("constant MIME type"),
                        ))
                })
                .await?
            }
            // Premiumize's API documents magnet links and host URLs, not
            // uploaded torrent metadata. The panel says so instead of
            // pretending it worked.
            Provider::Premiumize => {
                return Err(anyhow!(
                    "{}",
                    Provider::Premiumize
                        .unsupported_reason(Workflow::TorrentMetadata)
                        .unwrap_or("This provider does not accept .torrent metadata")
                ));
            }
        };
        let id = match self.provider {
            Provider::RealDebrid => value.get("id"),
            Provider::TorBox => value.pointer("/data/torrent_id"),
            Provider::AllDebrid => value.pointer("/data/files/0/id"),
            Provider::Premiumize => value.get("id"),
            Provider::DebridLink => value.pointer("/value/id"),
        }
        .ok_or_else(|| anyhow!("Provider did not return a torrent task ID"))?;
        let id = id
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| id.to_string());
        if !safe_id(&id) {
            return Err(anyhow!("Invalid torrent task ID"));
        }
        Ok(CloudJob {
            provider: self.provider,
            id,
            kind: Kind::Torrent,
            source: None,
            cached: None,
        })
    }

    /// Hand a hosting link to the provider.
    ///
    /// Real-Debrid resolves supported hosts to direct downloads right
    /// away; TorBox creates a hosting task that is polled like a torrent
    /// task.
    pub async fn hosting(
        &self,
        link: &str,
        password: &str,
    ) -> Result<(Option<CloudJob>, Vec<HostedFile>)> {
        let link = link.trim();
        let u = url::Url::parse(link)?;
        if !matches!(u.scheme(), "http" | "https")
            || !u.username().is_empty()
            || u.password().is_some()
        {
            return Err(anyhow!("Enter an HTTP or HTTPS hosting link"));
        }
        match self.provider {
            Provider::RealDebrid => {
                let value = self
                    .json(|| {
                        self.auth(self.client.post(self.url("unrestrict/link")))
                            .form(&[("link", link), ("password", password)])
                    })
                    .await?;
                let values = if let Some(values) = value.as_array() {
                    values.clone()
                } else {
                    vec![value]
                };
                let mut found = Vec::new();
                for value in values {
                    let text = value["download"]
                        .as_str()
                        .ok_or_else(|| anyhow!("Host did not provide a downloadable file"))?;
                    self.checked_link(text)?;
                    found.push(HostedFile {
                        url: text.into(),
                        name: value["filename"].as_str().unwrap_or("download").into(),
                        connections: value["chunks"]
                            .as_u64()
                            .filter(|n| *n > 0)
                            .map(|n| (n as usize).clamp(1, 16)),
                    });
                }
                Ok((None, found))
            }
            Provider::TorBox => {
                let value = self
                    .json(|| {
                        self.auth(self.client.post(self.url("webdl/createwebdownload")))
                            .form(&[("link", link), ("password", password)])
                    })
                    .await?;
                let id = value
                    .pointer("/data/webdownload_id")
                    .or_else(|| value.pointer("/data/web_id"))
                    .ok_or_else(|| anyhow!("Provider did not return a hosting task ID"))?;
                let id = id
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| id.to_string());
                if !id.chars().all(|c| c.is_ascii_digit()) {
                    return Err(anyhow!("Invalid hosting task ID"));
                }
                Ok((
                    Some(CloudJob {
                        provider: self.provider,
                        id,
                        kind: Kind::Hosting,
                        source: Some(link.to_string()),
                        cached: None,
                    }),
                    Vec::new(),
                ))
            }
            // AllDebrid unlocks a supported host straight away and hands
            // back the file itself.
            Provider::AllDebrid => {
                let value = self
                    .json(|| {
                        self.auth(self.client.post(self.url("link/unlock")))
                            .form(&[("link", link), ("password", password)])
                    })
                    .await?;
                let data = value.get("data").unwrap_or(&value);
                if data.get("delayed").and_then(Value::as_bool) == Some(true) {
                    return Err(anyhow!(
                        "AllDebrid is still preparing that link — try again in a moment"
                    ));
                }
                let text = data
                    .get("link")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!("Host did not provide a downloadable file"))?;
                self.checked_link(text)?;
                Ok((
                    None,
                    vec![HostedFile {
                        url: text.into(),
                        name: data
                            .get("filename")
                            .and_then(Value::as_str)
                            .unwrap_or("download")
                            .into(),
                        connections: None,
                    }],
                ))
            }
            // Premiumize resolves a supported host into a direct link. A
            // host that is not an instant one is queued as a transfer
            // instead, which is what its service list documents.
            Provider::Premiumize => {
                // The instant call takes no password, so a password on the
                // link goes straight to the queued call that does.
                let direct = if password.trim().is_empty() {
                    self.json(|| {
                        self.auth(self.client.post(self.url("transfer/directdl")))
                            .form(&[("src", link)])
                    })
                    .await
                } else {
                    Err(anyhow!("a password needs the queued call"))
                };
                let direct = match direct {
                    Ok(value) => {
                        let content = value
                            .get("content")
                            .and_then(Value::as_array)
                            .cloned()
                            .unwrap_or_default();
                        if content.is_empty() {
                            None
                        } else {
                            let mut found = Vec::new();
                            for item in &content {
                                let text = item
                                    .get("link")
                                    .and_then(Value::as_str)
                                    .ok_or_else(|| {
                                        anyhow!("Host did not provide a downloadable file")
                                    })?;
                                self.checked_link(text)?;
                                found.push(HostedFile {
                                    url: text.into(),
                                    name: file_name_of(item.get("path").and_then(Value::as_str)),
                                    connections: None,
                                });
                            }
                            Some(found)
                        }
                    }
                    Err(_) => None,
                };
                if let Some(found) = direct {
                    return Ok((None, found));
                }
                // The host cannot be resolved at once: queue it instead.
                let value = self
                    .json(|| {
                        self.auth(self.client.post(self.url("transfer/create")))
                            .form(&[("src", link), ("password", password)])
                    })
                    .await?;
                let id = value
                    .get("id")
                    .ok_or_else(|| anyhow!("Provider did not return a hosting task ID"))?;
                let id = id
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| id.to_string());
                if !safe_id(&id) {
                    return Err(anyhow!("Invalid hosting task ID"));
                }
                Ok((
                    Some(CloudJob {
                        provider: self.provider,
                        id,
                        kind: Kind::Hosting,
                        source: Some(link.to_string()),
                        cached: None,
                    }),
                    Vec::new(),
                ))
            }
            // Debrid-Link keeps a downloader record for the host link, so
            // the link is polled and can expire.
            Provider::DebridLink => {
                let value = self
                    .json(|| {
                        self.auth(self.client.post(self.url("downloader/add")))
                            .form(&[("url", link), ("password", password)])
                    })
                    .await?;
                let value = value.get("value").cloned().unwrap_or(value);
                let records = if let Some(rows) = value.as_array() {
                    rows.clone()
                } else {
                    vec![value]
                };
                // A folder link comes back as one record per file. Those
                // are handed out as direct links, so none of them is lost;
                // a single link becomes a record that can be polled and
                // removed.
                if records.len() > 1 {
                    let mut found = Vec::new();
                    for record in &records {
                        let text = record
                            .get("downloadUrl")
                            .and_then(Value::as_str)
                            .ok_or_else(|| anyhow!("Host did not provide a downloadable file"))?;
                        self.checked_link(text)?;
                        found.push(HostedFile {
                            url: text.into(),
                            name: record["name"].as_str().unwrap_or("download").into(),
                            connections: None,
                        });
                    }
                    return Ok((None, found));
                }
                let first = records
                    .first()
                    .ok_or_else(|| anyhow!("Provider did not return a hosting task ID"))?;
                let id = first
                    .get("id")
                    .and_then(|v| v.as_str().map(str::to_string).or_else(|| {
                        v.as_i64().map(|n| n.to_string())
                    }))
                    .ok_or_else(|| anyhow!("Provider did not return a hosting task ID"))?;
                if !safe_id(&id) {
                    return Err(anyhow!("Invalid hosting task ID"));
                }
                Ok((
                    Some(CloudJob {
                        provider: self.provider,
                        id,
                        kind: Kind::Hosting,
                        source: Some(link.to_string()),
                        cached: None,
                    }),
                    Vec::new(),
                ))
            }
        }
    }
    /// The provider's current list of supported hosts.
    pub async fn hosters(&self) -> Result<String> {
        // Premiumize lists services by name, not hosts, and splits them by
        // what can be done with them.
        if self.provider == Provider::Premiumize {
            let value = self
                .json(|| self.auth(self.client.get(self.url("services/list"))))
                .await?;
            let mut lines = Vec::new();
            for (label, key) in [
                ("Instant direct links", "directdl"),
                ("Queued into the cloud", "queue"),
                ("Already cached", "cache"),
            ] {
                let names = value
                    .get(key)
                    .and_then(Value::as_array)
                    .map(|rows| {
                        rows.iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                if !names.is_empty() {
                    lines.push(format!("{label}: {names}"));
                }
            }
            if lines.is_empty() {
                return Err(anyhow!("Provider returned an unknown host list format"));
            }
            return Ok(lines.join("\n\n"));
        }
        let path = match self.provider {
            Provider::RealDebrid => "hosts/domains",
            Provider::TorBox => "webdl/hosters",
            Provider::AllDebrid => "hosts",
            Provider::DebridLink => "downloader/hosts",
            Provider::Premiumize => "services/list",
        };
        let value = self
            .json(|| self.auth(self.client.get(self.url(path))))
            .await?;
        let body = match self.provider {
            Provider::TorBox | Provider::AllDebrid => value.get("data").unwrap_or(&value),
            Provider::DebridLink => value.get("value").unwrap_or(&value),
            Provider::RealDebrid | Provider::Premiumize => &value,
        };
        let rows = body
            .as_array()
            .ok_or_else(|| anyhow!("Provider returned an unknown host list format"))?;
        let mut lines = Vec::new();
        for row in rows.iter().take(500) {
            if let Some(domain) = row.as_str() {
                lines.push(domain.to_owned());
                continue;
            }
            let name = row["name"].as_str().unwrap_or("Host");
            // AllDebrid reports a boolean, Debrid-Link a number that counts
            // up while the host is online.
            let status = match row["status"].as_bool() {
                Some(flag) => flag,
                None => row["status"].as_i64().map(|n| n >= 1).unwrap_or(false),
            };
            let status = if status {
                "available"
            } else {
                "unavailable"
            };
            let domains = row["domains"]
                .as_array()
                .map(|d| {
                    d.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            let note = row["note"].as_str().unwrap_or("");
            lines.push(format!(
                "{name} — {status}\n{domains}{}",
                if note.is_empty() {
                    String::new()
                } else {
                    format!("\n{note}")
                }
            ));
        }
        Ok(lines.join("\n\n"))
    }

    /// Accept a provider-generated download address.
    ///
    /// Only HTTPS, no credentials, and never an address that exposes the
    /// API token.
    fn checked_link(&self, text: &str) -> Result<String> {
        let u = url::Url::parse(text).map_err(|_| anyhow!("Invalid cloud download address"))?;
        if u.scheme() != "https"
            || !u.username().is_empty()
            || u.password().is_some()
            || text.contains(&self.token)
        {
            return Err(anyhow!("Provider returned an unsafe download address"));
        }
        Ok(text.into())
    }

    /// Select every file of a torrent job (Real-Debrid only).
    pub async fn select_all(&self, job: &CloudJob) -> Result<()> {
        if self.provider == Provider::RealDebrid && job.kind == Kind::Torrent {
            self.json(|| {
                self.auth(
                    self.client
                        .post(self.url(&format!("torrents/selectFiles/{}", job.id))),
                )
                .form(&[("files", "all")])
            })
            .await?;
        }
        Ok(())
    }

    /// The current status and file list of a cloud task.
    pub async fn list(&self, job: &CloudJob) -> Result<Listing> {
        self.list_with(job, false).await
    }

    /// The current status of a cloud task, bypassing the provider's own
    /// cached answer.
    ///
    /// TorBox caches its torrent list; a status refresh must not read
    /// that cache, or a finished job looks unchanged. Real-Debrid has no
    /// such cache and answers the same either way.
    pub async fn list_fresh(&self, job: &CloudJob) -> Result<Listing> {
        self.list_with(job, true).await
    }

    async fn list_with(&self, job: &CloudJob, fresh: bool) -> Result<Listing> {
        match self.provider {
            Provider::RealDebrid => {
                let value = self
                    .json(|| {
                        self.auth(
                            self.client
                                .get(self.url(&format!("torrents/info/{}", job.id))),
                        )
                    })
                    .await?;
                parse_listing(self.provider, &value)
            }
            Provider::TorBox => {
                let path = if job.kind == Kind::Hosting {
                    "webdl/mylist"
                } else {
                    "torrents/mylist"
                };
                let value = self
                    .json(|| {
                        let mut request = self
                            .auth(self.client.get(self.url(path)))
                            .query(&[("id", job.id.as_str())]);
                        if fresh {
                            request = request.query(&[("bypass_cache", "true")]);
                        }
                        request
                    })
                    .await?;
                parse_listing(self.provider, &value)
            }
            Provider::AllDebrid => self.list_alldebrid(job).await,
            Provider::Premiumize => self.list_premiumize(job).await,
            Provider::DebridLink => self.list_debridlink(job).await,
        }
    }

    /// AllDebrid's own status and file list for a magnet.
    ///
    /// The status comes from `POST /v4.1/magnet/status` and the direct
    /// links from `POST /v4/magnet/files`: the status answer carries the
    /// file tree, the files answer carries the tree with the links on it.
    async fn list_alldebrid(&self, job: &CloudJob) -> Result<Listing> {
        let status = self
            .json(|| {
                self.auth(self.client.post(self.url_at(
                    "https://api.alldebrid.com/v4.1/",
                    "magnet/status",
                )))
                .form(&[("id", job.id.as_str())])
            })
            .await?;
        let magnet = status
            .pointer("/data/magnets/0")
            .cloned()
            .unwrap_or(Value::Null);
        let word = match magnet.get("statusCode").and_then(Value::as_i64) {
            // AllDebrid's status code is the documented answer; its plain
            // English word is only used when no code came with it.
            Some(code) => alldebrid_status_word(code),
            None => magnet
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("Waiting for cloud files")
                .to_string(),
        };
        let mut files = Vec::new();
        // The links live in their own endpoint. A provider that cannot
        // hand them out yet still has a status to show.
        if let Ok(answer) = self
            .json(|| {
                self.auth(self.client.post(self.url("magnet/files")))
                    .form(&[("id[]", job.id.as_str())])
            })
            .await
        {
            if let Some(nodes) = answer.pointer("/data/magnets/0/files").and_then(Value::as_array) {
                let mut next = 0u64;
                flatten_tree(nodes, "", &mut files, &mut next);
            }
        }
        Ok(Listing {
            state: normalize_status(self.provider, &word),
            status: word,
            files,
        })
    }

    /// Premiumize's own transfer status and file list.
    ///
    /// The status comes from `GET /transfer/list` and the files from
    /// `POST /transfer/directdl`, which resolves the transfer's source
    /// into one entry per file. The links are only asked for once the
    /// transfer is finished, so a queued transfer is not paid for twice.
    async fn list_premiumize(&self, job: &CloudJob) -> Result<Listing> {
        let list = self
            .json(|| self.auth(self.client.get(self.url("transfer/list"))))
            .await?;
        let entry = list
            .get("transfers")
            .and_then(Value::as_array)
            .and_then(|rows| {
                rows.iter()
                    .find(|t| t["id"].as_str() == Some(job.id.as_str()))
            })
            .cloned()
            .unwrap_or(Value::Null);
        let word = entry
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("Waiting for cloud files")
            .to_string();
        let mut files = Vec::new();
        let finished = matches!(word.as_str(), "finished" | "seeding");
        if finished {
            if let Some(source) = job.source.as_deref() {
                if let Ok(answer) = self
                    .json(|| {
                        self.auth(self.client.post(self.url("transfer/directdl")))
                            .form(&[("src", source)])
                    })
                    .await
                {
                    if let Some(content) = answer.get("content").and_then(Value::as_array) {
                        for (n, item) in content.iter().enumerate() {
                            files.push(CloudFile {
                                id: n as u64 + 1,
                                name: file_name_of(item.get("path").and_then(Value::as_str)),
                                bytes: item.get("size").and_then(Value::as_u64).unwrap_or(0),
                                selected: false,
                                expired: false,
                                link: item
                                    .get("link")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_string(),
                            });
                        }
                    }
                }
            }
        }
        Ok(Listing {
            state: normalize_status(self.provider, &word),
            status: word,
            files,
        })
    }

    /// Debrid-Link's own torrent or downloader record.
    ///
    /// A torrent is read from `GET /seedbox/list?ids=…` and a host link
    /// from `GET /downloader/list`. Debrid-Link reports a status number
    /// rather than a word, so the number is turned into the word the
    /// provider's documentation uses.
    async fn list_debridlink(&self, job: &CloudJob) -> Result<Listing> {
        let (value, entry) = match job.kind {
            Kind::Torrent => {
                let value = self
                    .json(|| {
                        self.auth(self.client.get(self.url(&format!(
                            "seedbox/list?ids={}",
                            job.id
                        ))))
                    })
                    .await?;
                let entry = value
                    .pointer("/value/0")
                    .cloned()
                    .unwrap_or(Value::Null);
                (value, entry)
            }
            Kind::Hosting => {
                let value = self
                    .json(|| self.auth(self.client.get(self.url("downloader/list"))))
                    .await?;
                let entry = value
                    .pointer("/value")
                    .and_then(Value::as_array)
                    .and_then(|rows| {
                        rows.iter()
                            .find(|r| r["id"].as_str() == Some(job.id.as_str()))
                    })
                    .cloned()
                    .unwrap_or(Value::Null);
                (value, entry)
            }
        };
        let _ = value;
        let word = match job.kind {
            Kind::Torrent => debridlink_torrent_status(entry.get("status").and_then(Value::as_i64)),
            Kind::Hosting => {
                if entry.get("isProcessing").and_then(Value::as_bool) == Some(true) {
                    "processing".to_string()
                } else {
                    "ready".to_string()
                }
            }
        };
        let mut files = Vec::new();
        match job.kind {
            Kind::Torrent => {
                if let Some(rows) = entry.get("files").and_then(Value::as_array) {
                    for row in rows {
                        let id = row["id"].as_u64().unwrap_or(0);
                        files.push(CloudFile {
                            id,
                            name: row["name"].as_str().unwrap_or("file").to_string(),
                            bytes: row["size"].as_u64().unwrap_or(0),
                            selected: false,
                            expired: false,
                            link: row["downloadUrl"].as_str().unwrap_or("").to_string(),
                        });
                    }
                }
            }
            Kind::Hosting => {
                files.push(CloudFile {
                    id: 1,
                    name: entry["name"].as_str().unwrap_or("download").to_string(),
                    bytes: entry["size"].as_u64().unwrap_or(0),
                    selected: false,
                    expired: entry["expired"].as_bool().unwrap_or(false),
                    link: entry["downloadUrl"].as_str().unwrap_or("").to_string(),
                });
            }
        }
        Ok(Listing {
            state: normalize_status(self.provider, &word),
            status: word,
            files,
        })
    }

    /// The direct download address of one cloud file.
    pub async fn link(&self, job: &CloudJob, file: &CloudFile) -> Result<(String, Option<usize>)> {
        let link = self.link_info(job, file).await?;
        Ok((link.url, link.connections))
    }

    /// The direct download address of one cloud file, with its lifetime.
    ///
    /// The provider's own expiry is used when it sends one; otherwise the
    /// provider's default lifetime applies. A link never contains the API
    /// token.
    pub async fn link_info(&self, job: &CloudJob, file: &CloudFile) -> Result<DirectLink> {
        let value = match self.provider {
            Provider::RealDebrid => {
                if file.link.is_empty() {
                    return Err(anyhow!("Cloud file is not ready"));
                }
                self.json(|| {
                    self.auth(self.client.post(self.url("unrestrict/link")))
                        .form(&[("link", file.link.as_str())])
                })
                .await?
            }
            Provider::TorBox => {
                self.json(|| {
                    self.client
                        .get(self.url(if job.kind == Kind::Hosting {
                            "webdl/requestdl"
                        } else {
                            "torrents/requestdl"
                        }))
                        .query(&[
                            ("token", self.token.clone()),
                            (
                                if job.kind == Kind::Hosting {
                                    "web_id"
                                } else {
                                    "torrent_id"
                                },
                                job.id.clone(),
                            ),
                            ("file_id", file.id.to_string()),
                            ("redirect", "false".into()),
                        ])
                })
                .await?
            }
            // AllDebrid, Premiumize and Debrid-Link hand the download
            // address out with the file itself, so there is nothing left
            // to ask for.
            Provider::AllDebrid | Provider::Premiumize | Provider::DebridLink => {
                if file.expired {
                    return Err(anyhow!(
                        "the provider's link for this file has expired — submit it again"
                    ));
                }
                if file.link.is_empty() {
                    return Err(anyhow!("Cloud file is not ready for download"));
                }
                let url = self.checked_link(file.link.as_str())?;
                let now = now_seconds();
                return Ok(DirectLink {
                    url,
                    connections: None,
                    expires_at: None,
                    generated_at: now,
                    lifetime_s: self.link_lifetime(),
                });
            }
        };
        let text = match self.provider {
            Provider::RealDebrid => value.get("download"),
            Provider::TorBox => value.get("data"),
            Provider::AllDebrid | Provider::Premiumize | Provider::DebridLink => {
                value.get("link")
            }
        }
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Cloud file is not ready for download"))?;
        let url = self.checked_link(text)?;
        let connections = value["chunks"]
            .as_u64()
            .filter(|n| *n > 0)
            .map(|n| (n as usize).clamp(1, 16));
        let now = now_seconds();
        Ok(DirectLink {
            url,
            connections,
            expires_at: link_expiry(&value),
            generated_at: now,
            lifetime_s: self.link_lifetime(),
        })
    }

    /// Ask the provider for a fresh address for one cloud file.
    ///
    /// This is the safe refresh: the provider is asked again for the
    /// original file, and the stale address is never reused.
    pub async fn refresh_link(&self, job: &CloudJob, file: &CloudFile) -> Result<DirectLink> {
        self.link_info(job, file).await
    }

    /// Delete a cloud task at the provider.
    ///
    /// Real-Debrid removes a torrent from its list; TorBox controls the
    /// torrent and the hosting task separately.
    pub async fn delete_job(&self, job: &CloudJob) -> Result<()> {
        match self.provider {
            Provider::RealDebrid => match job.kind {
                Kind::Torrent => {
                    self.json(|| {
                        self.auth(self.client.delete(self.url(&format!(
                            "torrents/delete/{}",
                            job.id
                        ))))
                    })
                    .await?;
                }
                Kind::Hosting => {
                    return Err(anyhow!(
                        "Real-Debrid resolves hosting links directly; there is no hosting job to delete"
                    ));
                }
            },
            Provider::TorBox => {
                let id = job
                    .id
                    .parse::<u64>()
                    .map_err(|_| anyhow!("Provider task ID is not a number"))?;
                match job.kind {
                    Kind::Torrent => {
                        self.json(|| {
                            self.auth(self.client.post(self.url("torrents/controltorrent")))
                                .json(&json!({ "operation": "delete", "torrent_id": id }))
                        })
                        .await?;
                    }
                    Kind::Hosting => {
                        self.json(|| {
                            self.auth(self.client.post(self.url("webdl/controlwebdownload")))
                                .json(&json!({ "operation": "delete", "webdl_id": id }))
                        })
                        .await?;
                    }
                }
            }
            // AllDebrid removes the magnet from its list.
            Provider::AllDebrid => {
                self.json(|| {
                    self.auth(self.client.post(self.url("magnet/delete")))
                        .form(&[("id", job.id.as_str())])
                })
                .await?;
            }
            // Premiumize deletes the transfer record.
            Provider::Premiumize => {
                self.json(|| {
                    self.auth(self.client.post(self.url("transfer/delete")))
                        .form(&[("id", job.id.as_str())])
                })
                .await?;
            }
            // Debrid-Link removes the torrent or the downloader record.
            Provider::DebridLink => {
                let path = match job.kind {
                    Kind::Torrent => format!("seedbox/{}/remove", job.id),
                    Kind::Hosting => format!("downloader/{}/remove", job.id),
                };
                self.json(|| self.auth(self.client.delete(self.url(&path))))
                    .await?;
            }
        }
        Ok(())
    }

    /// Ask the provider whether it already holds a torrent.
    ///
    /// `expected_files` is the number of files the torrent is known to
    /// have: when the provider holds fewer, the torrent is only partly
    /// cached. Real-Debrid answers `GET /torrents/instantAvailability/{hash}`,
    /// TorBox answers `GET /torrents/checkcached`.
    pub async fn cache_status(
        &self,
        hash: &str,
        expected_files: Option<u64>,
    ) -> Result<CacheStatus> {
        let hash = hash.trim();
        if !is_torrent_hash(hash) {
            return Err(anyhow!("Enter a torrent info hash"));
        }
        let value = match self.provider {
            Provider::RealDebrid => {
                self.json(|| {
                    self.auth(self.client.get(self.url(&format!(
                        "torrents/instantAvailability/{hash}"
                    ))))
                })
                .await?
            }
            Provider::TorBox => {
                self.json(|| {
                    self.auth(self.client.get(self.url("torrents/checkcached")))
                        .query(&[
                            ("hash", hash),
                            ("format", "list"),
                            ("list_files", "true"),
                        ])
                })
                .await?
            }
            // The other three have no instant-availability query:
            // AllDebrid answers it when the torrent is submitted,
            // Premiumize's cache query takes links rather than hashes,
            // and Debrid-Link removed the endpoint.
            Provider::AllDebrid | Provider::Premiumize | Provider::DebridLink => {
                return Err(anyhow!(
                    "{}",
                    self.provider
                        .unsupported_reason(Workflow::CacheQuery)
                        .unwrap_or("This provider cannot say whether it holds the torrent")
                ));
            }
        };
        Ok(parse_cache(self.provider, hash, &value, expected_files))
    }

    /// The lifetime used when the provider does not state one.
    fn link_lifetime(&self) -> u64 {
        match self.provider {
            Provider::RealDebrid => REALDEBRID_LINK_LIFETIME_S,
            Provider::TorBox => TORBOX_LINK_LIFETIME_S,
            Provider::AllDebrid | Provider::Premiumize | Provider::DebridLink => {
                DEFAULT_LINK_LIFETIME_S
            }
        }
    }

    /// Read the account behind the token.
    ///
    /// Real-Debrid answers `GET /user`, TorBox answers `GET /user/me`,
    /// AllDebrid answers `GET /user`, Premiumize answers
    /// `GET /account/info` and Debrid-Link answers `GET /account/infos`.
    /// A wrong token is refused here, before any torrent is submitted.
    pub async fn account(&self) -> Result<Account> {
        let value = match self.provider {
            Provider::RealDebrid | Provider::AllDebrid => {
                self.json(|| self.auth(self.client.get(self.url("user"))))
                    .await?
            }
            Provider::TorBox => {
                self.json(|| self.auth(self.client.get(self.url("user/me"))))
                    .await?
            }
            Provider::Premiumize => {
                self.json(|| self.auth(self.client.get(self.url("account/info"))))
                    .await?
            }
            Provider::DebridLink => {
                self.json(|| self.auth(self.client.get(self.url("account/infos"))))
                    .await?
            }
        };
        let data = if self.provider == Provider::TorBox {
            value.get("data").unwrap_or(&value)
        } else {
            &value
        };
        let (username, plan, premium, expires_at) = match self.provider {
            Provider::RealDebrid => {
                let kind = data["type"].as_str().unwrap_or("").to_string();
                (
                    data["username"].as_str().unwrap_or("").to_string(),
                    kind.clone(),
                    kind.eq_ignore_ascii_case("premium"),
                    data["expiration"].as_str().and_then(iso_seconds),
                )
            }
            Provider::TorBox => {
                let plan = data["plan"].as_i64().unwrap_or(0);
                (
                    data["email"].as_str().unwrap_or("").to_string(),
                    torbox_plan_name(plan).to_string(),
                    plan > 0,
                    data["premium_expires_at"]
                        .as_str()
                        .and_then(iso_seconds),
                )
            }
            // AllDebrid reports the premium state and the moment it ends
            // as a Unix timestamp.
            Provider::AllDebrid => {
                let user = value.pointer("/data/user").unwrap_or(&value);
                let premium = user["isPremium"].as_bool().unwrap_or(false);
                let until = user["premiumUntil"].as_u64().unwrap_or(0);
                (
                    user["username"].as_str().unwrap_or("").to_string(),
                    if premium { "premium" } else { "free" }.to_string(),
                    premium,
                    if until > 0 { Some(until) } else { None },
                )
            }
            // Premiumize names the account by its customer id and states
            // the premium moment as a Unix timestamp.
            Provider::Premiumize => {
                let until = value["premium_until"].as_u64();
                let premium = until.map(|t| t > now_seconds()).unwrap_or(false);
                (
                    value["customer_id"].as_str().unwrap_or("").to_string(),
                    if premium { "premium" } else { "free" }.to_string(),
                    premium,
                    until,
                )
            }
            // Debrid-Link states the account kind and the seconds of
            // premium that are left.
            Provider::DebridLink => {
                let data = value.get("value").unwrap_or(&value);
                let kind = data["accountType"].as_i64().unwrap_or(0);
                let left = data["premiumLeft"].as_u64().unwrap_or(0);
                let name = data["username"]
                    .as_str()
                    .or_else(|| data["email"].as_str())
                    .unwrap_or("")
                    .to_string();
                let plan = match kind {
                    0 => "free",
                    1 => "premium",
                    2 => "lifetime premium",
                    _ => "unknown",
                };
                let expires = if kind == 1 && left > 0 {
                    Some(now_seconds().saturating_add(left))
                } else {
                    None
                };
                (name, plan.to_string(), kind >= 1, expires)
            }
        };
        if username.is_empty() && plan.is_empty() {
            return Err(anyhow!(
                "{} did not return an account for this token",
                self.provider.label()
            ));
        }
        Ok(Account {
            provider: self.provider,
            username,
            plan,
            premium,
            expires_at,
        })
    }

    /// Tell the provider which files of a torrent it should fetch.
    ///
    /// Real-Debrid selects files itself. TorBox has no per-file selection:
    /// its API downloads every file of a torrent, and this says so instead
    /// of pretending to select.
    pub async fn select_files(&self, job: &CloudJob, ids: &[u64]) -> Result<()> {
        match self.provider {
            Provider::RealDebrid => {
                if job.kind != Kind::Torrent {
                    return Err(anyhow!("Only torrent jobs have selectable files"));
                }
                let list = ids
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                if list.is_empty() {
                    return Err(anyhow!("Select at least one file"));
                }
                self.json(|| {
                    self.auth(
                        self.client
                            .post(self.url(&format!("torrents/selectFiles/{}", job.id))),
                    )
                    .form(&[("files", list.as_str())])
                })
                .await?;
                Ok(())
            }
            Provider::TorBox => Err(anyhow!(
                "TorBox downloads every file of a torrent; the provider offers no per-file selection"
            )),
            // AllDebrid and Premiumize fetch every file of a torrent too.
            Provider::AllDebrid | Provider::Premiumize => Err(anyhow!(
                "{}",
                self.provider
                    .unsupported_reason(Workflow::ProviderFileSelection)
                    .unwrap_or("This provider offers no per-file selection")
            )),
            // Debrid-Link selects by exclusion: it is told which files it
            // must not fetch.
            Provider::DebridLink => self.exclude_files(job, ids).await,
        }
    }

    /// Name the files the provider must not fetch.
    ///
    /// Debrid-Link's seedbox narrows a waiting torrent by exclusion: the
    /// provider is handed the ids it should skip. An empty list means
    /// every file is wanted.
    pub async fn exclude_files(&self, job: &CloudJob, unwanted: &[u64]) -> Result<()> {
        if self.provider != Provider::DebridLink {
            return Err(anyhow!("This provider does not select files by exclusion"));
        }
        if job.kind != Kind::Torrent {
            return Err(anyhow!("Only torrent jobs have selectable files"));
        }
        let list = unwanted
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        self.json(|| {
            self.auth(self.client.post(self.url(&format!(
                "seedbox/{}/config",
                job.id
            ))))
            .form(&[("files-unwanted", list.as_str())])
        })
        .await?;
        Ok(())
    }
}

/// The plan name TorBox reports for a plan number.
fn torbox_plan_name(plan: i64) -> &'static str {
    match plan {
        0 => "Free",
        1 => "Essential",
        2 => "Pro",
        3 => "Standard",
        _ => "Unknown",
    }
}

/// An ISO 8601 timestamp as Unix seconds.
fn iso_seconds(text: &str) -> Option<u64> {
    chrono::DateTime::parse_from_rfc3339(text.trim())
        .ok()
        .map(|d| d.timestamp().max(0) as u64)
}

/// Unix seconds, or 0 when the clock is before the epoch.
fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// One cloud file, resolved to a direct link and ready for the queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueTarget {
    /// The direct HTTPS address. It never carries the API token.
    pub url: String,
    /// The file name the provider reported.
    pub filename: String,
    /// The size the provider reported, when it did.
    pub size: u64,
    /// The provider's connection limit for this file, when it sent one.
    pub connections: Option<usize>,
    /// Unix seconds when the provider says the link stops working.
    pub expires_at: Option<u64>,
}

/// A cloud task the user is working with.
///
/// The session owns the provider client, so the token never leaves the
/// process and is never written to a download row.
pub struct CloudSession {
    api: Api,
    /// The provider task, when the work is one.
    job: Option<CloudJob>,
    /// The files of the task.
    files: Vec<CloudFile>,
    /// Hosting links the provider resolved without a task.
    hosted: Vec<HostedFile>,
    /// The provider's own status word.
    status: String,
    /// The same status, normalized.
    state: CloudStatus,
    /// The file ids the user picked.
    selected: Vec<u64>,
    /// What the provider already held for this torrent, when it answered.
    cache: Option<CacheStatus>,
}

impl CloudSession {
    /// Submit a magnet link and read the task's first state.
    ///
    /// The provider is asked first whether it already holds the torrent,
    /// so the panel can say whether the file will be ready at once or has
    /// to be fetched first. A provider that cannot answer does not stop
    /// the submission: the cache state stays unknown.
    pub async fn submit_torrent(api: Api, magnet: &str) -> Result<Self> {
        let cache = match magnet_hash(magnet) {
            Some(hash) if is_torrent_hash(&hash) => api.cache_status(&hash, None).await.ok(),
            _ => None,
        };
        let job = api.add(magnet).await?;
        // A provider that cannot be asked before the submission may still
        // answer with it: AllDebrid's `ready` flag says whether the
        // torrent is already available.
        let cache = match (cache, job.cached) {
            (Some(cache), _) => Some(cache),
            (None, Some(ready)) => Some(CacheStatus {
                hash: magnet_hash(magnet).unwrap_or_default(),
                state: if ready {
                    CacheState::Cached
                } else {
                    CacheState::Uncached
                },
                files: Vec::new(),
                bytes: 0,
            }),
            (None, None) => None,
        };
        let mut session = Self {
            api,
            job: Some(job),
            files: Vec::new(),
            hosted: Vec::new(),
            status: "Submitted".into(),
            state: CloudStatus::Queued,
            selected: Vec::new(),
            cache,
        };
        session.refresh().await?;
        Ok(session)
    }

    /// Submit `.torrent` metadata and read the task's first state.
    ///
    /// The provider fetches the torrent on our behalf; this app has no
    /// local torrent engine.
    pub async fn submit_torrent_file(api: Api, name: String, bytes: Vec<u8>) -> Result<Self> {
        let job = api.torrent_file(name, bytes).await?;
        let mut session = Self {
            api,
            job: Some(job),
            files: Vec::new(),
            hosted: Vec::new(),
            status: "Submitted".into(),
            state: CloudStatus::Queued,
            selected: Vec::new(),
            cache: None,
        };
        session.refresh().await?;
        Ok(session)
    }

    /// Submit a hosting link.
    ///
    /// Real-Debrid resolves a supported host right away; TorBox creates a
    /// hosting task that is polled like a torrent task.
    pub async fn submit_hosting(api: Api, link: &str, password: &str) -> Result<Self> {
        let (job, hosted) = api.hosting(link, password).await?;
        let mut session = Self {
            api,
            job,
            files: Vec::new(),
            hosted,
            status: "Hosting link submitted".into(),
            state: CloudStatus::Queued,
            selected: Vec::new(),
            cache: None,
        };
        session.refresh().await?;
        Ok(session)
    }

    /// Read the task's current state and file list again.
    pub async fn refresh(&mut self) -> Result<()> {
        let Some(job) = self.job.clone() else {
            // A resolved hosting link has no task to poll.
            self.state = CloudStatus::Ready;
            self.status = "Hosting link resolved".into();
            if self.selected.is_empty() {
                self.select_all();
            }
            return Ok(());
        };
        // The provider's own cached list is skipped: TorBox caches its
        // torrent list, so a refresh that reads the cache would report a
        // finished job as still running.
        let listing = self.api.list_fresh(&job).await?;
        self.files = listing.files;
        self.status = listing.status;
        self.state = listing.state;
        // A file that vanished must not stay selected.
        self.selected
            .retain(|id| self.files.iter().any(|f| f.id == *id));
        // A ready task selects its files on its own. A provider that
        // holds a torrent back until the files are named does the same,
        // so the user starts with every file picked.
        if self.selected.is_empty()
            && (self.state == CloudStatus::Ready
                || self.api.provider.waits_for_file_selection())
        {
            self.select_all();
        }
        Ok(())
    }

    /// The provider's files.
    pub fn files(&self) -> &[CloudFile] {
        &self.files
    }

    /// The hosting files the provider resolved right away.
    ///
    /// Real-Debrid answers a hosting link with the file itself, so there
    /// is no task and no file list: the resolved links live here.
    pub fn hosted_files(&self) -> &[HostedFile] {
        &self.hosted
    }

    /// The provider's own status word.
    pub fn status(&self) -> &str {
        &self.status
    }

    /// The normalized state.
    pub fn state(&self) -> CloudStatus {
        self.state
    }

    /// The file ids the user picked.
    pub fn selected(&self) -> &[u64] {
        &self.selected
    }

    /// Pick or unpick one file.
    pub fn toggle(&mut self, id: u64) {
        if let Some(at) = self.selected.iter().position(|x| *x == id) {
            self.selected.remove(at);
        } else {
            self.selected.push(id);
        }
    }

    /// Pick every file the provider reported.
    pub fn select_all(&mut self) {
        self.selected = if self.job.is_some() {
            self.files.iter().map(|f| f.id).collect()
        } else {
            (1..=self.hosted.len() as u64).collect()
        };
    }

    /// Pick nothing.
    pub fn select_none(&mut self) {
        self.selected.clear();
    }

    /// The provider task, when the work is one.
    pub fn job(&self) -> Option<&CloudJob> {
        self.job.as_ref()
    }

    /// What the provider already held for this torrent.
    ///
    /// `Cached` means the file can be had at once, `Uncached` means the
    /// provider has to fetch it first, `Partial` means it holds some of
    /// the files, and `Unknown` means the provider did not say.
    pub fn cache_state(&self) -> CacheState {
        self.cache.as_ref().map(|c| c.state).unwrap_or(CacheState::Unknown)
    }

    /// The provider's answer about the torrent, when it gave one.
    pub fn cache(&self) -> Option<&CacheStatus> {
        self.cache.as_ref()
    }

    /// Ask the provider for the direct links of the selected files.
    ///
    /// The result is what the queue downloads: ordinary HTTPS addresses
    /// that carry no provider credential.
    pub async fn resolve(&self) -> Result<Vec<QueueTarget>> {
        let mut out = Vec::new();
        if let Some(job) = &self.job {
            if self.selected.is_empty() {
                return Err(anyhow!("Select at least one cloud file"));
            }
            // Real-Debrid needs to know which files to fetch in the cloud
            // before it can hand out links.
            match self.api.provider {
                Provider::RealDebrid => {
                    self.api.select_files(job, &self.selected).await?;
                }
                // Debrid-Link selects by exclusion: it is told which files
                // it must not fetch.
                Provider::DebridLink if job.kind == Kind::Torrent => {
                    let unwanted: Vec<u64> = self
                        .files
                        .iter()
                        .map(|f| f.id)
                        .filter(|id| !self.selected.contains(id))
                        .collect();
                    self.api.exclude_files(job, &unwanted).await?;
                }
                _ => {}
            }
            for file in &self.files {
                if !self.selected.contains(&file.id) {
                    continue;
                }
                let link = self.api.link_info(job, file).await?;
                out.push(QueueTarget {
                    url: link.url,
                    filename: file.name.clone(),
                    size: file.bytes,
                    connections: link.connections,
                    expires_at: link.expires_at,
                });
            }
            return Ok(out);
        }
        // A resolved hosting link needs no further provider call.
        for (n, hosted) in self.hosted.iter().enumerate() {
            if !self.selected.contains(&(n as u64 + 1)) {
                continue;
            }
            out.push(QueueTarget {
                url: hosted.url.clone(),
                filename: hosted.name.clone(),
                size: 0,
                connections: hosted.connections,
                expires_at: None,
            });
        }
        Ok(out)
    }

    /// Delete the task at the provider.
    pub async fn cancel(&self) -> Result<()> {
        let Some(job) = &self.job else {
            // A resolved hosting link has nothing to cancel.
            return Ok(());
        };
        self.api.delete_job(job).await
    }
}

/// A torrent info hash: hex only, so it can never change the request
/// path or query.
fn is_torrent_hash(hash: &str) -> bool {
    (hash.len() == 40 || hash.len() == 64) && hash.chars().all(|c| c.is_ascii_hexdigit())
}

/// The torrent info hash inside a magnet link.
///
/// The hash is the one part of a magnet link the provider can be asked
/// about, so it is the only part the panel sends.
fn magnet_hash(magnet: &str) -> Option<String> {
    let u = url::Url::parse(magnet.trim()).ok()?;
    if u.scheme() != "magnet" {
        return None;
    }
    u.query_pairs()
        .find(|(k, v)| k == "xt" && v.starts_with("urn:btih:"))
        .map(|(_, v)| v["urn:btih:".len()..].to_string())
}

/// Read a bounded response body.
async fn read_body(response: &mut reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow!("Provider response interrupted"))?
    {
        if bytes.len() + chunk.len() > limit {
            return Err(anyhow!("Provider response too large"));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// The expiry a provider stated for a generated link, if any.
fn link_expiry(value: &Value) -> Option<u64> {
    for key in ["expires", "expiry", "expire"] {
        if let Some(at) = value.get(key).and_then(Value::as_u64) {
            // A provider may send seconds instead of an absolute time.
            return Some(if at < 1_000_000_000 {
                now_seconds().saturating_add(at)
            } else {
                at
            });
        }
    }
    value
        .get("expires_in")
        .and_then(Value::as_u64)
        .map(|secs| now_seconds().saturating_add(secs))
}

/// Read the file list and status out of a provider answer.
fn parse_listing(provider: Provider, value: &Value) -> Result<Listing> {
    let data = if provider == Provider::TorBox {
        let d = &value["data"];
        if d.is_array() {
            d.get(0).unwrap_or(&Value::Null)
        } else {
            d
        }
    } else {
        value
    };
    let status = if provider == Provider::TorBox {
        data["download_state"]
            .as_str()
            .unwrap_or("Waiting for cloud files")
    } else {
        data["status"].as_str().unwrap_or("Waiting for cloud files")
    }
    .to_string();
    let links = data["links"].as_array();
    let mut at = 0;
    let files = data["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter_map(|f| {
                    let selected =
                        provider == Provider::TorBox || f["selected"].as_u64() == Some(1);
                    let link = if selected && provider == Provider::RealDebrid {
                        let l = links
                            .and_then(|l| l.get(at))
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        at += 1;
                        l
                    } else {
                        String::new()
                    };
                    Some(CloudFile {
                        id: f["id"].as_u64()?,
                        name: f[if provider == Provider::TorBox {
                            "name"
                        } else {
                            "path"
                        }]
                        .as_str()?
                        .to_string(),
                        bytes: f[if provider == Provider::TorBox {
                            "size"
                        } else {
                            "bytes"
                        }]
                        .as_u64()
                        .unwrap_or(0),
                        selected: false,
                        expired: false,
                        link,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Listing {
        files,
        state: normalize_status(provider, &status),
        status,
    })
}

/// Map a provider's own status word onto one normalized state.
fn normalize_status(provider: Provider, raw: &str) -> CloudStatus {
    let word = raw.trim().to_ascii_lowercase();
    match (provider, word.as_str()) {
        (Provider::RealDebrid, "downloaded" | "finished" | "uploading" | "seeding") => {
            CloudStatus::Ready
        }
        (Provider::RealDebrid, "downloading" | "checking" | "checking_files") => {
            CloudStatus::Downloading
        }
        (Provider::RealDebrid, "queued" | "starting" | "magnet_conversion" | "paused") => {
            CloudStatus::Queued
        }
        (Provider::RealDebrid, "waiting_files_selection") => CloudStatus::Partial,
        (Provider::RealDebrid, "magnet_error" | "error" | "virus" | "dead") => CloudStatus::Failed,
        (Provider::TorBox, "cached" | "completed") => CloudStatus::Ready,
        (Provider::TorBox, "downloading" | "checking" | "checking_formatted" | "uploading") => {
            CloudStatus::Downloading
        }
        (Provider::TorBox, "queued" | "queued_wanted" | "magnet_processing" | "stalled") => {
            CloudStatus::Queued
        }
        (Provider::TorBox, "failed" | "timed_out") => CloudStatus::Failed,
        // AllDebrid reports a status code; the word comes from its own
        // table of codes.
        (Provider::AllDebrid, "ready") => CloudStatus::Ready,
        (Provider::AllDebrid, "downloading" | "compressing" | "uploading") => {
            CloudStatus::Downloading
        }
        (Provider::AllDebrid, "queued" | "processing") => CloudStatus::Queued,
        (Provider::AllDebrid, "error") => CloudStatus::Failed,
        // Premiumize names the states of a transfer.
        (Provider::Premiumize, "finished" | "seeding") => CloudStatus::Ready,
        (Provider::Premiumize, "running") => CloudStatus::Downloading,
        (Provider::Premiumize, "queued") => CloudStatus::Queued,
        (Provider::Premiumize, "error") => CloudStatus::Failed,
        // Debrid-Link reports a status number; the word comes from its own
        // table of numbers.
        (Provider::DebridLink, "finished" | "seeding" | "ready") => CloudStatus::Ready,
        (
            Provider::DebridLink,
            "downloading" | "verifying" | "processing",
        ) => CloudStatus::Downloading,
        (Provider::DebridLink, "queued" | "paused") => CloudStatus::Queued,
        _ => CloudStatus::Unknown,
    }
}

/// AllDebrid's own status code as the word its documentation uses.
fn alldebrid_status_word(code: i64) -> String {
    match code {
        0 => "Queued",
        1 => "Downloading",
        2 => "Compressing",
        3 => "Uploading",
        4 => "Ready",
        _ => "Error",
    }
    .to_string()
}

/// Debrid-Link's own torrent status number as its documented word.
fn debridlink_torrent_status(code: Option<i64>) -> String {
    match code {
        Some(0) => "Paused".to_string(),
        Some(1) => "Queued".to_string(),
        Some(2) => "Verifying".to_string(),
        Some(4) => "Downloading".to_string(),
        Some(8) => "Seeding".to_string(),
        Some(100) => "Finished".to_string(),
        Some(other) => format!("Status {other}"),
        None => "Waiting for cloud files".to_string(),
    }
}

/// A task ID that can never change the request path.
fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The file name a provider path ends with.
fn file_name_of(path: Option<&str>) -> String {
    let path = path.unwrap_or("").trim();
    let name = path.rsplit('/').next().unwrap_or("").trim();
    if name.is_empty() {
        "download".to_string()
    } else {
        name.to_string()
    }
}

/// Flatten a provider's file tree into a flat file list.
///
/// The tree nodes carry a name, a size and — when the provider gives
/// them — the direct link of the file. A node with children is a folder:
/// its own name becomes part of the path of the files below it.
fn flatten_tree(nodes: &[Value], prefix: &str, out: &mut Vec<CloudFile>, next_id: &mut u64) {
    for node in nodes {
        let name = node["n"].as_str().unwrap_or("file").to_string();
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if let Some(children) = node["e"].as_array() {
            flatten_tree(children, &path, out, next_id);
            continue;
        }
        *next_id += 1;
        out.push(CloudFile {
            id: *next_id,
            name: path,
            bytes: node["s"].as_u64().unwrap_or(0),
            selected: false,
            link: node["l"].as_str().unwrap_or("").to_string(),
            expired: false,
        });
    }
}

/// Read the provider's answer about a torrent hash.
fn parse_cache(
    provider: Provider,
    hash: &str,
    value: &Value,
    expected_files: Option<u64>,
) -> CacheStatus {
    let (files, verdict) = match provider {
        Provider::RealDebrid => {
            let entry = value.get(hash);
            match entry {
                // The provider listed the hash but did not say whether it
                // holds it: the answer is unusable.
                Some(entry) if entry.get("rd").is_none() => (Vec::new(), None),
                // The hash is not in the answer: the provider does not
                // hold it.
                None => (Vec::new(), Some(false)),
                Some(entry) => {
                    let variants = entry.get("rd").and_then(Value::as_array);
                    let mut files = Vec::new();
                    if let Some(variants) = variants {
                        // Real-Debrid lists the available files in its
                        // first variant; later variants are other seeds of
                        // the same torrent.
                        if let Some(first) = variants.first().and_then(Value::as_object) {
                            for (id, file) in first {
                                let id = id.parse::<u64>().ok();
                                let name = file
                                    .get("filename")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .to_string();
                                let bytes =
                                    file.get("filesize").and_then(Value::as_u64).unwrap_or(0);
                                if let Some(id) = id {
                                    files.push(CachedFile { id, name, bytes });
                                }
                            }
                        }
                    }
                    (files, Some(variants.map(|v| !v.is_empty()).unwrap_or(false)))
                }
            }
        }
        Provider::TorBox => {
            let data = value.get("data").unwrap_or(value);
            let entry = if data.is_array() {
                data.get(0).unwrap_or(&Value::Null)
            } else {
                data
            };
            let mut files = Vec::new();
            if let Some(rows) = entry.get("files").and_then(Value::as_array) {
                for file in rows {
                    let id = file.get("id").and_then(Value::as_u64).unwrap_or(0);
                    let name = file
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    let bytes = file.get("size").and_then(Value::as_u64).unwrap_or(0);
                    files.push(CachedFile { id, name, bytes });
                }
            }
            // The provider said nothing about the hash: the answer is
            // unusable, which is not the same as "not cached".
            if entry.get("cached").is_none() && entry.get("files").is_none() {
                (Vec::new(), None)
            } else {
                let cached = entry.get("cached").and_then(Value::as_bool).unwrap_or(false);
                (files, Some(cached))
            }
        }
        // These providers have no instant-availability query, so there is
        // no answer to read: the state stays unknown, which is not the
        // same as "not cached".
        Provider::AllDebrid | Provider::Premiumize | Provider::DebridLink => {
            (Vec::new(), None)
        }
    };
    let bytes = files.iter().map(|f| f.bytes).sum();
    let state = match verdict {
        Some(true) => {
            // The provider holds it, but not every file of the torrent.
            if let Some(expected) = expected_files {
                if expected > 0 && (files.len() as u64) < expected {
                    CacheState::Partial
                } else {
                    CacheState::Cached
                }
            } else {
                CacheState::Cached
            }
        }
        Some(false) => {
            if files.is_empty() {
                CacheState::Uncached
            } else {
                CacheState::Partial
            }
        }
        None => CacheState::Unknown,
    };
    CacheStatus {
        hash: hash.to_string(),
        state,
        files,
        bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A magnet link with a full info hash, so the provider's cache check
    /// runs against it.
    const MAGNET: &str =
        "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567";

    /// The canned provider answer for a request line.
    fn mock_payload(provider: Provider, path: &str) -> String {
        // Real-Debrid keys its availability answer by the hash, so the
        // answer has to carry the hash that was asked for.
        if let Some(rest) = path.strip_prefix("GET /torrents/instantAvailability/") {
            let hash = rest.split(['?', ' ']).next().unwrap_or("").to_string();
            return format!(
                r#"{{"{hash}":{{"rd":[{{"1":{{"filename":"file.bin","filesize":123}}}}]}}}}"#
            );
        }
        mock_static_payload(provider, path).to_string()
    }

    /// The canned answers that do not depend on the request.
    ///
    /// Every provider answers in its own envelope, so the fixture answers
    /// in the envelope of the provider it serves.
    fn mock_static_payload(provider: Provider, path: &str) -> &'static str {
        match provider {
            Provider::RealDebrid => {
                if path.contains("unrestrict/link") {
                    r#"{"download":"https://cdn.example.test/file.bin","filename":"file.bin"}"#
                } else if path.contains("/user") {
                    r#"{"username":"tester","type":"premium","expiration":"2032-06-06T04:42:42.000Z"}"#
                } else if path.contains("torrents/info/") {
                    r#"{"status":"downloaded","links":["https://real-debrid.com/d/ABC"],"files":[{"id":1,"path":"file.bin","bytes":123,"selected":1}]}"#
                } else if path.contains("addMagnet") || path.contains("addTorrent") {
                    r#"{"id":"rd123"}"#
                } else {
                    r#"{"success":true}"#
                }
            }
            Provider::TorBox => {
                if path.contains("user/me") {
                    r#"{"success":true,"data":{"email":"tester@example.test","plan":2,"premium_expires_at":"2051-02-18T04:08:43Z"}}"#
                } else if path.contains("createwebdownload") {
                    r#"{"success":true,"data":{"webdownload_id":42}}"#
                } else if path.contains("checkcached") {
                    r#"{"success":true,"data":{"cached":true,"files":[{"id":3,"name":"file.bin","size":123}]}}"#
                } else if path.contains("webdl/mylist") {
                    r#"{"success":true,"data":{"download_state":"cached","files":[{"id":3,"name":"file.bin","size":123}]}}"#
                } else if path.contains("torrents/mylist") {
                    // TorBox answers the torrent list with an array of torrents.
                    r#"{"success":true,"data":[{"id":71,"download_state":"cached","files":[{"id":3,"name":"file.bin","size":123}]}]}"#
                } else if path.contains("requestdl") {
                    r#"{"success":true,"data":"https://cdn.example.test/file.bin"}"#
                } else if path.contains("createtorrent") {
                    r#"{"success":true,"data":{"torrent_id":71}}"#
                } else {
                    r#"{"success":true}"#
                }
            }
            Provider::AllDebrid => {
                if path.contains("link/unlock") {
                    r#"{"status":"success","data":{"link":"https://cdn.example.test/file.bin","filename":"file.bin","filesize":123,"host":"example.test"}}"#
                } else if path.contains("magnet/upload/file") {
                    r#"{"status":"success","data":{"files":[{"id":124,"name":"file.bin","size":123,"ready":true}]}}"#
                } else if path.contains("magnet/upload") {
                    r#"{"status":"success","data":{"magnets":[{"id":123,"name":"file.bin","hash":"abc","size":123,"ready":true}]}}"#
                } else if path.contains("magnet/status") {
                    r#"{"status":"success","data":{"magnets":[{"id":123,"filename":"file.bin","size":123,"status":"Ready","statusCode":4,"files":[{"n":"file.bin","s":123}]}]}}"#
                } else if path.contains("magnet/files") {
                    r#"{"status":"success","data":{"magnets":[{"id":123,"files":[{"n":"file.bin","s":123,"l":"https://cdn.example.test/file.bin"}]}]}}"#
                } else if path.contains("/user") {
                    r#"{"status":"success","data":{"user":{"username":"tester","email":"tester@example.test","isPremium":true,"premiumUntil":1893456000}}}"#
                } else {
                    r#"{"status":"success"}"#
                }
            }
            Provider::Premiumize => {
                if path.contains("transfer/directdl") {
                    r#"{"status":"success","content":[{"path":"Folder/file.bin","size":123,"link":"https://cdn.example.test/file.bin"}]}"#
                } else if path.contains("transfer/list") {
                    r#"{"status":"success","transfers":[{"id":"pm123","name":"file.bin","status":"finished","progress":1.0,"message":""}]}"#
                } else if path.contains("transfer/create") {
                    r#"{"status":"success","id":"pm123","name":"file.bin"}"#
                } else if path.contains("account/info") {
                    r#"{"status":"success","customer_id":"1234567","premium_until":1893456000,"limit_used":0.42}"#
                } else if path.contains("services/list") {
                    r#"{"status":"success","cache":["examplecache"],"directdl":["exampledirect"],"queue":["examplequeue"],"fairusefactor":{},"aliases":{},"regexpatterns":{}}"#
                } else {
                    r#"{"status":"success"}"#
                }
            }
            Provider::DebridLink => {
                if path.contains("seedbox/list") {
                    r#"{"success":true,"value":[{"id":"dl123","name":"file.bin","hashString":"abc","status":100,"files":[{"id":5,"name":"a.bin","size":123,"downloadUrl":"https://cdn.example.test/a.bin","downloadPercent":100},{"id":6,"name":"b.bin","size":124,"downloadUrl":"https://cdn.example.test/b.bin","downloadPercent":100}]}]}"#
                } else if path.contains("seedbox/add") {
                    r#"{"success":true,"value":{"id":"dl123","name":"file.bin","hashString":"abc","status":0}}"#
                } else if path.contains("downloader/list") {
                    r#"{"success":true,"value":[{"id":"dl9","name":"file.bin","size":123,"downloadUrl":"https://cdn.example.test/file.bin","expired":false}]}"#
                } else if path.contains("downloader/add") {
                    r#"{"success":true,"value":{"id":"dl9","name":"file.bin","size":123,"downloadUrl":"https://cdn.example.test/file.bin","expired":false}}"#
                } else if path.contains("account/infos") {
                    r#"{"success":true,"value":{"email":"tester@example.test","username":"tester","accountType":1,"premiumLeft":86400}}"#
                } else {
                    r#"{"success":true,"value":{}}"#
                }
            }
        }
    }

    /// A local provider fixture: records the raw requests and answers
    /// with canned provider payloads. No provider account is contacted.
    async fn mock(
        provider: Provider,
    ) -> (
        Api,
        std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let (api, requests, task) = mock_serving(provider, 0).await;
        (api, requests, task)
    }

    /// The same fixture, but it answers `429 Too Many Requests` for the
    /// first `limits` requests before serving normally.
    async fn mock_serving(
        provider: Provider,
        limits: usize,
    ) -> (
        Api,
        std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        tokio::task::JoinHandle<()>,
    ) {
        mock_with(provider, limits, Vec::new()).await
    }

    /// The same fixture, but one route answers with the provider's own
    /// error body instead of the normal answer.
    ///
    /// The route is matched on the request line, so a fragment like
    /// `"addMagnet"` picks the torrent submission.
    async fn mock_refusing(
        provider: Provider,
        route: &str,
        status: &str,
        body: &str,
    ) -> (
        Api,
        std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        tokio::task::JoinHandle<()>,
    ) {
        mock_with(
            provider,
            0,
            vec![(route.to_string(), status.to_string(), body.to_string())],
        )
        .await
    }

    /// The fixture itself: a provider answer for every request, with
    /// optional overrides for specific routes.
    async fn mock_with(
        provider: Provider,
        limits: usize,
        overrides: Vec<(String, String, String)>,
    ) -> (
        Api,
        std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let record = requests.clone();
        let limits = std::sync::Arc::new(std::sync::Mutex::new(limits));
        let overrides = std::sync::Arc::new(overrides);
        let task = tokio::spawn(async move {
            // The provider is copied into the fixture so every answer
            // comes in that provider's own envelope.
            let provider = provider;
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let record = record.clone();
                let limits = limits.clone();
                let overrides = overrides.clone();
                tokio::spawn(async move {
                    let mut bytes = Vec::new();
                    let mut b = [0; 1];
                    while bytes.len() < 65536 {
                        if socket.read_exact(&mut b).await.is_err() {
                            return;
                        }
                        bytes.push(b[0]);
                        if bytes.ends_with(b"\r\n\r\n") {
                            break;
                        }
                    }
                    let headers = String::from_utf8_lossy(&bytes).into_owned();
                    let len = headers
                        .lines()
                        .find_map(|l| {
                            l.split_once(':')
                                .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                                .and_then(|(_, v)| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    let mut body = vec![0; len];
                    if len > 0 {
                        if socket.read_exact(&mut body).await.is_err() {
                            return;
                        }
                    }
                    bytes.extend_from_slice(&body);
                    let text = String::from_utf8_lossy(&bytes).into_owned();
                    record.lock().unwrap().push(text.clone());
                    let path = headers.lines().next().unwrap_or("");
                    let limited = {
                        let mut remaining = limits.lock().unwrap();
                        if *remaining > 0 {
                            *remaining -= 1;
                            true
                        } else {
                            false
                        }
                    };
                    if limited {
                        let reply = "HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                        let _ = socket.write_all(reply.as_bytes()).await;
                        return;
                    }
                    // Real-Debrid answers a delete with 204 and no body.
                    if headers.lines().next().unwrap_or("").starts_with("DELETE ") {
                        let reply = "HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                        let _ = socket.write_all(reply.as_bytes()).await;
                        return;
                    }
                    // A route the test wants to answer differently.
                    let override_answer = overrides
                        .iter()
                        .find(|(route, _, _)| path.contains(route))
                        .map(|(_, status, body)| (status.to_string(), body.clone()));
                    if let Some((status, body)) = override_answer {
                        let reply = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                        let _ = socket.write_all(reply.as_bytes()).await;
                        return;
                    }
                    let payload = mock_payload(provider, path);
                    let reply=format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",payload.len(),payload);
                    let _ = socket.write_all(reply.as_bytes()).await;
                });
            }
        });
        let mut api = Api::new(provider, "private-api-token".into()).unwrap();
        api.test_base = Some(base);
        (api, requests, task)
    }

    #[tokio::test]
    async fn hosting_flows_preserve_mega_keys_and_use_correct_provider_endpoints() {
        let (api, requests, task) = mock(Provider::RealDebrid).await;
        let (job, files) = api
            .hosting("https://mega.nz/file/abc#decryption-key", "file-password")
            .await
            .unwrap();
        assert!(job.is_none());
        assert_eq!(files[0].url, "https://cdn.example.test/file.bin");
        let text = requests.lock().unwrap()[0].clone();
        assert!(text.contains("authorization: Bearer private-api-token"));
        assert!(text.contains("%23decryption-key"));
        assert!(text.contains("password=file-password"));
        task.abort();
        let (api, requests, task) = mock(Provider::TorBox).await;
        let (job, files) = api
            .hosting("https://mega.nz/file/abc#key", "")
            .await
            .unwrap();
        assert!(files.is_empty());
        let job = job.unwrap();
        assert!(job.kind == Kind::Hosting);
        let listing = api.list(&job).await.unwrap();
        let (link, _) = api.link(&job, &listing.files[0]).await.unwrap();
        assert_eq!(link, "https://cdn.example.test/file.bin");
        let requests = requests.lock().unwrap();
        assert!(requests[1].starts_with("GET /webdl/mylist?id=42 "));
        assert!(requests[2].contains("web_id=42"));
        assert!(!requests[2].contains("torrent_id="));
        assert!(!link.contains("private-api-token"));
        task.abort();
    }

    #[tokio::test]
    async fn cloud_magnets_are_submitted_to_each_provider_without_local_swarm_code() {
        for provider in [Provider::RealDebrid, Provider::TorBox] {
            let (api, requests, task) = mock(provider).await;
            let job = api
                .add("magnet:?xt=urn:btih:0123456789abcdef")
                .await
                .unwrap();
            assert!(job.kind == Kind::Torrent);
            let request = requests.lock().unwrap()[0].clone();
            assert!(request.contains("authorization: Bearer private-api-token"));
            if provider == Provider::TorBox {
                assert!(request.contains("multipart/form-data"));
                assert!(request.contains("name=\"magnet\""));
            } else {
                assert!(request.contains("magnet=magnet%3A"));
            }
            task.abort();
        }
    }

    #[tokio::test]
    async fn torrent_metadata_upload_uses_raw_put_or_multipart_as_documented() {
        for provider in [Provider::RealDebrid, Provider::TorBox] {
            let (api, requests, task) = mock(provider).await;
            api.torrent_file("example.torrent".into(), b"d4:infodee".to_vec())
                .await
                .unwrap();
            let request = requests.lock().unwrap()[0].clone();
            if provider == Provider::RealDebrid {
                assert!(request.starts_with("PUT /torrents/addTorrent "));
            } else {
                assert!(request.contains("name=\"file\""));
                assert!(request.contains("filename=\"example.torrent\""));
            }
            assert!(request.contains("d4:infodee"));
            task.abort();
        }
    }

    #[test]
    fn generated_downloads_reject_token_leaks_and_plaintext_links() {
        let api = Api::new(Provider::TorBox, "private-token".into()).unwrap();
        assert!(api.checked_link("http://cdn.example.test/file").is_err());
        assert!(api
            .checked_link("https://cdn.example.test/file?token=private-token")
            .is_err());
        assert!(api
            .checked_link("https://user:pass@cdn.example.test/file")
            .is_err());
    }

    #[test]
    fn links_follow_selected_files_only() {
        let j = serde_json::json!({"status":"downloaded","links":["https://cdn/a","https://cdn/c"],"files":[{"id":1,"path":"a","selected":1},{"id":2,"path":"b","selected":0},{"id":3,"path":"c","selected":1}]});
        let l = parse_listing(Provider::RealDebrid, &j).unwrap();
        assert_eq!(l.files[2].link, "https://cdn/c");
        assert!(l.files[1].link.is_empty());
    }

    #[test]
    fn torbox_list_can_be_an_object_or_array() {
        for data in [
            serde_json::json!({"files":[{"id":9,"name":"a","size":42}]}),
            serde_json::json!([{"files":[{"id":9,"name":"a","size":42}]}]),
        ] {
            let l = parse_listing(Provider::TorBox, &serde_json::json!({"data":data})).unwrap();
            assert_eq!(l.files[0].bytes, 42);
        }
    }

    // ---- Phase B ------------------------------------------------------

    #[test]
    fn submitted_magnets_and_hosting_links_are_trimmed() {
        let api = Api::new(Provider::RealDebrid, "private-api-token".into()).unwrap();
        // A magnet with surrounding whitespace still parses, and the
        // trimmed value is what goes to the provider.
        let magnet = "  magnet:?xt=urn:btih:0123456789abcdef \n";
        assert!(url::Url::parse(magnet.trim()).is_ok());
        assert_eq!(magnet.trim(), "magnet:?xt=urn:btih:0123456789abcdef");
        // A hosting link keeps its MEGA fragment, minus the whitespace.
        let link = "  https://mega.nz/file/abc#decryption-key  ";
        let parsed = url::Url::parse(link.trim()).unwrap();
        assert_eq!(parsed.fragment(), Some("decryption-key"));
        assert_eq!(api.provider, Provider::RealDebrid);
    }

    #[tokio::test]
    async fn submitted_values_reach_the_provider_without_whitespace() {
        let (api, requests, task) = mock(Provider::RealDebrid).await;
        api.add("  magnet:?xt=urn:btih:0123456789abcdef  ")
            .await
            .unwrap();
        let request = requests.lock().unwrap()[0].clone();
        // The body is exactly the trimmed magnet: no padding, no stray
        // space, no second field.
        let body = request.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
        assert_eq!(
            body,
            "magnet=magnet%3A%3Fxt%3Durn%3Abtih%3A0123456789abcdef"
        );
        task.abort();
    }

    #[test]
    fn cache_states_cover_cached_uncached_partial_and_unknown() {
        let hash = "0123456789abcdef0123456789abcdef01234567";
        // Real-Debrid: the hash is listed with files -> cached.
        let rd = serde_json::json!({
            hash: {"rd": [{"1": {"filename": "a.mkv", "filesize": 10}, "2": {"filename": "b.mkv", "filesize": 20}}]}
        });
        let status = parse_cache(Provider::RealDebrid, hash, &rd, Some(2));
        assert_eq!(status.state, CacheState::Cached);
        assert_eq!(status.files.len(), 2);
        assert_eq!(status.bytes, 30);
        // Fewer files than expected -> only partly cached.
        let status = parse_cache(Provider::RealDebrid, hash, &rd, Some(5));
        assert_eq!(status.state, CacheState::Partial);
        // The hash is not in the answer -> not cached.
        let empty = serde_json::json!({});
        let status = parse_cache(Provider::RealDebrid, hash, &empty, None);
        assert_eq!(status.state, CacheState::Uncached);
        // TorBox says so itself.
        let tb = serde_json::json!({"data": {"cached": true, "files": [{"id": 1, "name": "a", "size": 5}]}});
        let status = parse_cache(Provider::TorBox, hash, &tb, Some(1));
        assert_eq!(status.state, CacheState::Cached);
        let tb = serde_json::json!({"data": {"cached": false, "files": [{"id": 1, "name": "a", "size": 5}]}});
        let status = parse_cache(Provider::TorBox, hash, &tb, None);
        assert_eq!(status.state, CacheState::Partial);
        let tb = serde_json::json!({"data": {"cached": false, "files": []}});
        let status = parse_cache(Provider::TorBox, hash, &tb, None);
        assert_eq!(status.state, CacheState::Uncached);
        // A provider that does not answer the question is unknown, which is
        // not the same as "not cached".
        let silent = serde_json::json!({hash: {"other": []}});
        let status = parse_cache(Provider::RealDebrid, hash, &silent, None);
        assert_eq!(status.state, CacheState::Unknown);
        let silent = serde_json::json!({"data": {}});
        let status = parse_cache(Provider::TorBox, hash, &silent, None);
        assert_eq!(status.state, CacheState::Unknown);
        assert_eq!(status.state.label(), "unknown");
        assert_eq!(CacheState::Unknown.label(), "unknown");
        assert_eq!(CacheState::Partial.label(), "partial");
    }

    #[tokio::test]
    async fn cache_checks_use_the_documented_provider_endpoints() {
        for provider in [Provider::RealDebrid, Provider::TorBox] {
            let (api, requests, task) = mock(provider).await;
            let hash = "0123456789abcdef0123456789abcdef01234567";
            // The mock answers a generic success payload, so the state is
            // unknown, but the endpoint must be the provider's own.
            let status = api.cache_status(hash, None).await.unwrap();
            assert!(matches!(
                status.state,
                CacheState::Unknown | CacheState::Uncached | CacheState::Cached
            ));
            let request = requests.lock().unwrap()[0].clone();
            match provider {
                Provider::RealDebrid => assert!(
                    request.starts_with("GET /torrents/instantAvailability/0123456789abcdef"),
                    "wrong endpoint: {request}"
                ),
                Provider::TorBox => {
                    assert!(request.starts_with("GET /torrents/checkcached?"), "wrong: {request}");
                    assert!(request.contains("hash=0123456789abcdef"));
                    assert!(request.contains("list_files=true"));
                }
                // The other providers have no such query at all.
                Provider::AllDebrid | Provider::Premiumize | Provider::DebridLink => {
                    unreachable!("this provider has no cache query");
                }
            }
            task.abort();
        }
    }

    #[tokio::test]
    async fn cache_checks_refuse_a_hash_that_is_not_hex() {
        let (api, _requests, task) = mock(Provider::RealDebrid).await;
        // A hash with a path separator must never reach the request path.
        let err = api
            .cache_status("../../../torrents/delete/1", None)
            .await
            .expect_err("a non-hex hash must be refused");
        assert!(err.to_string().contains("info hash"));
        task.abort();
    }

    #[tokio::test]
    async fn jobs_can_be_deleted_at_the_provider() {
        let (api, requests, task) = mock(Provider::RealDebrid).await;
        let job = CloudJob {
            provider: Provider::RealDebrid,
            id: "rd123".into(),
            kind: Kind::Torrent,
            source: None,
            cached: None,
        };
        api.delete_job(&job).await.unwrap();
        let request = requests.lock().unwrap()[0].clone();
        assert!(
            request.starts_with("DELETE /torrents/delete/rd123 "),
            "wrong endpoint: {request}"
        );
        task.abort();
        let (api, requests, task) = mock(Provider::TorBox).await;
        let job = CloudJob {
            provider: Provider::TorBox,
            id: "71".into(),
            kind: Kind::Torrent,
            source: None,
            cached: None,
        };
        api.delete_job(&job).await.unwrap();
        let request = requests.lock().unwrap()[0].clone();
        assert!(
            request.starts_with("POST /torrents/controltorrent "),
            "wrong endpoint: {request}"
        );
        assert!(request.contains("\"operation\":\"delete\""));
        assert!(request.contains("\"torrent_id\":71"));
        task.abort();
    }

    #[tokio::test]
    async fn hosting_jobs_use_their_own_control_endpoint() {
        let (api, requests, task) = mock(Provider::TorBox).await;
        let job = CloudJob {
            provider: Provider::TorBox,
            id: "42".into(),
            kind: Kind::Hosting,
            source: None,
            cached: None,
        };
        api.delete_job(&job).await.unwrap();
        let request = requests.lock().unwrap()[0].clone();
        assert!(
            request.starts_with("POST /webdl/controlwebdownload "),
            "wrong endpoint: {request}"
        );
        assert!(request.contains("\"webdl_id\":42"));
        task.abort();
        // Real-Debrid resolves hosting links directly: there is no job.
        let (api, _requests, task) = mock(Provider::RealDebrid).await;
        let job = CloudJob {
            provider: Provider::RealDebrid,
            id: "rd123".into(),
            kind: Kind::Hosting,
            source: None,
            cached: None,
        };
        assert!(api.delete_job(&job).await.is_err());
        task.abort();
    }

    #[tokio::test]
    async fn rate_limited_requests_are_retried_with_a_backoff() {
        let (mut api, requests, task) = mock_serving(Provider::RealDebrid, 2).await;
        api.backoff = Duration::from_millis(1);
        let job = CloudJob {
            provider: Provider::RealDebrid,
            id: "rd123".into(),
            kind: Kind::Torrent,
            source: None,
            cached: None,
        };
        // Two rate-limited answers, then the real one: the call succeeds.
        api.list(&job).await.expect("the request must be retried");
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 3, "expected two retries");
        task.abort();
    }

    #[tokio::test]
    async fn rate_limited_requests_give_up_after_a_bounded_number_of_retries() {
        let (mut api, requests, task) = mock_serving(Provider::RealDebrid, 99).await;
        api.backoff = Duration::from_millis(1);
        let job = CloudJob {
            provider: Provider::RealDebrid,
            id: "rd123".into(),
            kind: Kind::Torrent,
            source: None,
            cached: None,
        };
        let err = api.list(&job).await.expect_err("the retries must be bounded");
        assert!(err.to_string().contains("429"));
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 4, "one attempt plus three retries");
        task.abort();
    }

    #[test]
    fn provider_errors_carry_their_own_code() {
        let api = Api::new(Provider::RealDebrid, "private-api-token".into()).unwrap();
        let rd = serde_json::json!({"error": "bad token", "error_code": 8});
        let err = api.provider_error(
            reqwest::StatusCode::UNAUTHORIZED,
            Some(&rd),
        );
        assert_eq!(err.code.as_deref(), Some("8"));
        let text = err.to_string();
        assert!(text.contains("Real-Debrid"));
        assert!(text.contains("8"));
        assert!(text.contains("bad token"));
        // The documented meaning is spelled out for the user.
        assert_eq!(realdebrid_error_text("34"), Some("too many requests"));
        assert_eq!(realdebrid_error_text("16"), Some("unsupported hoster"));
        assert_eq!(realdebrid_error_text("999"), None);
    }

    #[test]
    fn the_backoff_honours_retry_after_but_stays_bounded() {
        let api = Api::new(Provider::RealDebrid, "private-api-token".into()).unwrap();
        let mut headers = reqwest::header::HeaderMap::new();
        // A provider that says when to come back wins...
        headers.insert(
            reqwest::header::RETRY_AFTER,
            reqwest::header::HeaderValue::from_static("5"),
        );
        assert_eq!(api.backoff_for(0, &headers), Duration::from_secs(5));
        // ...but only up to the cap: a broken header must not stall the app.
        headers.insert(
            reqwest::header::RETRY_AFTER,
            reqwest::header::HeaderValue::from_static("3600"),
        );
        assert_eq!(api.backoff_for(0, &headers), RATE_BACKOFF_MAX);
        // Without a header the wait grows with every attempt.
        let empty = reqwest::header::HeaderMap::new();
        assert_eq!(api.backoff_for(0, &empty), Duration::from_millis(500));
        assert_eq!(api.backoff_for(1, &empty), Duration::from_millis(1000));
        assert_eq!(api.backoff_for(2, &empty), Duration::from_millis(2000));
    }

    #[tokio::test]
    async fn provider_failures_report_the_provider_code() {
        // A provider that answers with its own error body.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    use tokio::io::AsyncReadExt;
                    let mut bytes = Vec::new();
                    let mut b = [0; 1];
                    while bytes.len() < 65536 {
                        if socket.read_exact(&mut b).await.is_err() {
                            return;
                        }
                        bytes.push(b[0]);
                        if bytes.ends_with(b"\r\n\r\n") {
                            break;
                        }
                    }
                    let payload = r#"{"error":"bad token","error_code":8}"#;
                    let reply = format!("HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", payload.len(), payload);
                    let _ = socket.write_all(reply.as_bytes()).await;
                });
            }
        });
        let mut api = Api::new(Provider::RealDebrid, "private-api-token".into()).unwrap();
        api.test_base = Some(base);
        let job = CloudJob {
            provider: Provider::RealDebrid,
            id: "rd123".into(),
            kind: Kind::Torrent,
            source: None,
            cached: None,
        };
        let err = api.list(&job).await.expect_err("the refusal must surface");
        assert_eq!(provider_code(&err), Some("8"));
        assert!(err.to_string().contains("bad token"));
        task.abort();
    }

    #[test]
    fn cloud_status_is_normalized_across_providers() {
        assert_eq!(
            normalize_status(Provider::RealDebrid, "downloaded"),
            CloudStatus::Ready
        );
        assert_eq!(
            normalize_status(Provider::RealDebrid, "waiting_files_selection"),
            CloudStatus::Partial
        );
        assert_eq!(
            normalize_status(Provider::RealDebrid, "downloading"),
            CloudStatus::Downloading
        );
        assert_eq!(
            normalize_status(Provider::RealDebrid, "magnet_error"),
            CloudStatus::Failed
        );
        assert_eq!(
            normalize_status(Provider::TorBox, "cached"),
            CloudStatus::Ready
        );
        assert_eq!(
            normalize_status(Provider::TorBox, "downloading"),
            CloudStatus::Downloading
        );
        assert_eq!(
            normalize_status(Provider::TorBox, "timed_out"),
            CloudStatus::Failed
        );
        assert_eq!(
            normalize_status(Provider::TorBox, "something new"),
            CloudStatus::Unknown
        );
        assert_eq!(CloudStatus::Ready.label(), "ready");
    }

    #[tokio::test]
    async fn a_status_refresh_bypasses_the_provider_cache() {
        let (api, requests, task) = mock(Provider::TorBox).await;
        let job = CloudJob {
            provider: Provider::TorBox,
            id: "71".into(),
            kind: Kind::Torrent,
            source: None,
            cached: None,
        };
        api.list(&job).await.unwrap();
        api.list_fresh(&job).await.unwrap();
        let requests = requests.lock().unwrap();
        assert!(requests[0].starts_with("GET /torrents/mylist?id=71 "));
        assert!(!requests[0].contains("bypass_cache"));
        assert!(requests[1].contains("bypass_cache=true"));
        task.abort();
    }

    #[tokio::test]
    async fn direct_links_carry_their_lifetime_and_can_be_refreshed() {
        let (api, requests, task) = mock(Provider::TorBox).await;
        let job = CloudJob {
            provider: Provider::TorBox,
            id: "71".into(),
            kind: Kind::Torrent,
            source: None,
            cached: None,
        };
        let file = CloudFile {
            id: 3,
            name: "file.bin".into(),
            bytes: 123,
            selected: true,
            link: String::new(),
            expired: false,
        };
        let link = api.link_info(&job, &file).await.unwrap();
        assert_eq!(link.url, "https://cdn.example.test/file.bin");
        assert!(!link.url.contains("private-api-token"));
        // The provider stated no expiry: the TorBox default applies.
        assert!(!link.expiry_known());
        assert_eq!(link.lifetime_s, TORBOX_LINK_LIFETIME_S);
        assert!(!link.is_expired(link.generated_at));
        assert!(link.needs_refresh(link.generated_at + TORBOX_LINK_LIFETIME_S));
        // A refresh asks the provider again; the old address is never
        // reused.
        let refreshed = api.refresh_link(&job, &file).await.unwrap();
        assert_eq!(refreshed.url, link.url);
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2, "the refresh must call the provider");
        assert!(requests[1].contains("requestdl"));
        task.abort();
    }

    #[test]
    fn a_provider_stated_expiry_wins_over_the_default_lifetime() {        let now = 1_700_000_000u64;
        let value = serde_json::json!({"download": "https://cdn.example.test/f", "expires": now + 60});
        let expiry = link_expiry(&value).unwrap();
        assert_eq!(expiry, now + 60);
        let link = DirectLink {
            url: "https://cdn.example.test/f".into(),
            connections: None,
            expires_at: Some(expiry),
            generated_at: now,
            lifetime_s: REALDEBRID_LINK_LIFETIME_S,
        };
        assert!(link.expiry_known());
        assert!(!link.is_expired(now + 59));
        assert!(link.is_expired(now + 60));
        assert_eq!(link.expires_in(now + 30), 30);
        // A relative expiry is read as seconds from now.
        let value = serde_json::json!({"expires_in": 120});
        assert!(link_expiry(&value).unwrap() >= now + 119);
    }

    // ---- Phase C: account, files, queue hand-off ------------------------

    #[tokio::test]
    async fn an_account_is_validated_before_anything_is_submitted() {
        let (api, requests, task) = mock(Provider::RealDebrid).await;
        let account = api.account().await.unwrap();
        assert_eq!(account.provider, Provider::RealDebrid);
        assert_eq!(account.username, "tester");
        assert_eq!(account.plan, "premium");
        assert!(account.premium);
        assert!(account.expires_at.is_some());
        assert!(account.summary().contains("tester"));
        let request = requests.lock().unwrap()[0].clone();
        assert!(request.starts_with("GET /user "));
        assert!(request.contains("authorization: Bearer private-api-token"));
        task.abort();
        let (api, requests, task) = mock(Provider::TorBox).await;
        let account = api.account().await.unwrap();
        assert_eq!(account.username, "tester@example.test");
        assert_eq!(account.plan, "Pro");
        assert!(account.premium);
        assert!(account.expires_at.is_some());
        let request = requests.lock().unwrap()[0].clone();
        assert!(request.starts_with("GET /user/me "));
        task.abort();
    }

    #[tokio::test]
    async fn file_selection_follows_what_the_provider_offers() {
        let (api, requests, task) = mock(Provider::RealDebrid).await;
        let job = CloudJob {
            provider: Provider::RealDebrid,
            id: "rd123".into(),
            kind: Kind::Torrent,
            source: None,
            cached: None,
        };
        api.select_files(&job, &[1, 3]).await.unwrap();
        let request = requests.lock().unwrap()[0].clone();
        assert!(
            request.starts_with("POST /torrents/selectFiles/rd123 "),
            "wrong endpoint: {request}"
        );
        assert!(request.contains("files=1%2C3"));
        task.abort();
        // TorBox downloads every file: it offers no per-file selection.
        let (api, _requests, task) = mock(Provider::TorBox).await;
        let job = CloudJob {
            provider: Provider::TorBox,
            id: "71".into(),
            kind: Kind::Torrent,
            source: None,
            cached: None,
        };
        let err = api
            .select_files(&job, &[1])
            .await
            .expect_err("TorBox has no per-file selection");
        assert!(err.to_string().contains("every file"));
        task.abort();
    }

    #[tokio::test]
    async fn a_torrent_session_runs_from_magnet_to_queue_targets() {
        let (api, requests, task) = mock(Provider::RealDebrid).await;
        let mut session = CloudSession::submit_torrent(api, MAGNET)
            .await
            .unwrap();
        // The task was submitted and then read back.
        assert!(session.job().is_some());
        assert_eq!(session.state(), CloudStatus::Ready);
        assert_eq!(session.files().len(), 1);
        assert_eq!(session.files()[0].name, "file.bin");
        // The provider said it already holds this torrent.
        assert_eq!(session.cache_state(), CacheState::Cached);
        // A ready task selects its files on its own.
        assert_eq!(session.selected(), &[1]);
        // The user picks one file.
        session.toggle(1);
        assert!(session.selected().is_empty());
        session.toggle(1);
        assert_eq!(session.selected(), &[1]);
        let targets = session.resolve().await.unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].filename, "file.bin");
        assert_eq!(targets[0].size, 123);
        assert!(targets[0].url.starts_with("https://"));
        assert!(!targets[0].url.contains("private-api-token"));
        let requests = requests.lock().unwrap();
        // The provider was told which files to fetch before the link.
        let select = requests
            .iter()
            .position(|r| r.starts_with("POST /torrents/selectFiles/"))
            .expect("the provider must be told which files to fetch");
        let link = requests
            .iter()
            .position(|r| r.contains("unrestrict/link"))
            .expect("the provider must hand out the link");
        assert!(select < link, "selection must come before the link");
        task.abort();
    }

    #[tokio::test]
    async fn a_hosting_session_resolves_without_a_provider_task() {
        let (api, _requests, task) = mock(Provider::RealDebrid).await;
        let session = CloudSession::submit_hosting(api, "https://mega.nz/file/abc#key", "pw")
            .await
            .unwrap();
        // Real-Debrid resolves the host right away: there is no task.
        assert!(session.job().is_none());
        assert_eq!(session.state(), CloudStatus::Ready);
        let targets = session.resolve().await.unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].url, "https://cdn.example.test/file.bin");
        assert!(!targets[0].url.contains("private-api-token"));
        task.abort();
    }

    #[tokio::test]
    async fn a_torbox_torrent_session_reads_its_own_list() {
        let (api, requests, task) = mock(Provider::TorBox).await;
        let session = CloudSession::submit_torrent(api, MAGNET).await.unwrap();
        assert_eq!(session.state(), CloudStatus::Ready);
        assert_eq!(session.files().len(), 1);
        assert_eq!(session.files()[0].id, 3);
        // TorBox answers the cache question with its own list.
        assert_eq!(session.cache_state(), CacheState::Cached);
        let targets = session.resolve().await.unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].url, "https://cdn.example.test/file.bin");
        let requests = requests.lock().unwrap();
        // TorBox has no selectFiles step.
        assert!(!requests
            .iter()
            .any(|r| r.contains("selectFiles")));
        assert!(requests.iter().any(|r| r.contains("requestdl")));
        task.abort();
    }

    #[tokio::test]
    async fn a_cloud_session_can_be_cancelled_at_the_provider() {
        let (api, requests, task) = mock(Provider::RealDebrid).await;
        let session = CloudSession::submit_torrent(api, MAGNET).await.unwrap();
        session.cancel().await.unwrap();
        let requests = requests.lock().unwrap();
        assert!(
            requests
                .iter()
                .any(|r| r.starts_with("DELETE /torrents/delete/rd123 ")),
            "the task must be deleted at the provider: {requests:?}"
        );
        task.abort();
    }

    #[tokio::test]
    async fn a_session_never_resolves_without_a_selection() {
        let (api, _requests, task) = mock(Provider::RealDebrid).await;
        let mut session = CloudSession::submit_torrent(api, MAGNET).await.unwrap();
        session.select_none();
        let err = session
            .resolve()
            .await
            .expect_err("nothing selected means nothing to resolve");
        assert!(err.to_string().contains("at least one"));
        task.abort();
    }

    /// A resolved cloud file must be something the queue accepts.
    ///
    /// The queue refuses anything that is not a download URL, so the
    /// provider's link has to pass that same gate — and it must never
    /// carry the provider credential.
    #[tokio::test]
    async fn resolved_cloud_targets_can_go_into_the_queue() {
        let (api, _requests, task) = mock(Provider::RealDebrid).await;
        let session = CloudSession::submit_torrent(api, MAGNET).await.unwrap();
        let targets = session.resolve().await.unwrap();
        assert_eq!(targets.len(), 1);
        for t in &targets {
            assert!(
                crate::model::is_download_url(&t.url),
                "the queue would refuse this link: {t:?}"
            );
            assert!(!t.url.contains("private-api-token"));
            assert!(!t.filename.is_empty());
        }
        task.abort();

        // A hosting link that the provider resolved right away is queued
        // the same way.
        let (api, _requests, task) = mock(Provider::RealDebrid).await;
        let session = CloudSession::submit_hosting(api, "https://mega.nz/file/abc#key", "pw")
            .await
            .unwrap();
        let targets = session.resolve().await.unwrap();
        assert_eq!(targets.len(), 1);
        assert!(crate::model::is_download_url(&targets[0].url));
        assert!(!targets[0].url.contains("private-api-token"));
        task.abort();
    }

    /// A link's lifetime has to reach the queue row, so the user can see
    /// that the address is temporary.
    #[tokio::test]
    async fn a_link_carries_its_expiry_to_the_queue() {
        let expiry = now_seconds() + 3600;
        let body = format!(
            r#"{{"download":"https://cdn.example.test/file.bin","filename":"file.bin","expires":{expiry}}}"#
        );
        let (api, _requests, task) =
            mock_refusing(Provider::RealDebrid, "unrestrict/link", "200 OK", &body).await;
        let session = CloudSession::submit_torrent(api, MAGNET).await.unwrap();
        let targets = session.resolve().await.unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].expires_at, Some(expiry));
        task.abort();
    }

    /// A rate-limited answer must not lose the submission: the request is
    /// sent again, and the second attempt carries a fresh body.
    #[tokio::test]
    async fn a_rate_limited_submission_still_reaches_the_provider() {
        // The first request is answered with 429; the retry is served.
        let (api, requests, task) = mock_serving(Provider::RealDebrid, 1).await;
        let session = CloudSession::submit_torrent(api, MAGNET)
            .await
            .expect("the submission must survive a rate limit");
        assert_eq!(session.state(), CloudStatus::Ready);
        let requests = requests.lock().unwrap();
        assert!(
            requests.iter().any(|r| r.contains("addMagnet")),
            "the torrent must still be submitted: {requests:?}"
        );
        task.abort();
    }

    /// A refusal must reach the user with the provider's own code, and
    /// without the credential or the request address.
    #[tokio::test]
    async fn a_refused_submission_names_the_provider_code() {
        let (api, _requests, task) = mock_refusing(
            Provider::RealDebrid,
            "addMagnet",
            "400 Bad Request",
            r#"{"error":"torrent too big","error_code":29}"#,
        )
        .await;
        let err = match CloudSession::submit_torrent(api, MAGNET).await {
            Ok(_) => panic!("the refusal must surface"),
            Err(e) => e,
        };
        assert_eq!(provider_code(&err), Some("29"));
        let text = err.to_string();
        assert!(text.contains("torrent too big"));
        assert!(!text.contains("private-api-token"));
        assert!(!text.contains("127.0.0.1"));
        task.abort();
    }

    /// A wrong token must be refused before anything is submitted.
    #[tokio::test]
    async fn a_wrong_token_is_refused_before_any_torrent_is_submitted() {
        let (api, requests, task) = mock_refusing(
            Provider::RealDebrid,
            "GET /user ",
            "401 Unauthorized",
            r#"{"error":"bad token","error_code":8}"#,
        )
        .await;
        let err = api
            .account()
            .await
            .expect_err("a wrong token must be refused");
        assert_eq!(provider_code(&err), Some("8"));
        let requests = requests.lock().unwrap();
        assert!(
            !requests.iter().any(|r| r.contains("addMagnet")),
            "nothing may be submitted with a refused token: {requests:?}"
        );
        task.abort();
    }

    /// The provider's answer about a torrent decides what the panel says.
    #[tokio::test]
    async fn cache_states_are_read_from_the_provider_answer() {
        let hash = "0123456789abcdef0123456789abcdef01234567";
        // The provider holds it: an instant download.
        let cached = format!(
            r#"{{"{hash}":{{"rd":[{{"1":{{"filename":"file.bin","filesize":123}}}}]}}}}"#
        );
        let (api, _requests, task) =
            mock_refusing(Provider::RealDebrid, "instantAvailability", "200 OK", &cached).await;
        let status = api.cache_status(hash, None).await.unwrap();
        assert_eq!(status.state, CacheState::Cached);
        assert_eq!(status.files.len(), 1);
        assert_eq!(status.bytes, 123);
        task.abort();

        // The provider does not list it: it has to fetch it first.
        let (api, _requests, task) =
            mock_refusing(Provider::RealDebrid, "instantAvailability", "200 OK", "{}").await;
        let status = api.cache_status(hash, None).await.unwrap();
        assert_eq!(status.state, CacheState::Uncached);
        task.abort();

        // The provider lists it but does not say what it holds: unusable.
        let listed = format!(r#"{{"{hash}":{{}}}}"#);
        let (api, _requests, task) =
            mock_refusing(Provider::RealDebrid, "instantAvailability", "200 OK", &listed).await;
        let status = api.cache_status(hash, None).await.unwrap();
        assert_eq!(status.state, CacheState::Unknown);
        task.abort();

        // TorBox says it holds the files but not the whole torrent.
        let (api, _requests, task) = mock_refusing(
            Provider::TorBox,
            "checkcached",
            "200 OK",
            r#"{"success":true,"data":{"cached":false,"files":[{"id":3,"name":"file.bin","size":123}]}}"#,
        )
        .await;
        let status = api.cache_status(hash, None).await.unwrap();
        assert_eq!(status.state, CacheState::Partial);
        task.abort();
    }

    // ---- The extra providers -------------------------------------------

    /// Every provider hands a magnet to its own documented endpoint.
    #[tokio::test]
    async fn each_provider_submits_a_magnet_to_its_own_endpoint() {
        let (api, requests, task) = mock(Provider::AllDebrid).await;
        let job = api.add(MAGNET).await.unwrap();
        assert!(job.kind == Kind::Torrent);
        let request = requests.lock().unwrap()[0].clone();
        assert!(
            request.starts_with("POST /magnet/upload "),
            "wrong endpoint: {request}"
        );
        assert!(request.contains("magnets%5B%5D=magnet%3A"));
        assert!(request.contains("authorization: Bearer private-api-token"));
        task.abort();

        let (api, requests, task) = mock(Provider::Premiumize).await;
        api.add(MAGNET).await.unwrap();
        let request = requests.lock().unwrap()[0].clone();
        assert!(
            request.starts_with("POST /transfer/create "),
            "wrong endpoint: {request}"
        );
        assert!(request.contains("src=magnet%3A"));
        task.abort();

        let (api, requests, task) = mock(Provider::DebridLink).await;
        api.add(MAGNET).await.unwrap();
        let request = requests.lock().unwrap()[0].clone();
        assert!(
            request.starts_with("POST /seedbox/add "),
            "wrong endpoint: {request}"
        );
        assert!(request.contains("url=magnet%3A"));
        task.abort();
    }

    /// Each provider validates the account at its own endpoint.
    #[tokio::test]
    async fn accounts_are_read_from_each_providers_own_endpoint() {
        let (api, requests, task) = mock(Provider::AllDebrid).await;
        let account = api.account().await.unwrap();
        assert_eq!(account.username, "tester");
        assert_eq!(account.plan, "premium");
        assert!(account.premium);
        assert!(account.expires_at.is_some());
        assert!(requests.lock().unwrap()[0].starts_with("GET /user "));
        task.abort();

        let (api, requests, task) = mock(Provider::Premiumize).await;
        let account = api.account().await.unwrap();
        assert_eq!(account.username, "1234567");
        assert!(account.premium);
        assert!(requests.lock().unwrap()[0].starts_with("GET /account/info "));
        task.abort();

        let (api, requests, task) = mock(Provider::DebridLink).await;
        let account = api.account().await.unwrap();
        assert_eq!(account.username, "tester");
        assert_eq!(account.plan, "premium");
        assert!(account.premium);
        assert!(requests.lock().unwrap()[0].starts_with("GET /account/infos "));
        task.abort();
    }

    /// A hosting link is resolved where the provider resolves it, and
    /// becomes a task where it does not.
    #[tokio::test]
    async fn hosting_links_follow_each_providers_own_flow() {
        let (api, requests, task) = mock(Provider::AllDebrid).await;
        let (job, files) = api.hosting("https://mega.nz/file/abc#key", "pw").await.unwrap();
        assert!(job.is_none());
        assert_eq!(files[0].url, "https://cdn.example.test/file.bin");
        assert_eq!(files[0].name, "file.bin");
        let request = requests.lock().unwrap()[0].clone();
        assert!(
            request.starts_with("POST /link/unlock "),
            "wrong endpoint: {request}"
        );
        assert!(request.contains("password=pw"));
        task.abort();

        let (api, requests, task) = mock(Provider::Premiumize).await;
        let (job, files) = api.hosting("https://mega.nz/file/abc#key", "").await.unwrap();
        assert!(job.is_none());
        assert_eq!(files[0].url, "https://cdn.example.test/file.bin");
        assert_eq!(files[0].name, "file.bin");
        assert!(
            requests.lock().unwrap()[0].starts_with("POST /transfer/directdl "),
            "wrong endpoint"
        );
        task.abort();

        let (api, requests, task) = mock(Provider::DebridLink).await;
        let (job, files) = api.hosting("https://mega.nz/file/abc#key", "pw").await.unwrap();
        assert!(files.is_empty());
        let job = job.expect("Debrid-Link keeps a record for the host link");
        assert!(job.kind == Kind::Hosting);
        let request = requests.lock().unwrap()[0].clone();
        assert!(
            request.starts_with("POST /downloader/add "),
            "wrong endpoint: {request}"
        );
        assert!(request.contains("password=pw"));
        task.abort();
    }

    /// A `.torrent` file is uploaded only where the API takes one.
    #[tokio::test]
    async fn torrent_metadata_is_uploaded_only_where_the_api_takes_it() {
        let (api, requests, task) = mock(Provider::AllDebrid).await;
        let job = api
            .torrent_file("example.torrent".into(), b"d4:infodee".to_vec())
            .await
            .unwrap();
        assert!(job.kind == Kind::Torrent);
        let request = requests.lock().unwrap()[0].clone();
        assert!(
            request.starts_with("POST /magnet/upload/file "),
            "wrong endpoint: {request}"
        );
        assert!(request.contains("d4:infodee"));
        task.abort();

        let (api, requests, task) = mock(Provider::DebridLink).await;
        api.torrent_file("example.torrent".into(), b"d4:infodee".to_vec())
            .await
            .unwrap();
        let request = requests.lock().unwrap()[0].clone();
        assert!(
            request.starts_with("POST /seedbox/add "),
            "wrong endpoint: {request}"
        );
        assert!(request.contains("name=\"file\""));
        assert!(request.contains("d4:infodee"));
        task.abort();

        // Premiumize's API takes magnet links and host URLs only.
        let (api, _requests, task) = mock(Provider::Premiumize).await;
        let err = api
            .torrent_file("example.torrent".into(), b"d4:infodee".to_vec())
            .await
            .expect_err("Premiumize takes no .torrent metadata");
        assert!(err.to_string().contains("magnet links"));
        task.abort();
    }

    /// A provider that cannot answer the cache question says why, and is
    /// not asked at all.
    #[tokio::test]
    async fn providers_without_a_cache_query_say_why() {
        let hash = "0123456789abcdef0123456789abcdef01234567";
        for (provider, needle) in [
            (Provider::AllDebrid, "instant-availability"),
            (Provider::Premiumize, "download links"),
            (Provider::DebridLink, "removed"),
        ] {
            let (api, requests, task) = mock(provider).await;
            let err = api
                .cache_status(hash, None)
                .await
                .expect_err("this provider has no cache query");
            assert!(err.to_string().contains(needle), "wrong reason: {err}");
            assert!(
                requests.lock().unwrap().is_empty(),
                "the provider must not be asked at all"
            );
            task.abort();
        }
    }

    /// AllDebrid answers the cache question with the submission.
    #[tokio::test]
    async fn alldebrid_says_with_the_submission_whether_the_torrent_is_ready() {
        let (api, requests, task) = mock(Provider::AllDebrid).await;
        let session = CloudSession::submit_torrent(api, MAGNET).await.unwrap();
        assert_eq!(session.cache_state(), CacheState::Cached);
        assert_eq!(session.state(), CloudStatus::Ready);
        assert_eq!(session.files().len(), 1);
        assert_eq!(session.files()[0].name, "file.bin");
        assert_eq!(session.files()[0].bytes, 123);
        let targets = session.resolve().await.unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].url, "https://cdn.example.test/file.bin");
        assert!(!targets[0].url.contains("private-api-token"));
        let requests = requests.lock().unwrap();
        // The status and the links come from the provider's own two
        // endpoints.
        assert!(requests.iter().any(|r| r.contains("magnet/status")));
        assert!(requests.iter().any(|r| r.contains("magnet/files")));
        task.abort();
    }

    /// Premiumize resolves a transfer's files from its source.
    #[tokio::test]
    async fn premiumize_reads_its_transfer_and_its_files() {
        let (api, requests, task) = mock(Provider::Premiumize).await;
        let session = CloudSession::submit_torrent(api, MAGNET).await.unwrap();
        assert_eq!(session.state(), CloudStatus::Ready);
        assert_eq!(session.files().len(), 1);
        assert_eq!(session.files()[0].name, "file.bin");
        assert_eq!(session.files()[0].bytes, 123);
        let targets = session.resolve().await.unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].url, "https://cdn.example.test/file.bin");
        let requests = requests.lock().unwrap();
        assert!(requests.iter().any(|r| r.starts_with("POST /transfer/create ")));
        assert!(requests.iter().any(|r| r.starts_with("GET /transfer/list ")));
        assert!(requests
            .iter()
            .any(|r| r.starts_with("POST /transfer/directdl ")));
        task.abort();
    }

    /// Debrid-Link holds a torrent back until its files are named, and is
    /// told which files it must skip.
    #[tokio::test]
    async fn a_debridlink_session_tells_the_provider_what_to_skip() {
        let (api, requests, task) = mock(Provider::DebridLink).await;
        let mut session = CloudSession::submit_torrent(api, MAGNET).await.unwrap();
        assert_eq!(session.files().len(), 2);
        // The torrent waits, so every file starts picked.
        assert_eq!(session.selected(), &[5, 6]);
        // The user keeps one file: the provider is told to skip the other.
        session.toggle(6);
        let targets = session.resolve().await.unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].filename, "a.bin");
        assert_eq!(targets[0].url, "https://cdn.example.test/a.bin");
        let requests = requests.lock().unwrap();
        let config = requests
            .iter()
            .find(|r| r.contains("/config"))
            .expect("the provider must be told which files to skip");
        assert!(config.contains("files-unwanted=6"), "wrong: {config}");
        task.abort();
    }

    /// A Debrid-Link host link is polled like a task and can be removed.
    #[tokio::test]
    async fn a_debridlink_host_link_is_polled_and_can_be_deleted() {
        let (api, requests, task) = mock(Provider::DebridLink).await;
        let session = CloudSession::submit_hosting(api, "https://mega.nz/file/abc#key", "pw")
            .await
            .unwrap();
        assert_eq!(session.state(), CloudStatus::Ready);
        assert_eq!(session.files().len(), 1);
        assert_eq!(session.files()[0].name, "file.bin");
        let targets = session.resolve().await.unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].url, "https://cdn.example.test/file.bin");
        session.cancel().await.unwrap();
        let requests = requests.lock().unwrap();
        assert!(requests
            .iter()
            .any(|r| r.starts_with("DELETE /downloader/dl9/remove ")));
        task.abort();
    }

    /// A provider's task is removed at the provider in its own way.
    #[tokio::test]
    async fn the_extra_providers_delete_their_own_tasks() {
        let (api, requests, task) = mock(Provider::AllDebrid).await;
        let job = CloudJob {
            provider: Provider::AllDebrid,
            id: "123".into(),
            kind: Kind::Torrent,
            source: None,
            cached: None,
        };
        api.delete_job(&job).await.unwrap();
        assert!(
            requests.lock().unwrap()[0].starts_with("POST /magnet/delete "),
            "wrong endpoint"
        );
        task.abort();

        let (api, requests, task) = mock(Provider::Premiumize).await;
        let job = CloudJob {
            provider: Provider::Premiumize,
            id: "pm123".into(),
            kind: Kind::Torrent,
            source: None,
            cached: None,
        };
        api.delete_job(&job).await.unwrap();
        assert!(
            requests.lock().unwrap()[0].starts_with("POST /transfer/delete "),
            "wrong endpoint"
        );
        task.abort();

        let (api, requests, task) = mock(Provider::DebridLink).await;
        let job = CloudJob {
            provider: Provider::DebridLink,
            id: "dl123".into(),
            kind: Kind::Torrent,
            source: None,
            cached: None,
        };
        api.delete_job(&job).await.unwrap();
        assert!(
            requests.lock().unwrap()[0].starts_with("DELETE /seedbox/dl123/remove "),
            "wrong endpoint"
        );
        task.abort();
    }

    /// The panel may only offer what a provider's API documents.
    #[test]
    fn capabilities_state_what_each_provider_can_do() {
        // The cache query exists only where the provider has one.
        for provider in [Provider::RealDebrid, Provider::TorBox] {
            assert!(provider
                .unsupported_reason(Workflow::CacheQuery)
                .is_none());
        }
        for provider in [
            Provider::AllDebrid,
            Provider::Premiumize,
            Provider::DebridLink,
        ] {
            let reason = provider
                .unsupported_reason(Workflow::CacheQuery)
                .expect("this provider has no cache query");
            assert!(reason.contains(provider.label()), "wrong reason: {reason}");
        }
        // A provider that fetches every file says so instead of pretending
        // it can narrow them.
        assert!(Provider::RealDebrid
            .unsupported_reason(Workflow::ProviderFileSelection)
            .is_none());
        for provider in [
            Provider::TorBox,
            Provider::AllDebrid,
            Provider::Premiumize,
        ] {
            assert!(provider
                .unsupported_reason(Workflow::ProviderFileSelection)
                .is_some());
        }
        // Premiumize takes no .torrent metadata; the others do.
        assert!(Provider::Premiumize
            .unsupported_reason(Workflow::TorrentMetadata)
            .is_some());
        for provider in [
            Provider::RealDebrid,
            Provider::TorBox,
            Provider::AllDebrid,
            Provider::DebridLink,
        ] {
            assert!(provider
                .unsupported_reason(Workflow::TorrentMetadata)
                .is_none());
        }
        // Every provider takes a host password.
        for provider in Provider::ALL {
            assert!(provider
                .unsupported_reason(Workflow::HostingPassword)
                .is_none());
        }
    }

    /// A folder link that comes back as several records keeps every file.
    #[tokio::test]
    async fn a_debridlink_folder_link_keeps_every_file() {
        let body = r#"{"success":true,"value":[{"id":"dl9","name":"a.bin","downloadUrl":"https://cdn.example.test/a.bin"},{"id":"dl10","name":"b.bin","downloadUrl":"https://cdn.example.test/b.bin"}]}"#;
        let (api, _requests, task) =
            mock_refusing(Provider::DebridLink, "downloader/add", "200 OK", body).await;
        let (job, files) = api.hosting("https://mega.nz/file/abc", "").await.unwrap();
        assert!(job.is_none());
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].name, "a.bin");
        assert_eq!(files[1].url, "https://cdn.example.test/b.bin");
        task.abort();
    }

    /// A host link with a password goes to the queued call, because the
    /// instant call takes no password.
    #[tokio::test]
    async fn a_premiumize_host_link_with_a_password_is_queued() {
        let (api, requests, task) = mock(Provider::Premiumize).await;
        let (job, files) = api.hosting("https://mega.nz/file/abc#key", "pw").await.unwrap();
        assert!(files.is_empty());
        let job = job.expect("the link is queued as a transfer");
        assert!(job.kind == Kind::Hosting);
        let request = requests.lock().unwrap()[0].clone();
        assert!(
            request.starts_with("POST /transfer/create "),
            "wrong endpoint: {request}"
        );
        assert!(request.contains("password=pw"));
        task.abort();
    }

    /// A provider's file tree becomes a flat list with its links.
    #[test]
    fn a_file_tree_is_flattened_into_the_files_it_holds() {        let tree = serde_json::json!([
            {"n": "Folder", "e": [
                {"n": "a.mkv", "s": 10, "l": "https://cdn.example.test/a.mkv"},
                {"n": "b.mkv", "s": 20, "l": "https://cdn.example.test/b.mkv"}
            ]},
            {"n": "c.mkv", "s": 30, "l": "https://cdn.example.test/c.mkv"}
        ]);
        let mut files = Vec::new();
        let mut next = 0;
        flatten_tree(tree.as_array().unwrap(), "", &mut files, &mut next);
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].name, "Folder/a.mkv");
        assert_eq!(files[0].bytes, 10);
        assert_eq!(files[1].id, 2);
        assert_eq!(files[2].name, "c.mkv");
        assert_eq!(files[2].link, "https://cdn.example.test/c.mkv");
    }

    /// The extra providers' states are normalized the same way.
    #[test]
    fn the_extra_providers_statuses_are_normalized() {
        assert_eq!(
            normalize_status(Provider::AllDebrid, "Ready"),
            CloudStatus::Ready
        );
        assert_eq!(
            normalize_status(Provider::AllDebrid, "Downloading"),
            CloudStatus::Downloading
        );
        assert_eq!(
            normalize_status(Provider::AllDebrid, "Error"),
            CloudStatus::Failed
        );
        assert_eq!(
            normalize_status(Provider::Premiumize, "finished"),
            CloudStatus::Ready
        );
        assert_eq!(
            normalize_status(Provider::Premiumize, "running"),
            CloudStatus::Downloading
        );
        assert_eq!(
            normalize_status(Provider::Premiumize, "error"),
            CloudStatus::Failed
        );
        assert_eq!(
            normalize_status(Provider::DebridLink, "Finished"),
            CloudStatus::Ready
        );
        assert_eq!(
            normalize_status(Provider::DebridLink, "Seeding"),
            CloudStatus::Ready
        );
        assert_eq!(
            normalize_status(Provider::DebridLink, "Downloading"),
            CloudStatus::Downloading
        );
        assert_eq!(
            normalize_status(Provider::DebridLink, "Paused"),
            CloudStatus::Queued
        );
        // The status codes of AllDebrid and Debrid-Link become their words.
        assert_eq!(alldebrid_status_word(4), "Ready");
        assert_eq!(alldebrid_status_word(1), "Downloading");
        assert_eq!(alldebrid_status_word(9), "Error");
        assert_eq!(debridlink_torrent_status(Some(100)), "Finished");
        assert_eq!(debridlink_torrent_status(Some(4)), "Downloading");
    }

    /// A refusal keeps the provider's own code and sentence.
    #[test]
    fn the_extra_providers_errors_keep_their_own_code() {
        let api = Api::new(Provider::AllDebrid, "private-api-token".into()).unwrap();
        let answer = serde_json::json!({
            "status": "error",
            "error": {"code": "MAGNET_INVALID_URI", "message": "Magnet is not valid."}
        });
        let err = api.provider_error(reqwest::StatusCode::BAD_REQUEST, Some(&answer));
        assert_eq!(err.code.as_deref(), Some("MAGNET_INVALID_URI"));
        assert!(err.to_string().contains("Magnet is not valid."));

        let api = Api::new(Provider::Premiumize, "private-api-token".into()).unwrap();
        let answer = serde_json::json!({
            "status": "error",
            "code": "rate_limit_reached",
            "message": "Too many requests."
        });
        let err = api.provider_error(reqwest::StatusCode::TOO_MANY_REQUESTS, Some(&answer));
        assert_eq!(err.code.as_deref(), Some("rate_limit_reached"));
        assert!(err.to_string().contains("Too many requests."));

        let api = Api::new(Provider::DebridLink, "private-api-token".into()).unwrap();
        let answer = serde_json::json!({
            "success": false,
            "error": "unknowR",
            "error_description": "Unknown API endpoint."
        });
        let err = api.provider_error(reqwest::StatusCode::NOT_FOUND, Some(&answer));
        assert_eq!(err.code.as_deref(), Some("unknowR"));
        assert!(err.to_string().contains("Unknown API endpoint."));
    }

    /// A provider that answers `200 OK` with a refusal is a refusal.
    #[test]
    fn a_provider_that_answers_with_an_error_body_is_a_refusal() {
        let api = Api::new(Provider::Premiumize, "private-api-token".into()).unwrap();
        let answer = serde_json::json!({
            "status": "error",
            "code": "authentication_failed",
            "message": "Invalid token."
        });
        assert!(api.envelope_failed(&answer));
        let answer = serde_json::json!({"status": "success", "id": "pm1"});
        assert!(!api.envelope_failed(&answer));

        let api = Api::new(Provider::DebridLink, "private-api-token".into()).unwrap();
        assert!(api.envelope_failed(&serde_json::json!({
            "success": false,
            "error": "unknowR"
        })));
        assert!(!api.envelope_failed(&serde_json::json!({
            "success": true,
            "value": {}
        })));

        let api = Api::new(Provider::AllDebrid, "private-api-token".into()).unwrap();
        assert!(api.envelope_failed(&serde_json::json!({
            "status": "error",
            "error": {"code": "X", "message": "y"}
        })));
        assert!(!api.envelope_failed(&serde_json::json!({
            "status": "success",
            "data": {}
        })));
    }

    /// A task id can never change the request path.
    #[test]
    fn a_task_id_can_never_change_the_request_path() {
        assert!(safe_id("rd123"));
        assert!(safe_id("pm-123_4"));
        assert!(!safe_id(""));
        assert!(!safe_id("../delete"));
        assert!(!safe_id("a/b"));
        assert!(!safe_id("a?b"));
        assert!(!safe_id("a#b"));
    }
}
