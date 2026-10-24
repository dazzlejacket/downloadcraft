//! Native-messaging host for the browser extension (`--native-host`).
//!
//! Protocol (Chrome/Edge/Firefox native messaging = 4-byte LE length + UTF-8 JSON):
//!   -> {"type":"ping"}                                  <- {"type":"pong","version":1}
//!   -> {"type":"download","url":"https://...","filename":"optional",
//!       "pageUrl":"https://page...","referer":"...","cookie":"...","mime":"...",
//!       "userAgent":"Mozilla/5.0 ...","headers":{"origin":"...",
//!       "authorization":"...","x-...":"..."}}
//!                                                      <- {"type":"ok","queued":true}
//!
//! `userAgent` is optional (older extensions omit it): it carries the
//! browser's own User-Agent for the capture, so the app can talk to the
//! server with the same voice the browser used. `headers` carries the
//! Origin / Authorization / x-* headers the player sent with the media.
//!
//! Besides `ping`, `query` and `download` the host answers `prefs` (the
//! app's theme) and `list`, `record`, `control` and `links` (the app's
//! downloads, a browser download record, a queue control request and the
//! links of a page).
//!
//! Download requests are appended to an inbox file that the GUI polls and
//! imports as queued (auto-started) downloads.

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct Incoming {
    #[serde(rename = "type")]
    kind: String,
    url: Option<String>,
    filename: Option<String>,
    /// Page the download was captured from (IDM-style referer fallback).
    #[serde(default, alias = "pageUrl")]
    page_url: Option<String>,
    #[serde(default)]
    referer: Option<String>,
    #[serde(default)]
    cookie: Option<String>,
    /// The browser's own User-Agent for this capture. It is the agent that
    /// already passed whatever check the site applies, so the app should
    /// speak with the same voice (optional: older extensions omit it).
    #[serde(default, alias = "userAgent")]
    user_agent: Option<String>,
    /// Origin / Authorization / x-* headers the player sent with this media.
    #[serde(default)]
    headers: Option<std::collections::BTreeMap<String, String>>,
    #[serde(default)]
    mime: Option<String>,
    /// Browser-measured size bytes (HEAD/Range from the page session).
    /// The engine can't always re-probe media URLs (403 off-session), so
    /// the dialog prefers this over probing.
    #[serde(default)]
    size: Option<u64>,
    /// Adaptive pair audio URL (video-only + audio merge). The extension
    /// sends these in camelCase (`pairUrl`/`pairMime`) like `pageUrl`, so
    /// without the aliases the pair was silently dropped and the merge never
    /// happened.
    #[serde(default, alias = "pairUrl")]
    pub pair_url: Option<String>,
    #[serde(default, alias = "pairMime")]
    pub pair_mime: Option<String>,
    /// Batch presence check (`{"type":"query"}`): which of these URLs are
    /// already queued. Answered from queue.json without side effects.
    /// (Only read on Incoming; stored nowhere.)
    #[serde(default)]
    pub urls: Option<Vec<String>>,
    /// Preferred YouTube quality from the extension quality menu.
    #[serde(default)]
    pub quality: Option<String>,
    /// Media duration in seconds (page player / JSON, best-effort).
    /// Shown in the confirm dialog and checked against the completed file.
    #[serde(default)]
    pub dur: Option<u64>,
    #[serde(default, alias = "audioExpected")]
    pub audio_expected: bool,
    /// "Download all links" (`{"type":"links"}`): every link on a page.
    #[serde(default)]
    pub links: Option<Vec<PageLink>>,
    /// Page title for the links window.
    #[serde(default)]
    pub title: Option<String>,
    /// `{"type":"control"}`: which queue row, and what to do with it.
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default)]
    pub action: Option<String>,
}

/// One link from a page ("Download all links").
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct PageLink {
    pub url: String,
    #[serde(default)]
    pub text: String,
}

/// What `--links <file>` reads: written by the host, deleted by the window.
///
/// The page's cookie is deliberately NOT in this file. It travels to the
/// window through the inherited environment instead, so no session cookie
/// is ever written to disk in plain text. See `LINKS_COOKIE_ENV`.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct LinksBatch {
    pub page_url: String,
    #[serde(default)]
    pub title: String,
    pub links: Vec<PageLink>,
}

