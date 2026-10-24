// Included in engine so captions share the existing credential and helper policies.
fn youtube_host(host: &str) -> bool {
    host == "youtube.com" || host.ends_with(".youtube.com") || host == "youtu.be"
}

#[derive(Debug)]
struct CaptionTrack {
    video: String,
    lang: String,
    kind: String,
    name: String,
    translated: String,
    variant: String,
    format: String,
}
impl CaptionTrack {
    fn parse(raw: &str) -> Result<Self> {
        let u = url::Url::parse(raw)?;
        let get = |key: &str| u.query_pairs().find(|(k, _)| k == key).map(|(_, v)| v.into_owned()).unwrap_or_default();
        let track = Self { video: get("v"), lang: get("lang"), kind: get("kind"), name: get("name"), translated: get("tlang"), variant: get("variant"), format: get("fmt") };
        if !is_youtube_caption_url(raw) || track.video.is_empty() || track.lang.is_empty()
            || !track.video.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            || !matches!(track.format.as_str(), "vtt" | "srt") {
            return Err(anyhow!("Unsupported caption URL: select a VTT or SRT track again from the video page"));
        }
        Ok(track)
    }
    fn fresh_url(&self, info: &serde_json::Value) -> Option<String> {
        let key = if self.kind == "asr" { "automatic_captions" } else { "subtitles" };
        // yt-dlp labels original ASR as e.g. es-orig; match the URL track
        // identity rather than its display dictionary key.
        info.get(key)?.as_object()?.values().filter_map(|v| v.as_array()).flatten().find_map(|entry| {
            if entry.get("ext")?.as_str()? != self.format { return None; }
            let raw = entry.get("url")?.as_str()?;
            let fresh = Self::parse(raw).ok()?;
            (fresh.video == self.video && fresh.lang == self.lang && fresh.kind == self.kind
                && fresh.name == self.name && fresh.translated == self.translated && fresh.variant == self.variant && fresh.format == self.format)
                .then(|| raw.to_string())
        })
    }
}

fn validate_caption(bytes: &[u8], format: &str) -> Result<()> {
    let text = std::str::from_utf8(bytes).map_err(|_| anyhow!("invalid subtitle encoding"))?;
    let text = text.trim_start_matches('\u{feff}').trim_start();
    let low = text.to_ascii_lowercase();
    if low.contains("<html") || low.contains("<!doctype") || low.contains("<script") {
        return Err(anyhow!("caption server returned HTML (login, consent, challenge or error page)"));
    }
    let valid = if format == "vtt" {
        text.strip_prefix("WEBVTT").map(|tail| tail.is_empty() || tail.starts_with(char::is_whitespace)).unwrap_or(false)
            && text.contains("-->")
    } else {
        text.lines().any(|l| l.contains(" --> ") && l.contains(','))
    };
    if !valid { return Err(anyhow!("caption response was empty or not valid {} subtitle data", format)); }
    Ok(())
}

// GET only, no HEAD/range probe. Refuse redirects outside YouTube before sending credentials.
async fn fetch_caption(raw: &str, format: &str, opts: &EngineOpts, shared: &Arc<SharedState>) -> Result<Vec<u8>> {
    let client = opts.build_client_redirects(false)?;
    let mut current = raw.to_string();
    let mut hops = Vec::new();
    for _ in 0..=5 {
        wait_if_paused(shared).await?;
        let u = url::Url::parse(&current)?;
        if !(u.scheme() == "https" && youtube_host(u.host_str().unwrap_or("")))
            && !cfg!(test).then(|| u.scheme() == "http" && u.host_str() == Some("127.0.0.1")).unwrap_or(false) {
            return Err(anyhow!("caption redirected outside YouTube; recapture from its page"));
        }
        let mut response = opts.get(&client, &current).timeout(std::time::Duration::from_secs(30)).send().await.map_err(|_| anyhow!("caption request failed"))?;
        let status = response.status();
        let mime = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("unknown").split(';').next().unwrap_or("unknown").to_string();
        hops.push(format!("{} {}{}", status.as_u16(), u.host_str().unwrap_or(""), u.path()));
        if status.is_redirection() {
            current = u.join(response.headers().get(reqwest::header::LOCATION).and_then(|v| v.to_str().ok()).ok_or_else(|| anyhow!("caption redirect missing location"))?)?.to_string();
            continue;
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| anyhow!("caption response interrupted"))? {
            wait_if_paused(shared).await?;
            if body.len() + chunk.len() > 8 * 1024 * 1024 { return Err(anyhow!("caption exceeds 8 MiB limit")); }
            body.extend_from_slice(&chunk);
        }
        if cloudflare_challenge_body(&String::from_utf8_lossy(&body)) || cloudflare_challenge_header(response.headers()) {
            shared.challenged.store(true, Ordering::SeqCst);
            return Err(anyhow!("caption server requires a browser check; open the video in your browser and recapture"));
        }
        let validation = validate_caption(&body, format);
        let signature = if validation.is_ok() { format } else if body.is_empty() { "empty" } else if String::from_utf8_lossy(&body).to_ascii_lowercase().contains("<html") { "HTML" } else { "invalid" };
        shared.set_note(format!("Caption HTTP: {}; Content-Type: {}; signature: {}", hops.join(" -> "), mime, signature));
        if !status.is_success() { return Err(anyhow!("caption HTTP {} (expired or access-restricted)", status.as_u16())); }
        validation?;
        return Ok(body);
    }
    Err(anyhow!("too many caption redirects"))
}

