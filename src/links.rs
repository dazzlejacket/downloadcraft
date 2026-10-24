//! IDM-style "Download all links" window (`--links <file>`).
//!
//! The browser extension collects every link on a page (or in the selection)
//! and the native host writes them to `links_<ms>.json` in the config dir.
//! This window lists them with a checkbox each, fills in type and size with
//! background HEAD probes, and queues the checked ones straight into the
//! shared `queue.json` (+ `wake.json` to start them), like the confirm dialog.
//!
//! Layout follows `redesign/download_all_links.html`: a page header with
//! counts, a toolbar (search, type chips, "Hide web pages"), a painted table
//! (type icons, two-line names with the extension, a category pill, sortable
//! and resizable columns, Shift-click ranges) and a footer (selection, Save to,
//! actions). A right-click on a row opens a small menu of its own: "Copy URL"
//! and "Download".

use crate::app::pl;
use crate::engine::{self, EngineOpts};
use crate::fonts::{icon, reg, semi};
use crate::model::{category_of, config_dir, filename_from_url, format_bytes, DownloadItem};
use crate::native_host::LinksBatch;
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};
use std::sync::mpsc::Receiver;

/// Probes in flight at once (one per host would be politer, but pages that
/// list files usually serve them all from one host anyway).
const PROBE_CONCURRENCY: usize = 6;
/// Links beyond this are listed but not probed (type from the name only).
const PROBE_MAX: usize = 1000;

const MI_GLOBE: &str = "\u{e80b}";
const MI_CHECK: &str = "\u{e5ca}";
const MI_DOWNLOAD: &str = "\u{e2c4}";
const MI_QUEUE: &str = "\u{e03b}";
const MI_FOLDER: &str = "\u{e2c7}";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Probe {
    Pending,
    Done,
    Failed,
}

/// What a link is, for its icon, colour, pill and type chip.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Video,
    Audio,
    Image,
    Doc,
    Zip,
    App,
    Page,
    Other,
}

impl Kind {
    const ALL: [Kind; 8] = [Kind::Video, Kind::Audio, Kind::Image, Kind::Doc, Kind::Zip, Kind::App, Kind::Page, Kind::Other];
    fn of(name: &str, html: bool, image: bool) -> Kind {
        if html {
            return Kind::Page;
        }
        if image {
            return Kind::Image;
        }
        Kind::of_category(category_of(name))
    }
    /// The kind a category name stands for. The audio category is called
    /// "Audio"; "Music" is what an older build says, and still counts.
    fn of_category(cat: &str) -> Kind {
        match cat {
            "Video" => Kind::Video,
            "Audio" | "Music" => Kind::Audio,
            "Image" => Kind::Image,
            "Document" => Kind::Doc,
            "Compressed" => Kind::Zip,
            "Apps" => Kind::App,
            _ => Kind::Other,
        }
    }
    /// Chip label (plural) and category label (singular).
    fn labels(self) -> (&'static str, &'static str) {
        match self {
            Kind::Video => ("Video", "Video"),
            Kind::Audio => ("Audio", "Audio"),
            Kind::Image => ("Images", "Image"),
            Kind::Doc => ("Documents", "Document"),
            Kind::Zip => ("Archives", "Archive"),
            Kind::App => ("Programs", "Program"),
            Kind::Page => ("Web pages", "Page"),
            Kind::Other => ("Other", "Other"),
        }
    }
    /// Where this kind sits in `Kind::ALL`: the order the chips are in, and
    /// the order the Category column sorts by.
    fn rank(self) -> usize {
        Kind::ALL.iter().position(|k| *k == self).unwrap_or(Kind::ALL.len())
    }
    fn glyph(self) -> &'static str {
        match self {
            Kind::Video => crate::app::MI_SLIDESHOW,
            Kind::Audio => crate::app::MI_MUSIC,
            Kind::Image => crate::app::MI_IMAGE,
            Kind::Doc => crate::app::MI_DOC,
            Kind::Zip => crate::app::MI_ARCHIVE,
            Kind::App => crate::app::MI_GRID,
            Kind::Page => MI_GLOBE,
            Kind::Other => crate::app::MI_INSERT_FILE,
        }
    }
    /// The mock-up's per-type colours, brighter on the dark theme.
    fn color(self, dark: bool) -> Color32 {
        let (d, l) = match self {
            Kind::Video => (0xa78bfa, 0x7c3aed),
            Kind::Audio => (0xf472b6, 0xdb2777),
            Kind::Image => (0x22d3ee, 0x0891b2),
            Kind::Doc => (0xfb923c, 0xea580c),
            Kind::Zip => (0xfacc15, 0xca8a04),
            Kind::App => (0x4ade80, 0x16a34a),
            Kind::Page | Kind::Other => (0x94a3b8, 0x64748b),
        };
        let v: u32 = if dark { d } else { l };
        Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }
}

struct Row {
    url: String,
    text: String,
    name: String,
    /// The name is still our guess from the URL (a server name may replace it).
    guessed_name: bool,
    kind: Kind,
    size: Option<u64>,
    checked: bool,
    probe: Probe,
}

/// What the file is, from its name and (once probed) its Content-Type:
/// (label, is a web page, is an image).
fn type_of(name: &str, ctype: Option<&str>) -> (String, bool, bool) {
    let mime = ctype
        .map(|c| c.split(';').next().unwrap_or("").trim().to_ascii_lowercase())
        .filter(|c| !c.is_empty() && c != "application/octet-stream");
    if let Some(m) = mime.as_deref() {
        if m == "text/html" || m == "application/xhtml+xml" {
            return ("Web page".to_string(), true, false);
        }
        if m.starts_with("image/") {
            return (format!("{} image", m[6..].to_ascii_uppercase()), false, true);
        }
    }
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "htm" | "html" | "php" | "asp" | "aspx" | "jsp" | "cgi" | "shtml" => {
            return ("Web page".to_string(), true, false)
        }
        "" if mime.is_none() => return ("Web page".to_string(), true, false),
        _ => {}
    }
    let cat = category_of(name);
    if cat == "Image" {
        return (format!("{} image", ext.to_ascii_uppercase()), false, true);
    }
    if !ext.is_empty() && cat != "Other" {
        return (format!("{} {}", ext.to_ascii_uppercase(), cat.to_ascii_lowercase()), false, false);
    }
    match mime {
        Some(m) => (m, false, false),
        None => (format!("{} file", ext.to_ascii_uppercase()), false, false),
    }
}

/// Same host (cookies of the page are only sent back to it).
fn same_host(a: &str, b: &str) -> bool {
    let host = |u: &str| reqwest::Url::parse(u).ok().and_then(|x| x.host_str().map(|h| h.to_ascii_lowercase()));
    matches!((host(a), host(b)), (Some(x), Some(y)) if x == y)
}

/// Host and the last folder of a link ("download.sunplex.net", "S01").
fn host_and_folder(url: &str) -> (String, String) {
    let Ok(u) = reqwest::Url::parse(url) else { return (url.to_string(), String::new()) };
    let host = u.host_str().unwrap_or("").to_string();
    let segs: Vec<&str> = u.path_segments().map(|s| s.filter(|x| !x.is_empty()).collect()).unwrap_or_default();
    let folder = if segs.len() >= 2 { segs[segs.len() - 2] } else { "" };
    let folder = percent_decode(folder);
    (host, folder)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Read the host's links file, but only from our own config dir and only
/// under the name the host gives it; the file is removed once read.
fn read_batch(path: &std::path::Path) -> Result<LinksBatch, String> {
    let dir = config_dir().ok_or("no config dir")?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let inside = path.parent().map(|p| p == dir).unwrap_or(false);
    if !inside || !name.starts_with("links_") || !name.ends_with(".json") {
        return Err("not a links file from the browser extension".into());
    }
    let data = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(path);
    serde_json::from_str(&data).map_err(|e| e.to_string())
}

/// Sortable columns.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SortBy {
    Name,
    Size,
    Category,
    From,
}

/// The row menu a right-click opens: which row, and where.
#[derive(Clone, Copy)]
struct RowMenu {
    row: usize,
    pos: Pos2,
}

/// The name without its extension, and the extension itself.
///
/// Only a short trailing word counts as an extension, so "A.Knight.of.the.
/// Seven.Kingdoms" keeps its name while "track.mp3" gives up its ".mp3".
fn split_ext(name: &str) -> (&str, &str) {
    match name.rsplit_once('.') {
        Some((base, ext))
            if !base.is_empty()
                && !ext.is_empty()
                && ext.len() <= 6
                && ext.chars().all(|c| c.is_ascii_alphanumeric()) =>
        {
            (base, ext)
        }
        _ => (name, ""),
    }
}

