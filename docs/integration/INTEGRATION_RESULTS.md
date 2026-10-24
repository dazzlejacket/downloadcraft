# Upstream v0.9.8 integration — results

**Branch:** `integration/upstream-v0.9.8`  **Worktree:** `integration-098/`
**Base:** `feature/ui-redesign` @ `5c26558`  **Merged:** upstream `v0.9.8` @ `5ac3134`
**Merge base:** `c6ca86f` (32 local-only commits, 17 upstream-only commits)

The merge and every conflict decision are recorded in
[`UPSTREAM_098_CHECKLIST.md`](UPSTREAM_098_CHECKLIST.md). This file is the
outcome: what was done, what was verified, and what is still open.

---

## The headline numbers

| | Before | After |
|---|---|---|
| Rust tests | 209 passed, 0 failed | **228 passed, 0 failed** |
| JS tests | 222 passed, 0 failed | **258 passed, 0 failed** |
| Rust warnings (`cargo build --locked --release`) | 112 | **112** (unchanged) |
| Conflicting files / hunks | — | 16 / 55, all resolved |

The test count grew by 19 Rust tests and 36 JS tests: upstream's own tests
came in with the merge, and three tests were added with the security work.
The warning count did not move — upstream's new code raised it by 14, and
all 14 were fixed (see *Warnings* below).

---

## What was done, wave by wave

### Wave 0 — baseline

* Local HEAD, the merge base, the toolchain and the clean/dirty state were
  recorded **before** anything changed.
* The pre-change suites were run and their counts captured
  (`evidence/wave0-baseline-rust-tests.log`, `evidence/wave0-baseline-node-tests.log`).

**The "222 versus 207" discrepancy is resolved: no test was missing.**
The audit's static count could not see 15 tests:

* `browser-extension/detection.test.js` generates **14** tests from a
  `for (const f of fixtures) test(...)` loop over
  `browser-extension/fixtures/detection-responses.json`. The `test(` call is
  not at the start of a line, so a line-anchored grep misses it.
* `browser-extension/fb.test.js` has one `it(` that shares a line with its
  `describe(` opener (line 174), so the grep misses that too.

207 + 14 + 1 = **222**, exactly what `node --test` reports.

* Sanitised representatives of a live `queue.json`, `ui.json` and
  `settings.json` were written to `tests/fixtures/config/` by
  `scripts/make_config_fixtures.py`. Cookies, usernames, passwords, the
  proxy URL, media titles and absolute user paths are all replaced by
  sentinels — nothing is copied verbatim.

### Wave 1 — the merge

The merge produced **exactly** the 16 files and 55 hunks the audit predicted,
so the conflict map could be applied hunk for hunk. All 55 were resolved by
hand against `validation/upstream-098/CONFLICT_MAP.md`; the file-by-file
account is in the checklist.

Two resolutions are worth calling out because they were not mechanical:

* **`confirm.rs` had two writers for one file.** Upstream's `wake_targets`
  wrote `{"start": [...]}` and the fork's `focus_targets` wrote
  `{"focus": [...]}` — each replaced the other's content, so one request was
  silently lost. There is now one writer, `wake_requests(start, focus)`,
  that merges both arrays. The main window's `check_wake()` already read
  both keys from the same file.
* **`engine.rs` had a corrupted-restart bug.** The fork's QR-02 code tested
  `start_at > 0` *after* it had already reset `start_at` to 0, so a restart
  over an existing `.part` file kept the old tail. Upstream's branch covers
  the fork's case as well, so the local `truncate_to_zero` variable went
  away.

### Wave 2 — security

Ten findings from the audit. Six were fixed, two accepted with a reason,
and two were already satisfied.