fn caption_extract_args(page: &str) -> Vec<String> {
    ["--ignore-config", "--no-playlist", "--skip-download", "--dump-single-json", "--no-warnings", "--", page].iter().map(|s| s.to_string()).collect()
}

async fn refresh_caption(track: &CaptionTrack, opts: &EngineOpts, shared: &Arc<SharedState>) -> Result<String> {
    let client = opts.build_client()?;
    let exe = ensure_ytdlp(&client).await?;
    refresh_caption_with_exe(&exe, track, opts, shared).await
}

async fn refresh_caption_with_exe(exe: &std::path::Path, track: &CaptionTrack, opts: &EngineOpts, shared: &Arc<SharedState>) -> Result<String> {
    let page = format!("https://www.youtube.com/watch?v={}", track.video);
    let jar = write_cookie_jar(&opts.cookie, ".youtube.com").await?;
    let mut cmd = tokio::process::Command::new(exe);
    cmd.kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    if let Some(jar) = &jar { cmd.arg("--cookies").arg(&jar.0); }
    cmd.arg("--user-agent").arg(&opts.user_agent);
    if !opts.proxy_url.is_empty() { cmd.arg("--proxy").arg(&opts.proxy_url); }
    cmd.args(caption_extract_args(&page));
    cmd.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
    let mut child = cmd.spawn()?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow!("no subtitle metadata output"))?;
    let output = async {
        let mut bytes = Vec::new();
        stdout.take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes).await?;
        if bytes.len() > 16 * 1024 * 1024 { return Err(anyhow!("subtitle metadata exceeds 16 MiB limit")); }
        Ok::<_, anyhow::Error>((child.wait().await?, bytes))
    };
    tokio::pin!(output);
    let deadline = tokio::time::sleep(std::time::Duration::from_secs(90));
    tokio::pin!(deadline);
    let out = loop {
        tokio::select! {
            out = &mut output => break out?,
            _ = &mut deadline => return Err(anyhow!("subtitle extraction timed out")),
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => { wait_if_paused(shared).await?; }
        }
    };
    // Never expose helper stderr: it can contain signed URLs or session data.
    if !out.0.success() { return Err(anyhow!("yt-dlp could not extract authorized captions")); }
    let info: serde_json::Value = serde_json::from_slice(&out.1)?;
    track.fresh_url(&info).ok_or_else(|| anyhow!("selected caption language, format or track is unavailable; recapture from the video page"))
}