struct LinksApp {
    page_url: String,
    title: String,
    cookie: String,
    rows: Vec<Row>,
    filter: String,
    /// Type chip: None = all.
    chip: Option<Kind>,
    hide_pages: bool,
    sort: Option<(SortBy, bool)>,
    /// Row index of the last plain click (Shift-click checks a range).
    anchor: Option<usize>,
    /// true = every file into its category folder (Settings rules);
    /// false = all into `one_dir`.
    by_category: bool,
    one_dir: String,
    prefs: crate::confirm::UiPrefs,
    /// Widths of the sized columns (see `COLS`), dragged by their dividers.
    cols: [f32; 3],
    /// The row menu a right-click opened, if it is open.
    menu: Option<RowMenu>,
    probe_rx: Option<Receiver<(usize, Option<u64>, Option<String>, Option<String>)>>,
    probes_left: usize,
    _rt: std::sync::Arc<tokio::runtime::Runtime>,
    notice: String,
    done: bool,
    exited: bool,
    /// Where this window sits in the cascade of open dialogs.
    slot: crate::window::CascadeSlot,
    /// Our own window handle, for the activation rule.
    hwnd: Option<isize>,
}

impl LinksApp {
    fn visible(&self, r: &Row) -> bool {
        if let Some(k) = self.chip {
            if r.kind != k {
                return false;
            }
        } else if self.hide_pages && r.kind == Kind::Page {
            return false;
        }
        let f = self.filter.trim().to_lowercase();
        f.is_empty()
            || r.name.to_lowercase().contains(&f)
            || r.url.to_lowercase().contains(&f)
            || r.text.to_lowercase().contains(&f)
            || r.kind.labels().0.to_lowercase().contains(&f)
    }

    /// Indexes of the rows shown, in display order.
    fn shown(&self) -> Vec<usize> {
        let mut v: Vec<usize> = (0..self.rows.len()).filter(|&i| self.visible(&self.rows[i])).collect();
        if let Some((by, asc)) = self.sort {
            let r = &self.rows;
            v.sort_by(|&a, &b| {
                let o = match by {
                    SortBy::Name => r[a].name.to_lowercase().cmp(&r[b].name.to_lowercase()),
                    SortBy::Size => r[a].size.unwrap_or(0).cmp(&r[b].size.unwrap_or(0)),
                    // The category sorts in the order the chips are in.
                    SortBy::Category => r[a].kind.rank().cmp(&r[b].kind.rank()),
                    SortBy::From => r[a].url.cmp(&r[b].url),
                };
                if asc { o } else { o.reverse() }
            });
        }
        v
    }

    fn start_probes(&mut self, ctx: egui::Context) {
        let (tx, rx) = std::sync::mpsc::channel();
        self.probe_rx = Some(rx);
        let jobs: Vec<(usize, String)> = self
            .rows
            .iter()
            .enumerate()
            .take(PROBE_MAX)
            .map(|(i, r)| (i, r.url.clone()))
            .collect();
        self.probes_left = jobs.len();
        for r in self.rows.iter_mut().skip(PROBE_MAX) {
            r.probe = Probe::Failed;
        }
        let base = EngineOpts {
            user_agent: if self.prefs.user_agent.trim().is_empty() {
                format!("download_craft/{}", crate::model::APP_VERSION)
            } else {
                self.prefs.user_agent.clone()
            },
            timeout_secs: self.prefs.timeout_secs.min(20),
            proxy_url: self.prefs.proxy_url.clone(),
            referer: self.page_url.clone(),
            ..Default::default()
        };
        let with_cookie = EngineOpts { cookie: self.cookie.clone(), ..base.clone() };
        let page = self.page_url.clone();
        self._rt.spawn(async move {
            use futures_util::StreamExt;
            let (Ok(plain), Ok(session)) = (base.build_client(), with_cookie.build_client()) else {
                for (i, _) in jobs {
                    let _ = tx.send((i, None, None, None));
                }
                ctx.request_repaint();
                return;
            };
            futures_util::stream::iter(jobs)
                .for_each_concurrent(PROBE_CONCURRENCY, |(i, url)| {
                    let (client, opts) = if same_host(&url, &page) {
                        (&session, &with_cookie)
                    } else {
                        (&plain, &base)
                    };
                    let tx = tx.clone();
                    let ctx = ctx.clone();
                    async move {
                        let (total, _, ctype, disp) = engine::probe(client, opts, &url).await;
                        let _ = tx.send((i, total, ctype, disp));
                        ctx.request_repaint();
                    }
                })
                .await;
        });
    }

    fn collect_probes(&mut self) {
        let Some(rx) = self.probe_rx.as_ref() else { return };
        while let Ok((i, total, ctype, disp)) = rx.try_recv() {
            self.probes_left = self.probes_left.saturating_sub(1);
            let Some(r) = self.rows.get_mut(i) else { continue };
            if total.is_none() && ctype.is_none() {
                r.probe = Probe::Failed;
                continue;
            }
            r.probe = Probe::Done;
            if r.guessed_name {
                if let Some(d) = disp.filter(|d| !d.trim().is_empty()) {
                    r.name = engine::sanitize_filename(&d);
                } else if !r.name.contains('.') {
                    if let Some(ext) = ctype.as_deref().and_then(crate::model::ext_for_mime) {
                        r.name.push_str(ext);
                    }
                }
            }
            let (_, html, image) = type_of(&r.name, ctype.as_deref());
            r.kind = Kind::of(&r.name, html, image);
            // A page's length is its HTML, not a download size.
            r.size = total.filter(|s| *s > 0 && !html);
        }
    }

    fn dir_for(&self, name: &str) -> String {
        if !self.by_category {
            return self.one_dir.clone();
        }
        let mut p = std::path::PathBuf::from(&self.prefs.save_dir);
        match self.prefs.folder_rules.iter().find(|r| r.matches(name)) {
            Some(rule) if !rule.folder.trim().is_empty() => p.push(rule.folder.trim()),
            _ => p.push(category_of(name)),
        }
        p.to_string_lossy().to_string()
    }

    /// The queue entry for one row: the rename template, the folder rule and
    /// the cookies of the page, all applied.
    fn item_for(&self, r: &Row) -> DownloadItem {
        let name = crate::model::apply_rename_template(
            &self.prefs.rename_template,
            &engine::sanitize_filename(r.name.trim()),
            &r.url,
        );
        let mut item = DownloadItem::new(
            0,
            r.url.clone(),
            engine::sanitize_filename(&name),
            self.dir_for(&name),
            self.prefs.connections,
        );
        item.referer = self.page_url.clone();
        if same_host(&r.url, &self.page_url) {
            item.cookie = self.cookie.clone();
        }
        item.description = r.text.clone();
        item.total_bytes = r.size;
        item
    }

    /// Queue the checked, visible rows; `start` also starts them.
    fn commit(&mut self, start: bool) {
        let picked: Vec<usize> = (0..self.rows.len())
            .filter(|&i| self.rows[i].checked && self.visible(&self.rows[i]))
            .collect();
        if picked.is_empty() {
            self.notice = "Check the links to download first.".into();
            return;
        }
        let (mut added, mut already, mut failed) = (Vec::new(), 0usize, None);
        for i in picked {
            let mut item = self.item_for(&self.rows[i]);
            match crate::model::append_to_queue(&mut item) {
                crate::model::AppendResult::Added(id) => added.push(id),
                crate::model::AppendResult::AlreadyQueued(_) => already += 1,
                crate::model::AppendResult::Error(e) => failed = Some(e),
            }
        }
        if start && !added.is_empty() {
            crate::confirm::wake_ids(&added);
        }
        if let Some(e) = failed {
            self.notice = format!("Queued {}; could not update the queue: {e}", added.len());
            return;
        }
        if added.is_empty() {
            self.notice = format!("All {already} checked link(s) are already in the queue.");
            return;
        }
        self.done = true;
    }