| # | Finding | Outcome |
|---|---|---|
| S-1 | Upstream downloads and executes an **unpinned** binary | **Fixed.** Pinned to Deno `2.9.8`; the archive's SHA-256 is checked against the release's own checksum asset **before** it is unpacked. A mismatch writes nothing and runs nothing. Hashes for all six platforms are in `engine::deno_zip()`. |
| S-2 | New credential-at-rest surface | **Fixed.** `extra_headers` is `#[serde(skip)]` — it never reaches `queue.json`. It lives in memory for the transfer and is cleared on completion, like the cookie. |
| S-3 | Signed media URLs leave the app | **Fixed.** `app_downloads()` returns the URL only for rows that are still running. |
| S-4 | A path from the browser is trusted | **Fixed.** The file must sit inside the user's `Downloads` folder or the app's download folder; `path_is_under` needs a whole-path prefix, so `Downloads Evil` does not pass as `Downloads`. |
| S-5 | Page cookies written to disk in plaintext | **Fixed.** `LinksBatch` has no `cookie` field. The cookie travels with the process (`DOWNLOADCRAFT_LINKS_COOKIE`); the batch file is deleted when it is read, and a stale batch is cleared before a new one is written. |
| S-6 | Extra outbound traffic | Accepted: the release-tag check already runs only on the explicit `--update` path. |
| S-7 | Extra request per ambiguous URL | Accepted: the 4 KiB cap is kept and the manifest is reused for the same URL. |
| S-8 | BOM bug in the `query` reply path | **Fixed.** `queued_urls()` strips a BOM exactly like `app_downloads()`. |
| S-9 | Upstream's two extension security fixes | Adopted: no cookies for page-only-reported URLs; `blob:` URLs rejected. |
| S-10 | No native torrent/P2P engine | Unchanged: every magnet still goes to the cloud provider API, and a magnet can never enter the download queue or the Links window. |

Three tests were added with the work: a magnet is never a download URL, the
links batch never carries the cookie, and a browser path must sit inside a
download folder.

**Pre-existing, not from this merge:** `yt-dlp` and `ffmpeg-static` are
still fetched from a moving `releases/latest` URL. That is the fork's
long-standing behaviour and it was left alone, but it deserves the same
treatment as Deno in a later pass.

### Wave 3 and 4 — the extension

All 26 JavaScript conflict hunks were resolved and the merged extension
passes **258 of 258** tests.

The most important decision was about the **media observer**. The merge
briefly ran two of them side by side: the fork's `observeMediaResponse` and
upstream's `onMediaResponse`. Two observers record the same response twice
and probe the same manifest twice, and upstream's rule is looser than the
fork's on one point the fork has deliberately tightened — a binary installer,
a WASM file or an HTML error that claims to be a video is **not** media
(`browser-extension/fixtures/detection-responses.json`).

There is now **one** observer. The fork's rule decides what a media response
is, and a response it rejects costs no further traffic; upstream's
classifier then *refines* what was already accepted (its MIME and extension
knowledge, and a bounded byte sniff). The fork's rule never widens.

The other invariants kept:

* the classic bar is still `[action][?][X]`, and the `?` opens an
  in-product popover that never links out;
* the modern round overlay is still modern;
* **one** width rule: `classicBarWidth` is the rule, `barHostWidth` is its
  single reader, and upstream's parallel `barWidth()` rule was not
  introduced;
* the drag listens on the window (so it keeps working when the pointer
  leaves the button) and the element is still not reparented on
  pointerdown;
* cookies still come from the *download* host, and upstream's `pageOnly`
  guard is kept beside that;
* exactly one `restoreVideoButtons` control.

Three test harnesses were updated for behaviour that upstream changed on
purpose — the drag now listens on the window, the width rule has one name,
and a download link is only captured on a **trusted** click (upstream's
hardening against a page script calling `a.click()`).

### Wave 5 — desktop

* The **links window** arrived with upstream asking for always-on-top. It
  now takes a cascade slot, is positioned by the cascade rule, sits at the
  normal Z-order, only the first dialog of a cascade may take focus on
  appearance, a click lets a suppressed window take focus, and the slot is
  released when the window closes.
* `src/dm.rs` is **ours**: upstream never touched it since the merge base,
  so the "Compact Blue" design system is intact.

### Wave 6 — version and packaging

| Where | Before | After |
|---|---|---|
| `Cargo.toml`, `Cargo.lock`, `VERSION` | `0.9.7` | `0.9.8` |
| `browser-extension/manifest.json` | `0.9.7.3` | `0.9.8.1` |
| `browser-extension/manifest-firefox.json` | `0.9.7.3` | `0.9.8.1` |
| `background.js`, `content.js` (`EXT_VER`) | `0.9.7.3` | `0.9.8.1` |