async fn download_caption(raw: &str, mut dest: PathBuf, opts: &EngineOpts, shared: &Arc<SharedState>) -> Result<()> {
    let track = CaptionTrack::parse(raw)?;
    dest.set_extension(&track.format);
    let _job = claim_dest_job(&dest).ok_or_else(|| anyhow!("this destination is already downloading elsewhere"))?;
    if dest.exists() && !opts.overwrite_existing { dest = crate::model::conflict_free_dest(&dest); }
    if let Some(name) = dest.file_name() { mutex_guard(&shared.resolved_name).replace(name.to_string_lossy().into_owned()); }
    let bytes = match fetch_caption(raw, &track.format, opts, shared).await {
        Ok(bytes) => bytes,
        Err(direct) => {
            if shared.challenged.load(Ordering::SeqCst) { return Err(direct); }
            // Refresh metadata through the supported extractor; never run video download code.
            let fresh = refresh_caption(&track, opts, shared).await.map_err(|_| anyhow!("Captions unavailable, expired or access-restricted ({direct}). Open the video in your browser, confirm captions are available, and select the track again. No subtitle was saved."))?;
            fetch_caption(&fresh, &track.format, opts, shared).await.map_err(|_| anyhow!("Refreshed captions unavailable or access-restricted. Select the track again in your browser. No subtitle was saved."))?
        }
    };
    wait_if_paused(shared).await?;
    if let Some(parent) = dest.parent() { tokio::fs::create_dir_all(parent).await?; }
    let part = dest.with_extension("dcpart");
    tokio::fs::write(&part, &bytes).await?;
    if let Err(error) = verify_checksum(&part, &opts.expected_sha256).await {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(error);
    }
    tokio::fs::rename(&part, &dest).await?;
    shared.total.store(bytes.len() as u64, Ordering::SeqCst);
    shared.has_total.store(true, Ordering::SeqCst);
    shared.downloaded.store(bytes.len() as u64, Ordering::SeqCst);
    Ok(())
}
#[cfg(test)]
mod caption_tests {
    use super::*;
    fn url() -> &'static str { "https://www.youtube.com/api/timedtext?v=abc123&lang=en&fmt=vtt&kind=asr&expire=1" }
    #[test]
    fn html_never_routes_captions_to_video_resolution() {
        assert!(!needs_resolve(url(), Some("text/html")));
        assert!(!is_youtube_caption_url("https://youtube.com.evil.test/api/timedtext?v=x&lang=en&fmt=vtt"));
        assert!(!is_youtube_video_page("https://example.org/?url=youtube.com"));
    }
    #[test]
    fn subtitle_signatures_win_over_imperfect_mime() {
        assert!(validate_caption(b"\xef\xbb\xbfWEBVTT\n\n00:00.000 --> 00:01.000\nhello", "vtt").is_ok());
        assert!(validate_caption(b"1\n00:00:00,000 --> 00:00:01,000\nhello", "srt").is_ok());
        for bytes in [b"".as_slice(), b"<html>consent</html>", b"WEBVTT\n<html>login</html>", b"video bytes"] {
            assert!(validate_caption(bytes, "vtt").is_err());
        }
    }
    #[test]
    fn expired_track_refresh_preserves_language_kind_name_and_format() {
        let track = CaptionTrack::parse(url()).unwrap();
        let fresh = url().replace("expire=1", "expire=9999999999");
        let info = serde_json::json!({"automatic_captions":{"en":[{"ext":"vtt","url":fresh}]}});
        assert_eq!(track.fresh_url(&info), Some(fresh));
        for bad in [url().replace("kind=asr", "kind="), url().replace("lang=en", "lang=de"), url().replace("fmt=vtt", "fmt=srt"), format!("{}&name=another", url()), format!("{}&variant=gemini", url()), format!("{}&tlang=de", url())] {
            assert!(track.fresh_url(&serde_json::json!({"automatic_captions":{"en":[{"ext":"vtt","url":bad}]}})).is_none());
        }
        assert!(track.fresh_url(&serde_json::json!({"subtitles":{"en":[{"ext":"vtt","url":url()}]}})).is_none());
    }
    #[test]
    fn original_asr_dictionary_alias_preserves_gemini_variant() {
        let raw = format!("{}&variant=gemini", url());
        let track = CaptionTrack::parse(&raw).unwrap();
        let info = serde_json::json!({"automatic_captions":{"es-orig":[{"ext":"vtt","url":raw}]}});
        assert_eq!(track.fresh_url(&info), Some(raw));
    }
    #[test]
    fn fallback_only_extracts_metadata_and_never_downloads_video() {
        let args = caption_extract_args("https://www.youtube.com/watch?v=abc123");
        assert!(args.iter().any(|v| v == "--skip-download"));
        assert!(args.iter().any(|v| v == "--ignore-config"));
        assert!(args.iter().any(|v| v == "--dump-single-json"));
        assert!(!args.iter().any(|v| v == "--write-subs" || v == "-o" || v == "-f"));
    }
    #[tokio::test]
    async fn fallback_runs_metadata_helper_and_keeps_exact_track_without_media_files() {
        let root = std::env::temp_dir().join(format!("dc-caption-helper-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let fresh = url().replace("expire=1", "expire=9999999999");
        let info = serde_json::json!({"automatic_captions":{"en-orig":[{"ext":"vtt","url":fresh}]}});
        std::fs::write(root.join("metadata.json"), info.to_string()).unwrap();
        #[cfg(windows)]
        let exe = {
            let exe = root.join("helper.cmd");
            std::fs::write(&exe, "@echo off\r\necho %* > \"%~dp0args.txt\"\r\ntype \"%~dp0metadata.json\"\r\n").unwrap();
            exe
        };
        #[cfg(unix)]
        let exe = {
            use std::os::unix::fs::PermissionsExt;
            let exe = root.join("helper.sh");
            std::fs::write(&exe, format!("#!/bin/sh\nprintf '%s' \"$*\" > '{}/args.txt'\ncat '{}/metadata.json'\n", root.display(), root.display())).unwrap();
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o700)).unwrap();
            exe
        };
        let got = refresh_caption_with_exe(&exe, &CaptionTrack::parse(url()).unwrap(), &EngineOpts::default(), &Arc::new(SharedState::new())).await.unwrap();
        assert_eq!(got, fresh);
        let args = std::fs::read_to_string(root.join("args.txt")).unwrap();
        assert!(args.contains("--skip-download")); assert!(args.contains("--ignore-config"));
        assert!(args.contains("https://www.youtube.com/watch?v=abc123"));
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 3, "metadata, arguments and mock helper only; no video or subtitle side effects");
        std::fs::remove_file(exe).unwrap(); std::fs::remove_file(root.join("metadata.json")).unwrap();
        std::fs::remove_file(root.join("args.txt")).unwrap(); std::fs::remove_dir(root).unwrap();
    }
    async fn serve(status: &str, mime: &str, body: &[u8]) -> (String, std::thread::JoinHandle<String>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let reply = format!("HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
        let body = body.to_vec();
        let thread = std::thread::spawn(move || {
            use std::io::{Read, Write};
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 8192];
            let n = socket.read(&mut request).unwrap();
            socket.write_all(&reply).unwrap(); socket.write_all(&body).unwrap();
            String::from_utf8_lossy(&request[..n]).into_owned()
        });
        (format!("http://{address}/api/timedtext?secret=not-logged&fmt=vtt"), thread)
    }
    #[tokio::test]
    async fn direct_get_with_session_and_imperfect_mime_is_subtitle_only() {
        let body = b"WEBVTT\n\n00:00.000 --> 00:01.000\ncaption";
        let (raw, server) = serve("200 OK", "application/octet-stream", body).await;
        let shared = Arc::new(SharedState::new());
        let opts = EngineOpts { cookie: "session=sentinel".into(), referer: "https://www.youtube.com/watch?v=abc123".into(), ..Default::default() };
        assert_eq!(fetch_caption(&raw, "vtt", &opts, &shared).await.unwrap(), body);
        let request = server.join().unwrap().to_ascii_lowercase();
        assert!(request.starts_with("get /api/timedtext?secret=not-logged&fmt=vtt "));
        assert!(request.contains("cookie: session=sentinel"));
        assert!(!request.contains("range:"));
        let note = shared.note.lock().unwrap().clone().unwrap();
        assert!(!note.contains("not-logged"));
        assert!(note.contains("signature: vtt"));
    }
    #[tokio::test]
    async fn direct_html_empty_and_expired_responses_fail_validation() {
        for (status, body) in [("200 OK", b"<html>login</html>".as_slice()), ("200 OK", b""), ("403 Forbidden", b"expired")] {
            let (raw, server) = serve(status, "text/plain", body).await;
            assert!(fetch_caption(&raw, "vtt", &EngineOpts::default(), &Arc::new(SharedState::new())).await.is_err());
            server.join().unwrap();
        }
    }
}