    /// Queue one row and start it (the row menu's "Download").
    ///
    /// The window stays open: the links file has been read already, so the
    /// user may well want another row from the same page.
    fn download_row(&mut self, i: usize) {
        let Some(r) = self.rows.get(i) else { return };
        let name = r.name.clone();
        let mut item = self.item_for(r);
        match crate::model::append_to_queue(&mut item) {
            crate::model::AppendResult::Added(id) => {
                crate::confirm::wake_ids(&[id]);
                self.notice = format!("Downloading {name}.");
            }
            crate::model::AppendResult::AlreadyQueued(_) => {
                self.notice = "That link is already in the queue.".into();
            }
            crate::model::AppendResult::Error(e) => {
                self.notice = format!("Could not update the queue: {e}");
            }
        }
    }

    /// Copy one row's URL to the clipboard.
    fn copy_url(&mut self, i: usize) {
        let Some(r) = self.rows.get(i) else { return };
        let url = r.url.clone();
        match arboard::Clipboard::new() {
            Ok(mut cb) => {
                let _ = cb.set_text(url);
                self.notice = "URL copied to the clipboard.".into();
            }
            Err(_) => self.notice = "Could not reach the clipboard.".into(),
        }
    }

    /// Click on a row: toggle it, or with Shift every row between it and the
    /// last clicked one (to the clicked row's new state).
    fn click_row(&mut self, i: usize, shift: bool, shown: &[usize]) {
        let on = !self.rows[i].checked;
        match (shift, self.anchor.and_then(|a| shown.iter().position(|&x| x == a)), shown.iter().position(|&x| x == i)) {
            (true, Some(a), Some(b)) => {
                for &k in &shown[a.min(b)..=a.max(b)] {
                    self.rows[k].checked = on;
                }
            }
            _ => self.rows[i].checked = on,
        }
        self.anchor = Some(i);
    }

    // ------------------------------------------------------------- header --

