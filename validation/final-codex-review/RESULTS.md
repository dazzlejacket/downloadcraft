# DownloadCraft 0.9.8 — final Codex audit and release candidate

Date: 2026-10-24 (America/New_York). Desktop 0.9.8; extension 0.9.8.2.
Base: integration/upstream-v0.9.8 at 2a5925a737c07d07371c2d54292de8bcf3af482d.
Code commit: 599e46853b477d158830909f3f30133b12cf23e6. The documentation commit following it is the final local HEAD (see `git log -2`). Nothing pushed, published or submitted as a PR.

## Outcome

Fixed subtitle-only downloading. The exact user-supplied Spanish ASR Gemini track was downloaded by the rebuilt Windows executable: 220,928 bytes, WEBVTT header, 1,509 cues, no HTML, and just one .vtt output. The final packaged EXE also recovered deliberately expired selections for the same video/language/type/variant in VTT (220,928 bytes, 1,509 cues) and SRT (49,134 bytes, 755 cues). No video was downloaded. Live tests used isolated application settings and no browser cookies. Existing debrid code was not changed; the user's confirmed successful debrid workflow is preserved.

## Caption investigation and request trace

1. `content.js::ytCaptionTracks` reads the player caption base URL and appends fmt=vtt when no format is present. The signed query, language, kind, variant, name and translation parameters are retained. The chip calls `sendDownload` with the selected URL and subtitle name/MIME. SRT chips now advertise SRT and carry .srt rather than .vtt.
2. `background.js::sendToHost` sends the exact URL. It selects observed request credentials when available, otherwise the download host's cookies, page Referer and browser User-Agent. The existing page-only credential guard is preserved. Observed Origin/Authorization/x-* headers remain bound to the captured site. No missing query-parameter mutation was found along this path.
3. `native_host.rs::inbox_item` and the confirm process arguments retain that URL. Cookie/UA/Referer and the filtered header handoff flow into EngineOpts; confirmation and main-window probes use `needs_resolve`.
4. The prior predicate excluded timedtext from YouTube video detection, but HTML still independently made `needs_resolve` true. The transfer also had a generic HTML resolver path. Therefore an empty timedtext response labelled text/html could enter media resolution or generic webpage refusal. There was no dedicated subtitle validation/refresh route.
5. Captions now enter `download_caption` before all Vimeo, manifest and video routing. `needs_resolve` returns false for timedtext even when its MIME is HTML. URL classification uses parsed hosts and endpoint paths rather than substring host matches.
6. Direct caption retrieval is a GET with the existing EngineOpts session context; it makes no HEAD or Range request. Redirects are bounded, manually inspected, and restricted to HTTPS YouTube hosts before credentials are sent. Diagnostics record status chain, final host/path, MIME and a signature category; queries, cookies, response text and helper stderr are excluded.
7. The supplied URL returned HTTP 200, text/html; charset=UTF-8, **zero bytes**, no redirects, final host/path www.youtube.com/api/timedtext. Both the desktop-default UA and a browser UA with Referer produced that result. Its expire value was 2026-10-25 05:01:12 UTC, after the 2026-10-24 22:11:31 UTC request: expiry was not the cause of this observation. The in-app browser displayed an empty document at the same URL. No consent/login/challenge HTML body was returned in these checks. A signed-in original-browser request and its exact network headers were unavailable; browser-UA HTTP requests are not claimed to reproduce that session or TLS stack.
8. Supported yt-dlp metadata extraction from the original video URL succeeded. yt-dlp exposes the original ASR track under an `es-orig` dictionary alias, so matching uses the actual caption URL identity rather than that display key. The fallback accepts only matching video, language, kind, name, translation, Gemini variant and format, then retrieves and validates the fresh timedtext URL.

The fallback invokes `--ignore-config --no-playlist --skip-download --dump-single-json`; it does not invoke media downloading, subtitle conversion, ffmpeg or video format selection. Existing helper source preferences, proxy/UA and a temporary captured-cookie jar are reused. Extraction has a 90-second timeout, kill-on-drop cancellation, suppressed sensitive stderr and a 16 MiB metadata limit. Direct responses have a 30-second request deadline and 8 MiB size cap. Browser-check markers stop the attempt. No CAPTCHA, DRM or authentication bypass was added.

