# DownloadCraft project status

Updated 2026-10-24 (America/New_York), final Codex audit.

- Active branch: integration/upstream-v0.9.8. Desktop 0.9.8; extension 0.9.8.2.
- Upstream merge and post-merge work preserved. Debrid workflow is confirmed successful by the user; provider code unchanged.
- VTT/SRT downloads use validated direct subtitle GETs and supported metadata-only yt-dlp refresh. Exact supplied Spanish ASR Gemini caption downloaded successfully without video. Final packaged EXE recovered expired VTT and SRT selections, preserving the track.
- Final audit fixed a segmented resume validator race and archive escape through existing junctions/symlinks.
- Validation: 258 Rust release tests, 279 JS tests, successful release build with unchanged 112 warnings, Edge and Brave media smoke 9/9 each, verified extension and Windows packaging.
- Windows EXE, portable ZIP, Chromium ZIP and unsigned Firefox XPI are in dist/final-codex-review. Full absolute paths and hashes are in validation/final-codex-review/artifacts.json.
- GUI interactions, Chrome/Firefox runtime and private/session-backed captions remain unverified. Existing moving helper downloads, per-user credential-at-rest/Windows ACL gaps and unsigned packaging remain documented limitations.
- All work is local: no push, PR or public release. Existing untracked integration evidence was preserved.

See [the final audit](validation/final-codex-review/RESULTS.md) for the actual request trace, findings, checks, security limitations and artifact paths. Prior integration outcomes remain in docs/integration/INTEGRATION_RESULTS.md and docs/integration/POSTMERGE_098_RESULTS.md. The original workspace status is one directory above this checkout.
