//! Google Drive download URLs.
//!
//! A Drive file is not served from the URL that links to it. The link points
//! at a page, and the page decides how the bytes are handed over:
//!
//! * A small file is answered with a redirect straight to the file.
//! * A bigger one is answered with a web page that carries a form. That form
//!   points at the real download and carries a one-time confirm token.
//!
//! The token cannot be made up: the only way to have one is to read it off
//! the page the server just sent. This module does exactly that and nothing
//! more — it never invents a token and never claims a file it did not see.

use std::collections::HashMap;

/// The hosts Google Drive serves files from.
const DRIVE_HOSTS: &[&str] = &[
    "drive.google.com",
    "drive.usercontent.google.com",
    "docs.google.com",
];

/// Where the form on the interstitial page posts to.
const DOWNLOAD_HOST: &str = "https://drive.usercontent.google.com/download";

/// Is this a Google Drive URL?
pub fn is_drive_url(url: &str) -> bool {
    let Some(host) = host_of(url) else {
        return false;
    };
    DRIVE_HOSTS.contains(&host.as_str())
}

/// The host of a URL, lowercased.
fn host_of(url: &str) -> Option<String> {
    let rest = url
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or(url);
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit('@').next()?;
    // Drop the port.
    let host = host.split(':').next()?;
    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

/// The query of a URL as a map.
fn query_of(url: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Some((_, q)) = url.split_once('?') else {
        return out;
    };
    let q = q.split('#').next().unwrap_or("");
    for pair in q.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => (pair, ""),
        };
        out.insert(decode(k), decode(v));
    }
    out
}

/// Percent-decode a query piece.
fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The stable file id of a Drive URL.
///
/// It is the `id` query parameter, or the `<ID>` in a `/file/d/<ID>/view`
/// link. Two different ids are two different files and are never treated as
/// one.
pub fn file_id(url: &str) -> Option<String> {
    if !is_drive_url(url) {
        return None;
    }
    let q = query_of(url);
    if let Some(id) = q.get("id") {
        if !id.trim().is_empty() {
            return Some(id.trim().to_string());
        }
    }
    // "/file/d/<ID>/view" and "/open?id=<ID>"
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let path = match rest.find('/') {
        Some(i) => &rest[i..],
        None => "",
    };
    let path = path.split(['?', '#']).next().unwrap_or("");
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() >= 3 && parts[0] == "file" && parts[1] == "d" {
        let id = parts[2];
        if !id.is_empty() {
            return Some(id.to_string());
        }
    }
    None
}

/// The URL that asks Drive for the download of a file id.
pub fn download_url(id: &str) -> String {
    format!("https://drive.google.com/uc?export=download&id={}", encode(id))
}

/// Percent-encode a query value.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The value of an attribute inside a tag.
fn attr(tag: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=");
    let mut from = 0usize;
    while let Some(pos) = tag[from..].find(&needle) {
        let at = from + pos;
        // The attribute name must start a word, so `data-id=` is not `id=`.
        let before_ok = at == 0
            || tag.as_bytes()[at - 1].is_ascii_whitespace()
            || tag.as_bytes()[at - 1] == b'/';
        if before_ok {
            let rest = &tag[at + needle.len()..];
            let (quote, body) = rest.split_at(1);
            let q = quote.chars().next().unwrap_or('"');
            if q == '"' || q == '\'' {
                let end = body.find(q)?;
                return Some(body[..end].to_string());
            }
            let end = body.find(|c: char| c.is_whitespace() || c == '>')?;
            return Some(body[..end].to_string());
        }
        from = at + needle.len();
    }
    None
}

