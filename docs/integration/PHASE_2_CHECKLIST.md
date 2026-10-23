# Phase 2 selective integration checklist

Phase 3 update: the historical results below remain the Phase 2 snapshot. The current extension is 0.9.7.3; see [media detection research and current validation](../research/MEDIA_DETECTION_ANALYSIS.md) for the subsequent implementation, 157-test suite and passing native Edge/Brave smoke results.

Completed local implementation pass: 2026-10-22. Branch: `integration/phase-1-upstream-baseline` in the existing `upstream-baseline` checkout. Official master was fetched/rechecked and remained `c6ca86fa23bded14048aeb12c131d7e26fc80297`. Experimental reference remains `51b12f65883cde382ed36c6caca4439abc114b0c`.

The Phase 1 reports in workspace-root `docs/integration/` were read before changes. Their native-build gate remains unresolved; this pass deliberately implements and validates extension JavaScript only. Desktop Rust application logic and Cargo dependencies are byte-for-byte unchanged relative to the official baseline. This is an unreleased local extension build, **0.9.7.2**, compatible with the existing native-message protocol; desktop version remains **0.9.7**.

## Completed changes and local commits

| Done | Commit | Implementation and regression evidence |
| --- | --- | --- |
| [x] | `7586c24` | Require native download `ok` acceptance; reject `pong`, malformed responses and `queued:false`, retain useful host errors, clear stale errors after success. Unexpected host rejection and download-UI storage failure cannot prevent browser fallback after cancellation. 109 tests passed. |
| [x] | `6dd9c5f` | Suppress repeated identical media handoffs and archive-link clicks while awaiting a response. Keep distinct quality selections and later retries; failed link handoff replays the original anchor once. 113 tests passed. |
| [x] | `c3397f9` | Save normalized drag coordinates in local extension storage across browser restarts. Migrate valid prior session positions without overwriting a newer saved preference. Ignore corrupt coordinates and tolerate browsers without session storage. 115 tests passed. |
| [x] | `8c21747` | Add Download this audio controls for visible native HTTP(S) audio-file players, with matching action/dismiss/accessibility labels and audio filenames/MIME. Position outside native controls, skip hidden/blob/adaptive audio and small previews, hide bars for unrelated fullscreen content, remove bars for previously dismissed sources on reused players. Explicit audio MIME cannot borrow unrelated video expectations. 121 tests passed. |
| [x] | `e97abc3` | Align both extension manifests and script constants at local 0.9.7.2; add version consistency regression and verify staged Chromium/Firefox extension packages. 122 tests passed. |
| [x] | `32c3e5a` | Remove old quality menus when a dynamic page changes, dismisses or removes its player. 123 tests passed. Packages rebuilt after this change. |

Changes were applied as targeted edits rather than experimental file replacements. Each implementation commit followed JavaScript parsing checks, the full extension regression suite and Git diff review/check. All commits are local; no push, release publication, PR, fork merge, account operation or extension installation occurred.

## What 9.6.1 contributed versus this pass

| Area | Contribution already present in official upstream | What actually changed in Phase 2 |
| --- | --- | --- |
| Browser takeover | PR #51 already supplies IDM-style handoff, Chromium download-event deduplication, POST protection and failure replay; upstream additionally fixes Firefox-only listener registration on Chrome. | Stronger acceptance/failure handling and pending-click duplicate suppression. The Chrome listener guard is preserved. |
| Embedded controls | Compact draggable video toolbar, separate X, session placement, fullscreen handling, per-source dismissal and popup restore were already merged. | Persistent profile placement, native audio-file bars, tighter fullscreen/source/menu cleanup. No broad page overlay or custom-player DOM replacement was introduced. |
| Vimeo and media | Exact Vimeo frame/player routing, complete-manifest handling, paired audio/video and FFmpeg processing already exist. | Media handoff acknowledgement is stricter and audio MIME no longer borrows video expectations. Existing Vimeo and media tests continue passing. |
| Quality/progress/yt-dlp | Upstream already sends quality/pair/duration metadata and has engine quality selection, yt-dlp progress handling, child cancellation and completion checks. | Retained. Concurrent different-quality handoffs are explicitly regression-tested; no older engine/UI replacement. |
| Cross-platform/UI | Existing contribution and upstream review already provide platform paths, packaging and UI changes. | Retained, including Windows dynamic title-bar API and full FFmpeg settings controls. |
| New unpublished experimental subsystems | Safe resume, dynamic ranges, named queues, address refresh, synchronization, mirrors and cloud providers are genuine unique development work. | Not imported in this extension-focused pass; see deferred gate below. |

