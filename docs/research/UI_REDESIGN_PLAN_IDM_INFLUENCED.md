# UI Redesign Plan — IDM-informed, DownloadCraft-native

**Status:** proposal only. Nothing in this document has been implemented.
**Date:** 2026-10-24
**Scope:** desktop UI (`src/`), browser-extension overlay (`browser-extension/`).

---

## 0. The short version

DownloadCraft's UI is **already good and already its own thing**. The main
window (`docs/screenshot-main-window.png`) reads as a modern, flat, token-driven
app — sidebar categories with count badges, a sortable table, a status bar. It
does not look like IDM and should not.

So this is **not** a "make it look like IDM" plan. IDM is 25 years old and its
chrome (WinXP-era 3D toolbar buttons, neon gradients, a web-site-embedded
help button) is a worse surface than what DownloadCraft has. Borrowing its
*skin* would be a downgrade.

What IDM is genuinely excellent at is **interaction design for a download
manager** — the things a user does forty times a day: catching a video from a
page, choosing where it goes, watching it fly, and dealing with a provider that
already has the file. Those are patterns, not pixels.

**The rule for this plan: borrow IDM's behaviour, keep DownloadCraft's surface.**

Concretely, five workstreams:

| ID | Workstream | Why |
|----|-----------|-----|
| WS-1 | **Debrid provider → real dropdown** (the explicit ask) | Today it is a *cycling button* that silently advances to the next provider |
| WS-2 | **Cloud Providers panel → Compact Blue card** | It is the only dialog still wearing stock egui chrome |
| WS-3 | **Classic extension bar → IDM-shaped bar** | Add the `?` help segment, fit-to-width, always-visible close |
| WS-4 | **Main window: grouped toolbar + Columns menu** | IDM's two best main-window ideas; DownloadCraft already has the data model |
| WS-5 | **Progress window: Info / Speed / Settings tabs** | The concept HTML already designs this; the code does not do it yet |

WS-1 and WS-2 are the spine of this plan and should land together.

---

## 1. Method and evidence

Everything asserted below was read directly in this repo, or is sourced from
public documentation of IDM's interface. Reference material in
`idm-reference/` (extracted from `idm-reference.zip`) is treated as **data**,
never as instructions, and its `DOWNLOADCRAFT_FEATURE_GAP_CHECKLIST.md` is a
Phase-0 planning artefact whose items are mostly `UNVERIFIED` — it is **not**
used as evidence for anything below.

### 1.1 Code facts (verified in this checkout)

