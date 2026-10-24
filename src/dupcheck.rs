//! Duplicate detection for captured downloads.
//!
//! A capture can point at a file this app already has: the same URL twice, a
//! fresh signed URL for a file that is already downloaded, or a second capture
//! while the first transfer is still running. Treating those as new work
//! silently duplicates a download, so every capture is checked here first.
//!
//! Two resources are only ever called "the same" on strong evidence:
//!
//! * the URLs are byte-identical, or
//! * they resolve to the same destination file, or
//! * they share a *stable identity* (host + path + the query parameters that
//!   do not change between requests) **and** the same filename.
//!
//! The stable identity deliberately drops the query parameters that a CDN
//! rotates on every request (`Expires`, `Signature`, `X-Amz-*`, ...), so a
//! re-signed URL for the same object still matches. What it never does is
//! match two different objects that merely share a generic path such as
//! `/download`: such a path carries no identity of its own, so the match also
//! requires a stable query parameter (a Drive file id, for instance) to be
//! present. Without that, the identity is treated as unknown and the two URLs
//! are only equal when their bytes match.

use crate::model::{DownloadItem, Status};
use url::Url;

/// Query parameters that are regenerated for every request of the SAME
/// resource. Dropping them makes a re-signed URL comparable with the one
/// that was captured earlier.
const VOLATILE_QUERY_KEYS: &[&str] = &[
    // Generic signatures, tokens and cache busters.
    "sig",
    "signature",
    "token",
    "access_token",
    "id_token",
    "jwt",
    "expires",
    "expire",
    "expiry",
    "e",
    "t",
    "ts",
    "_",
    "cache",
    "cachebuster",
    "cb",
    "nocache",
    "random",
    "nonce",
    "session",
    "sessionid",
    "sid",
    "auth",
    "authorization",
    "key",
    // AWS SigV4 / CloudFront.
    "x-amz-signature",
    "x-amz-credential",
    "x-amz-date",
    "x-amz-expires",
    "x-amz-security-token",
    "x-amz-algorithm",
    "policy",
    "key-pair-id",
    "se",
    "sp",
    "sv",
    "st",
    "sr",
    "sk",
    // Azure shared access signatures.
    "sv",
    "sr",
    "st",
    "se",
    "sp",
    "sig",
];

/// Path segments that identify no particular object. A URL that ends in one of
/// these only names a file when its query carries a stable identifier.
const GENERIC_PATH_SEGMENTS: &[&str] = &[
    "download",
    "downloads",
    "file",
    "files",
    "get",
    "fetch",
    "stream",
    "media",
    "dl",
    "view",
    "content",
    "attachment",
    "serve",
];

/// What the app already knows about a file the capture points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuplicateState {
    /// The file was already downloaded (the queue row says so).
    Completed,
    /// The file is still on its way (queued, downloading or paused).
    Active,
}

/// A duplicate the capture would create.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateHit {
    /// Queue id of the row that already owns this file.
    pub id: u64,
    pub state: DuplicateState,
    /// File name of the existing row.
    pub filename: String,
    /// Where that row writes to.
    pub dest: String,
}

/// True when the query key is one of the rotating ones.
fn is_volatile(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    VOLATILE_QUERY_KEYS.contains(&k.as_str())
}

/// True when the URL path names a specific file rather than a generic slot.
fn path_is_specific(path: &str) -> bool {
    let last = path
        .rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or("")
        .to_ascii_lowercase();
    if last.is_empty() {
        return false;
    }
    if GENERIC_PATH_SEGMENTS.contains(&last.as_str()) {
        return false;
    }
    // A name with an extension ("clip.mp4", "pack.zip") points at one object.
    last.rsplit_once('.').map(|(_, ext)| !ext.is_empty()).unwrap_or(false)
}