Pending-click suppression is not a new persisted duplicate-download dialog or an automatic permission to replace completed files. A deliberate later request still reaches the existing desktop workflow. No acknowledgement timeout/retry was added: replaying after an uncertain timeout could start a second download if the desktop already accepted the first.

## Validation results and usable local artifacts

- [x] Baseline before changes: Node v24.19.0, 106 tests passed.
- [x] Final: `node --test browser-extension/*.test.js` — **123 passed, 0 failed, 0 skipped**, 17 suites.
- [x] `node --check` passed for all extension `.js` files; affected files were checked after each logical change.
- [x] `python scripts/package_extensions.py --output dist/phase2-extensions` succeeded using Python 3.14. Both browser variants are staged, and an unsigned Firefox XPI is built.
- [x] Package inspection verifies correct Chromium service-worker and Firefox background-script manifests, version 0.9.7.2, exact source bytes for background/content/rank/popup scripts, and exclusion of test files from XPI.
- [x] Final Firefox XPI SHA-256: `214eaea9ee08a1bb63dd4cff78a51947844160267a94f50298ffc1507983d581` (local artifact; ZIP metadata may vary on a rebuild).
- [x] Git source diff reviewed; no Rust, Cargo, desktop version, Windows linker, helper UI or fork workflow changes.
- [ ] `cargo test --locked --release` and `cargo build --locked --release`: attempted again, **blocked because Cargo is not installed/on PATH**. No Rust executable was produced; this is not a successful desktop build.
- [ ] Native Chrome/Brave/Edge/Firefox installation and UI smoke tests: not run. Browser APIs and DOM behavior are covered with local harnesses, which do not establish native runtime parity.
- [ ] Live Vimeo/yt-dlp/FFmpeg downloads, cancellation, progress and output verification: not exercised on this machine. Existing source behavior is preserved, not newly runtime-certified.
- [ ] Native Linux/macOS builds and package/UI smoke checks: deferred.

Evidence is committed in `docs/integration/evidence/phase2-*.log`, including per-change tests, final tests, packaging verification and Rust toolchain errors. Workspace-root Phase 1 evidence is preserved separately.

Locally staged build outputs (ignored by Git):

- `dist/phase2-extensions/browser-extension/` — Chromium-family unpacked extension.
- `dist/phase2-extensions/browser-extension-firefox/` — Firefox staged source.
- `dist/phase2-extensions/DownloadCraft-Firefox-0.9.7.2.xpi` — unsigned development add-on.

Reload the development extension and existing media tabs when testing. Use an isolated browser/app profile for native smoke tests; the packaged extension is not installed automatically.

## Deferred or incompatible items

| Item | Reason / next validation gate |
| --- | --- |
| Safe resume, staged output replacement, address refresh | High-value next Rust work. Provision stable Rust and Windows linker/resource tooling, establish baseline release build/tests, then port one subsystem with changed-source/cancellation/Windows filesystem fixtures. Address refresh depends on preserving correct saved-byte validation. |
| Dynamic ranges and persisted ledger | Architectural engine change; isolate after resume validation. Requires interruption/coverage/legacy-part tests and host-cap/rate-limit benchmarks, not a bulk cherry-pick. |
| Named queues/retries/synchronization | Persistence and scheduler changes; require migration/restart/manual-pause/overnight/other-queue action tests and validated safe replacement. |
| Static mirror and debrid | Separate substantial workflows; mirror parsing/limits need adversarial fixtures, providers need authorized account/runtime validation. Do not forward normal URLs automatically or import tokens/config. |
| Audio-only blob/adaptive streams and custom audio widgets | New audio bar intentionally targets visible native audio-file players. Existing popup detection remains available; complete adaptive audio resolution needs engine/media fixtures before broadening this UI. |
| Persisted duplicate/re-download decision UI | Current change only suppresses accidental in-flight duplicates. A new desktop decision dialog needs validated queue/file-state semantics and Rust/UI builds. |
| Experimental Chrome listener, Windows static API import, removed FFmpeg controls, version rollback/update policy | Incompatible regressions/policy changes; excluded. |
| Additional fork workflow | No unique app code. Automatic publishing, unvalidated action refs and removal of macOS coverage excluded; optional extension artifact packaging can be considered independently. |

The next native validation sequence is: provision toolchain, run baseline locked release tests/build, then test extension handoff success/failure with an isolated profile on each supported browser, drag/resize/fullscreen and SPA source changes, and representative media output/cancellation checks. Only then expand into the unique Rust subsystems.