    fn header(&mut self, ui: &mut egui::Ui) {
        let t = pl();
        let files: Vec<&Row> = self.rows.iter().filter(|r| r.kind != Kind::Page).collect();
        let found: u64 = files.iter().map(|r| r.size.unwrap_or(0)).sum();
        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::hover());
        let p = ui.painter();
        // Page mark.
        let fav = Rect::from_min_size(rect.min + Vec2::new(0.0, 3.0), Vec2::splat(34.0));
        p.rect_filled(fav, 8.0, t.primary.gamma_multiply(0.16));
        p.text(fav.center(), Align2::CENTER_CENTER, MI_GLOBE, icon(18.0), t.primary);
        // Stats, right to left.
        let mut right = rect.max.x;
        let stats = [
            (format_bytes(found), " found"),
            (files.len().to_string(), " files"),
            (self.rows.len().to_string(), " links"),
        ];
        for (n, label) in stats {
            let wn = crate::shaped::width(p, &n, &semi(11.5));
            let wl = crate::shaped::width(p, label, &reg(11.5));
            let w = wn + wl + 18.0;
            let r = Rect::from_min_size(Pos2::new(right - w, rect.center().y - 12.0), Vec2::new(w, 24.0));
            p.rect(r, 6.0, t.surface, Stroke::new(1.0_f32, t.card_bd), egui::StrokeKind::Inside);
            p.text(Pos2::new(r.min.x + 9.0, r.center().y), Align2::LEFT_CENTER, &n, semi(11.5), t.txt);
            p.text(Pos2::new(r.min.x + 9.0 + wn, r.center().y), Align2::LEFT_CENTER, label, reg(11.5), t.dim);
            right = r.min.x - 6.0;
        }
        // Title and address.
        let x = fav.max.x + 12.0;
        let max_w = (right - x - 12.0).max(80.0);
        let title = if self.title.trim().is_empty() { "Download all links".to_string() } else { self.title.clone() };
        let title = crate::app::truncate_to_width(p, &title, semi(14.5), t.txt, max_w);
        crate::shaped::draw(p, Pos2::new(x, rect.min.y + 4.0), Align2::LEFT_TOP, &title, semi(14.5), t.txt);
        let url = crate::app::truncate_to_width(p, &self.page_url, reg(11.0), t.dim, max_w);
        p.text(Pos2::new(x, rect.min.y + 25.0), Align2::LEFT_TOP, url, reg(11.0), t.dim);
    }

    // ------------------------------------------------------------ toolbar --

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let t = pl();
        let dark = ui.visuals().dark_mode;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            // Search field.
            let (r, _) = ui.allocate_exact_size(Vec2::new(250.0, 30.0), Sense::hover());
            let focused = ui.memory(|m| m.focused() == Some(egui::Id::new("links-q")));
            ui.painter().rect(r, 6.0, t.bg, Stroke::new(1.0_f32, if focused { t.primary } else { t.card_bd }), egui::StrokeKind::Inside);
            ui.painter().text(Pos2::new(r.min.x + 16.0, r.center().y), Align2::CENTER_CENTER, crate::app::MI_SEARCH, icon(15.0), t.dim);
            ui.put(
                Rect::from_min_max(Pos2::new(r.min.x + 28.0, r.min.y + 4.0), Pos2::new(r.max.x - 6.0, r.max.y - 4.0)),
                egui::TextEdit::singleline(&mut self.filter)
                    .id(egui::Id::new("links-q"))
                    .frame(false)
                    .font(reg(12.0))
                    .hint_text("Filter by name, type or address")
                    .vertical_align(egui::Align::Center),
            );
            // Type chips with counts (only the types present).
            let mut counts = [0usize; 8];
            for r in &self.rows {
                counts[Kind::ALL.iter().position(|k| *k == r.kind).unwrap_or(7)] += 1;
            }
            let mut chips: Vec<(Option<Kind>, String, usize)> = vec![(None, "All".into(), self.rows.len())];
            for (k, n) in Kind::ALL.iter().zip(counts) {
                if n > 0 {
                    chips.push((Some(*k), k.labels().0.into(), n));
                }
            }
            let toggle_w = 132.0;
            let avail = ui.available_width() - toggle_w - 8.0;
            egui::ScrollArea::horizontal()
                .id_salt("links-chips")
                .max_width(avail)
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for (k, label, n) in chips {
                            let on = self.chip == k;
                            let num = n.to_string();
                            let dot = if k.is_some() { 12.0 } else { 0.0 };
                            let w = dot + crate::shaped::width(ui.painter(), &label, &reg(11.5)) + crate::shaped::width(ui.painter(), &num, &reg(10.5)) + 26.0;
                            let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 28.0), Sense::click());
                            let p = ui.painter();
                            let (fill, bd, fg) = if on {
                                (t.primary.gamma_multiply(0.14), t.primary, t.primary)
                            } else if resp.hovered() {
                                (t.surface_lt, t.card_bd, t.txt)
                            } else {
                                (t.surface, t.card_bd, t.dim)
                            };
                            p.rect(r, 14.0, fill, Stroke::new(1.0_f32, bd), egui::StrokeKind::Inside);
                            let mut x = r.min.x + 10.0;
                            if let Some(k) = k {
                                p.circle_filled(Pos2::new(x + 3.5, r.center().y), 3.5, k.color(dark));
                                x += dot;
                            }
                            let lw = crate::shaped::width(p, &label, &reg(11.5));
                            p.text(Pos2::new(x, r.center().y), Align2::LEFT_CENTER, &label, reg(11.5), fg);
                            p.text(Pos2::new(x + lw + 5.0, r.center().y), Align2::LEFT_CENTER, &num, reg(10.5), if on { t.primary } else { t.dim });
                            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                self.chip = k;
                            }
                        }
                    });
                });
            // "Hide web pages" switch, right-aligned.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (r, resp) = ui.allocate_exact_size(Vec2::new(toggle_w, 28.0), Sense::click());
                let p = ui.painter();
                let sw = Rect::from_min_size(Pos2::new(r.min.x, r.center().y - 8.0), Vec2::new(28.0, 16.0));
                p.rect_filled(sw, 8.0, if self.hide_pages { t.primary } else { t.surface_lt });
                let kx = if self.hide_pages { sw.max.x - 8.0 } else { sw.min.x + 8.0 };
                p.circle_filled(Pos2::new(kx, sw.center().y), 6.0, Color32::WHITE);
                p.text(Pos2::new(sw.max.x + 8.0, r.center().y), Align2::LEFT_CENTER, "Hide web pages", reg(11.5), if resp.hovered() { t.txt } else { t.dim });
                if resp.on_hover_text("Folder listings, site pages and social links").clicked() {
                    self.hide_pages = !self.hide_pages;
                }
            });
        });
    }

    // -------------------------------------------------------------- table --

    fn table(&mut self, ui: &mut egui::Ui) {
        let t = pl();
        let dark = ui.visuals().dark_mode;
        let shown = self.shown();
        let card = ui.available_rect_before_wrap();
        ui.painter().rect(card, 8.0, t.bg, Stroke::new(1.0_f32, t.card_bd), egui::StrokeKind::Inside);
        let inner = card.shrink(1.0);
        let widths = col_widths(&self.cols, inner.width() - CB_W - 10.0);
        let xs = {
            let mut x = inner.min.x + CB_W;
            let mut v = [0.0; 4];
            for (k, w) in widths.iter().enumerate() {
                v[k] = x;
                x += w;
            }
            v
        };
        // ---- header ----
        let hdr = Rect::from_min_size(inner.min, Vec2::new(inner.width(), HDR_H));
        let p = ui.painter().clone();
        p.rect_filled(hdr, CornerRadius { nw: 7, ne: 7, sw: 0, se: 0 }, t.surface);
        p.hline(hdr.x_range(), hdr.max.y, Stroke::new(1.0_f32, t.card_bd));
        // Tri-state check over what is shown.
        let n_on = shown.iter().filter(|&&i| self.rows[i].checked).count();
        let cbr = Rect::from_center_size(Pos2::new(inner.min.x + CB_W / 2.0, hdr.center().y), Vec2::splat(15.0));
        let resp = ui.interact(cbr.expand(4.0), egui::Id::new("links-all"), Sense::click()).on_hover_text("Check or uncheck everything shown");
        paint_check(&p, cbr, n_on > 0 && n_on == shown.len(), n_on > 0 && n_on < shown.len(), resp.hovered());
        if resp.clicked() {
            let on = n_on != shown.len();
            for &i in &shown {
                self.rows[i].checked = on;
            }
        }
        for (k, (label, by)) in COLS.iter().enumerate() {
            let r = Rect::from_min_size(Pos2::new(xs[k], hdr.min.y), Vec2::new(widths[k], HDR_H));
            let resp = ui.interact(r, egui::Id::new(("links-sort", k)), Sense::click());
            let sorted = self.sort.map(|(b, _)| b == *by).unwrap_or(false);
            let fg = if sorted || resp.hovered() { t.txt } else { t.dim };
            let arrow = match self.sort {
                Some((b, asc)) if b == *by => if asc { " ▲" } else { " ▼" },
                _ => "",
            };
            let text = format!("{}{arrow}", label.to_uppercase());
            if *by == SortBy::Size {
                p.text(Pos2::new(r.max.x - 10.0, r.center().y), Align2::RIGHT_CENTER, text, semi(10.5), fg);
            } else {
                p.text(Pos2::new(r.min.x + 10.0, r.center().y), Align2::LEFT_CENTER, text, semi(10.5), fg);
            }
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                self.sort = Some((*by, if sorted { !self.sort.map(|(_, a)| a).unwrap_or(true) } else { true }));
            }
            // Resize strip on the right edge of every column but the last:
            // a faint line always, an accent bar while hovered or dragged.
            if k + 1 < COLS.len() {
                let zone = Rect::from_center_size(Pos2::new(r.max.x, hdr.center().y), Vec2::new(8.0, HDR_H));
                let g = ui
                    .interact(zone, egui::Id::new(("links-col", k)), Sense::drag())
                    .on_hover_cursor(egui::CursorIcon::ResizeColumn)
                    .on_hover_text("Drag to resize");
                let active = g.hovered() || g.dragged();
                let bar = if active {
                    Rect::from_center_size(zone.center(), Vec2::new(3.0, HDR_H - 10.0))
                } else {
                    Rect::from_center_size(zone.center(), Vec2::new(1.0, HDR_H - 18.0))
                };
                p.rect_filled(bar, 1.5, if active { t.primary } else { t.card_bd });
                if g.dragged() && g.drag_delta().x != 0.0 {
                    self.cols = drag_divider(&self.cols, k, g.drag_delta().x);
                }
                if g.drag_stopped() {
                    save_cols(&self.cols);
                }
            }
        }
        // ---- rows ----
        let body = Rect::from_min_max(Pos2::new(inner.min.x, hdr.max.y + 1.0), inner.max);
        if shown.is_empty() {
            p.text(body.center(), Align2::CENTER_CENTER, "Nothing matches — clear the search or pick another type.", reg(12.0), t.dim);
            ui.allocate_rect(card, Sense::hover());
            return;
        }
        let shift = ui.input(|i| i.modifiers.shift);
        let mut clicked: Option<usize> = None;
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
        egui::ScrollArea::vertical()
            .id_salt("links-rows")
            .auto_shrink([false, false])
            .show_rows(&mut child, ROW_H, shown.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let time = ui.input(|i| i.time) as f32;
                for &i in &shown[range] {
                    let (r, resp) = ui.allocate_exact_size(Vec2::new(body.width(), ROW_H), Sense::click());
                    let row = &self.rows[i];
                    let p = ui.painter();
                    if row.checked {
                        p.rect_filled(r, 0.0, t.sel_bg);
                    } else if resp.hovered() {
                        p.rect_filled(r, 0.0, t.hover_bg);
                    }
                    p.hline(r.x_range(), r.max.y - 0.5, Stroke::new(1.0_f32, t.card_bd.gamma_multiply(0.5)));
                    let dim = if row.kind == Kind::Page { 0.6 } else { 1.0 };
                    let cy = r.center().y;
                    paint_check(p, Rect::from_center_size(Pos2::new(r.min.x + CB_W / 2.0, cy), Vec2::splat(15.0)), row.checked, false, resp.hovered());
                    // Name: type tile + name + link text.
                    let col = row.kind.color(dark).gamma_multiply(dim);
                    let tile = Rect::from_min_size(Pos2::new(xs[0] + 8.0, cy - 13.0), Vec2::splat(26.0));
                    p.rect_filled(tile, 6.0, col.gamma_multiply(0.16));
                    p.text(tile.center(), Align2::CENTER_CENTER, row.kind.glyph(), icon(15.0), col);
                    let nx = tile.max.x + 9.0;
                    let nw = (xs[0] + widths[0] - nx - 8.0).max(20.0);
                    // The name, with its extension dimmed behind it: the old
                    // "Type" column is gone, but .mp3 must still read as .mp3.
                    let (base, ext) = split_ext(&row.name);
                    let ext_w = if ext.is_empty() { 0.0 } else { crate::shaped::width(p, &format!(".{ext}"), &reg(11.0)) + 5.0 };
                    let name = crate::app::truncate_to_width(p, base, semi(12.0), t.txt, (nw - ext_w).max(20.0));
                    crate::shaped::draw(p, Pos2::new(nx, cy - 1.0), Align2::LEFT_BOTTOM, &name, semi(12.0), t.txt.gamma_multiply(dim));
                    if !ext.is_empty() {
                        let ex = nx + crate::shaped::width(p, &name, &semi(12.0)) + 5.0;
                        crate::shaped::draw(p, Pos2::new(ex, cy - 1.0), Align2::LEFT_BOTTOM, &format!(".{ext}"), reg(11.0), t.dim.gamma_multiply(dim));
                    }
                    let sub = crate::app::truncate_to_width(p, &row.text, reg(10.5), t.dim, nw);
                    crate::shaped::draw(p, Pos2::new(nx, cy + 1.0), Align2::LEFT_TOP, &sub, reg(10.5), t.dim);
                    // Category: what the file is, in the colour of its kind.
                    let cat = row.kind.labels().1;
                    let gw = crate::shaped::width(p, row.kind.glyph(), &icon(13.0));
                    let cw = crate::shaped::width(p, cat, &semi(10.5));
                    let pill = Rect::from_min_size(Pos2::new(xs[2] + 10.0, cy - 10.0), Vec2::new((gw + cw + 26.0).min(widths[2] - 16.0), 20.0));
                    p.rect_filled(pill, 5.0, col.gamma_multiply(0.16));
                    p.text(Pos2::new(pill.min.x + 8.0, cy), Align2::LEFT_CENTER, row.kind.glyph(), icon(13.0), col);
                    let ct = crate::app::truncate_to_width(p, cat, semi(10.5), col, pill.width() - gw - 22.0);
                    p.text(Pos2::new(pill.min.x + 8.0 + gw + 5.0, cy), Align2::LEFT_CENTER, ct, semi(10.5), col);
                    // Size, or a shimmer while it is being checked.
                    let sx = xs[1] + widths[1] - 10.0;
                    match (row.size, row.probe) {
                        (Some(s), _) => {
                            p.text(Pos2::new(sx, cy), Align2::RIGHT_CENTER, format_bytes(s), reg(12.0), t.txt.gamma_multiply(dim));
                        }
                        (None, Probe::Pending) => {
                            let bar = Rect::from_min_size(Pos2::new(sx - 58.0, cy - 5.0), Vec2::new(58.0, 10.0));
                            let a = 0.35 + 0.25 * (time * 3.0 + i as f32 * 0.4).sin().abs();
                            p.rect_filled(bar, 4.0, t.surface_lt.gamma_multiply(a + 0.4));
                            ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
                        }
                        _ => {
                            p.text(Pos2::new(sx, cy), Align2::RIGHT_CENTER, "—", reg(12.0), t.dim);
                        }
                    }
                    // Download from: host + last folder tag.
                    let (host, folder) = host_and_folder(&row.url);
                    let hx = xs[3] + 10.0;
                    let hw = (widths[3] - 20.0).max(20.0);
                    let host_s = crate::app::truncate_to_width(p, &host, reg(12.0), t.dim, hw * 0.65);
                    let host_w = crate::shaped::width(p, &host_s, &reg(12.0));
                    p.text(Pos2::new(hx, cy), Align2::LEFT_CENTER, &host_s, reg(12.0), t.dim);
                    if !folder.is_empty() {
                        let room = hw - host_w - 14.0;
                        if room > 24.0 {
                            let f = crate::app::truncate_to_width(p, &folder, reg(9.5), t.dim, room - 10.0);
                            let fw = crate::shaped::width(p, &f, &reg(9.5)) + 10.0;
                            let tag = Rect::from_min_size(Pos2::new(hx + host_w + 6.0, cy - 8.0), Vec2::new(fw, 16.0));
                            p.rect_filled(tag, 4.0, t.surface_lt);
                            crate::shaped::draw(p, Pos2::new(tag.min.x + 5.0, cy), Align2::LEFT_CENTER, &f, reg(9.5), t.dim);
                        }
                    }
                    // A right-click opens the row menu on this row.
                    let menu_pos = resp.secondary_clicked().then(|| {
                        resp.interact_pointer_pos()
                            .or_else(|| ui.input(|i| i.pointer.latest_pos()))
                            .unwrap_or(r.center())
                    });
                    if resp.on_hover_text(&row.url).clicked() {
                        clicked = Some(i);
                    }
                    if let Some(pos) = menu_pos {
                        self.menu = Some(RowMenu { row: i, pos });
                    }
                }
            });
        // While the menu is open it owns the pointer: the click that dismisses
        // it must not tick the row underneath as well.
        if let Some(i) = clicked {
            if self.menu.is_none() {
                self.click_row(i, shift, &shown);
            }
        }
        ui.allocate_rect(card, Sense::hover());
    }

    // ------------------------------------------------------------- footer --

    fn footer(&mut self, ui: &mut egui::Ui) {
        let t = pl();
        let checked: Vec<&Row> = self.rows.iter().filter(|r| r.checked && self.visible(r)).collect();
        let n = checked.len();
        let bytes: u64 = checked.iter().map(|r| r.size.unwrap_or(0)).sum();
        let total: u64 = self.rows.iter().filter(|r| r.kind != Kind::Page).map(|r| r.size.unwrap_or(0)).sum();
        let (area, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 74.0), Sense::hover());
        let p = ui.painter().clone();
        // The actions, right-aligned on the bottom line. "Download selected"
        // says plainly that only the ticked rows are taken, and how many.
        let dl_label = if n > 0 { format!("Download selected ({n})") } else { "Download selected".to_string() };
        let buttons: [(&str, String, bool, bool, &str); 3] = [
            ("dl", dl_label, true, n > 0, "Download the checked files and start them"),
            ("queue", "Add to queue".into(), false, n > 0, "Queue the checked files without starting them"),
            ("cancel", "Cancel".into(), false, true, "Close without downloading"),
        ];
        let btn_w: Vec<f32> = buttons
            .iter()
            .map(|(id, label, _, _, _)| crate::shaped::width(&p, label, &semi(12.0)) + if *id == "cancel" { 32.0 } else { 48.0 })
            .collect();
        let btns_left = area.max.x - btn_w.iter().sum::<f32>() - 8.0 * (buttons.len() as f32 - 1.0);
        // Selection line + bar.
        let line_y = area.min.y + 10.0;
        if n > 0 {
            let a = format!("{n}");
            let b = format!(" of {} checked · ", self.rows.len());
            let c = format_bytes(bytes);
            let mut x = area.min.x;
            for (s, f, col) in [(a, semi(12.0), t.txt), (b, reg(12.0), t.dim), (c, semi(12.0), t.txt)] {
                let w = crate::shaped::width(&p, &s, &f);
                p.text(Pos2::new(x, line_y), Align2::LEFT_CENTER, s, f, col);
                x += w;
            }
        } else {
            p.text(Pos2::new(area.min.x, line_y), Align2::LEFT_CENTER, "Nothing checked yet — tick the files you want.", reg(12.0), t.dim);
        }
        let bar = Rect::from_min_size(Pos2::new(area.min.x, line_y + 12.0), Vec2::new(260.0, 4.0));
        p.rect_filled(bar, 2.0, t.surface_lt);
        if total > 0 && bytes > 0 {
            let f = (bytes as f32 / total as f32).clamp(0.0, 1.0);
            p.rect_filled(Rect::from_min_size(bar.min, Vec2::new(bar.width() * f, 4.0)), 2.0, t.primary);
        }
        if self.probes_left > 0 {
            p.text(Pos2::new(bar.max.x + 12.0, bar.center().y), Align2::LEFT_CENTER, format!("checking {} …", self.probes_left), reg(10.5), t.dim);
        }
        // The notice has the top line to itself, on the right.
        if !self.notice.is_empty() {
            p.text(Pos2::new(area.max.x, line_y), Align2::RIGHT_CENTER, &self.notice, reg(11.0), t.warning);
        }
        // Save to: label, segmented choice, folder field + Browse.
        let row_y = area.max.y - 18.0;
        p.text(Pos2::new(area.min.x, row_y), Align2::LEFT_CENTER, "Save to", reg(11.5), t.dim);
        let seg = Rect::from_min_size(Pos2::new(area.min.x + 50.0, row_y - 15.0), Vec2::new(214.0, 30.0));
        p.rect(seg, 6.0, t.surface, Stroke::new(1.0_f32, t.card_bd), egui::StrokeKind::Inside);
        for (k, (label, val)) in [("Category folders", true), ("One folder", false)].into_iter().enumerate() {
            let r = Rect::from_min_size(Pos2::new(seg.min.x + 3.0 + k as f32 * 117.0, seg.min.y + 3.0), Vec2::new(if k == 0 { 114.0 } else { 91.0 }, 24.0));
            let resp = ui.interact(r, egui::Id::new(("links-seg", k)), Sense::click());
            let on = self.by_category == val;
            if on {
                p.rect_filled(r, 4.0, t.primary);
            } else if resp.hovered() {
                p.rect_filled(r, 4.0, t.surface_lt);
            }
            p.text(r.center(), Align2::CENTER_CENTER, label, reg(11.5), if on { Color32::WHITE } else { t.dim });
            let resp = if val {
                resp.on_hover_text("Every file into its category folder (Settings → Folders)")
            } else {
                resp.on_hover_text("All checked files into the folder on the right")
            };
            if resp.clicked() {
                self.by_category = val;
            }
        }
        // The folder field takes what the buttons leave, so the two never meet.
        let path_w = (btns_left - 8.0 - (seg.max.x + 8.0)).clamp(120.0, 330.0);
        let path = Rect::from_min_size(Pos2::new(seg.max.x + 8.0, row_y - 15.0), Vec2::new(path_w, 30.0));
        p.rect(path, 6.0, t.bg, Stroke::new(1.0_f32, t.card_bd), egui::StrokeKind::Inside);
        let browse = Rect::from_min_size(Pos2::new(path.max.x - 70.0, path.min.y + 4.0), Vec2::new(66.0, 22.0));
        if self.by_category {
            // What "category folders" means, spelled out.
            let shown = format!("{}\\<type>", self.prefs.save_dir.trim_end_matches(['\\', '/']));
            let s = crate::app::truncate_to_width(&p, &shown, reg(11.0), t.dim, path.width() - 20.0);
            p.text(Pos2::new(path.min.x + 9.0, path.center().y), Align2::LEFT_CENTER, s, reg(11.0), t.dim.gamma_multiply(0.7));
        } else {
            ui.put(
                Rect::from_min_max(Pos2::new(path.min.x + 6.0, path.min.y + 4.0), Pos2::new(browse.min.x - 4.0, path.max.y - 4.0)),
                egui::TextEdit::singleline(&mut self.one_dir).frame(false).font(reg(11.0)).vertical_align(egui::Align::Center),
            );
            let resp = ui.interact(browse, egui::Id::new("links-browse"), Sense::click());
            p.rect(browse, 4.0, if resp.hovered() { t.surface_lt } else { t.surface }, Stroke::new(1.0_f32, t.card_bd), egui::StrokeKind::Inside);
            p.text(Pos2::new(browse.min.x + 8.0, browse.center().y), Align2::LEFT_CENTER, MI_FOLDER, icon(12.0), t.dim);
            p.text(Pos2::new(browse.min.x + 24.0, browse.center().y), Align2::LEFT_CENTER, "Browse", reg(10.5), t.txt);
            if resp.clicked() {
                if let Some(d) = rfd::FileDialog::new().set_directory(&self.one_dir).pick_folder() {
                    self.one_dir = d.to_string_lossy().to_string();
                }
            }
        }
        // Actions, right-aligned on the bottom line.
        let mut right = area.max.x;
        for (k, (id, label, primary, enabled, tip)) in buttons.into_iter().enumerate() {
            let glyph = match id { "dl" => Some(MI_DOWNLOAD), "queue" => Some(MI_QUEUE), _ => None };
            let w = btn_w[k];
            let r = Rect::from_min_size(Pos2::new(right - w, area.max.y - 34.0), Vec2::new(w, 32.0));
            right = r.min.x - 8.0;
            let resp = ui.interact(r, egui::Id::new(("links-btn", id)), if enabled { Sense::click() } else { Sense::hover() });
            let (fill, bd, fg) = if primary {
                (if resp.hovered() && enabled { crate::dm::DM_ACC.gamma_multiply(0.85) } else { t.primary }, t.primary, Color32::WHITE)
            } else {
                (if resp.hovered() && enabled { t.surface_lt } else { t.surface }, t.card_bd, t.txt)
            };
            let a = if enabled { 1.0 } else { 0.45 };
            p.rect(r, 6.0, fill.gamma_multiply(a), Stroke::new(1.0_f32, bd.gamma_multiply(a)), egui::StrokeKind::Inside);
            let mut x = r.min.x + 16.0;
            if let Some(g) = glyph {
                p.text(Pos2::new(x + 7.0, r.center().y), Align2::CENTER_CENTER, g, icon(15.0), fg.gamma_multiply(a));
                x += 20.0;
            }
            p.text(Pos2::new(x, r.center().y), Align2::LEFT_CENTER, label, semi(12.0), fg.gamma_multiply(a));
            if resp.on_hover_text(tip).clicked() {
                match id {
                    "dl" => self.commit(true),
                    "queue" => self.commit(false),
                    _ => self.done = true,
                }
            }
        }
    }

    // ---------------------------------------------------------- row menu --

    /// Where the row menu sits: at the pointer, kept inside the screen.
    fn menu_rect(&self, ctx: &egui::Context) -> Rect {
        let Some(menu) = self.menu else { return Rect::ZERO };
        let scr = ctx.screen_rect();
        let pos = Pos2::new(
            menu.pos.x.min(scr.max.x - MENU_W - 8.0).max(8.0),
            menu.pos.y.min(scr.max.y - MENU_H - 8.0).max(8.0),
        );
        Rect::from_min_size(pos, Vec2::new(MENU_W, MENU_H))
    }

    /// The row menu: "Copy URL" and "Download".
    ///
    /// It is drawn with the app's own tokens, like the manager's context menu,
    /// so it does not look like a borrowed egui menu.
    fn row_menu(&mut self, ctx: &egui::Context) {
        let Some(menu) = self.menu else { return };
        let t = pl();
        let rect = self.menu_rect(ctx);
        let row = menu.row;
        egui::Area::new(egui::Id::new("links-row-menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(t.modal_bg)
                    .stroke(Stroke::new(1.0_f32, t.modal_bd))
                    .corner_radius(crate::app::RADIUS)
                    .inner_margin(egui::Margin::same(6))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for (k, (label, glyph)) in MENU_ITEMS.iter().enumerate() {
                            let (r, _) = ui.allocate_exact_size(Vec2::new(MENU_W - 12.0, MENU_ITEM_H), Sense::hover());
                            let resp = ui.interact(r, egui::Id::new(("links-menu-item", k)), Sense::click());
                            if resp.hovered() {
                                ui.painter().rect_filled(r, 4.0, t.surface_lt);
                            }
                            ui.painter().text(Pos2::new(r.min.x + 10.0, r.center().y), Align2::LEFT_CENTER, *glyph, icon(14.0), t.dim);
                            ui.painter().text(Pos2::new(r.min.x + 32.0, r.center().y), Align2::LEFT_CENTER, *label, reg(12.5), t.txt);
                            if resp.clicked() {
                                self.menu = None;
                                match k {
                                    0 => self.copy_url(row),
                                    _ => self.download_row(row),
                                }
                            }
                        }
                    });
            });
    }
}

