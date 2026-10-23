# Media detection: evidence, changes and remaining limits

Research dates: 2026-10-22–23 (America/New_York). Phase 3 starts from local Phase 2 commit `a156308` on `integration/phase-1-upstream-baseline`; its official foundation remains `c6ca86fa23bded14048aeb12c131d7e26fc80297`. Desktop 0.9.7 is preserved; the independently modified extension is local version **0.9.7.3**. Nothing was pushed or published.

## What was actually supplied

The input names suggest decompilations, but the actual files available in this workspace are **research reports**:

- Workspace-root `idm-reference.zip`: six Markdown files, including `IDM_FEATURE_RESEARCH.md`, browser/engine/recovery requirements, a gap checklist and an implementation plan. There are no executables, C/H decompilations or browser-extension scripts in this ZIP. Its SHA-256 is `a676a26d281ba168fd49beb47ae38ed367fed76df08523e7ff857296dfe7abd3`.
- Workspace-root `chrome-extension-reference/`: eight Markdown reports covering architecture, techniques, evidence, compatibility, a roadmap and test plans. There is **no CRX, raw manifest, extracted JavaScript, asset or license file** in this directory. The architecture report's SHA-256 is `bfb2868e277037bb3bf3cf7f22803bd5569bee4cb2c616faca98143d5908eda0`.

These inputs were listed/read as data. ZIP member text was read with Python's ZIP reader; no reference script/program was run and no embedded instruction was adopted. The reports refer to underlying CRX/Ghidra artifacts that are not supplied here. Their claims about those artifacts are therefore secondary evidence, not a fresh inspection of proprietary implementations. Reference files remain outside this application repository and Git history; only original analysis, implementation, tests and test output are committed.

Evidence terminology below:

- **Observed locally:** file inventory, DownloadCraft source and measured tests.
- **Publicly corroborated:** a primary public page independently supports a claim.
- **Reported:** an attached report states it, but raw source/binary was unavailable.
- **Inference:** plausible explanation or design implication, not demonstrated comparative performance.

## Chrome-extension identity, architecture and licensing