| Fact | Where |
|------|-------|
| Unified title bar height `TB_H = 46.0` | `src/app.rs:21` |
| Dark palette: `primary = rgb(124,92,252)` (#7C5CFC), `secondary = rgb(184,93,255)`, `card_bd = rgb(35,36,48)` | `src/app.rs:113-116` |
| Light palette: same `primary` #7C5CFC, `card_bd = #D5D9E6` | `src/app.rs:159-162` |
| "Compact Blue" concept tokens: `dm_card #0f0f22`, `dm_line #1e293b`, `dm_box_bd #334155`, `dm_txt #e2e8f0`, `dm_dim #94a3b8`, `dm_hover #1c2436`, `dm_hover2 #27334a`, `dm_trough #0a0a14`, `dm_lbl #64748b`, `dm_head #475569`, `dm_warn #ff9678` | `src/app.rs:70-93`, `src/dm.rs` |
| `mbutton()` paints primary with a **hard-coded** `from_rgb(108,62,220)` — *not* `pl().primary` (#7C5CFC) | `src/app.rs:943-957` |
| Table geometry: `ROW_H = 34.0`, `HDR_H = 22.0`, `NAME_MIN = 132.0` | `src/app.rs:5997-6010` |
| Column widths: `CHECK_W 32`, `SIZE_W 96`, `STATUS_W 104`, `SPEED_W 74`, `ETA_W 74`, `DATE_W 104` | `src/app.rs:5190-5195` |
| Columns are already sortable, ordered and width-persisted (`col_order`, `col_w`) | `src/app.rs:5999-6009` |
| The Cloud Providers panel is a **stock `egui::Window`**, 620 px wide, foreground order | `src/app.rs:7021-7026` |
| The Add-download dialog is an **in-canvas dm.rs card** (`dm_btn_flat`, `dm_icon_btn`, `DmIcon::*`) | `src/app.rs:6500-6714` |
| Provider selection is a **cycling button**: `self.debrid_provider_index = (self.debrid_provider_index + 1) % ALL.len()` | `src/app.rs:8622-8626` |
| Switching provider clears `debrid_account`, `debrid_session`, `debrid_notice` — **but NOT `debrid_token`** | `src/app.rs:8627-8629` |
| `debrid_provider_index` is **memory-only**; it resets to `0` (Real-Debrid) on every launch. A stale comment says "0 = Real-Debrid, 1 = TorBox" | `src/app.rs:2055-2056`, `src/app.rs:2233` |
| A **generic dropdown mechanism already exists**: `menu_items(menu)` → `drop_rows(menu)`, with a `✓` prefix pattern already used by the theme menu (menu 3) | `src/app.rs:8366-8388` |
| `commit_add` rejects anything that is not http/https — **a magnet cannot enter the normal queue** | `src/app.rs:8439-8443` |
| `Provider::ALL` = Real-Debrid, TorBox, AllDebrid, Premiumize, Debrid-Link; each has `label()`, `capabilities()` (6 flags) and `unsupported_reason(Workflow)` | `src/debrid.rs:31-37`, `src/debrid.rs:80-190` |
| Extension overlay style (`classic` \| `modern`) is already a **persisted, app-owned setting** shared with the extension | `src/settings.rs:71-74`, `src/settings.rs:159-164`, `src/settings.rs:312`, `src/settings.rs:3139-3155` |
| Classic bar: fixed **240 × 28 px**, `.grip` (18 px, 6-dot) on the left, green arrow `#168047`, label `"Download this " + kind`, `.close` (25 px) with `close.hidden = true` | `browser-extension/content.js:1518`, `content.js:1524-1556` |
| Bar CSS: bg `#f8fafc`, border `#aab7c4`, radius 5px, shadow `0 1px 5px #0003`, hover bg `#e7eff7`, focus ring `#2563eb` | `content.js:1524-1537` |
| **Both** the main action button and the grip are drag handles, with a 5 px threshold and click suppression → the grip is already redundant | `content.js:1185-1217` |
| The 240 px width is hard-coded in **three** separate places | `content.js:1124`, `content.js:1178`, `content.js:2068` |
| The classic bar renders in **Shadow DOM** (page CSS isolation) | `content.js:1519` |

### 1.2 IDM interface facts (public documentation / product behaviour)

- **Browser catch panel**: a floating bar reading `[green arrow icon][action label] | [?][X]`.
  Labels include "Download this video", "Download audio from this page",
  "Download all links with IDM". The `?` opens help; the `X` dismisses the bar.
  On hover the label shifts to an accent colour. (User-supplied screenshots; this
  is the exact shape to borrow.)
- **Main window**: a categories tree on the left (All / Categories / Queues /
  Downloads / Completed / Interrupted / Finished), a **grouped** toolbar above
  the list, sortable columns, a right-click menu on the column header that
  opens a **Columns** manager (toggle / reorder / resize), a `Q` queue column,
  and a status bar carrying a queue progress indicator plus pause/stop-all.
- **Download File Info dialog**: `General / Options / Proxy / Schedule` tabs;
  General carries File name, Save to folder, Description, and the
  Start / Download later / Cancel row.
- **Progress dialog**: percentage, average speed, ETA, a **per-connection
  segment chart**, Pause / Stop / Resume, a per-task **speed limit**, and a
  "progress dialog start view" option (Don't show / Show minimised / Show normal).
- **Options → Downloads**: per-task and global behaviour switches (always show
  the dialog, start queued, silent mode).

---

## 2. What to borrow, what to adapt, what to refuse

The hard part of this plan is not finding IDM features. It is refusing most of
them.

### 2.1 BORROW — the interaction, in DownloadCraft's own surface

| IDM pattern | Why it is good | DownloadCraft form |
|---|---|---|
| Catch bar `[icon][label][?][X]` | One clear action, plus a way to learn and a way to dismiss | WS-3 |
| Catch label states ("Download this video" vs "Download audio from this page") | Says what will actually be captured | WS-3 |
| Column-header right-click → **Columns** manager | Power users reshape the table without leaving it | WS-4 |
| **Grouped** toolbar | 10 unseparated buttons is a wall; groups make it scannable | WS-4 |
| Queue indicator in the status bar | Answers "is anything running?" at a glance | WS-4 |
| Per-task **speed limit** in the progress UI | The one control users reach for most | WS-5 |
| **Download later** vs **Start now** | Already mirrored by Add vs Download | keep |
| Provider choice that is *visible and explicit* | (IDM's torrent client is a separate app — DownloadCraft's debrid panel is the equivalent surface) | WS-1 |

### 2.2 ADAPT — right idea, wrong skin

| IDM original | DownloadCraft adaptation |
|---|---|
| Tabbed `General/Options/Proxy/Schedule` dialog | Keep the gear + advanced panel. Three tabs of one field each is worse than a flat card. |
| `Q` column | A small queue chip on the row, not a whole column |
| Big skeuomorphic toolbar buttons | Flat icon+label buttons with **group separators** |
| Orange hover text on the catch bar | Keep the `#e7eff7` hover wash — it is calmer and already ours |
| IDM's `?` opens a website | Our `?` opens an **in-product** help popover |
| IDM's "silent mode" | Already covered by the existing capture-receipt behaviour |

### 2.3 REFUSE — do not let these in

- **IDM's visual chrome.** 3D/Neon toolbar buttons, gradient chrome, WinXP-era
  panel framing. The `redesign/*.html` concepts and `src/dm.rs` are a better
  system; they are the house style.
- **IDM's help button as a website link.** A `?` that navigates away from the
  page the user is on is a hostile default in 2026.
- **A native torrent client.** Hard rule in `AGENTS.md`: every magnet and every
  `.torrent` is provider-side. The UI must never imply a peer connection.
- **Fake capability.** The provider dropdown must not show a workflow a
  provider's API does not offer, per `AGENTS.md` and
  `Provider::unsupported_reason()`.
- **A token in any persisted file.** The provider *choice* may persist; the
  token may not.
- **Collapsing DownloadCraft's confirmation model.** "A capture never silently
  duplicates a file" and "the already-downloaded / still-downloading cases are
  reported" must survive untouched.

---

## 3. WS-1 — Debrid provider becomes a dropdown

### 3.1 The problem, stated precisely

`src/app.rs:7048` registers a `ButtonId::DebridProvider` hit; its handler at
`src/app.rs:8622-8629` does:

```rust
self.debrid_provider_index =
    (self.debrid_provider_index + 1) % crate::debrid::Provider::ALL.len();
```

So the control is **not a dropdown**. It is a button that advances to the next
provider in a fixed order (Real-Debrid → TorBox → AllDebrid → Premiumize →
Debrid-Link → Real-Debrid). The user must click it up to four times to reach
the provider they want, and there is no way to see the list.

Three concrete defects fall out of this:

1. **The token survives a provider switch.** `src/app.rs:8627-8629` clears the
   account, the session and the notice — but not `debrid_token`
   (`src/app.rs:2059`). After one click, a Real-Debrid token is sitting in the
   field under a TorBox label. Pressing *Validate* then sends it to TorBox.
   This is a credential-leak-by-accident and it is a **bug today**, independent
   of any redesign.
2. **The choice is not remembered.** `debrid_provider_index` is an in-memory
   field initialised to `0` (`src/app.rs:2233`) and is not in `src/settings.rs`.
   Every launch starts at Real-Debrid even if you use TorBox daily.
3. **The stale comment** at `src/app.rs:2055` ("0 = Real-Debrid, 1 = TorBox")
   is wrong — there are five providers. It should be corrected whichever way
   the selection is stored.

The dropdown mechanism **already exists**: `menu_items(menu)` → `drop_rows(menu)`
(`src/app.rs:8366-8388`), and the theme menu already renders a `✓` on the
active entry. WS-1 is mostly a matter of *using* that mechanism properly and
adding the provider-specific affordances.

### 3.2 The control

Replace the cycling `mbutton` with a **combobox**:

```
┌────────────────────────────────┐
│ ▣  Real-Debrid              ▾  │      220 × 32, dm.rs field style
└────────────────────────────────┘
```

- Field fill `dm_line` (#1e293b), border `dm_box_bd` (#334155), radius 6 px,
  12 px label in `dm_txt` (#e2e8f0).
- A provider glyph on the left, a `DmIcon::Chevron` on the right that rotates
  180° while open.
- Label is the provider's `Provider::label()` — not `"Provider: X"`. The word
  "Provider" belongs to the field's uppercase micro-label above it, so it is
  not repeated inside the control.
- Keyboard: `↑`/`↓` move the highlight, `Enter` commits, `Esc` closes and
  returns focus, `Tab` closes. Focus is taken only when the user opened the
  dropdown by keyboard — the dialog cascade rule in `src/window.rs` still
  applies, so the panel never steals the keyboard from the rest of the app.

### 3.3 The dropdown contents

Five rows, one per provider, each a two-line row:

```
┌──────────────────────────────────────────────────────┐
│ ✓ Real-Debrid                        ● signed in     │
│     instant availability · per-file selection        │
├──────────────────────────────────────────────────────┤
│   TorBox                              ○ not connected │
│     no instant availability · fetches every file     │
├──────────────────────────────────────────────────────┤
│   AllDebrid                           ○ not connected │
│     instant download · no cache query                │
├──────────────────────────────────────────────────────┤
│   Premiumize                          ○ not connected │
│     instant download · magnets only                  │
├──────────────────────────────────────────────────────┤
│   Debrid-Link                         ○ not connected │
│     per-file selection · link becomes a record        │
└──────────────────────────────────────────────────────┘
```

Rules:

- The `✓` marks the **current** provider, exactly as the theme menu does.
- The **status dot** is real state, not decoration:
  `●` green = a validated session exists for that provider *in this session*;
  `○` = not validated yet.
- The **capability line** is generated from `Provider::capabilities()` and
  `Provider::unsupported_reason()` — never hand-written per provider in the UI.
  A provider whose API lacks a workflow says so in the row, which is exactly
  the `AGENTS.md` rule "Never invent a capability … the panel disables it with
  the reason from `Provider::unsupported_reason()`" turned into a UI affordance.
- Rows are **not** disabled. Switching provider is always legal; the
  *workflows* become unavailable, not the provider.

### 3.4 On commit

```rust
// Provider changed: the account, the session, the notice AND the token
// are all provider-specific. Keeping the old token would send a
// Real-Debrid token to TorBox.
self.debrid_provider_index = idx;
self.debrid_account = None;
self.debrid_session = None;
self.debrid_notice.clear();
self.debrid_token = String::new();   // <-- the fix
```

Better than clearing: keep an **in-memory per-provider token map**
(`HashMap<Provider, String>`), so switching away and back restores that
provider's own token. This is still strictly memory-only — it is never
serialised to settings, never to the queue file, never to a download row — so
it satisfies `AGENTS.md` ("The token stays in memory and is never written to a
settings file, a queue file or a download row"). The map must live in a field
that is explicitly excluded from every save path, and a test must assert that
no serialised artefact contains the token.

**Decision for the reviewer:** clear-on-switch (simplest, matches today's
account/session behaviour) versus per-provider map (better UX, slightly more
surface area to keep token-free). *Recommendation: per-provider map, with the
clear-on-switch behaviour as its degenerate case.*

### 3.5 Persistence

Add to `src/settings.rs`:

```
debrid_provider: String   // "realdebrid" | "torbox" | "alldebrid" | "premiumize" | "debridlink"
```

- Default `"realdebrid"` (today's behaviour).
- Parsed with the same closed-set pattern `overlay_style` already uses
  (`src/settings.rs:159-164`): an unknown value keeps the default rather than
  leaving the panel in a broken state.
- **The token is not in this struct, and never will be.**
- The panel reads this on open and writes it on commit.

### 3.6 The panel also gains an honest capability readout

Under the dropdown, a single muted line (`dm_dim`, #94a3b8) states what the
*selected* provider cannot do, using `unsupported_reason()` verbatim. For
TorBox, for example:

> TorBox: no instant-availability query — a magnet is fetched first. Every file
> of a torrent is downloaded; per-file selection is not available.

This turns a wall of provider-specific behaviour into one readable sentence and
removes the reason a user would otherwise keep clicking *Validate* on a
provider that cannot answer.

### 3.7 Magnet capture outside the panel (small, high-value)

`commit_add` (`src/app.rs:8443`) rejects a magnet with
`"invalid URL (need http/https)"`. A magnet pasted into the Add-download dialog
therefore dead-ends. Proposal:

- Detect a magnet in the Add dialog's URL field.
- Replace the `Add` / `Download` primary row with a single
  **"Resolve with a cloud provider"** button (secondary style, `dm_dim` note
  beneath: *"A magnet is resolved by your cloud provider — the file is fetched
  by them, not by a torrent client on this machine."*).
- Clicking it opens the Cloud Providers panel with the magnet prefilled and the
  provider dropdown set to the persisted choice.

The second sentence is not decoration. `AGENTS.md` forbids a native torrent
client, and a user reading "magnet" needs to know that DownloadCraft will not
open peer connections.

---

## 4. WS-2 — The Cloud Providers panel becomes a DownloadCraft card

### 4.1 What is wrong today

Evidence: `docs/integration/evidence/debrid-panel-real-debrid.png`.

- It is an `egui::Window` (`src/app.rs:7021`), so it wears stock egui chrome:
  the title "Cloud Providers" floats centred above the content in the window's
  own header strip, disconnected from the card below it.
- The **Submit** button is a stock `egui::Button` — egui's default purple, which
  is neither the app's #7C5CFC `primary` nor the dm.rs blue→violet gradient
  used by every other dialog.
- `Provider:` and `Validate` are `mbutton`s, which paint their primary state
  with a **hard-coded** `from_rgb(108,62,220)` (`src/app.rs:954`) rather than
  `pl().primary`. Three different purples are on screen.
- The rest of the panel uses stock `ui.label` / `ui.weak` / `ui.separator`.
- The panel is the only dialog in the app that is *not* a dm.rs card. The
  Add-download dialog (`src/app.rs:6500-6714`) is the reference implementation
  of the correct thing: `dm_btn_flat`, `dm_icon_btn`, `dm_icon_btn_box`,
  `DmIcon::*`, uppercase micro-labels, placeholders instead of field labels.

There is also a **z-order defect**: in the screenshot, the toolbar tooltip
*"Resolve magnet and hosting links with a cloud provider"* paints **on top of**
the open modal, overlapping the panel header. A tooltip from the window behind
must not overlay a foreground dialog.

### 4.2 The target

Rebuild the panel as a dm.rs card, in the same in-canvas modal mechanism as the
Add dialog (`show_add`), not as an `egui::Window`:

```
┌──────────────────────────────────────────────────────────────┐
│  ☁  CLOUD PROVIDERS                                  ✕       │  header strip
├──────────────────────────────────────────────────────────────┤
│                                                              │
│  PROVIDER                                                    │  micro-label
│  ┌────────────────────────────────────────┐  ┌────────────┐ │
│  │ ▣  Real-Debrid                      ▾  │  │  Validate  │ │
│  └────────────────────────────────────────┘  └────────────┘ │
│                                                              │
│  TorBox: no instant-availability query — a magnet is …       │  dim hint
│                                                              │
│  API TOKEN                                                   │
│  ┌────────────────────────────────────────────────────────┐ │
│  │                                                        │ │
│  └────────────────────────────────────────────────────────┘ │
│  The token stays in memory only. It is never written to a    │  dim
│  settings file, a queue file or a download row.              │
│                                                              │
│  MAGNET OR HOSTING LINK                                     │
│  ┌────────────────────────────────────────────────────────┐ │
│  │ magnet:… or https://host/…                             │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                              │
│  HOST PASSWORD (OPTIONAL)                                   │
│  ┌────────────────────────────────────────────────────────┐ │
│  └────────────────────────────────────────────────────────┘ │
│                                                              │
│                        ┌──────────────┐                      │
│                        │    Submit    │                      │  gradient CTA
│                        └──────────────┘                      │
│                                          ┌──────────┐        │
│                                          │  Close   │        │
│                                          └──────────┘        │
└──────────────────────────────────────────────────────────────┘
```

- Fill `dm_card` #0f0f22, border `dm_line` #1e293b, radius 8 px.
- Header strip: `dm_lbl` uppercase micro-label, `DmIcon::Close` button on the
  right. **No stock egui title bar.**
- The primary CTA uses the dm.rs accent gradient (`#3b82f6 → #8b5cf6`), the
  same as the Add dialog's **Download** button — not egui's purple.
- Uppercase micro-labels above fields (`dm_lbl` #64748b), matching the concept.
- Secondary buttons (`Close`) use the outline style.
- The existing **stock-egui** pieces inside the panel (`ui.label`, `ui.weak`,
  `ui.separator`) are replaced with the dm.rs equivalents so the panel has one
  visual language.
- The `?` of "what is a cloud provider" becomes a small `DmIcon::Info` next to
  the header title that toggles a one-line explanation — the in-product cousin
  of the IDM `?` (see WS-3).

### 4.3 Constraints

- **Do not change behaviour.** `src/debrid.rs` is the API layer and it stays
  exactly as it is; only the drawing changes.
- The token text must remain visibly labelled as memory-only — that line is a
  promise to the user and it must stay.
- The file list (`DebridFile(u64)` hits, `src/app.rs:898`) and the
  queue/cancel buttons keep their current behaviour; they get dm.rs styling.
- The panel must not become always-on-top and must keep the normal Z-order and
  the cascade behaviour in `src/window.rs`.

---

## 5. WS-3 — The catch bar becomes IDM-shaped (without becoming IDM)

### 5.1 Target shape

```
┌────────────────────────────────────────────────┐ │ ┌───┐
│ ▶  Download audio from this page               │ │ │ ? │ │ ┌───┐
└────────────────────────────────────────────────┘ │ └───┘ │ │ X │ │
                                                      └─────┘
```

against today's:

```
┌──┬──────────────────────────────────┬───┐
│ ⠿│ ▶  Download this video            │ ✕ │
└──┴──────────────────────────────────┴───┘
   grip (18px)                          (hidden until hover)
```

### 5.2 Changes

| # | Change | Rationale |
|---|--------|-----------|
| 1 | **Add a `?` segment** between the action and the close | The user's reference; it is the one genuinely missing affordance. Ours opens an in-product help popover, not a website. |
| 2 | **Remove the grip** | `content.js:1185` already installs the drag handler on `main` as well as `grip`, with a 5 px threshold and click suppression. The grip is 18 px of visual noise for no capability. The whole bar becomes the handle, as in IDM. |
| 3 | **Fit-to-width**, clamped to `[240, 460] px` | The label "Download audio from this page" is ellipsised at 240 px today. IDM's bar is sized to its text. |
| 4 | **Always show the close** | `content.js:1547` sets `close.hidden = true`. IDM always shows it. A bar that only reveals its dismiss control on hover is a hover-only affordance — bad for touch, bad for keyboard. |
| 5 | **Copy matches the action** | IDM says "Download audio from this page". Ours says "Download this audio", which is wrong for an `<audio>` element and for a page with several audio items. |
| 6 | **Single source of truth for the width** | 240 px is hard-coded in three places (`content.js:1124`, `1178`, `2068`). Auto-width needs one constant, and the drag maths must read the *rendered* width. |
| 7 | **Keep** the green arrow `#168047`, the `#f8fafc` surface, the 5 px radius, the `#0003` shadow, the hover wash `#e7eff7`, the focus ring `#2563eb` | These are already DownloadCraft's and already match IDM's read. |
| 8 | **Keep** Shadow DOM, the `data-dc-pill` host contract, `installBarDrag`'s click-suppression, the saved-position logic, the fullscreen re-parenting | Frame/document isolation and the drag/click disambiguation are load-bearing behaviours under `AGENTS.md`. |

### 5.3 The `?` popover

Clicking `?` opens a small Shadow-DOM card anchored under the bar:

> **What this captures**
> · The direct media file for this element, with the page's cookies and
>   User-Agent, so the download speaks with the same voice as your browser.
> · The quality is chosen in Download Craft, not here.
> · If a browser check appears, Download Craft reports it and stops — it does
>   not try to get around it.

That third bullet is the DownloadCraft version of IDM's help, and it is
truthful to `AGENTS.md` ("A browser check … is **reported, never solved**").

The popover must:
- be dismissible by click-away and `Esc` (the existing `installDcCloser`
  pattern at `content.js:1581-1588` already does this for the quality panels);
- never intercept clicks meant for the page (the existing panels already stop
  their own clicks from reaching the document);
- not appear for the quality-menu button, which keeps its own label
  (`content.js:2191` forces `style: "classic"` for the quality button).

### 5.4 The modern round mark

Untouched. It is a different element, not a restyled bar (`content.js:1421-1423`),
and it is the right answer for a small player.

### 5.5 Overlay style setting

`src/settings.rs:71-74` already owns `overlay_style` ∈ {`classic`, `modern`},
parsed as a closed set and shared with the extension through the app-owned
`overlayStyleSource` mechanism. **No new setting is needed for WS-3** — the
bar changes shape within `classic`. If a third style is ever wanted, it is a
new value in that closed set, not a new mechanism.

---

## 6. WS-4 — Main window: two ideas from IDM

### 6.1 Grouped toolbar

Today the toolbar is one unbroken run of ten icon+label buttons
(`Add URL, Resume, Stop, Stop All, Delete, Start Queue, Stop Queue, Open Queue,
Site Grabber, Settings`). IDM separates its toolbar into groups. Adopt the
grouping, not IDM's 3D buttons:

```
┌───────────────────────────────────────────────────────────────────┐
│  + Add URL      │  ▷ Resume  ■ Stop  ⏹ Stop All  🗑 Delete        │
│                 │                                                │
│  ⊞ Site Grabber │  ▶ Start Queue  ⏸ Stop Queue  ☰ Open Queue     │
│                 │                                   ⚙ Settings    │
└───────────────────────────────────────────────────────────────────┘
```

Three groups separated by 1 px `card_bd` rules with 12 px of air:
**New** (Add URL, Site Grabber) · **Control** (Resume, Stop, Stop All,
Delete) · **Queue** (Start Queue, Stop Queue, Open Queue) · **App**
(Settings, right-aligned).

IDM puts the "new download" group first and the destructive controls in the
middle; keeping *Stop All* away from *Settings* is the point. The destructive
buttons keep their existing colours (`error` / `warning` from the palette) so
the grouping never becomes the only signal.

### 6.2 The Columns menu

DownloadCraft **already has the model**: `col_order` and `col_w` are persisted
(`src/app.rs:5999-6009`) and the columns are sortable. What is missing is any
way to *choose* the set. IDM solves this with a right-click on the column
header.

Proposal: right-clicking the table header opens a context menu with:

- a **Columns** submenu — toggle visibility per column, with `Name` locked on;
- **Reset width**, **Reset order**;
- the existing sort action for that column.

This needs one new piece of model: a per-column **visible** flag alongside
`col_order` and `col_w`. That is a persisted UI-state addition in the same
file, same style.

*Cost check:* this is the largest new UI surface in the plan and it is
**optional**. It is called out separately so it can be dropped without
touching WS-1..WS-3. The Columns menu is the single best idea in IDM's main
window; it is also the most work.

### 6.3 Status bar

IDM's status bar carries a queue progress indicator and a pause/stop-all
cluster. DownloadCraft's carries only `Ready` at the far right. Add a slim
aggregate progress track (`bar_bd` / `track` tokens already exist in the
palette) showing "3 of 7 active · 18.4 MB/s", using the same hairline
treatment as the concept's progress rules.

### 6.4 Not doing

- IDM's **grabber** (embedded browser control) is already covered by
  DownloadCraft's Site Grabber. No change.
- IDM's **skinned toolbar** — no.

---

## 7. WS-5 — Progress window tabs

`src/dm.rs:155-169` already defines `DmIcon::Info`, `DmIcon::Chart`,
`DmIcon::Sliders`, `DmIcon::Pause`, `DmIcon::Play` — the icon set for the
**Info / Speed / Settings** tabbed progress dialog that
`redesign/Downloading gui concepts.html` designs. `src/progress.rs` (54 KB)
implements the progress window, but the tabbed structure is a concept, not
code.

IDM's progress dialog earns its keep on exactly these three axes: *what is
happening* (Info), *how fast* (Speed, with the per-connection segment chart),
and *what can I change* (Settings — including the per-task speed limit).

Proposal:

- Implement the three tabs from the existing concept.
- **Info**: progress bar, percentage, average speed, ETA, state, and the
  current-file line.
- **Speed**: the per-connection segment chart, the live speed trace, and the
  per-task **speed limit** (the data model already exists —
  `self.speed_windows` in `src/app.rs:2244`).
- **Settings**: connections per host, retry policy, start-queued behaviour.
- Buttons: Pause / Resume / Stop, plus a minimise-to-tray.

*Scope warning:* WS-5 is the largest workstream and it touches the transfer
engine's presentation. It is listed last for that reason. It is also the one
with the clearest existing design, so the risk is mostly in wiring, not in
inventing.

---

## 8. Sequencing

Each wave is independently shippable and independently verifiable.

| Wave | Contents | Risk | Depends on |
|------|----------|------|------------|
| **W0** | Fix the token-carryover defect (`src/app.rs:8627-8629` clears the account but not the token). Fix the stale "0 = Real-Debrid, 1 = TorBox" comment. Fix the tooltip painting over the open modal. | Low | — |
| **W1** | **WS-1** provider dropdown + capability hint + persistence of the choice. | Medium | W0 |
| **W2** | **WS-2** Cloud Providers panel → dm.rs card. | Low (visual only; `src/debrid.rs` untouched) | W1 |
| **W3** | **WS-3** catch bar: `?` segment, fit-to-width, no grip, always-visible close, copy. | Medium (extension JS) | — |
| **W4** | **WS-4** toolbar grouping + status bar. **WS-4b** Columns menu. | Low / Medium | — |
| **W5** | **WS-5** progress tabs + per-task speed limit. | High | W4 (status bar conventions) |

W0 is worth doing on its own today: it is a small, correct, low-risk fix that
does not depend on any design decision.

---

## 9. Acceptance criteria

### WS-1

- [ ] The provider control opens a list of all five providers; it no longer
      cycles on click.
- [ ] The current provider is marked; the committed provider is the one shown
      in the field.
- [ ] Each row states what that provider cannot do, derived from
      `Provider::unsupported_reason()` — not hand-written strings.
- [ ] Switching provider never leaves a previous provider's token in the field
      or in memory under the new provider's name.
- [ ] The token appears in no settings file, no queue file, no download row,
      and no log line. *(Asserted by a test.)*
- [ ] The selected provider survives an app restart; the token does not.
- [ ] `↑`/`↓`/`Enter`/`Esc` work, and the app does not keep the keyboard when
      the dropdown is closed.
- [ ] An unknown persisted value falls back to Real-Debrid rather than
      leaving the panel broken.

### WS-2

- [ ] The panel renders as a dm.rs card with no stock egui title bar.
- [ ] The primary CTA uses the accent gradient; no stock egui purple remains
      on screen.
- [ ] Every panel element uses the dm.rs token set; the panel matches the
      Add-download dialog's visual language.
- [ ] The memory-only token notice is still present and legible.
- [ ] The panel keeps normal Z-order, cascades correctly, and does not take
      the keyboard except as the first of a cascade.
- [ ] No tooltip from the underlying window paints over the open panel.

### WS-3

- [ ] The bar shows `[icon][label][?][X]`.
- [ ] The `?` opens an in-product popover; it never navigates to a website.
- [ ] The label is fully readable for "Download audio from this page" — no
      ellipsis at the clamped widths.
- [ ] The close control is visible without a hover.
- [ ] The whole bar drags; a click still triggers the download (5 px threshold
      preserved); the saved position still round-trips.
- [ ] Shadow DOM isolation is preserved: page CSS cannot restyle the bar.
- [ ] The modern round mark is unchanged.
- [ ] `node --check` and `node --test browser-extension/*.test.js` stay green.

### WS-4

- [ ] The toolbar is grouped; destructive controls are not adjacent to
      `Settings`.
- [ ] Column widths, order and (new) visibility persist across restarts.
- [ ] The status bar shows live aggregate state.

### WS-5

- [ ] Info / Speed / Settings tabs match the concept.
- [ ] A per-task speed limit takes effect without restarting the transfer.

### Build gate (every wave that touches the desktop)

Per `AGENTS.md`, from `upstream-baseline/`:

```
cargo test --locked --release
cargo build --locked --release
```

The Rust baseline is **112 warnings** and **0 failing tests**. New code must
not raise the warning count.

For extension work:

```
node --check browser-extension/background.js
node --check browser-extension/content.js
node --test browser-extension/*.test.js
python scripts/package_extensions.py --output dist/phase3-extensions
```

Plus `git diff --check`. Extension packaging is **not** a desktop build and
must never be reported as one.

---

## 10. Open decisions

These change the shape of the work and are worth a decision before W1 starts.

1. **Token on provider switch** — clear the token, or keep a per-provider
   in-memory map? *(Recommendation: per-provider map; still never persisted.)*
2. **Persisted provider** — should the chosen provider survive a restart at
   all, or always start at Real-Debrid? *(Recommendation: persist. It is a
   UI preference, not a credential.)*
3. **IDM's `?` semantics** — an in-product popover, or a link to the app's
   Help? *(Recommendation: in-product popover with a Help link inside it.)*
4. **Grip removal** — drop the 18 px grip (the whole bar already drags), or
   keep it? *(Recommendation: drop it; it matches IDM and removes dead
   chrome.)*
5. **WS-4b Columns menu** — in scope for this pass, or deferred? *(It is the
   most valuable main-window borrow and the most work.)*
6. **WS-5 progress tabs** — same question. *(Recommendation: defer to its own
   pass; it touches the engine's presentation.)*
7. **Magnet route** — should a magnet pasted into the Add dialog route to the
   provider panel (WS-1 §3.7), or stay an error there? *(Recommendation: route
   it; the current error message is a dead end.)*

---

## 11. What this plan deliberately does not do

- It does not touch `src/debrid.rs`, `src/engine.rs`, or the transfer engine.
- It does not add a torrent client, a peer connection, or any seeding.
- It does not change what the extension detects, how it detects it, or the
  frame/document isolation rules.
- It does not change the capture-confirmation model, the duplicate
  suppression, or the POST protection.
- It does not change the archive extraction rules.
- It does not redesign the DownloadCraft aesthetic. It applies the existing
  `src/dm.rs` / `redesign/*.html` system to two surfaces that were never
  converted to it.
