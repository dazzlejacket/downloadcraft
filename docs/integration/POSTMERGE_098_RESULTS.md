# Post-merge fixes — results (upstream v0.9.8)

The six P0 bugs and the P1 polish that followed the upstream v0.9.8 merge,
and the security follow-up that closed the wave.

The full document — every root cause, every accepted residual and what was
**not** verified — is `validation/postmerge-098/RESULTS.md` in the workspace
root, next to this checkout. This file is the in-repo summary.

## The commits

| Commit | What it did |
|---|---|
| `0e422f9` | The provider list opens. A click on the provider button was counted as a click *elsewhere*, so the end-of-frame rule closed the popup in the same frame it opened. The rule is now one named function, and the selection is a testable state machine (`ProviderPicker`). |
| `c231f75` | One overlay, one switch, and Bandcamp's own tracks. The round **Modern** button, the **Modern / Classic** selector and the `?` segment are gone; the page has one switch (*Show download buttons on this page*) that hides the buttons without discarding what was detected; a Bandcamp MP3 is detected as audio and named after **its own track**. |
| `a3ed3a7` | `overlay_style` and `overlay_style_source` are gone from the settings, the native host and the `ui.json` that carries them. An old file's obsolete keys are dropped on the next save. |
| `4964493` | The extension has an **Audio** category of its own, in one table (`rank.js::categoryOf`) that the background worker delegates to. |
| `df718ec` | The app agrees: `model::category_of` knows the whole audio family, and the "Download all links" window says what a row is (`Name \| Size \| Category \| Download from`, **Download selected (n)**, a row menu with **Copy URL** and **Download**). |
| `4a10e9c` | The extension is version **0.9.8.2**. The native-host protocol version is untouched, so both sides stay compatible. |
| `2178c02` | The security follow-up. |
| `f8776da` | A caption track downloads the caption, not the video. |
| `4a8d630` | A caption file keeps its own name (`.vtt`), not an unnamed blob. |

## The security follow-up

Four confirmed problems, each with a test that would have caught it.

1. **A download name could walk out of the download folder.** The name taken
   from a URL is now one file name and never a path (`safe_segment`), and the
   Add dialog runs it through `sanitize_filename` before joining it onto the
   folder.
2. **The browser's saved path was compared as a string.** `C:\Downloads\..\Startup\clip.mp4`
   counted as being inside `C:\Downloads`. Both sides are resolved before
   they are compared now.
3. **A captured bearer token was written to the disk.** `Authorization` is a
   credential, and `native_inbox.json` is a file another program running as
   the same user can read. The headers now travel to the Download File Info
   dialog through the **inherited environment**, the way the Links window
   already receives its cookie, and the dialog drops the variable before it
   starts anything of its own. The inbox holds them only when the dialog
   could not be started at all.
4. **A page-supplied file name could reach a command line.** A name must now
   be a plain single file name on both sides; one that is not is dropped and
   the app derives a name from the URL instead.

Checked and deliberately left alone: the pinned, checksum-verified Deno; the
popup's sensitive URLs, which are already bounded to active rows; the Links
cookie, which travels by environment and never by disk; the headers' binding
to the download's own site; and the fact that no manifest declares
`externally_connectable`, so a page cannot message the extension. The session
cookie in `queue.json` and the moving "latest" URL for yt-dlp and
ffmpeg-static are both real gaps with the reasoning written down in
`validation/postmerge-098/RESULTS.md` §9.

## A caption must stay a caption

The in-page panel offers a caption track as a chip next to the video's
qualities. Clicking it sent the caption URL to the app — and the app
downloaded **the video**.

YouTube's caption endpoint is
`https://www.youtube.com/api/timedtext?v=<video id>&…&fmt=vtt`, and the app
decided "this is a YouTube link, so it is a page to resolve" with a plain
`url.contains("youtube.com")`. The caption URL carries the *video's id*, so
the resolver handed back the video's stream. Three places made that call:
the transfer itself, the Download File Info dialog's probe and the main
window's add-dialog probe.

A caption URL is a **file**, not a page. `engine::is_youtube_video_page()`
separates the two — the caption endpoint, and any YouTube URL that names its
own subtitle format — and the three call sites now share one predicate
(`engine::needs_resolve()`), so they cannot drift apart again. The file also
keeps its own suffix: a caption URL names no file, so the MIME gives it one
(`text/vtt` → `.vtt`).

`validation/postmerge-098/RESULTS.md` §10 has the details.

## The numbers

| Check | Before | After |
|---|---|---|
| `cargo test --locked --release` | 228 passed | **249 passed, 0 failed** |
| `cargo build --locked --release` warnings | 112 | **112** (unchanged) |
| `node --test browser-extension/*.test.js` | 258 passed | **278 passed, 0 failed** |
| `python scripts/media_detection_smoke.py --browser …msedge.exe` | passed | **passed, 9/9 checks** |
| `python scripts/check_merge_symbols.py --base v0.9.8` | ok | **ok** — "no symbol was lost (2 replacement(s) on record)" |

The browser smoke run's evidence is in
`docs/integration/evidence/postmerge-098/edge-media-detection-smoke.json`.

## What was not verified

- The desktop UI was **not** driven by hand: there is no display here. The
  provider dropdown, the per-page switch, the links window's row menu and the
  settings window's Escape are covered by their state machines and unit
  tests. The extension side *was* driven in a real browser.
- The bearer token's path through the environment was not exercised by a
  live capture in this round; it is covered by unit tests on both sides.
- No file was moved: the category rename changes what is *shown*, and a
  finished download stays where it is.