/// The entries of the row menu, in the order they are shown.
const MENU_ITEMS: [(&str, &str); 2] = [("Copy URL", crate::app::MI_COPY), ("Download", MI_DOWNLOAD)];
const MENU_W: f32 = 168.0;
const MENU_ITEM_H: f32 = 28.0;
const MENU_H: f32 = 2.0 * MENU_ITEM_H + 12.0;

/// The row menu closes on Escape, and on a click outside it.
fn menu_dismissed(escape: bool, click: Option<Pos2>, menu: Rect) -> bool {
    escape || click.map_or(false, |p| !menu.contains(p))
}

/// Checkbox in the mock-up's style: accent fill with a tick, a bar when only
/// some rows are checked.
fn paint_check(p: &egui::Painter, r: Rect, on: bool, mixed: bool, hover: bool) {
    let t = pl();
    if on || mixed {
        p.rect_filled(r, 4.0, t.primary);
        if on {
            p.text(r.center(), Align2::CENTER_CENTER, MI_CHECK, icon(13.0), Color32::WHITE);
        } else {
            p.rect_filled(Rect::from_center_size(r.center(), Vec2::new(7.0, 2.0)), 1.0, Color32::WHITE);
        }
    } else {
        p.rect(r, 4.0, t.bg, Stroke::new(1.5_f32, if hover { t.primary } else { t.card_bd }), egui::StrokeKind::Inside);
    }
}

