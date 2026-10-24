# Upstream v0.9.8 integration checklist

The upstream repository released tag **v0.9.8** (`5ac3134`) while this fork was
on `feature/ui-redesign` (`5c26558`). The two lines share the merge base
`c6ca86f`: 32 commits local-only, 17 commits upstream-only.

The merge was performed on an isolated worktree so the working checkout was
never touched:

* branch `integration/upstream-v0.9.8`
* worktree `../integration-098` (relative to `upstream-baseline/`)

The audit predicted **16 conflicting files and 55 conflict hunks**. The real
merge produced **exactly 16 files and 55 hunks**, so the plan could be applied
hunk for hunk.

---

## Wave 0 — baseline

| Fact | Value |
|---|---|
| Local HEAD before the merge | `5c26558` on `feature/ui-redesign`, clean tree |
| Upstream tag | `v0.9.8` = `5ac3134` |
| Merge base | `c6ca86f` |
| Commits local-only / upstream-only | 32 / 17 |
| Rust tests before the merge | **209 passed, 0 failed** |
| Rust warnings before the merge | **112** (app binary, `cargo build --locked --release`) |
| JS tests before the merge | **222 passed, 0 failed, 22 suites** |
| Conflicting files / hunks | **16 / 55** |

Evidence: `evidence/wave0-baseline-rust-tests.log`,
`evidence/wave0-baseline-node-tests.log`.

### The "222 versus 207" discrepancy — resolved

The audit's static count said 207 JS tests; the suite actually runs 222. No
test is missing: the difference is 15 tests a line-oriented grep cannot see.

| File | Static `test(`/`it(` calls | Tests at runtime | Why |
|---|---|---|---|
| `browser-extension/detection.test.js` | 6 | 20 | One `for (const f of fixtures) test(...)` loop generates 14 tests from `browser-extension/fixtures/detection-responses.json`. The loop's `test(` is not at the start of a line, so a line-anchored grep misses it. |
| `browser-extension/fb.test.js` | 37 | 38 | `describe("file naming and quality", () => {  it("names the file after the description", () => {` — the `it(` shares a line with the `describe(` opener. |

207 + 14 + 1 = **222**, which is what `node --test` reports. The audit number
was an artefact of counting, not a regression.

### Configuration fixtures

`scripts/make_config_fixtures.py` writes sanitised representatives of the
live `queue.json`, `ui.json` and `settings.json` into
`tests/fixtures/config/`. Every secret-bearing value (cookies, usernames,
passwords, proxy URLs, media titles, absolute user paths) is replaced by a
sentinel; nothing is copied verbatim from the live configuration.

---

## Wave 1 — the 16 conflicting files

Each conflict was resolved by hand against
`validation/upstream-098/CONFLICT_MAP.md` (sections 3–8).

### F-01 — `Cargo.toml` (1 hunk) — union

Both sides list `windows-sys` features. The fork needs
`Win32_UI_WindowsAndMessaging`, `Win32_Graphics_Gdi` and `Win32_UI_HiDpi` for
`window.rs` and `dm.rs`; upstream needs `Win32_Storage_FileSystem` for the
new links window. **Result:** all seven features.

### F-02 — `browser-extension/background.js` (9 hunks) — upstream's architecture + our two deltas

Upstream replaced the capture model (`tabMedia` + `mediaAuth` +
`onMediaResponse`, `classifyResponse`, `sniffBytes`). Our side kept:

1. **host-scoped cookies** — the cookies sent with a capture come from the
   *download* host, not from the page. Upstream's `pageOnly` guard is kept
   *in addition*: a URL that is known only from a page-script report gets no
   cookies at all.
2. the **overlay-style messages** (`prefs`, `setOverlayStyle`) and the
   `overlayStyle` / `overlayStyleSource` ownership rule.

Upstream's incognito guard on the legacy `downloadCraftNet` snapshot is
kept (semantic conflict S-05): `storage.local` is on disk and would outlive
the private window.

### F-03 — `browser-extension/content.js` (9 hunks) — ours for shape, upstream for behaviour

* The **classic bar keeps its shape**: `[action][?][X]`, the `?` opens an
  in-product popover and never navigates to a website.