/// The environment variable that carries the page's cookie to the Links
/// window. It lives only as long as that process.
pub const LINKS_COOKIE_ENV: &str = "DOWNLOADCRAFT_LINKS_COOKIE";

/// Validate a page's links and keep the downloadable ones (http/https,
/// sane lengths, no duplicates), capped like the extension's collector.
pub fn links_batch(page_url: &str, title: &str, links: &[PageLink]) -> LinksBatch {
    let mut seen = std::collections::HashSet::new();
    let kept = links
        .iter()
        .filter(|l| crate::model::is_download_url(&l.url) && l.url.len() <= 4096)
        .filter(|l| seen.insert(l.url.clone()))
        .take(5000)
        .map(|l| PageLink {
            url: l.url.clone(),
            text: l.text.chars().filter(|c| !c.is_control()).take(200).collect(),
        })
        .collect();
    LinksBatch {
        page_url: page_url.to_string(),
        title: title.chars().filter(|c| !c.is_control()).take(200).collect(),
        links: kept,
    }
}

fn spawn_links(msg: &Incoming) -> Result<(), String> {
    let page = msg.page_url.clone().unwrap_or_default();
    let batch = links_batch(
        &page,
        msg.title.as_deref().unwrap_or(""),
        msg.links.as_deref().unwrap_or(&[]),
    );
    if batch.links.is_empty() {
        return Err("no downloadable links on this page".into());
    }
    let dir = crate::model::config_dir().ok_or_else(|| "no config dir".to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // Clear any batch a previous window never read, so no stale file (and
    // no stale cookie) is left lying in the config dir.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("links_") && name.ends_with(".json") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    let path = dir.join(format!("links_{}.json", chrono::Local::now().timestamp_millis()));
    let json = serde_json::to_string(&batch).map_err(|e| e.to_string())?;
    crate::model::atomic_write(&path, json.as_bytes()).map_err(|e| e.to_string())?;
    let exe = crate::platform::launch_path().map_err(|e| e.to_string())?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--links").arg(&path);
    // The page's cookie travels with the process, not through the file.
    if let Some(cookie) = msg.cookie.as_deref() {
        if !cookie.trim().is_empty() {
            cmd.env(LINKS_COOKIE_ENV, cookie);
        }
    }
    spawn_detached(cmd);
    Ok(())
}

/// Spawn a window that outlives this short-lived host.
fn spawn_detached(mut cmd: std::process::Command) {
    // Never hand our stdio to the window: an inherited stdout keeps the
    // browser's pipe open after this host exits.
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        // DETACHED_PROCESS so the window outlives this short-lived host, and
        // BREAKAWAY_FROM_JOB because Firefox runs native hosts inside a job
        // object it kills when the host exits: children inherit that job, so a
        // plain detached dialog was being torn down the moment Firefox closed
        // our stdin (verified live — Chrome does not use such a job, which is
        // why only Firefox looked like it did nothing). A job that forbids
        // breaking away makes the spawn fail, and a plain detached window
        // still beats none at all.
        cmd.creation_flags(DETACHED_PROCESS | CREATE_BREAKAWAY_FROM_JOB);
        if cmd.spawn().is_ok() {
            return;
        }
        cmd.creation_flags(DETACHED_PROCESS);
    }
    let _ = cmd.spawn();
}

#[derive(Debug, Serialize)]
struct Reply {
    #[serde(rename = "type")]
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    queued: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    urls: Option<Vec<String>>,
    /// The app's theme ("light" | "dark" | "system") on pong, so the
    /// extension's in-page controls match it.
    #[serde(skip_serializing_if = "Option::is_none")]
    theme: Option<String>,
    /// `{"type":"list"}`: the app's active and recent downloads.
    #[serde(skip_serializing_if = "Option::is_none")]
    items: Option<Vec<serde_json::Value>>,
}

impl Reply {
    fn ok() -> Self {
        Reply { kind: "ok".into(), version: None, queued: Some(true), error: None, urls: None, theme: None, items: None }
    }
    fn err(e: impl Into<String>) -> Self {
        Reply { kind: "error".into(), version: None, queued: None, error: Some(e.into()), urls: None, theme: None, items: None }
    }
}