const CB_W: f32 = 40.0;
const HDR_H: f32 = 32.0;
const ROW_H: f32 = 40.0;
/// Column titles and what clicking them sorts by. "Download from" (last)
/// takes whatever room the others leave.
const COLS: [(&str, SortBy); 4] = [
    ("Name", SortBy::Name),
    ("Size", SortBy::Size),
    ("Category", SortBy::Category),
    ("Download from", SortBy::From),
];
const COL_MIN: f32 = 60.0;
/// Stored widths of the three sized columns (name, size, category).
const COLS_DEFAULT: [f32; 3] = [340.0, 96.0, 130.0];
/// Where the widths live in ui.json. The key is new with the new columns: an
/// older ui.json holds the widths of the old ones (name, type, size) under
/// "links_cols3", and those must not be read as the widths of name, size and
/// category.
const COLS_KEY: &str = "links_cols4";

/// All four widths for a table `avail` wide: the stored three, and the rest
/// for "Download from" (never under 140).
fn col_widths(stored: &[f32; 3], avail: f32) -> [f32; 4] {
    let [n, s, c] = *stored;
    let flex = (avail - (n + s + c)).max(140.0);
    [n, s, c, flex]
}

/// Drag the divider after column `i` by `dx`: it follows the pointer, so the
/// column on its left grows by what the one on its right loses; the flexible
/// "Download from" column absorbs the change when it is one of the two.
fn drag_divider(stored: &[f32; 3], i: usize, dx: f32) -> [f32; 3] {
    let mut out = *stored;
    if i + 1 >= 3 {
        // Category | Download from: Category grows, the flexible column gives way.
        out[i] = (out[i] + dx).clamp(COL_MIN, 900.0);
        return out;
    }
    let dx = dx.clamp(COL_MIN - out[i], out[i + 1] - COL_MIN);
    out[i] += dx;
    out[i + 1] -= dx;
    out
}