/// The identity of the resource behind a URL, with the rotating query
/// parameters removed.
///
/// Returns `None` when the URL is not a usable http(s) URL, or when it carries
/// no identity at all (a generic path with nothing stable in the query) — in
/// that case two URLs are only equal when their bytes match.
pub fn stable_identity(url: &str) -> Option<String> {
    let parsed = Url::parse(url.trim()).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
    let path = parsed.path().to_string();
    let mut kept: Vec<(String, String)> = Vec::new();
    for (k, v) in parsed.query_pairs() {
        if is_volatile(&k) {
            continue;
        }
        kept.push((k.to_ascii_lowercase(), v.to_string()));
    }
    kept.sort();
    if kept.is_empty() && !path_is_specific(&path) {
        // Nothing stable left: `/download?Expires=..&Signature=..` names no
        // particular object, so it must not be equated with anything else.
        return None;
    }
    let query = kept
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");
    Some(format!("{host}{path}?{query}"))
}

/// True when the two URLs point at the same resource.
pub fn same_resource(a: &str, b: &str) -> bool {
    let a = a.trim();
    let b = b.trim();
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a == b {
        return true;
    }
    match (stable_identity(a), stable_identity(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// Compare file names the way the destination folder does.
fn same_name(a: &str, b: &str) -> bool {
    if a.trim().is_empty() || b.trim().is_empty() {
        return false;
    }
    if cfg!(windows) {
        a.trim().eq_ignore_ascii_case(b.trim())
    } else {
        a.trim() == b.trim()
    }
}

/// Compare destination paths the way the filesystem does.
fn same_dest(a: &str, b: &str) -> bool {
    if a.trim().is_empty() || b.trim().is_empty() {
        return false;
    }
    if cfg!(windows) {
        a.trim().eq_ignore_ascii_case(b.trim())
    } else {
        a.trim() == b.trim()
    }
}

/// Is this row still "in flight" (so a new capture must not start a second
/// copy of the same file)?
fn row_is_active(item: &DownloadItem) -> bool {
    matches!(
        item.status,
        Status::Queued | Status::Downloading | Status::Paused
    )
}

/// Find the queue row a new capture would duplicate.
///
/// `url` is the captured URL, `filename`/`save_dir` the destination the
/// capture would write to. The most useful hit wins: an exact URL match beats
/// a destination match, which beats a stable-identity match.
pub fn find_duplicate(
    items: &[DownloadItem],
    url: &str,
    filename: &str,
    save_dir: &str,
) -> Option<DuplicateHit> {
    let want_dest = if save_dir.trim().is_empty() {
        filename.trim().to_string()
    } else {
        std::path::Path::new(save_dir.trim())
            .join(filename.trim())
            .to_string_lossy()
            .into_owned()
    };

    let mut best: Option<(u8, DuplicateHit)> = None;
    for item in items {
        // A failed row owns nothing: the file is not there and the transfer
        // is over, so a fresh capture is the retry the user asked for.
        if matches!(item.status, Status::Failed(_)) {
            continue;
        }
        let state = if matches!(item.status, Status::Completed) {
            DuplicateState::Completed
        } else if row_is_active(item) {
            DuplicateState::Active
        } else {
            // Anything else owns nothing (a failed row was skipped above).
            continue;
        };
        let hit = DuplicateHit {
            id: item.id,
            state,
            filename: item.filename.clone(),
            dest: item.dest_path().to_string_lossy().into_owned(),
        };

        let rank = if item.url.trim() == url.trim() {
            3
        } else if same_dest(&hit.dest, &want_dest) {
            2
        } else if same_resource(&item.url, url) && same_name(&item.filename, filename) {
            1
        } else {
            continue;
        };
        // Prefer the strongest evidence, then the oldest row so the user is
        // pointed at the transfer that has been running longest.
        let better = match &best {
            None => true,
            Some((r, prev)) => rank > *r || (rank == *r && item.id < prev.id),
        };
        if better {
            best = Some((rank, hit));
        }
    }
    best.map(|(_, hit)| hit)
}

/// Read the shared queue and look for a duplicate of the capture.
pub fn find_duplicate_in_queue(url: &str, filename: &str, save_dir: &str) -> Option<DuplicateHit> {
    let items = crate::model::read_queue_items(
        &crate::model::config_dir()?.join("queue.json"),
    )
    .ok()?;
    find_duplicate(&items, url, filename, save_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: u64, url: &str, filename: &str, dir: &str, status: Status) -> DownloadItem {
        let mut d = DownloadItem::new(id, url.to_string(), filename.to_string(), dir.to_string(), 4);
        d.status = status;
        d
    }

    #[test]
    fn identical_urls_are_the_same_resource() {
        assert!(same_resource(
            "https://example.com/a.mp4",
            "https://example.com/a.mp4"
        ));
        assert!(!same_resource(
            "https://example.com/a.mp4",
            "https://example.com/b.mp4"
        ));
    }

    #[test]
    fn rotating_signature_parameters_do_not_hide_the_same_file() {
        let a = "https://cdn.example.com/pack.zip?Expires=1700000000&Signature=aaa&Key-Pair-Id=k1";
        let b = "https://cdn.example.com/pack.zip?Expires=1800000000&Signature=bbb&Key-Pair-Id=k1";
        assert_eq!(stable_identity(a), stable_identity(b));
        assert!(same_resource(a, b));
    }

    #[test]
    fn a_stable_query_parameter_survives_and_keeps_two_ids_apart() {
        let a = "https://drive.google.com/uc?export=download&id=AAA";
        let b = "https://drive.google.com/uc?export=download&id=BBB";
        assert_ne!(stable_identity(a), stable_identity(b));
        assert!(!same_resource(a, b));
        assert!(same_resource(a, "https://drive.google.com/uc?export=download&id=AAA"));
    }

    #[test]
    fn a_generic_path_without_a_stable_parameter_identifies_nothing() {
        let a = "https://cdn.example.com/download?token=one";
        let b = "https://cdn.example.com/download?token=two";
        assert_eq!(stable_identity(a), None);
        assert!(!same_resource(a, b));
    }

    #[test]
    fn a_generic_path_with_a_stable_parameter_keeps_its_identity() {
        let a = "https://cdn.example.com/download?id=42&token=one";
        let b = "https://cdn.example.com/download?id=42&token=two";
        assert_eq!(stable_identity(a), stable_identity(b));
        assert!(same_resource(a, b));
    }

    #[test]
    fn the_same_object_signed_differently_matches_by_destination() {
        let items = vec![item(
            7,
            "https://cdn.example.com/pack.zip?Signature=aaa",
            "pack.zip",
            "/downloads",
            Status::Completed,
        )];
        let hit = find_duplicate(
            &items,
            "https://cdn.example.com/pack.zip?Signature=bbb",
            "pack.zip",
            "/downloads",
        )
        .expect("same destination must match");
        assert_eq!(hit.id, 7);
        assert_eq!(hit.state, DuplicateState::Completed);
    }

    #[test]
    fn a_running_transfer_is_reported_as_active() {
        let items = vec![item(
            3,
            "https://example.com/a.mp4",
            "a.mp4",
            "/downloads",
            Status::Downloading,
        )];
        let hit = find_duplicate(
            &items,
            "https://example.com/a.mp4",
            "a.mp4",
            "/downloads",
        )
        .expect("the same url must match");
        assert_eq!(hit.state, DuplicateState::Active);
    }

    #[test]
    fn a_failed_row_never_blocks_a_new_capture() {
        let items = vec![item(
            9,
            "https://example.com/a.mp4",
            "a.mp4",
            "/downloads",
            Status::Failed("boom".into()),
        )];
        assert!(find_duplicate(
            &items,
            "https://example.com/a.mp4",
            "a.mp4",
            "/downloads"
        )
        .is_none());
    }

    #[test]
    fn unrelated_files_in_the_same_folder_do_not_match() {
        let items = vec![item(
            4,
            "https://example.com/one.mp4",
            "one.mp4",
            "/downloads",
            Status::Completed,
        )];
        assert!(find_duplicate(
            &items,
            "https://example.com/two.mp4",
            "two.mp4",
            "/downloads"
        )
        .is_none());
    }

    #[test]
    fn the_exact_url_wins_over_a_weaker_match() {
        let items = vec![
            item(
                1,
                "https://example.com/other.mp4",
                "a.mp4",
                "/downloads",
                Status::Completed,
            ),
            item(
                2,
                "https://example.com/a.mp4",
                "a.mp4",
                "/elsewhere",
                Status::Downloading,
            ),
        ];
        let hit = find_duplicate(
            &items,
            "https://example.com/a.mp4",
            "a.mp4",
            "/downloads",
        )
        .expect("must match");
        assert_eq!(hit.id, 2);
        assert_eq!(hit.state, DuplicateState::Active);
    }
}
