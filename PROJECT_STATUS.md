# DownloadCraft project status

Updated 2026-10-24 (America/New_York), final Codex audit, plus the Linux/macOS CI fix.

- Active branch: integration/upstream-v0.9.8. Desktop 0.9.8; extension 0.9.8.2.
- Upstream merge and post-merge work preserved. Debrid workflow is confirmed successful by the user; provider code unchanged.
- VTT/SRT downloads use validated direct subtitle GETs and supported metadata-only yt-dlp refresh. Exact supplied Spanish ASR Gemini caption downloaded successfully without video. Final packaged EXE recovered expired VTT and SRT selections, preserving the track.
- Final audit fixed a segmented resume validator race and archive escape through existing junctions/symlinks.
- Validation: 259 Rust release tests, 279 JS tests, successful release build with unchanged 112 warnings, Edge and Brave media smoke 9/9 each, verified extension and Windows packaging.
- **Linux and macOS CI (run 40140208731, exit 101) was a test bug, not a build bug:** the `#[cfg(unix)]` yt-dlp helper fakes read the bare `-o` name as a path and ignored `-P home:`, so they wrote into the working directory and the engine refused the file ("yt-dlp finished without writing a file"). The fakes now join the two as real yt-dlp does (upstream's own fix, `bfc0940`, which the merge base missed), and a new cross-platform test pins that contract so a Windows run sees the drift too.
- Windows EXE, portable ZIP, Chromium ZIP and unsigned Firefox XPI are in dist/final-codex-review. Full absolute paths and hashes are in validation/final-codex-review/artifacts.json.
- GUI interactions, Chrome/Firefox runtime and private/session-backed captions remain unverified. Existing moving helper downloads, per-user credential-at-rest/Windows ACL gaps and unsigned packaging remain documented limitations.
- The Linux and macOS packages (DEB, RPM, AppImage, universal macOS ZIP) are produced by CI only; there is no Linux or macOS runtime on the build machine. Upstream's own run of the same workflow is green, and the merge is left unmerged pending review.
- All work is local: no push, PR or public release. Existing untracked integration evidence was preserved.

See [the final audit](validation/final-codex-review/RESULTS.md) for the actual request trace, findings, checks, security limitations and artifact paths. Prior integration outcomes remain in docs/integration/INTEGRATION_RESULTS.md and docs/integration/POSTMERGE_098_RESULTS.md. The original workspace status is one directory above this checkout, and holds the full CI failure write-up.