/// The widths a ui.json holds for the sized columns, or the defaults when it
/// holds none (an older file, or one written before the new columns).
fn cols_from_json(v: &serde_json::Value) -> [f32; 3] {
    let mut out = COLS_DEFAULT;
    if let Some(a) = v.get(COLS_KEY).and_then(|x| x.as_array()) {
        for (i, w) in a.iter().take(3).enumerate() {
            if let Some(w) = w.as_f64().filter(|w| w.is_finite()) {
                out[i] = (w as f32).clamp(COL_MIN, 900.0);
            }
        }
    }
    out
}

/// Column widths kept in ui.json (`links_cols4`), so the window reopens as left.
fn load_cols() -> [f32; 3] {
    let v: serde_json::Value = config_dir()
        .and_then(|d| std::fs::read_to_string(d.join("ui.json")).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null);
    cols_from_json(&v)
}

fn save_cols(cols: &[f32; 3]) {
    let Some(path) = config_dir().map(|d| d.join("ui.json")) else { return };
    let mut v: serde_json::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if let Some(obj) = v.as_object_mut() {
        obj.insert(COLS_KEY.into(), serde_json::json!(cols.iter().map(|w| w.round()).collect::<Vec<_>>()));
        if let Ok(json) = serde_json::to_string_pretty(&v) {
            let _ = crate::model::atomic_write(&path, json.as_bytes());
        }
    }
}

/// A name for a link whose URL names no file (a folder listing, a page):
/// its link text, else the last part of the URL, as an .html page.
fn page_name(url: &str, text: &str) -> String {
    let from_url = || {
        let u = reqwest::Url::parse(url).ok()?;
        let tail = u
            .query_pairs()
            .last()
            .map(|(_, v)| v.into_owned())
            .or_else(|| u.path_segments().and_then(|mut s| s.next_back().map(|x| x.to_string())))
            .filter(|t| !t.trim().is_empty())
            .or_else(|| u.host_str().map(|h| h.to_string()))?;
        Some(tail.rsplit('/').find(|p| !p.trim().is_empty()).unwrap_or(&tail).to_string())
    };
    let t = text.trim();
    let base = if !t.is_empty() && t.chars().count() <= 80 && !t.contains('{') {
        t.to_string()
    } else {
        from_url().unwrap_or_else(|| "page".into())
    };
    engine::sanitize_filename(&format!("{base}.html"))
}

/// Entry point for `--links <file>`.
pub fn run() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let path = args
        .iter()
        .position(|a| a == "--links")
        .and_then(|i| args.get(i + 1))
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    let batch = match read_batch(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("--links: {e}");
            std::process::exit(2);
        }
    };
    let prefs = crate::confirm::load_prefs();
    // Take the next slot of the cascade before the window is built: the
    // position depends on it.
    let slot = crate::window::take_cascade_slot();
    let rows = batch
        .links
        .into_iter()
        .map(|l| {
            let mut name = engine::sanitize_filename(&filename_from_url(&l.url));
            if crate::model::is_fallback_filename(&name) {
                name = page_name(&l.url, &l.text);
            }
            let (_, html, image) = type_of(&name, None);
            let kind = Kind::of(&name, html, image);
            // Files start checked; pages and links to other sites do not.
            let checked = kind != Kind::Page && kind != Kind::Other && kind != Kind::Image;
            Row { url: l.url, text: l.text, name, guessed_name: true, kind, size: None, checked, probe: Probe::Pending }
        })
        .collect();
    let rt = std::sync::Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("tokio runtime"),
    );
    let mut app = LinksApp {
        page_url: batch.page_url,
        title: batch.title,
        // The cookie came with the process, not through the file on disk.
        cookie: std::env::var(crate::native_host::LINKS_COOKIE_ENV).unwrap_or_default(),
        rows,
        filter: String::new(),
        chip: None,
        hide_pages: true,
        sort: None,
        anchor: None,
        by_category: true,
        one_dir: prefs.save_dir.clone(),
        cols: load_cols(),
        menu: None,
        prefs,
        probe_rx: None,
        probes_left: 0,
        _rt: rt,
        notice: String::new(),
        done: false,
        exited: false,
        slot,
        hwnd: None,
    };
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Download all links — Download Craft")
        .with_inner_size([LINKS_W, LINKS_H])
        .with_min_inner_size([840.0, 420.0])
        .with_resizable(true)
        // Normal Z-order: this is an ordinary window, not a layer that
        // floats above everything else. It takes a place in the cascade
        // with the other open dialogs instead.
        .with_window_level(egui::WindowLevel::Normal);
    // Take the next slot of the cascade, on the monitor the pointer is on.
    let (cx, cy) = crate::window::cascade_position((LINKS_W, LINKS_H), slot.index);
    viewport = viewport.with_position(egui::pos2(cx, cy));
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")) {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }
    eframe::run_native(
        "Download all links",
        eframe::NativeOptions { viewport, ..Default::default() },
        Box::new(|cc| {
            crate::app::setup_fonts(cc);
            crate::app::apply_theme(&cc.egui_ctx, &crate::app::stored_theme());
            app.start_probes(cc.egui_ctx.clone());
            Ok(Box::new(app))
        }),
    )?;
    // The window is gone: free its place in the cascade.
    crate::window::release_cascade_slot();
    Ok(())
}

const LINKS_W: f32 = 1000.0;
const LINKS_H: f32 = 640.0;