* **One width rule.** `classicBarWidth(labelText)` is the rule; `barHostWidth(host)`
  is the single reader of it (`host.__dcWidth` is a getter over
  `classicBarWidth`). Upstream's separate `barWidth()` rule was **not**
  introduced, so the bar cannot jump while it is dragged.
* Upstream's **drag repair** (window-level `pointermove`/`pointerup`/
  `pointercancel`) is adopted, but the element is still **not reparented on
  pointerdown**, and our no-grip call shape `installBarDrag(host, main, null)`
  is kept.
* Our **modern round overlay** stays modern; upstream's theme block
  (`barBusy`, `sniffedNow`, `repaintTheme`, `applyAppTheme`) is kept beside
  our overlay block, and `applyAppTheme` calls our `rerenderOverlays()`.
* The bar is marked `data-dc-bar="1"` so `repaintTheme()` can find it.

### F-04 / F-05 — `browser-extension/manifest.json`, `manifest-firefox.json` (1 hunk each) — one coordinated bump

Both manifests move to **0.9.8.1** (see *Version mapping* below).

### F-06 / F-07 — `browser-extension/popup.html`, `popup.js` (3 hunks each) — upstream's shell + our two controls

Upstream's redesigned popup (toolbar, type filter, title search, auto-send,
detection on/off, pause all, rescan, clear; *Download with Download Craft*
and *Download in browser* sections; a **Downloads** tab) is the shell. Our
two rows are carried into it:

* **Overlay style** (`modern` / `classic`)
* **Hide the overlay on this page**

There is exactly **one** `restoreVideoButtons` control — upstream's popup
already had one, so a second was not added.

### F-08 — `browser-extension/rank.js` (2 hunks) — union of the export surface

Kept: our `mediaResponse`, `manifestUrl`, Drive helpers.
Added: upstream's `fbVideos`, `fbPathOf`, `classifyResponse`, `sniffBytes`,
`forwardHeaders`, `extOf`, `dispositionFilename`.

`kindOf` keeps our wider `audio/(?:x-)?mpegurl` alternative *and* upstream's
`entry.whole -> "file"` rule and lower-case mime normalisation. There is
**one** `mediaResponse` in the file — no rename was needed.

### F-09 — `scripts/package_extensions.py` (1 hunk) — union of two ignore lists

`shutil.ignore_patterns('*.test.js','*.md','testdata','fixtures','*.ps1')` —
`fixtures` is ours, `testdata` is upstream's. Upstream's Firefox-specific
removal of `offscreen.html` / `offscreen.js` is kept.

### F-10 — `src/app.rs` (6 hunks) — union of disjoint helpers and fields

* the column chooser (`visible_cols`, `toggle_hidden_col`) **and** upstream's
  pointer-following divider (`divider_widths`);
* the `SortCol` derive (identical on both sides, attribute order only);
* the add-dialog capture keeps **all three** of `add_user_agent`,
  `add_headers` and `det_start_draft`;
* the capture fills the row from **both** the agent and the headers, and the
  commit writes **both** (`item.extra_headers = std::mem::take(...)` after
  `item.user_agent = ...`).

### F-11 — `src/confirm.rs` (5 hunks) — union + **one** `wake.json` writer

The committed row keeps both the browser agent and the captured headers; the
capture locals keep both; the cascade slot and the rename template are both
kept.

The two `wake.json` writers are the dangerous part. Upstream's `wake_targets`
wrote `{"start": [...]}` and ours wrote `{"focus": [...]}` — each replaced
the other's content, so one request was silently lost. There is now exactly
**one** writer:

```rust
fn wake_requests(start: &[u64], focus: &[u64])   // merges both arrays
pub(crate) fn wake_ids(targets: &[u64])          // start + focus
pub(crate) fn wake_focus(targets: &[u64])        // focus only
```

The main window's `check_wake()` already reads **both** keys from the same
file.

### F-12 — `src/engine.rs` (1 hunk) — upstream's block

Upstream fixes a corruption bug our fork still had: the old code tested
`start_at > 0` *after* the QR-02 reset had already set `start_at = 0`, so a
restart over an existing `.part` file kept the old tail. Upstream's branch
(`start_at > 0 && is_partial` → seek, else → `set_len(0)`) covers our QR-02
case as well, so the local `truncate_to_zero` variable was removed.