The reported extension is **MAX Video Downloader – download HLS, DASH, MP4/WEBM, LIVE streams**, ID `kjinbaahkmjgkkedfdgpkkelehofieke`, version **2.41.0**. The public [Chrome Web Store listing](https://chromewebstore.google.com/detail/max-video-downloader-%E2%80%93-do/kjinbaahkmjgkkedfdgpkkelehofieke) independently shows that name/version, updated October 14, 2026. This corroborates product identity, not the byte identity or signature of an absent CRX.

The following details are **reported in the supplied architecture/evidence documents**, not independently extracted here:

| Area | Reported facts |
| --- | --- |
| Manifest | MV3, minimum Chrome 109; module service worker `background.js`; content scripts for Telegram, Meta, Bilibili, Vimeo and iQIYI. |
| Permissions | `webRequest`, `declarativeNetRequest`, `offscreen`, `nativeMessaging`, `scripting`, `webNavigation`, `downloads`, `downloads.open`, `storage`, `unlimitedStorage`, `tabs`, `sidePanel`, `contextMenus`, `notifications`, `favicon`; host access `<all_urls>`. Raw manifest is unavailable for independent validation. |
| Discovery | Response/request observation plus targeted site adapters, selected main-world page-state bridges, and DOM/resource observation for some sites. HLS/DASH parsing identifies renditions/segments; navigation/session state separates detections. |
| Content scripts | Reports name `content/{telegram,meta,bilibili,vimeo,iqiyi}-content.js`, with main-world companions for selected sites and a metadata scraper. These files themselves are absent. |
| Media processing | Offscreen document/workers, a custom libav/FFmpeg WASM build, temporary browser filesystem storage, and an optional native companion. This is distinct from detection. |
| Communications | Runtime messages/ports between extension contexts; namespace-tagged page bridges; native host `pro.maxvideodownloader.coapp`. Reports also name feedback and entitlement endpoints on `maxvideodownloader.pro`. No endpoint or native companion was contacted in this work. |
| Persistence | Reports describe restoring per-tab detections when a service worker restarts. Not verified against the absent implementation. |

No license granting reuse of MAX's browser-extension implementation or assets is supplied. The public listing describes its *optional companion* as open source; that does not establish a license for the browser extension. The reports attribute licenses to embedded third-party libraries, but their build configuration, modifications and notices are unavailable. No proprietary code/assets or reported private protocol/algorithm were copied, translated or imported. No new dependency was introduced. DownloadCraft's existing root `LICENSE` is MIT; the changes are original code within that project.

## IDM findings and limits

The IDM report describes monitoring/integration helpers, a native-messaging-to-desktop bridge, browser handoff/duplicate decisions, floating media controls, queues, resume validation and stream assembly. It claims its absent Ghidra inputs covered seven Windows binaries, while omitting important DLLs and extension packages. Those binary-specific claims cannot be verified from this ZIP.

The useful behavioral requirements are independent of the claimed internal details: observe a request without mistaking every payload for a complete file; preserve authentication context appropriately; confirm receipt before considering a handoff successful; keep download/quality choices separate from playback fragments; retain recoverable work after failure. DownloadCraft already implements substantial parts of this, including Phase 2 acknowledgement/fallback and duplicate-click fixes. No IDM named-pipe, undocumented hook, private algorithm or licensing mechanism was implemented.

There was no side-by-side live IDM/MAX benchmark. The statement that a reference detects media “more effectively” remains the user's observation; the architectural explanations below are inferences checked against concrete DownloadCraft gaps.

## Why detection can differ

| Scenario | Reference reports suggest | Observed DownloadCraft gap / Phase 3 result |
| --- | --- | --- |
| Ordinary HTML5 video/audio | Network evidence and media analysis complement page adapters. | Existing DOM extraction and overlays already work. Generic octet-stream responses were also admitted; the new response classifier requires real media evidence and rejects conflicting HTML/JSON/image responses. |
| Dynamic source changes | Page/DOM observers and navigation state refresh discovery. | Existing observer watched child insertion only. Media `src`, `poster`, `type`, `controls` changes and `loadedmetadata` now trigger one debounced refresh; unrelated image attributes are ignored. |
| Embedded players and frames | Frame/navigation correlation is reported. | Network observer skipped media documents (`main_frame`/`sub_frame`/`object`), deduplicated only by URL, and `getMedia` could return another frame's streams. These are fixed with response-type coverage and sender/frame/document provenance. |
| HLS/DASH manifests | Manifest parsing and alternate tracks improve context. | Opaque HLS `audio/x-mpegurl` was not consistently classified. It now is. Optional enrichment is bounded and records explicit HLS references. DASH MIME/MPD detection stays supported; full DASH track parsing remains in the desktop engine. |
| Playback segments | A complete manifest is distinguished from individual parts. | A lone unrelated manifest in the same frame could be treated as a fragment's parent. Only explicit manifest-reference paths now establish that relationship; ambiguous roots do not choose a random parent. |
| Separate audio/video and qualities | Reported manifest/group parsing and media processing expose tracks. | HLS audio-rendition and quality references now remain attached to their actual master, preserving exact URLs. Existing native pairing/FFmpeg/quality paths are retained. A new language/track-selector UI is not implemented. |
| Expiring URLs | Reports describe session/header context and freshness. | Exact signed query strings remain unchanged, repeated evidence refreshes metadata/timestamps, and failed enrichment retains the original observed URL. Automatic token renewal or reliable expiry prediction is not implemented. |
| Browser downloads/redirects | Download observation and request context are distinct from media detection. | Existing browser takeover, POST protection and fallback are preserved. Redirect/error *responses* are excluded from media detection; a successful final response can be observed normally. Optional manifest re-fetch refuses redirects and may therefore lack enrichment. |
| Resources missed before injection or on unusual sites | Reported targeted page adapters and resource timing can help. | No arbitrary response-body capture, generic MAIN-world monkey-patch or site-private extractor was added. Cache/pre-injection, MSE, service-worker and closed-shadow-root gaps remain. |

The public [Chrome webRequest documentation](https://developer.chrome.com/docs/extensions/reference/api/webRequest) establishes observation and host-permission boundaries; it does not promise complete visibility into every browser request. Firefox's [response event documentation](https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/API/webRequest/onHeadersReceived) documents its related API. This work adds no permission and does not reinstate Firefox-only blocking behavior on Chrome.

## Implemented local commits

| Commit | Change |
| --- | --- |
| `709ab46` | Pure response-evidence classifier; permitted media document/frame types; conservative MIME/status/method checks, whole-file range sizes, and zero size threshold support. |
| `e6ff146` | Immediate candidates, merged repeated observations, bounded/coalesced optional metadata, and rejection of async results after navigation/tab removal. |
| `420e9af` | Frame/document-scoped content queries and adaptive/Vimeo handoffs; no arbitrary iframe selection for a shared URL. |
| `02fa2f1` | Explicit HLS child/rendition association, bounded references, key-URI exclusion, unique-root selection and ambiguity/cycle protection. |
| `d8cfbb3` | Native smoke test caught that extension pages can themselves have `sender.tab`. Distinguish their selected-tab queries from content-script queries; regression fixed. |
| `8fd918d` | Debounced source-attribute and metadata-event discovery refreshes. |
| `70cf3a9` | Local extension 0.9.7.3, fixture exclusion from packages and reproducible isolated Edge test tooling. |
| `5492aca` | Additional regression coverage for global probe concurrency and retained multi-track metadata on re-observation. |
| `ca4f6d7` | Generic browser argument for the smoke tool, validated against the installed Brave browser. |
| `59a2de0` | Refresh unchanged playback observations in memory without repeated snapshot/badge writes; publish only new evidence. |

## Validation and browser compatibility

**Automated regression suite:** Phase 2 baseline 123 tests; final **157 passed, zero failed/skipped**, 17 suites. JavaScript syntax checks and Python test-tool compilation passed. Coverage includes positive/negative response fixtures, partial-response sizes, exact signed URLs, HLS alternate audio/qualities, oversized/failed probes, concurrent probe budgets, navigation races, source changes, frame ambiguity, and all prior interception/media regressions.

**Actual browser:** isolated headless **Microsoft Edge 156.0.4314.42** on Windows passed **nine checks** with the real unpacked extension: direct MP4, native audio, HLS, DASH, permitted cross-origin iframe video (`localhost` versus `127.0.0.1`), exclusion of binary/HTML/403 fixtures, video/audio labels and control rebinding after an audio-source change. The test-only manifest removes native messaging and disables automatic capture in that new profile, so the desktop and user's normal profile are not involved. Test orchestration uses DevTools; application scripts run through their normal extension lifecycle.

The same nine checks also pass in installed **Brave**, whose executable reports product version **155.1.97.57**, using another isolated profile. Chrome and Firefox were not found at the checked system/user install paths. Test runtimes: Node 24.19.0, Python 3.14.7 and FFmpeg/ffprobe 9.0.2. These native tests validate detection/control behavior on two Chromium-family browsers, not all browser/platform combinations.

Samples are independently generated with installed FFmpeg: a two-second test pattern/tone, WAV, HLS and separate-stream DASH. ffprobe confirms audio and video in the generated MP4. Static multi-track HLS test text is also authored here; it verifies associations, not successful playback of every declared rendition. No reference media/program is used.

Early headless harness attempts did not complete. After startup/readiness handling was corrected, the test exposed a real extension-page query regression, which was fixed before the final pass. Failed diagnostic logs are retained rather than represented as passing compatibility results.

| Environment | Finding |
| --- | --- |
| Edge / Windows | Native detection/control smoke passes in an isolated profile; full desktop handoff intentionally disabled in this smoke. |
| Brave / Windows | All nine native detection/control checks pass, with native messaging disabled in the test-only profile. |
| Chrome | Shared Chromium source/manifest and regression harnesses pass; native Chrome binary unavailable at checked system/user install paths. |
| Firefox | Shared-source tests, Firefox-only listener guard and Firefox package validation pass; no native Firefox runtime test on this machine. |
| Linux/macOS | No native browser/desktop test in this phase. |
| Desktop Rust | Source/Cargo/version untouched. Prior missing-Cargo build gate remains; no new Rust build or complete browser-to-desktop download claim. |

`python scripts/package_extensions.py --output dist/phase3-extensions` stages both browser variants and an unsigned Firefox XPI; inspection verifies script bytes, manifests and exclusion of tests/fixtures. Logs are committed under `docs/research/evidence/`.

## Limitations and next work

- Detection is a candidate, not proof of complete downloading, correct muxing, media duration or successful cancellation. Live Vimeo/yt-dlp and native desktop output still need separate tests.
- MV3 in-memory detected-media state is not restored after worker termination. Session-scoped, bounded persistence is a useful next change, with navigation/privacy tests before implementation.
- Requests before extension activation, browser/service-worker caches, unsupported request types and prohibited origins may remain invisible. No broad permission, debugger attachment in production or network-hook workaround was added.
- Reused iframe documents without browser-provided document identifiers remain less precise. Explicit page mismatches now yield no candidates rather than falling back to an old page; this can be conservative on lagging SPA metadata.
- Multiple players in one frame can remain ambiguous. No guessed cross-player video/audio pairing is performed. A complete selected manifest is preserved or an existing single-player-page resolver is used when appropriate.
- Alternate language/subtitle/codec track UI, full browser DASH representation metadata and consented request-context replay remain future work. DRM labeling is not expanded, and no new decryption/key-fetching/bypass mechanism is introduced; existing desktop protected-stream guards are unchanged.
- Metadata inspection is optional: authenticated, one-use or redirected manifests may not enrich. HTTP errors do not refresh an old candidate into a valid new one, and signed-link expiry is not automatically repaired. Reload/replay in the original page may be required.
- Only 25 candidates per tab are retained as before. Budget exhaustion skips optional metadata instead of blocking detection. Full-field telemetry and persistent request headers/cookies were not added.

For complete extraction rather than discovery hints, DownloadCraft's existing yt-dlp path is preferable to an ad-hoc private-site parser. Its public [format-selection and listing documentation](https://github.com/yt-dlp/yt-dlp#format-selection) supports planning a richer track UI; integrating that output still requires validated desktop builds. HLS relationships here follow public [RFC 8216](https://www.rfc-editor.org/rfc/rfc8216.html), not reference application source.