/// Read the target of the download form out of a Drive interstitial page.
///
/// The page looks like this (the shape is what Drive serves; the values are
/// per file and per session):
///
/// ```html
/// <form id="download-form" action="https://drive.usercontent.google.com/download" method="get">
///   <input type="hidden" name="id" value="FILE_ID">
///   <input type="hidden" name="export" value="download">
///   <input type="hidden" name="confirm" value="t">
/// </form>
/// ```
///
/// The result is the URL to follow. `None` means the page was not a Drive
/// interstitial.
pub fn interstitial_target(html: &str) -> Option<String> {
    // Find the form and the part of the page it covers.
    let form_at = html.find("<form")?;
    let form_end = html[form_at..].find('>')? + form_at;
    let form_tag = &html[form_at..=form_end];
    let body_start = form_end + 1;
    let body_end = html[body_start..]
        .find("</form>")
        .map(|p| body_start + p)
        .unwrap_or(html.len());
    let body = &html[body_start..body_end];

    let action = attr(form_tag, "action")?;
    let action = action.trim();
    if action.is_empty() {
        return None;
    }
    // A relative action belongs to the host that served the page.
    let action = if action.starts_with("http://") || action.starts_with("https://") {
        action.to_string()
    } else if let Some(rest) = action.strip_prefix("//") {
        format!("https://{rest}")
    } else if action.starts_with('/') {
        format!("https://drive.usercontent.google.com{action}")
    } else {
        format!("{DOWNLOAD_HOST}/{action}")
    };

    // Collect the hidden fields of the form.
    let mut params: Vec<(String, String)> = Vec::new();
    let mut at = 0usize;
    while let Some(pos) = body[at..].find("<input") {
        let start = at + pos;
        let end = match body[start..].find('>') {
            Some(e) => start + e,
            None => break,
        };
        let tag = &body[start..=end];
        at = end + 1;
        let Some(name) = attr(tag, "name") else {
            continue;
        };
        let value = attr(tag, "value").unwrap_or_default();
        params.push((name, value));
    }
    if params.is_empty() {
        return None;
    }

    // Keep the query the action already carries and add the form fields.
    let mut query: Vec<String> = Vec::new();
    if let Some((_, q)) = action.split_once('?') {
        for pair in q.split('&') {
            if !pair.is_empty() {
                query.push(pair.to_string());
            }
        }
    }
    let mut seen: Vec<String> = Vec::new();
    for (name, value) in params {
        if seen.iter().any(|s| *s == name) {
            continue;
        }
        seen.push(name.clone());
        query.push(format!("{}={}", encode(&name), encode(&value)));
    }
    let base = action.split('?').next().unwrap_or(&action);
    Some(format!("{base}?{}", query.join("&")))
}

/// Is this page a Drive interstitial (a page instead of the file)?
pub fn looks_like_interstitial(html: &str) -> bool {
    let head: String = html.chars().take(4000).collect::<String>().to_ascii_lowercase();
    head.contains("drive.usercontent.google.com") || (head.contains("<form") && head.contains("download-form"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reconstruction of the page Drive serves for a file that needs the
    /// confirm token. The shape is Drive's; the values are made up.
    const PAGE: &str = r#"<!DOCTYPE html><html><head><title>Google Drive - Virus scan warning</title></head>
<body><form id="download-form" action="https://drive.usercontent.google.com/download" method="get">
<input type="hidden" name="id" value="1AbCdEfGhIjKlMnOp">
<input type="hidden" name="export" value="download">
<input type="hidden" name="confirm" value="t0k3n">
<input type="hidden" name="uuid" value="9f8e7d6c">
</form></body></html>"#;

    #[test]
    fn drive_hosts_are_recognised() {
        assert!(is_drive_url("https://drive.google.com/uc?export=download&id=X"));
        assert!(is_drive_url("https://drive.usercontent.google.com/download?id=X"));
        assert!(!is_drive_url("https://example.com/drive.google.com"));
        assert!(!is_drive_url("https://notdrive.google.com.evil.test/x"));
        assert!(!is_drive_url("https://example.com/x"));
    }

    #[test]
    fn the_file_id_is_read_from_the_url() {
        assert_eq!(
            file_id("https://drive.google.com/uc?export=download&id=ABC123"),
            Some("ABC123".to_string())
        );
        assert_eq!(
            file_id("https://drive.google.com/file/d/XYZ789/view?usp=sharing"),
            Some("XYZ789".to_string())
        );
        assert_eq!(file_id("https://example.com/x"), None);
    }

    #[test]
    fn two_different_ids_stay_two_different_files() {
        assert_ne!(
            file_id("https://drive.google.com/uc?export=download&id=AAA"),
            file_id("https://drive.google.com/uc?export=download&id=BBB")
        );
    }

    #[test]
    fn the_interstitial_form_is_read() {
        let target = interstitial_target(PAGE).expect("the form must be found");
        assert!(target.starts_with("https://drive.usercontent.google.com/download?"));
        assert!(target.contains("id=1AbCdEfGhIjKlMnOp"));
        assert!(target.contains("export=download"));
        assert!(target.contains("confirm=t0k3n"));
    }

    #[test]
    fn the_token_is_read_from_the_page_and_never_invented() {
        // A page without a form has no target: nothing is guessed.
        assert!(interstitial_target("<html><body>nothing here</body></html>").is_none());
        assert!(interstitial_target("").is_none());
    }

    #[test]
    fn a_relative_action_is_resolved() {
        let page = r#"<form id="download-form" action="/download" method="get">
        <input type="hidden" name="id" value="X"></form>"#;
        let target = interstitial_target(page).expect("form");
        assert!(target.starts_with("https://drive.usercontent.google.com/download?"));
        assert!(target.contains("id=X"));
    }

    #[test]
    fn an_interstitial_is_told_apart_from_a_real_page() {
        assert!(looks_like_interstitial(PAGE));
        assert!(!looks_like_interstitial(
            "<html><body><h1>Google Drive - Sign in</h1></body></html>"
        ));
    }

    #[test]
    fn the_download_url_carries_the_id() {
        assert_eq!(
            download_url("ABC 123"),
            "https://drive.google.com/uc?export=download&id=ABC%20123"
        );
    }
}