The application is `0.9.8` because it carries upstream v0.9.8's features.
The **extension** is `0.9.8.1`: four numeric parts are valid for Chrome and
Firefox, it sorts after upstream's `0.9.8`, and it marks this as the fork's
own build rather than the official upstream release.

Both packaging paths now use **one** exclusion rule
(`platform::is_extension_build_file`), so the package the app writes and the
package the script writes hold the same files. The app previously copied the
tests, their fixtures and the docs into its own package.

---

## Verification

Everything below was run in the merged tree at `753ae3c`.

| Command | Result |
|---|---|
| `cargo test --locked --release` | **228 passed, 0 failed** (110 warnings) |
| `cargo build --locked --release` | **OK, 112 warnings** — unchanged from the baseline |
| `node --check` on all 10 changed/new JS files | **all exit 0** |
| `node --test browser-extension/*.test.js` | **258 passed, 0 failed, 30 suites** |
| `python scripts/package_extensions.py --output dist/phase4-extensions` | Chrome folder, Firefox folder, `DownloadCraft-Firefox-0.9.8.1.xpi` |
| `python scripts/package.py` | `DownloadCraft-0.9.8-windows-AMD64.zip` (9.7 MB) |
| `python scripts/media_detection_smoke.py --browser ".../msedge.exe"` | **passed** — all nine checks true, "Download this video" label intact |
| `git diff --check` | clean |

The smoke run's checks: `directVideo`, `audio`, `embeddedVideo`, `hls`,
`dash`, `falsePositivesExcluded`, `audioControl`, `videoControl`,
`dynamicAudioControl` — every one `true`.

### Warnings

Upstream's new code raised the warning count by 14, all of them the same
rustc lint: *"falling back to `f32` as the trait bound `f32: From<f64>` is
not satisfied"*. Every one was fixed with the compiler's own suggestion
(`1.0` → `1.0_f32`), at the positions the compiler pointed at — 11 in the
new `links.rs`, 2 in `settings.rs`, 1 in `progress.rs`. The count is back
at **112**, so no new code raised it.

### A guard against silent loss

One hand-written resolution silently dropped four upstream functions
(`app_theme`, `app_downloads`, `record_browser_download`, `app_control`),
and another dropped the fork's `para()` helper. Both were caught by
comparing the inventory of public items in every merged file against
upstream's **and** against the fork's, in both directions. The check is
repeated after every merge:

```
python scripts/check_merge_symbols.py --repo <worktree> --base v0.9.8
```

---

## What is still open

These are real gaps, stated plainly rather than papered over.

1. **Upstream's new UI surfaces are stock egui.** The browser-saves card in
   Settings and the whole "Download all links" window are upstream's
   original widgets. They are not re-skinned onto `dm.rs`, so they do not
   yet look like the rest of the app. The design system itself is intact
   and nothing was lost in the merge — this is follow-on work.
2. **The Links window has no column-width persistence.** Upstream's
   `links.rs` has `load_cols`/`save_cols`, but the app's `columns_hidden`
   setting lives in `ui.json` and is not shared with that window yet.
3. **`yt-dlp` and `ffmpeg-static` still download from a moving
   `releases/latest` URL.** Pre-existing fork behaviour, not from this
   merge; it deserves the same pinned-and-verified treatment as Deno.
4. **The smoke tool ran on Edge only** in this pass. Brave and Chrome were
   not exercised on this machine.
5. **The macOS and Linux packages were not built here.** This is a Windows
   machine; `scripts/package.py` supports all three, but only the Windows
   artifacts were produced and inspected.

---

## Commits

```
753ae3c Make both packaging paths agree, and add sanitised config fixtures
1f54e03 Give the links window a place in the cascade
6a143da Merge upstream v0.9.8: resolve 55 conflicts across 16 files
```

The merge commit carries the conflict resolutions, and with them the
security and observer changes that live inside the sixteen conflicted files
(a merge commit has to record the resolved state of those files). The two
follow-up commits hold the work that sits outside them.