VTT/SRT signatures are validated independently of MIME before a file is created. HTML, empty and non-subtitle responses fail; invalid UTF-8 fails. Names receive the selected extension, existing files get a conflict-free name, requested checksums are verified before final rename, and progress reflects subtitle bytes. Unavailable/expired/private/restricted captions produce an actionable recapture error without saving a subtitle or falling back to video.

## Confirmed audit fixes

- **Segmented resume race:** the first unrestricted full suite had 254 passes and one failure in `segmented_parts_of_a_changed_resource_are_not_merged`. A fresh segment could update the shared sidecar validator before a resumed sibling read it, mixing versions. Each worker now receives the saved validator snapshot from the start of the segmented attempt. Existing mismatch/restart logic is retained. The full suite passes after this targeted change.
- **Archive junction traversal:** `target_path` checked lexical containment but could follow an existing directory junction/symlink inside the extraction destination. Each existing path component is now checked; symlinks and Windows reparse points are refused. A real Windows junction fixture verifies that `write_entry` refuses the write and no outside file appears. Extraction still does not execute files or overwrite existing files by default.

## Independent integrated-code review

This was a review of the checked-out source, not a re-merge or rewrite of earlier fixes. Read parent AGENTS.md, PROJECT_STATUS.md and validation/postmerge-098/RESULTS.md, in-repo integration reports/checklist and recent caption/security commits. Parent documents are outside this Git worktree; a current status is now also recorded in this repository.

| Area | Actual code reviewed and evidence | Result / scope |
| --- | --- | --- |
| Five debrid providers | debrid.rs provider capabilities/endpoints/envelopes, TokenStore, CloudSession, account validation, checked_link, safe_id, bounded retries; app provider picker | No changes. Full mocked provider tests pass, including selection, cloud jobs and token isolation. User confirms live workflow; no live accounts retested. |
| Browser/desktop handoff and duplicates | background sendToHost, native host framing/inbox/header transport, confirm credential claim, dupcheck identities and pending-click handling | Existing tests pass. URL/query retained. Full installed-browser-to-GUI handoff was not driven live. |
| Bandcamp, Audio category/folders | rank.js Bandcamp stream/track/name mapping, categoryOf, model category_of/default_folder_rules | Existing regression tests pass; no changes or file moves. |
| Overlay | content/popup controls and removal/migration tests | Single classic action/close bar; no question-mark control restored. |
| Browser compatibility | both MV3 manifests, Chrome extraHeaders guard, Firefox background scripts; package manifests and exclusions | Versions remain 0.9.8.2. Edge and Brave real isolated smoke pass. Chrome/Firefox runtime behavior was not exercised. |
| HLS/DASH, yt-dlp, ffmpeg | playlist routing, manifest validation, existing helper selection and output verification | Existing media/resume/local HTTP tests pass; real caption extraction passes. Full live video muxing not repeated. |
| Queue/resume | model queue corruption handling/locks, validators/If-Range, single/chunked/segmented paths | Snapshot race fixed; complete release tests pass. |
| Archives | ZIP/RAR/7z safe-relative paths, target paths, extraction bounds and no-execution rules | Junction gap fixed; archive tests pass including real Windows junction. |
| Credentials/native messaging | per-provider tokens, filtered captured headers, temporary cookie jar, max native message size, filename sanitization, canonical allowed roots | Existing security tests pass. See residuals below. |
| Migration/packaging | model obsolete overlay keys/settings, platform exclusions, package scripts, manifests | Migration tests pass. All final ZIP/XPI entries checked; no tests/fixtures packaged, versions verified, archive CRC tests pass. |

## Validation actually run