### F-13 — `src/model.rs` (2 hunks) — union of two additive fields

`DownloadItem` keeps **both** `user_agent` **and** `extra_headers`.

### F-14 — `src/native_host.rs` (9 hunks) — union of two disjoint protocol surfaces

`Incoming` gains the browser agent **and** upstream's `headers`, plus
upstream's `links` / `title` / `id` / `action`. `Reply` gains
`overlay_style` / `overlay_style_source` **and** upstream's `theme` /
`items`. `InboxItem` gains the agent **and** the headers.

Upstream's `PageLink`, `LinksBatch`, `links_batch()`, `spawn_links()`,
`spawn_detached()`, `app_downloads()`, `record_browser_download()`,
`app_control()` and `app_theme()` are restored verbatim.

### F-15 — `src/progress.rs` (1 hunk) — union of disjoint functions

Our speed-graph block (`SPEED_SAMPLES`, `spark_points`) **and** upstream's
`fmt_duration` both survive; the concept-geometry comment appears once.

### F-16 — `src/settings.rs` (1 hunk) — union, ours after upstream's

Upstream's browser cards (what the browser downloaded, and the state of each
browser's native host) come first; our **Video overlay** row and our
**Set up the extension** card follow. Both are wanted: upstream's card
shows what the browser downloaded, ours is the one-time setup the app can
repair the native host from.

Our `para()` helper was restored — upstream had deleted it, but our
extension-setup card still calls it.

---

## Wave 2 — the semantic conflicts

These merge cleanly and break at run time; each was checked by hand.

| # | Item | How it was resolved |
|---|---|---|
| S-01 | `queued_entry(url)` → `queued_entry(url, quality)` | The call site in `confirm.rs` picked up upstream's new argument in the clean merge and compiles. |
| S-02 | `next_queue_id()` replaces `max(id)+1` | Accepted upstream's. Both `cli.rs` sites and `model.rs` use it. |
| S-03 | two `wake.json` writers | One writer, two keys — see F-11. |
| S-04 | `barHostWidth` versus `barWidth` | Only `barHostWidth` reads the width, and it reads `classicBarWidth`. Upstream's separate `barWidth()` rule was not introduced. |
| S-05 | `downloadCraftNet` incognito persistence | Upstream's guard kept: the legacy snapshot is never written from an incognito tab. |
| S-06 | `columns_hidden` in `ui.json` | Verified: written at `app.rs`, read at `app.rs`. Upstream's `save_ui_state` merges keys, so the key survives. |
| S-07 | `ui.json` "columns" / "widths" | Both sides use the same key names and shapes; neither was renamed. |
| S-08 | `EXT_VER` in four places | One coordinated bump to **0.9.8.1** in `background.js`, `content.js`, `manifest.json`, `manifest-firefox.json`. |
| S-09 | our six `settings.json` keys | `apply_settings()` is additive and `app.rs` overlays keys, so `overlay_style`, `auto_extract`, `extract_keep_original`, `extract_target`, `extract_custom_dir` and `extract_ask` all survive. Verified in the merged tree. |
| S-10 | `overlay_style_source` ownership | Both the five-second poll and the `source === "app"` adoption rule are kept, so the app and the extension cannot fight over the style. |
| S-11 | `is_magnet` does not exist upstream | Kept, and `is_download_url` rejects `magnet:` (it only accepts `http`/`https`/`ftp`/`ftps`). A test now asserts it. |
| S-12 | `upstream::mediaResponse` versus ours | Only one `mediaResponse` exists in `rank.js`; no rename was needed. |
| S-13 | two packaging paths | The app's staging copy and its zip now use the **same** exclusion set as `scripts/package_extensions.py` (`platform::is_extension_build_file`). |
| S-14 | the smoke tool's label assertions | The classic bar's copy is unchanged ("Download this video" / "Download audio from this page"), so `scripts/media_detection_smoke.py` and `browser-extension/overlay.test.js` need no change. |
| S-15 | `Content-Type` guard in `grabber.rs` | Accepted upstream's (it streams and caps at 4 MiB). |