/// The popup's Downloads tab: every active row, then the newest finished or
/// failed ones, with live bytes and speed from the app's progress snapshot.
fn app_downloads() -> Vec<serde_json::Value> {
    use crate::model::{DownloadItem, Status};
    let dir = match crate::model::config_dir() {
        Some(d) => d,
        None => return Vec::new(),
    };
    let items: Vec<DownloadItem> = std::fs::read_to_string(dir.join("queue.json"))
        .ok()
        .and_then(|s| serde_json::from_str(s.trim_start_matches('\u{feff}')).ok())
        .unwrap_or_default();
    let live: serde_json::Value = std::fs::read_to_string(dir.join("progress.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null);
    let mut rows: Vec<&DownloadItem> = items.iter().collect();
    rows.sort_by(|a, b| b.id.cmp(&a.id));
    let active = |d: &&DownloadItem| matches!(d.status, Status::Downloading | Status::Queued | Status::Paused);
    let mut out: Vec<&DownloadItem> = rows.iter().copied().filter(active).collect();
    out.extend(rows.iter().copied().filter(|d| !active(d)).take(15));
    out.into_iter()
        .map(|d| {
            let snap = live.get("items").and_then(|m| m.get(d.id.to_string()));
            let num = |k: &str| snap.and_then(|s| s.get(k)).and_then(|x| x.as_f64());
            let (state, error) = match &d.status {
                Status::Downloading => ("downloading", String::new()),
                Status::Queued => ("queued", String::new()),
                Status::Paused => ("paused", String::new()),
                Status::Completed => ("done", String::new()),
                Status::Failed(e) => ("failed", e.clone()),
            };
            // A finished row's address can still be a live signed CDN link,
            // so it is not handed back to the extension. Only a row that is
            // still running needs its URL.
            let mut row = serde_json::json!({
                "id": d.id,
                "name": d.filename,
                "state": state,
                "error": error,
                "total": num("total").map(|t| t as u64).or(d.total_bytes),
                "done": num("downloaded").map(|t| t as u64).unwrap_or(d.downloaded_bytes),
                "speed": num("speed").unwrap_or(0.0),
            });
            if active(&d) {
                row["url"] = serde_json::json!(d.url);
            }
            row
        })
        .collect()
}

/// The folders a browser-saved file may legitimately come from: the user's
/// own Downloads folder, and the folder the app downloads into.
///
/// A path that arrives from the browser is not trusted on its own, so it
/// has to sit under one of these roots.
fn allowed_download_roots() -> Vec<std::path::PathBuf> {
    let mut roots = Vec::new();
    let home = std::env::var("USERPROFILE")
        .ok()
        .or_else(|| std::env::var("HOME").ok());
    if let Some(home) = home {
        let home = std::path::PathBuf::from(home);
        roots.push(home.join("Downloads"));
        roots.push(home.join("downloads"));
    }
    let save_dir = crate::model::ui_pref_str("save_dir", "");
    if !save_dir.trim().is_empty() {
        roots.push(std::path::PathBuf::from(save_dir));
    }
    roots
}

/// Is `path` inside `root`? A prefix match on the whole path is not enough:
/// "C:\Users\x\Downloads Evil" must not pass as "C:\Users\x\Downloads".
fn path_is_under(path: &std::path::Path, root: &std::path::Path) -> bool {
    let p = path.to_string_lossy().replace('/', "\\").to_lowercase();
    let r = root
        .to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase();
    if r.is_empty() {
        return false;
    }
    p.starts_with(&format!("{r}\\"))
}

/// A file the extension saved itself ("Download in browser"): list it in the
/// app as finished. `filename` is the saved file's full path; it must exist.
fn record_browser_download(msg: &Incoming) -> Result<(), String> {
    let url = msg.url.clone().filter(|u| u.starts_with("http")).unwrap_or_default();
    let path = std::path::PathBuf::from(msg.filename.clone().unwrap_or_default());
    if !path.is_absolute() || !path.is_file() {
        return Err("the saved file was not found".into());
    }
    if !allowed_download_roots().iter().any(|r| path_is_under(&path, r)) {
        return Err("the saved file is not in a download folder".into());
    }
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).ok_or("no file name")?;
    let dir = path.parent().map(|d| d.to_string_lossy().to_string()).ok_or("no folder")?;
    let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let mut item = crate::model::DownloadItem::new(0, if url.is_empty() { format!("file:///{}", path.display()) } else { url }, name, dir, 1);
    item.total_bytes = Some(len);
    item.downloaded_bytes = len;
    item.referer = msg.page_url.clone().unwrap_or_default();
    item.description = "Downloaded in the browser".into();
    match crate::model::record_finished(&mut item) {
        crate::model::AppendResult::Error(e) => Err(e),
        _ => Ok(()),
    }
}

/// Queue a pause/resume/cancel order for the app (the same cmd.json the
/// progress windows use).
fn app_control(id: Option<u64>, action: &str) -> Result<(), String> {
    let allowed = ["pause", "resume", "cancel", "pause_all", "resume_all"];
    if !allowed.contains(&action) {
        return Err("unknown action".into());
    }
    let p = crate::model::config_dir().ok_or("no config dir")?.join("cmd.json");
    let _guard = crate::model::fs_lock("cmd");
    if !_guard.owned() {
        return Err("the app is busy; retry".into());
    }
    let mut v: serde_json::Value = std::fs::read_to_string(&p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!([]));
    let arr = v.as_array_mut().ok_or("cmd file unreadable")?;
    let mut o = serde_json::json!({ "action": action });
    if let Some(id) = id {
        o["id"] = serde_json::json!(id);
    }
    arr.push(o);
    let json = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    crate::model::atomic_write(&p, json.as_bytes()).map_err(|e| e.to_string())
}

/// The app theme as the extension understands it.
fn app_theme() -> String {
    match crate::model::ui_pref_str("theme", "system").as_str() {
        "light" => "light".to_string(),
        "dark" | "black" => "dark".to_string(),
        _ => "system".to_string(),
    }
}

/// Which of `urls` are already in the shared queue (read-only, locked).
fn queued_urls(urls: &[String]) -> Vec<String> {
    if urls.is_empty() {
        return Vec::new();
    }
    let _guard = crate::model::fs_lock("queue");
    if !_guard.owned() { return Vec::new(); }
    let items: Vec<crate::model::DownloadItem> = crate::model::config_dir()
        .and_then(|d| std::fs::read_to_string(d.join("queue.json")).ok())
        // Strip a BOM: a queue.json written by another tool can start with
        // one, and serde_json refuses the whole file then. `app_downloads`
        // strips it too, so both readers agree on what is queued.
        .and_then(|s| serde_json::from_str(s.trim_start_matches('\u{feff}')).ok())
        .unwrap_or_default();
    urls.iter()
        .filter(|u| items.iter().any(|d| &d.url == *u))
        .cloned()
        .collect()
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InboxItem {
    pub url: String,
    pub filename: Option<String>,
    pub received_at: String,
    /// Page URL the media was captured from (used as Referer fallback).
    #[serde(default)]
    pub page_url: Option<String>,
    #[serde(default)]
    pub referer: Option<String>,
    #[serde(default)]
    pub cookie: Option<String>,
    /// The browser's own User-Agent for this capture. It is the agent that
    /// already passed whatever check the site applies, so the app should
    /// speak with the same voice (optional: older extensions omit it).
    #[serde(default, alias = "userAgent")]
    pub user_agent: Option<String>,
    /// Captured request headers (credentials: kept in the private inbox,
    /// never on a command line).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<std::collections::BTreeMap<String, String>>,
    #[serde(default)]
    pub mime: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub pair_url: Option<String>,
    #[serde(default)]
    pub pair_mime: Option<String>,
    /// Batch presence check (`{"type":"query"}`): which of these URLs are
    /// already queued. Answered from queue.json without side effects.
    /// (Only read on Incoming; stored nowhere.)
    #[serde(default)]
    pub urls: Option<Vec<String>>,
    /// Preferred YouTube quality from the extension quality menu.
    #[serde(default)]
    pub quality: Option<String>,
    /// Media duration in seconds (page player / JSON, best-effort).
    /// Shown in the confirm dialog and checked against the completed file.
    #[serde(default)]
    pub dur: Option<u64>,
    #[serde(default, alias = "audioExpected")]
    pub audio_expected: bool,
}

pub fn inbox_path() -> Option<std::path::PathBuf> {
    crate::model::config_dir().map(|p| p.join("native_inbox.json"))
}

/// One stdin frame: clean EOF (browser gone → exit the host) vs a broken
/// frame (reply with an error and keep serving — one bad message must not
/// kill the host and abandon everything behind it).
enum HostFrame {
    Eof,
    Bad(String),
    Msg(Incoming),
}

fn read_message() -> HostFrame {
    use std::io::Read;
    let mut len_buf = [0u8; 4];
    match std::io::stdin().read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return HostFrame::Eof,
        Err(_) => return HostFrame::Eof, // pipe broken: nothing more to serve
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    if len == 0 {
        return HostFrame::Eof; // clean close
    }
    if len > 4 * 1024 * 1024 {
        // Bigger than any real request. The length itself is valid framing,
        // so skip exactly that many bytes and keep serving: exiting dropped
        // every later capture on this port. Only a stream that ends early
        // (a corrupt prefix) closes the host.
        let skipped = std::io::copy(&mut std::io::stdin().take(len as u64), &mut std::io::sink());
        return match skipped {
            Ok(n) if n == len as u64 => HostFrame::Bad(format!("message too large ({len} bytes)")),
            _ => HostFrame::Eof,
        };
    }
    let mut buf = vec![0u8; len];
    if std::io::stdin().read_exact(&mut buf).is_err() {
        return HostFrame::Eof;
    }
    match serde_json::from_slice::<Incoming>(&buf) {
        Ok(m) => HostFrame::Msg(m),
        Err(e) => HostFrame::Bad(format!("bad message: {e}")),
    }
}

fn write_message(reply: &Reply) {
    if let Ok(data) = serde_json::to_vec(reply) {
        let len = (data.len() as u32).to_le_bytes();
        let mut out = std::io::stdout();
        use std::io::Write;
        let _ = out.write_all(&len);
        let _ = out.write_all(&data);
        let _ = out.flush();
    }
}

/// Keep only headers the engine may replay, with sane sizes. The extension
/// already filters; this is the trust boundary, so filter again.
fn replayable_headers(
    headers: Option<&std::collections::BTreeMap<String, String>>,
) -> Option<std::collections::BTreeMap<String, String>> {
    let kept: std::collections::BTreeMap<String, String> = headers?
        .iter()
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .filter(|(k, v)| {
            crate::engine::replayable_header(k)
                && k.len() <= 64
                && !v.is_empty()
                && v.len() <= 4096
                && !v.contains(['\r', '\n'])
        })
        .take(20)
        .collect();
    (!kept.is_empty()).then_some(kept)
}

fn queue_download(msg: &Incoming) -> Result<bool, String> {
    const MAX_INBOX: usize = 200;
    let url = msg.url.clone().unwrap_or_default();
    let path = inbox_path().ok_or_else(|| "no config dir".to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let _guard = crate::model::fs_lock("inbox");
    if !_guard.owned() { return Err("Download inbox is busy; retry shortly".into()); }
    // Never destroy pending captures on an unreadable inbox.
    let data = std::fs::read_to_string(&path).ok();
    let mut items: Vec<InboxItem> = match data {
        None => Vec::new(), // no file yet
        Some(s) => serde_json::from_str(&s)
            .map_err(|_| "inbox unreadable — kept for the next poll".to_string())?,
    };
    let referer = msg
        .referer
        .clone()
        .or_else(|| msg.page_url.clone())
        .filter(|s| !s.trim().is_empty());
    let fresh = InboxItem {
        url: url.clone(),
        filename: msg.filename.clone(),
        received_at: chrono::Local::now().to_rfc3339(),
        page_url: msg.page_url.clone(),
        referer,
        cookie: msg.cookie.clone().filter(|s| !s.trim().is_empty()),
        user_agent: msg.user_agent.clone().filter(|s| !s.trim().is_empty()),
        headers: replayable_headers(msg.headers.as_ref()),
        mime: msg.mime.clone().filter(|s| !s.trim().is_empty()),
        size: msg.size.filter(|s| *s > 0),
        pair_url: msg.pair_url.clone().filter(|s| s.starts_with("http")),
        pair_mime: msg.pair_mime.clone().filter(|s| !s.trim().is_empty()),
        urls: None,
        quality: msg.quality.clone().filter(|s| !s.trim().is_empty()),
        dur: msg.dur.filter(|s| *s > 0),
        audio_expected: msg.audio_expected,
    };
    // Same URL re-captured: refresh metadata instead of ignoring it (the
    // filename/referer/cookie may be corrected this time).
    let is_new = match items.iter_mut().find(|i| i.url == url) {
        Some(existing) => {
            *existing = fresh;
            false
        }
        None => {
            items.push(fresh);
            true
        }
    };
    if items.len() > MAX_INBOX {
        items.sort_by(|a, b| b.received_at.cmp(&a.received_at));
        items.truncate(MAX_INBOX);
    }
    let json = serde_json::to_string_pretty(&items).map_err(|e| e.to_string())?;
    crate::model::atomic_write(&path,json.as_bytes()).map_err(|e|e.to_string())?;
    Ok(is_new)
}

/// Fire-and-forget the standalone Download File Info dialog (detached, so
/// it outlives this short-lived native-host process).
fn spawn_confirm(msg: &Incoming) {
    let url = msg.url.clone().unwrap_or_default();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return;
    }
    let Ok(exe) = crate::platform::launch_path() else {
        return;
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--confirm-download").arg("--url").arg(&url);
    if let Some(f) = msg.filename.clone().filter(|s| !s.trim().is_empty()) {
        cmd.arg("--filename").arg(f);
    }
    // The engine reads this as both the Referer and "the page the media came
    // from" (YouTube fresh-link retry, social page fallback), so the page
    // wins over an origin-only Referer ("https://www.youtube.com/"). A
    // Referer with a real path is an embedded player's page, which the CDN
    // checks (sending the article URL instead got 403s): that one wins.
    let page = msg.page_url.clone().filter(|s| !s.trim().is_empty());
    let captured = msg.referer.clone().filter(|s| !s.trim().is_empty());
    let has_path = |u: &str| reqwest::Url::parse(u).map(|x| x.path() != "/" && !x.path().is_empty()).unwrap_or(false);
    let pick = match (page, captured) {
        (Some(p), Some(r)) if r != p && has_path(&r) => Some(r),
        (Some(p), _) => Some(p),
        (None, r) => r,
    };
    if let Some(r) = pick {
        cmd.arg("--referer").arg(r);
    }
    // The dialog claims credentials from the owner-only inbox.
    if let Some(m) = msg.mime.clone().filter(|s| !s.trim().is_empty()) {
        cmd.arg("--mime").arg(m);
    }
    if let Some(s) = msg.size.filter(|s| *s > 0) {
        cmd.arg("--size").arg(s.to_string());
    }
    if let Some(p) = msg.pair_url.clone().filter(|s| s.starts_with("http")) {
        cmd.arg("--pair-url").arg(p);
        if let Some(m) = msg.pair_mime.clone().filter(|s| !s.trim().is_empty()) {
            cmd.arg("--pair-mime").arg(m);
        }
    }
    if let Some(q) = msg.quality.clone().filter(|s| !s.trim().is_empty()) {
        cmd.arg("--quality").arg(q);
    }
    if msg.audio_expected { cmd.arg("--expect-audio"); }
    if let Some(d) = msg.dur.filter(|s| *s > 0) {
        cmd.arg("--dur").arg(d.to_string());
    }
    spawn_detached(cmd);
}

/// Entry point when launched with `--native-host`. Never returns normally.
pub fn run() -> eframe::Result<()> {
    // Keep stderr quiet; any panic text would corrupt the pipe (stdout stays clean).
    loop {
        match read_message() {
            HostFrame::Eof => break,
            HostFrame::Bad(e) => {
                write_message(&Reply::err(e));
                continue;
            }
            HostFrame::Msg(msg) => match msg.kind.as_str() {
            "ping" => write_message(&Reply {
                kind: "pong".into(),
                version: Some(1),
                queued: None,
                error: None,
                urls: None,
                theme: Some(app_theme()),
                items: None,
            }),
            "prefs" => {
                // What the app wants the extension to look like. There is one
                // overlay now, so only the theme is left to hand over.
                write_message(&Reply {
                    kind: "prefs".into(),
                    version: None,
                    queued: None,
                    error: None,
                    urls: None,
                    theme: Some(app_theme()),
                    items: None,
                });
            }
            "query" => {
                // Presence check only: no queueing, no dialogs, no locks held
                // beyond the read.
                let urls = msg.urls.clone().unwrap_or_default();
                let hits = queued_urls(&urls);
                write_message(&Reply {
                    kind: "queued".into(),
                    version: None,
                    queued: None,
                    error: None,
                    urls: Some(hits),
                    theme: None,
                    items: None,
                });
            }
            "download" => {
                let url = msg.url.clone().unwrap_or_default();
                let ok = url.starts_with("http://") || url.starts_with("https://");
                if !ok {
                    write_message(&Reply::err("need http/https url"));
                    continue;
                }
                // Blob/media-source URLs are not directly downloadable by the
                // engine — reject loudly so the extension can fall back to the
                // page URL instead of queueing a dead link.
                if url.starts_with("blob:") {
                    write_message(&Reply::err(
                        "blob: URLs need the page URL — send the page link instead",
                    ));
                    continue;
                }
                match queue_download(&msg) {
                    Ok(_is_new) => {
                        write_message(&Reply::ok());
                        // IDM-style: pop the standalone Download File Info
                        // window in front. The inbox entry doubles as a
                        // fallback — the dialog claims it on open, and the
                        // main window only prompts for entries the dialog
                        // never claimed (see poll_inbox age gate).
                        spawn_confirm(&msg);
                    }
                    Err(e) => write_message(&Reply::err(e)),
                }
            }
            "list" => write_message(&Reply {
                kind: "list".into(),
                version: None,
                queued: None,
                error: None,
                urls: None,
                theme: None,
                items: Some(app_downloads()),
            }),
            "record" => match record_browser_download(&msg) {
                Ok(()) => write_message(&Reply::ok()),
                Err(e) => write_message(&Reply::err(e)),
            },
            "control" => match app_control(msg.id, msg.action.as_deref().unwrap_or("")) {
                Ok(()) => write_message(&Reply::ok()),
                Err(e) => write_message(&Reply::err(e)),
            },
            "links" => match spawn_links(&msg) {
                Ok(()) => write_message(&Reply::ok()),
                Err(e) => write_message(&Reply::err(e)),
            },
            _ => write_message(&Reply::err("unknown message")),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod media_tests {
    use super::*;
    #[test]
    fn player_expectations_survive_browser_and_inbox_formats() {
        let msg: Incoming = serde_json::from_str(r#"{"type":"download","url":"https://cdn.example.org/video.mp4","dur":2700,"audioExpected":true}"#).unwrap();
        assert_eq!(msg.dur, Some(2700));
        assert!(msg.audio_expected);
        let stored: InboxItem = serde_json::from_str(r#"{"url":"https://cdn.example.org/video.mp4","filename":null,"received_at":"now","dur":2700,"audio_expected":true}"#).unwrap();
        assert_eq!(stored.dur, Some(2700));
        assert!(stored.audio_expected);
        let old: Incoming = serde_json::from_str(r#"{"type":"download","url":"https://cdn.example.org/video.mp4"}"#).unwrap();
        assert!(!old.audio_expected);
        assert!(old.headers.is_none());
    }

    #[test]
    fn captured_headers_keep_only_replayable_tokens() {
        let msg: Incoming = serde_json::from_str(r#"{"type":"download","url":"https://cdn.example.org/v",
            "headers":{"Origin":"https://site.example","authorization":"Bearer t","x-token":"1",
                       "x-client-data":"variations","Range":"bytes=0-","host":"evil","x-bad":"a\r\nb: c"}}"#).unwrap();
        let kept = replayable_headers(msg.headers.as_ref()).unwrap();
        assert_eq!(kept.keys().cloned().collect::<Vec<_>>(), ["authorization", "origin", "x-token"]);
        assert!(replayable_headers(None).is_none());
        let none = std::collections::BTreeMap::from([("cookie".to_string(), "a=b".to_string())]);
        assert!(replayable_headers(Some(&none)).is_none());
    }

    #[test]
    fn page_links_keep_only_unique_web_urls() {
        let msg: Incoming = serde_json::from_str(r#"{"type":"links","pageUrl":"https://site.example/dir/",
            "links":[{"url":"https://site.example/a.mp4","text":"A\u0007 video"},
                     {"url":"https://site.example/a.mp4","text":"again"},
                     {"url":"javascript:alert(1)"},{"url":"file:///c:/x"},
                     {"url":"http://other.example/b.zip"}]}"#).unwrap();
        let b = links_batch("https://site.example/dir/", "", msg.links.as_deref().unwrap());
        assert_eq!(b.links.len(), 2);
        assert_eq!(b.links[0].text, "A video");
        assert_eq!(b.links[1].url, "http://other.example/b.zip");
    }

    /// The page's cookie must never reach the disk: the batch file carries
    /// the links only, and the cookie travels with the process.
    #[test]
    fn the_links_batch_never_carries_the_cookie() {
        let msg: Incoming = serde_json::from_str(r#"{"type":"links","pageUrl":"https://site.example/dir/",
            "cookie":"session=secret-value",
            "links":[{"url":"https://site.example/a.mp4","text":"A video"}]}"#).unwrap();
        let b = links_batch(
            "https://site.example/dir/",
            "",
            msg.links.as_deref().unwrap(),
        );
        let json = serde_json::to_string(&b).unwrap();
        assert!(!json.contains("secret-value"), "the cookie reached the batch file: {json}");
        assert!(!json.contains("cookie"), "the batch file still has a cookie field: {json}");
    }

    /// A path that arrives from the browser is only accepted when it really
    /// is inside a download folder. A folder that merely starts with the
    /// same characters is not one.
    #[test]
    fn a_browser_path_must_sit_inside_a_download_folder() {
        let root = std::path::Path::new(r"C:\Users\tester\Downloads");
        assert!(path_is_under(
            std::path::Path::new(r"C:\Users\tester\Downloads\clip.mp4"),
            root
        ));
        assert!(path_is_under(
            std::path::Path::new(r"C:\Users\tester\Downloads\tv\clip.mp4"),
            root
        ));
        // "Downloads Evil" is a different folder.
        assert!(!path_is_under(
            std::path::Path::new(r"C:\Users\tester\Downloads Evil\clip.mp4"),
            root
        ));
        assert!(!path_is_under(std::path::Path::new(r"C:\Windows\System32\clip.mp4"), root));
        // The root itself is not a file inside it.
        assert!(!path_is_under(root, root));
    }

    /// The capture carries the browser's own User-Agent: the extension sends
    /// `userAgent` (camelCase, like `pageUrl`), and the inbox keeps it so
    /// the engine can speak with the same voice the browser used.
    #[test]
    fn browser_user_agent_survives_the_capture() {
        let msg: Incoming = serde_json::from_str(
            r#"{"type":"download","url":"https://archive.chirpmyradio.com/download?stream=next","userAgent":"Mozilla/5.0 (X11; Linux x86_64) Firefox/128.0"}"#,
        )
        .unwrap();
        assert_eq!(
            msg.user_agent.as_deref(),
            Some("Mozilla/5.0 (X11; Linux x86_64) Firefox/128.0")
        );
        // The snake_case spelling is accepted too, and an old extension that
        // sends no User-Agent at all still parses.
        let alt: Incoming = serde_json::from_str(
            r#"{"type":"download","url":"https://h/f.zip","user_agent":"Mozilla/5.0 Chrome/126.0"}"#,
        )
        .unwrap();
        assert_eq!(alt.user_agent.as_deref(), Some("Mozilla/5.0 Chrome/126.0"));
        let old: Incoming =
            serde_json::from_str(r#"{"type":"download","url":"https://h/f.zip"}"#).unwrap();
        assert!(old.user_agent.is_none());
        // The stored inbox entry keeps the field (and tolerates its absence).
        let stored: InboxItem = serde_json::from_str(
            r#"{"url":"https://h/f.zip","filename":null,"received_at":"now","user_agent":"Mozilla/5.0 Chrome/126.0"}"#,
        )
        .unwrap();
        assert_eq!(stored.user_agent.as_deref(), Some("Mozilla/5.0 Chrome/126.0"));
        let legacy: InboxItem =
            serde_json::from_str(r#"{"url":"https://h/f.zip","filename":null,"received_at":"now"}"#)
                .unwrap();
        assert!(legacy.user_agent.is_none());
    }
}