impl eframe::App for LinksApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.done {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            if !self.exited {
                self.exited = true;
                // Let wake_ids launch the main window before this exits.
                std::thread::spawn(|| {
                    std::thread::sleep(std::time::Duration::from_millis(600));
                    std::process::exit(0);
                });
            }
            return;
        }
        // The window must not take the keyboard away from the user: only
        // the first dialog of a cascade may take focus on appearance. The
        // others wait until the user clicks them.
        match self.hwnd {
            None => {
                if let Some(h) = crate::window::own_window_handle("Download all links") {
                    self.hwnd = Some(h);
                    if !self.slot.may_activate() {
                        crate::window::suppress_activation(h);
                    }
                }
            }
            Some(h) => {
                if crate::window::activation_suppressed(h)
                    && ctx.input(|i| i.pointer.any_pressed())
                    && crate::window::pointer_inside(h)
                {
                    crate::window::allow_activation(h);
                }
            }
        }
        self.collect_probes();
        let frame = egui::Frame::new().fill(pl().bg).inner_margin(egui::Margin { left: 16, right: 16, top: 12, bottom: 12 });
        egui::TopBottomPanel::top("links_top").frame(frame).show_separator_line(false).show(ctx, |ui| {
            self.header(ui);
            ui.add_space(10.0);
            self.toolbar(ui);
        });
        egui::TopBottomPanel::bottom("links_bottom").frame(frame).show_separator_line(false).show(ctx, |ui| {
            self.footer(ui);
        });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(pl().bg).inner_margin(egui::Margin { left: 16, right: 16, top: 0, bottom: 0 }))
            .show(ctx, |ui| self.table(ui));
        // The row menu is drawn last, over everything: it closes on Escape
        // and on a click outside it.
        if self.menu.is_some() {
            let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
            let click = if ctx.input(|i| i.pointer.primary_pressed()) {
                ctx.input(|i| i.pointer.latest_pos())
            } else {
                None
            };
            if menu_dismissed(escape, click, self.menu_rect(ctx)) {
                self.menu = None;
            }
        }
        self.row_menu(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_come_from_content_type_then_name() {
        assert_eq!(type_of("A.Knight.S01E01.mp4", None), ("MP4 video".to_string(), false, false));
        assert_eq!(type_of("index.php", None).1, true);
        assert_eq!(type_of("S01", None).1, true, "no extension, not probed: a directory page");
        assert_eq!(type_of("S01", Some("text/html; charset=utf-8")).0, "Web page");
        assert!(type_of("logo.png", None).2);
        assert!(type_of("photo", Some("image/jpeg")).2);
        assert_eq!(type_of("file.bin", Some("application/octet-stream")).0, "BIN file");
        assert_eq!(type_of("watch", Some("video/mp4")).0, "video/mp4");
        // Kinds for the chips, icons and colours.
        assert_eq!(Kind::of("a.mp4", false, false), Kind::Video);
        assert_eq!(Kind::of("a.mp3", false, false), Kind::Audio);
        assert_eq!(Kind::of("a.zip", false, false), Kind::Zip);
        assert_eq!(Kind::of("S01.html", true, false), Kind::Page);
        assert_eq!(Kind::of("x", false, true), Kind::Image);
    }

    #[test]
    fn dividers_follow_the_pointer_and_name_pages() {
        let c = COLS_DEFAULT;
        // Name | Size: both sized, the sum kept.
        let d = drag_divider(&c, 0, 20.0);
        assert_eq!((d[0], d[1]), (c[0] + 20.0, c[1] - 20.0));
        // A wider drag stops at the minimum of the column that gives way.
        let d = drag_divider(&c, 0, 40.0);
        assert_eq!(d[0] + d[1], c[0] + c[1]);
        assert_eq!(d[1], COL_MIN);
        // Size | Category: the one on the left grows, the one on the right gives way.
        assert_eq!(drag_divider(&c, 1, 20.0), [c[0], c[1] + 20.0, c[2] - 20.0]);
        // Category | Download from: Category grows, the flexible column gives way.
        assert_eq!(drag_divider(&c, 2, 30.0)[2], c[2] + 30.0);
        // Minimums hold.
        assert_eq!(drag_divider(&c, 0, 900.0)[1], COL_MIN);
        let w = col_widths(&c, 1000.0);
        assert_eq!(w.iter().sum::<f32>(), 1000.0);
        assert_eq!(col_widths(&c, 100.0)[3], 140.0);
        // Folder links name a page, not "download_<time>.bin".
        assert_eq!(page_name("https://d.example/?dir=Series/English/A", "A"), "A.html");
        assert_eq!(page_name("https://d.example/?dir=Series/English/A", ""), "A.html");
        assert_eq!(page_name("https://d.example/", ".cls-1{fill:url(#g);}"), "d.example.html");
        assert!(type_of(&page_name("https://d.example/?dir=S01", "S01"), None).1);
    }

    #[test]
    fn the_audio_category_is_called_audio() {
        // The app calls the audio category "Audio"; an older build says
        // "Music", and that still has to land on the audio kind.
        assert_eq!(Kind::of_category("Audio"), Kind::Audio);
        assert_eq!(Kind::of_category("Music"), Kind::Audio);
        assert_eq!(Kind::of_category("Video"), Kind::Video);
        assert_eq!(Kind::of_category("Image"), Kind::Image);
        assert_eq!(Kind::of_category("Document"), Kind::Doc);
        assert_eq!(Kind::of_category("Compressed"), Kind::Zip);
        assert_eq!(Kind::of_category("Apps"), Kind::App);
        assert_eq!(Kind::of_category("Other"), Kind::Other);
        // A file of the category is of that kind.
        assert_eq!(Kind::of("a.mp3", false, false), Kind::Audio);
    }

    #[test]
    fn the_columns_are_name_size_category_and_download_from() {
        assert_eq!(COLS.map(|c| c.0), ["Name", "Size", "Category", "Download from"]);
        assert_eq!(COLS.map(|c| c.1), [SortBy::Name, SortBy::Size, SortBy::Category, SortBy::From]);
        // "Download from" takes the rest and never goes under 140.
        let w = col_widths(&COLS_DEFAULT, 1000.0);
        assert_eq!(w[0], COLS_DEFAULT[0]);
        assert_eq!(w[1], COLS_DEFAULT[1]);
        assert_eq!(w[2], COLS_DEFAULT[2]);
        assert_eq!(w[3], 1000.0 - COLS_DEFAULT.iter().sum::<f32>());
        assert!(w[3] >= 140.0);
    }

    #[test]
    fn the_category_sorts_in_the_order_the_chips_are_in() {
        // The chips run in the order of Kind::ALL, and so does the column.
        assert_eq!(Kind::Video.rank(), 0);
        assert_eq!(Kind::Audio.rank(), 1);
        assert_eq!(Kind::Other.rank(), Kind::ALL.len() - 1);
        assert!(Kind::ALL.windows(2).all(|w| w[0].rank() < w[1].rank()));
        // The category names the column shows.
        assert_eq!(Kind::Video.labels().1, "Video");
        assert_eq!(Kind::Audio.labels().1, "Audio");
        assert_eq!(Kind::Image.labels().1, "Image");
        assert_eq!(Kind::Doc.labels().1, "Document");
        assert_eq!(Kind::Zip.labels().1, "Archive");
        assert_eq!(Kind::App.labels().1, "Program");
        assert_eq!(Kind::Page.labels().1, "Page");
        assert_eq!(Kind::Other.labels().1, "Other");
    }

    #[test]
    fn the_extension_stays_with_the_name() {
        // The old "Type" column is gone, so the name carries its extension.
        assert_eq!(split_ext("A.Knight.S01E01.mp4"), ("A.Knight.S01E01", "mp4"));
        assert_eq!(split_ext("track.mp3"), ("track", "mp3"));
        assert_eq!(split_ext("song.m4a"), ("song", "m4a"));
        // A name that only ends in a full stop keeps all of it.
        assert_eq!(split_ext("A.Knight.of.the.Seven.Kingdoms"), ("A.Knight.of.the.Seven.Kingdoms", ""));
        assert_eq!(split_ext("noext"), ("noext", ""));
        assert_eq!(split_ext(".mp3"), (".mp3", ""));
    }

    #[test]
    fn old_column_widths_are_not_used_for_the_new_columns() {
        // An older ui.json holds the widths of name, type and size under
        // "links_cols3": they must not be read as name, size and category.
        let old = serde_json::json!({ "links_cols3": [340.0, 130.0, 96.0] });
        assert_eq!(cols_from_json(&old), COLS_DEFAULT);
        // The new key is read.
        let new = serde_json::json!({ "links_cols4": [300.0, 90.0, 120.0] });
        assert_eq!(cols_from_json(&new), [300.0, 90.0, 120.0]);
        // Out-of-range widths are clamped, as they always were.
        let wild = serde_json::json!({ "links_cols4": [10.0, 5000.0, 120.0] });
        assert_eq!(cols_from_json(&wild), [COL_MIN, 900.0, 120.0]);
        // A file without them falls back to the defaults.
        assert_eq!(cols_from_json(&serde_json::json!({})), COLS_DEFAULT);
    }

    #[test]
    fn the_row_menu_closes_on_escape_and_on_a_click_outside_it() {
        let menu = Rect::from_min_size(Pos2::new(100.0, 100.0), Vec2::new(MENU_W, MENU_H));
        assert!(menu_dismissed(true, Some(menu.center()), menu), "Escape closes it");
        assert!(menu_dismissed(false, Some(Pos2::new(10.0, 10.0)), menu), "a click outside closes it");
        assert!(!menu_dismissed(false, Some(menu.center()), menu), "a click on it keeps it open");
        assert!(!menu_dismissed(false, None, menu), "nothing pressed keeps it open");
        // It offers exactly the two entries.
        assert_eq!(MENU_ITEMS.len(), 2);
        assert_eq!(MENU_ITEMS[0].0, "Copy URL");
        assert_eq!(MENU_ITEMS[1].0, "Download");
    }

    #[test]
    fn host_and_folder_for_the_from_column() {
        assert_eq!(
            host_and_folder("https://download.sunplex.net/Series/English/A/A.Knight.of.the.Seven.Kingdoms/S01/E01.mp4"),
            ("download.sunplex.net".to_string(), "S01".to_string())
        );
        assert_eq!(host_and_folder("https://sunplex.net/"), ("sunplex.net".to_string(), String::new()));
        assert_eq!(host_and_folder("https://h.example/My%20Show/x.mp4").1, "My Show");
    }

    #[test]
    fn page_cookies_stay_on_the_page_host() {
        assert!(same_host("https://download.example.net/a.mp4", "https://download.example.net/?dir=x"));
        assert!(!same_host("https://twitter.com/x", "https://download.example.net/"));
        assert!(!same_host("not a url", "https://download.example.net/"));
    }

    #[test]
    fn links_file_must_come_from_the_config_dir() {
        assert!(read_batch(std::path::Path::new("C:/Windows/links_1.json")).is_err());
        if let Some(dir) = config_dir() {
            assert!(read_batch(&dir.join("queue.json")).is_err());
        }
    }
}