| Check | Result |
| --- | --- |
| cargo test --locked --release | **258 passed, 0 failed**, 30.44 s final run |
| cargo build --locked --release | **Success**, 112 Rust warnings (baseline unchanged); resource warning remains |
| node --test browser-extension/*.test.js | **279 passed, 0 failed**, 35 suites |
| node --check browser-extension/content.js | Success |
| python scripts/check_merge_symbols.py --base v0.9.8 | No symbol lost; 2 intentional replacements |
| python scripts/media_detection_smoke.py --browser <Edge> | **9/9**, passed |
| same smoke tool with Brave | **9/9**, passed |
| python scripts/package_extensions.py --output dist/final-codex-review/extensions | Chromium folder, Firefox folder and XPI built |
| python scripts/package.py --platform windows --arch AMD64 --output dist/final-codex-review/windows | Portable Windows ZIP built |
| ZIP/XPI inspection | CRCs pass, extension version 0.9.8.2, test/fixture exclusions verified |
| Real supplied caption request and rebuilt EXE | Empty direct response reproduced; fallback saved valid VTT only |
| Final packaged EXE, expired VTT/SRT selections | Both succeeded; only selected subtitle output; see final-live-engine.json |
| git diff --check | Clean |
| Desktop GUI smoke | Not driven: native UI automation is disabled in this session. CLI engine and real browser extension were exercised. |

New caption regressions cover direct success with imperfect MIME and captured cookie/Referer, HTML/empty/403 failures, expired-track refresh selection, Gemini/translation/name/type/format isolation, original-ASR dictionary aliasing, and a real mock helper subprocess whose arguments require metadata-only mode and produce no media. Existing resume regression caught the snapshot race. A new Windows junction test covers extraction containment. The exact live subtitle-only outputs complement those offline regressions.

Initial sandboxed Rust/browser runs could not use required loopback/temp fixtures. They were rerun with the necessary access; failures from that environment are not reported as application passes. A Windows PowerShell log file lock was resolved before final reruns. Packaging first hit restricted system-temp permissions, then succeeded with process-local TEMP/TMP under the checkout. No system settings changed. Full final logs remain under dist/final-codex-review; sanitized evidence and summary.json are committed here. Pre-existing untracked integration logs were left untouched and uncommitted.

## Security findings and remaining limitations

- Fixed: archive junction escape and segmented mixed-version resume; caption responses cannot enter video routing or save HTML. Credential-bearing redirects are limited to approved YouTube hosts on this path.
- Existing residual: yt-dlp and ffmpeg-static use moving latest download URLs without a pinned payload digest. Deno remains pinned and checked. Changing helper update policy is outside this targeted fix.
- Existing residual: queue/inbox session cookies and signed URLs can persist in per-user state until normal cleanup. Windows config-directory hardening is still absent (`secure_config_directory` hardens Unix only). No token-store serialization was added. Temporary yt-dlp cookie jars are deleted on normal drop, but a process crash can leave one behind; Windows uses inherited per-user temp ACLs.
- Extraction refuses pre-existing links/reparse points. It is not claimed to defeat a malicious local process racing filesystem changes between validation and opening a file; handle-relative filesystem operations would be a separate hardening project.
- Caption direct transfer is intentionally small and restarts instead of range-resuming. SRT/VTT only; other timedtext serialization formats receive a clear unsupported-format error. Empty tracks without cues are treated as unavailable.
- A live authorized private-caption/session capture, browser-native-to-confirm GUI flow, CAPTCHA refusal UI, desktop dialog interactions, Chrome and Firefox runtime smoke, and other OS builds remain unverified. No claim of bypassing restricted/private tracks or all YouTube client variants is made.
- XPI is unsigned (temporary installation/developer use; ordinary Firefox distribution requires Mozilla signing). Windows binary is unsigned; build.rs still reports missing windres icon/version resource embedding. Existing compiler warnings remain.

## Artifacts

Absolute paths and SHA-256 values are in artifacts.json. These are local candidates, not a public release.

- `C:\Users\grego\Documents\Codex Projects\DownloadCraft\integration-098\dist\final-codex-review\extensions\DownloadCraft-Chromium-0.9.8.2.zip` — 161,092 bytes; SHA-256 `87aa10991838b9edf969ad31766f10ec7997bd2027f5d54a70d6dd3cfc9878ab`
- `C:\Users\grego\Documents\Codex Projects\DownloadCraft\integration-098\dist\final-codex-review\extensions\DownloadCraft-Firefox-0.9.8.2.xpi` — 158,905 bytes; SHA-256 `64a4315326be92b1708cb3f68948a3662060c31e41706ed522f0c8f37d2fd639`
- `C:\Users\grego\Documents\Codex Projects\DownloadCraft\integration-098\dist\final-codex-review\windows\DownloadCraft-0.9.8-windows-AMD64.zip` — 9,752,077 bytes; SHA-256 `1ee8faf89cc44620cdc8eb3373acb72bba1f42a4284ceb54c152d08664dade86`
- `C:\Users\grego\Documents\Codex Projects\DownloadCraft\integration-098\dist\final-codex-review\windows\DownloadCraft-0.9.8.exe` — 21,512,704 bytes; SHA-256 `6b896e6a3721909095d65f08042f54e6d942b09288043947dc204f2f16b71337`

Official helper option reference: https://github.com/yt-dlp/yt-dlp/blob/master/README.md (metadata output, skip-download and subtitle extraction options).