---

## Wave 2 — the security findings

| # | Finding | What was done |
|---|---|---|
| S-1 | Upstream downloads and executes an unpinned binary | **Fixed.** The Deno release is pinned to `2.9.8` (`engine::DENO_VERSION`) and the archive's SHA-256 is checked against the release's own `*.sha256sum` asset **before** the archive is unpacked. A mismatch writes nothing and runs nothing. The expected hash for all six supported platforms is in `engine::deno_zip()`. |
| S-2 | New credential-at-rest surface (`extra_headers`) | **Fixed.** `extra_headers` is `#[serde(skip)]`: it never reaches `queue.json`. It lives in memory for the transfer and is cleared on completion, as the cookie already was. |
| S-3 | Signed media URLs leave the app | **Fixed.** `app_downloads()` returns the `url` only for rows that are still running; a finished row's (often still valid) signed CDN address is not sent back to the extension. |
| S-4 | `record_browser_download()` trusts a path from the browser | **Fixed.** The path must sit inside the user's `Downloads` folder or the app's configured download folder. `path_is_under` requires a whole-path prefix, so `Downloads Evil` does not pass as `Downloads`. |
| S-5 | Page cookies written to disk in plaintext | **Fixed.** `LinksBatch` no longer has a `cookie` field. The cookie travels to the Links window in the inherited environment (`DOWNLOADCRAFT_LINKS_COOKIE`), and the batch file is deleted when it is read; a stale batch is cleared before a new one is written. |
| S-6 | Extra outbound traffic | Accepted: the release-tag check already runs only on the explicit `--update` path. |
| S-7 | Extra request per ambiguous URL | Accepted upstream's: the 4 KiB cap is kept and the manifest is reused for the same URL. |
| S-8 | BOM bug in the `query` reply path | **Fixed.** `queued_urls()` strips a BOM exactly like `app_downloads()` does, so both readers agree on what is queued. |
| S-9 | Upstream's two extension security fixes | Adopted: no cookies for page-only-reported URLs, and `blob:` URLs are rejected. |
| S-10 | No native torrent/P2P engine | Unchanged: every magnet still goes to the cloud provider API, and a magnet can never enter the download queue or the Links window. |

### Tests added with the security work

* `model::a_magnet_is_never_a_download_url` — a magnet is not a download URL.
* `native_host::the_links_batch_never_carries_the_cookie` — no cookie in the file.
* `native_host::a_browser_path_must_sit_inside_a_download_folder` — the path
  rule, including the `Downloads Evil` case.

---

## Wave 5 — the design system and the cascade

* `src/links.rs` no longer calls `with_always_on_top()`. The window takes a
  **cascade slot** (`window::take_cascade_slot`), is positioned with
  `window::cascade_position`, uses `WindowLevel::Normal`, only the first
  dialog of a cascade may take focus on appearance
  (`slot.may_activate()`), a click lets a suppressed window take focus, and
  the slot is released when the window closes.
* `src/dm.rs` is **ours**: upstream never touched it since the merge base, so
  the "Compact Blue" design system is intact. Upstream's new UI surfaces (the
  browser card, the links window) are stock egui and are **not** yet re-skinned
  onto `dm.rs` — see *Known gaps*.

---

## Version mapping

| Where | Before | After |
|---|---|---|
| `Cargo.toml` / `Cargo.lock` | `0.9.7` | `0.9.8` |
| `VERSION` | `0.9.7` | `0.9.8` |
| `browser-extension/manifest.json` | `0.9.7.3` | `0.9.8.1` |
| `browser-extension/manifest-firefox.json` | `0.9.7.3` | `0.9.8.1` |
| `background.js` `EXT_VER` | `0.9.7.3` | `0.9.8.1` |
| `content.js` `EXT_VER` | `0.9.7.3` | `0.9.8.1` |

The application version is `0.9.8` because it now carries upstream v0.9.8's
features. The **extension** version is `0.9.8.1`: four numeric parts are
valid for Chrome and Firefox, it sorts after upstream's `0.9.8`, and it marks
this as the fork's own build rather than the official upstream release.
