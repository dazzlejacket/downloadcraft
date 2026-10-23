// Background worker: every browser download is captured
// automatically, cancelled in-browser, and handed to the native host.
// No clicks, no pasting — zero config.

const HOST = "com.download_craft";
const GRACE_MS = 15_000; // ignore our own fallback re-downloads
const seen = new Map(); // url -> timestamp (loop guard)
let hostOk = true;
let lastHostError = "";
let lastFallbackError = "";
// Must match manifest version + content.js EXT_VER. Tabs running an older
// content script are rejected with a "reload the tab" error instead of
// failing mysteriously (their extension APIs are gone after a reload).
const EXT_VER = "0.9.7.3";

// Promise-style calls (the `await api.*` ones below) go through this handle so
// one file serves both engines: Firefox exposes the promise-returning
// `browser` namespace, while recent Chrome versions expose it as well. Use
// Firefox's runtime.getBrowserInfo API (not the namespace name) when enabling
// Firefox-only blocking webRequest behavior.
const api = (typeof browser !== "undefined" && browser.runtime) ? browser : chrome;

// Shared ranking (P4): playlist/segment/file kinds, scores, duplicate
// collapse. Service workers support importScripts; if it ever fails the
// guarded fallbacks below keep the old behavior instead of breaking.
try {
  if (typeof importScripts === "function") importScripts("rank.js");
} catch (e) { /* RANK stays null -> legacy paths */ }

function markSeen(url, filename) {
  seen.set(url + "|" + (filename || ""), Date.now());
  if (seen.size > 200) {
    const cut = Date.now() - GRACE_MS;
    for (const [u, t] of seen) if (t < cut) seen.delete(u);
  }
}
function freshSeen(url, filename) {
  const t = seen.get(url + "|" + (filename || "")) ?? seen.get(url + "|");
  return t !== undefined && Date.now() - t < GRACE_MS;
}

async function settings() {
  const s = await api.storage.local.get({
    autoCapture: true,
    hideBrowserUI: false,
    minSizeKB: 100,
    blockHosts: "",
  });
  return s;
}

// Chromium's UI switch affects the whole profile, so it is opt-in and
// explained in the popup. Release it when capture is disabled or unavailable.
async function syncBrowserDownloadUI() {
  if (!api.downloads || typeof api.downloads.setUiOptions !== "function") return {supported:false};
  const cfg = await settings();
  const hidden = typeof DCRank !== "undefined" && DCRank.hideDownloadUI(cfg, hostOk);
  try {
    await api.downloads.setUiOptions({enabled:!hidden});
    return {supported:true, hidden};
  } catch (e) { return {supported:true, error:(e && e.message) || String(e)}; }
}

function hostBlocked(url, blockHosts) {
  try {
    const h = new URL(url).hostname.toLowerCase();
    const blocked = String(blockHosts || "")
      .split(/[\s,;]+/)
      .map((x) => x.trim().toLowerCase())
      .filter(Boolean);
    return blocked.some((b) => h === b || h.endsWith("." + b));
  } catch (e) {
    return false;
  }
}

// Curated cookies for ONE url only (same-host). Never logged. Whole
// name=value pairs only — a byte cut could split a pair and corrupt auth.
async function cookiesFor(url) {
  try {
    if (!chrome.cookies || !chrome.cookies.getAll) return null;
    const list = await api.cookies.getAll({ url });
    if (!list || !list.length) return null;
    const pairs = list
      .filter((c) => c && c.name)
      .slice(0, 40)
      .map((c) => `${c.name}=${c.value || ""}`);
    let s = "";
    for (const p of pairs) {
      const add = (s ? "; " : "") + p;
      if ((s + add).length > 6000) break;
      s += add;
    }
    return s || null;
  } catch (e) {
    return null;
  }
}

function stripFbSlice(url) {
  try {
    const u = new URL(url);
    const h = u.hostname.toLowerCase();
    const social = h.includes("fbcdn") || h.includes("scontent") || h.includes("facebook")
      || h.includes("fbsbx") || h.includes("lookaside") || h.includes("cdninstagram") || h.includes("instagram");
    if (!social) return url;
    if (!u.searchParams.has("bytestart") && !u.searchParams.has("byteend")) return url;
    u.searchParams.delete("bytestart");
    u.searchParams.delete("byteend");
    return u.toString();
  } catch (e) { return url; }
}

async function sendToHost(url, filename, extra) {
  try {
    const capturesFor = target => {
      const matches=[];
      for(const [tabId,items] of tabMedia)for(const item of items){
        if(item.url!==target)continue;
        if(extra && extra._tabId!==undefined && tabId!==extra._tabId)continue;
        if(extra && extra._frameId!==undefined && item.frameId!==extra._frameId)continue;
        if(extra && extra._documentId && item.documentId && item.documentId!==extra._documentId)continue;
        matches.push({item,items,tabId});
      }
      return matches;
    };
    url = stripFbSlice(url);
    if (typeof DCRank !== "undefined" && DCRank.isVimeoAdaptive(url)) {
      // The captured document is the exact iframe which requested this
      // asset, unlike the popup's top-level page (possibly several videos).
      let capture = null;
      let captureTab = null;
      const matches=capturesFor(url);
      if(matches.length===1){capture=matches[0].item;captureTab=matches[0].tabId;}
      let framePage = capture && capture.pageUrl;
      // Chromium omits documentUrl on webRequest events. Ask the content
      // script in the requesting frame instead; do not guess from a feed.
      if (!DCRank.vimeoVideoPage(framePage) && capture && capture.frameId >= 0) {
        try {
          const reply = await api.tabs.sendMessage(captureTab,
            { type: "vimeoFramePage" }, { frameId: capture.frameId });
          framePage = reply && reply.pageUrl;
        } catch (e) { /* frame may have navigated or been removed */ }
      }
      const player = DCRank.vimeoVideoPage(framePage)
        || DCRank.vimeoVideoPage(extra && extra.pageUrl)
        || DCRank.vimeoVideoPage(extra && extra.referer);
      if (!player && !DCRank.isPlaylistUrl(url)) throw new Error("Vimeo playback fragment: reload the video page and capture its full stream or Vimeo player link.");
      // A full manifest can be resolved by yt-dlp without a player URL.
      // Never feed it to the built-in video-only HLS concatenator.
      url = player || url;
      // Fragment byte counts/pairs do not describe the resolved full video.
      extra = { ...extra, size: null, pairUrl: null, pairMime: null,
        cookie: null, mime: null };
    }
    const observedPart = capturesFor(url).some(hit=>hit.item.segment);
    if (typeof DCRank !== "undefined" && (DCRank.isSegmentUrl(url) || observedPart) && !DCRank.isPlaylistUrl(url)) {
      let capture = null, items = [], tab = null;
      const matches=capturesFor(url);
      if(matches.length===1){capture=matches[0].item;items=matches[0].items;tab=matches[0].tabId;}
      let frameInfo = null;
      if (capture && capture.frameId >= 0) {
        try { frameInfo = await api.tabs.sendMessage(tab,
          {type:"mediaFrameInfo"}, {frameId:capture.frameId}); } catch (e) {}
      }
      const whole = capture && DCRank.completePlaylist(capture, items);
      if (whole) {
        url = whole.url;
      } else if (frameInfo && frameInfo.videoCount === 1 && /^https?:\/\//i.test(frameInfo.pageUrl || "")) {
        // Keep the fragment URL as evidence. The engine resolves its exact
        // single-player frame page with yt-dlp; it never saves the fragment.
        extra = {...extra, referer:frameInfo.pageUrl, pageUrl:frameInfo.pageUrl};
      } else {
        throw new Error("This capture is a playback fragment. Reload the video and capture its full playlist or a page containing one player.");
      }
      extra = {...extra, size:null, pairUrl:null, pairMime:null,
        dur:(frameInfo && frameInfo.durationSecs) || (extra && extra.dur) || null,
        audioExpected:!!((frameInfo && frameInfo.audioExpected) || (extra && extra.audioExpected))};
    }
    if (typeof DCRank !== "undefined") {
      const matches=capturesFor(url);
      if(matches.length===1 && DCRank.kindOf(matches[0].item)==="playlist") {
        const root = DCRank.completePlaylist(matches[0].item, matches[0].items);
        if (root && root.url !== url) {
          url = root.url;
          extra = {...extra, size:null, pairUrl:null, pairMime:null};
        }
      }
    }
    let cookie = (extra && extra.cookie) || null;
    if (!cookie && /^https?:\/\//i.test(url)) {
      cookie = await cookiesFor(url); // best-effort, same-host only
      // CDN hosts (fbcdn/lookaside) carry no cookies of their own — fall
      // back to the page's session cookies so expiring signed URLs keep
      // working without a page reload.
      const pageUrl = extra && (extra.pageUrl || extra.referer);
      const vimeoTarget = typeof DCRank !== "undefined"
        && (DCRank.isVimeoAdaptive(url) || DCRank.vimeoVideoPage(url));
      if (!cookie && pageUrl && /^https?:\/\//i.test(pageUrl)
          && (!vimeoTarget || DCRank.vimeoVideoPage(pageUrl))) {
        try {
          const uh = new URL(url).hostname.toLowerCase();
          const ph = new URL(pageUrl).hostname.toLowerCase();
          const socialPage = /(?:^|\.)(?:facebook\.com|instagram\.com)$/.test(ph);
          const socialTarget = /(?:^|\.)(?:fbcdn\.net|facebook\.com|instagram\.com|cdninstagram\.com|fbsbx\.com)$/.test(uh)
            || uh.includes("scontent") || uh.includes("lookaside");
          const social = socialPage && socialTarget;
          // Cross-host session cookies are limited to known platform delivery hosts.
          const youtubeBridge = /(^|\.)youtube\.com$/.test(ph) && /(^|\.)googlevideo\.com$/.test(uh);
          if (uh !== ph && (social || youtubeBridge || (vimeoTarget && DCRank.vimeoVideoPage(pageUrl)))) cookie = await cookiesFor(pageUrl);
        } catch (e) { /* host parse failed: no cookies */ }
      }
    }
    const res = await api.runtime.sendNativeMessage(HOST, {
      type: "download",
      url,
      filename: filename || null,
      pageUrl: (extra && extra.pageUrl) || null,
      referer: (extra && extra.referer) || (extra && extra.pageUrl) || null,
      cookie,
      mime: (extra && extra.mime) || null,
      size: (extra && extra.size) || null,
      pairUrl: (extra && extra.pairUrl) || null,
      pairMime: (extra && extra.pairMime) || null,
      quality: (extra && extra.quality) || null,
      dur: (extra && extra.dur) || null,
      audioExpected: !!(extra && extra.audioExpected),
    });
    // A ping proves availability, not acceptance of this download. Never
    // discard the browser's request on an unrelated or rejected response.
    if (!res || res.type !== "ok" || res.queued === false) {
      lastHostError = (res && res.error) || "native host did not accept the download";
      return false;
    }
    lastHostError = "";
    return true;
  } catch (e) {
    // Console-visible: this is the first thing to look at when clicks seem to
    // do nothing (browser console: "No such native application", etc.).
    lastHostError = (e && e.message) || String(e);
    console.error("[download-craft] native host '" + HOST + "' failed:", lastHostError);
    return false;
  }
}

// ---- IDM-style network sniffer ----
// Watches response headers for media payloads (direct mp4/webm/mkv, audio,
// HLS playlists, DASH manifests). Candidates are stored per-tab so the
// popup + content overlay can offer them even when the DOM only holds a
// blob: URL (un-downloadable directly). Clean-room behavior, no IDM code.
const tabSessions = new Map(); // object identity rejects async work from an old navigation
let activePlaylistProbes = 0;
const tabMedia = new Map(); // tabId -> [{url, mime, size, pageUrl, seenAt}]
const tabPlaylist = new Map(); // tabId -> playlist url (P4: demotes loose segments)
const seenTabs = new Map(); // canonical url -> Set(tabId), for the "×N tabs" hint
const sizeCache = new Map(); // url -> bytes (quality-menu sizes)

function rankKind(url, mime) {
  if (typeof DCRank !== "undefined") return DCRank.kindOf({url, mime});
  if (typeof DCRank !== "undefined" && DCRank.isPlaylistUrl(url)) return "playlist";
  if (typeof DCRank !== "undefined" && DCRank.isSegmentUrl(url)) return "segment";
  // Legacy fallback when rank.js failed to load (same shapes, inline).
  if (/(\.m3u8|\.mpd)(\?|#|$)/i.test(String(url).split("?")[0]) || /m3u8|mpegurl|dash/i.test(String(url))) return "playlist";
  if (/(\.ts|\.m4s|\.cmfv|\.cmfa)(\?|#|$)/i.test(String(url).split("?")[0])) return "segment";
  return "file";
}

function canonKey(url) {
  if (typeof DCRank !== "undefined") return DCRank.canonicalUrl(url);
  return String(url || "").split("#")[0];
}

function noteMedia(tabId, entry) {
  if (tabId === undefined || tabId < 0) return;
  if (!entry || !entry.url || !/^https?:\/\//i.test(entry.url)) return;
  entry.url = stripFbSlice(entry.url);
  if (entry.url.length > 4000) return;
  const kind = rankKind(entry.url, entry.mime);
  // P4: a playlist on the tab demotes loose segments — they are children
  // the engine fetches itself (ad slivers hide among them), so menus must
  // offer the whole, and the badge must not count parts as downloads.
  if (kind === "playlist") tabPlaylist.set(tabId, entry.url);
  let list = tabMedia.get(tabId);
  if (!list) { list = []; tabMedia.set(tabId, list); }
  if (kind === "segment" && list.some(m=>rankKind(m.url,m.mime)==="playlist" && DCRank.samePlayer(m,entry))) entry.child = true;
  const existing=list.find(e=>e.url===entry.url && e.frameId===entry.frameId
    && (!e.pageUrl || !entry.pageUrl || e.pageUrl===entry.pageUrl)
    && (!e.documentId || !entry.documentId || e.documentId===entry.documentId));
  if (existing) {
    Object.assign(existing,entry,{kind,seenAt:Date.now(),mime:entry.mime || existing.mime,
      size:entry.size || existing.size || 0, pageUrl:entry.pageUrl || existing.pageUrl,
      master:!!(entry.master || existing.master), children:entry.children || existing.children});
  } else list.push({ ...entry, kind, seenAt: Date.now() });
  // An observed playlist reference is stronger evidence than a filename:
  // some sites name every individual segment .mp4.
  if (typeof DCRank !== "undefined") {
    for (const item of list) {
      if (DCRank.kindOf(item) === "playlist") continue;
      if (list.some(parent => DCRank.kindOf(parent) === "playlist"
          && DCRank.samePlayer(parent, item) && (parent.children || []).includes(item.url))) {
        item.segment = true; item.kind = "segment"; item.child = true;
      }
    }
  }
  if (list.length > 25) {
    while (list.length > 25) {
      const part = list.findIndex(m => rankKind(m.url, m.mime) !== "playlist");
      const [d] = list.splice(part < 0 ? 0 : part, 1);
      pruneSeen(d.url, tabId);
    }
  }
  // Cross-tab duplicate collapse (P4): one canonical entry, tab count.
  const ck = canonKey(entry.url);
  let tabs = seenTabs.get(ck);
  if (!tabs) { tabs = new Set(); seenTabs.set(ck, tabs); }
  tabs.add(tabId);
  if (seenTabs.size > 500) {
    seenTabs.clear(); // bounded best-effort; rebuilt by live traffic
  }
  // Legacy global snapshot: fallback only (popup prefers per-tab peek).
  try {
    chrome.storage.local.set({ downloadCraftNet: list.slice(-25) });
  } catch (e) { /* ignore */ }
  // Badge counts downloadable candidates, never playlist children.
  const badgeN = list.filter((e) => !e.child).length;
  try {
    api.action.setBadgeText({ text: String(badgeN), tabId }).catch(() => {});
    api.action.setBadgeBackgroundColor({ color: "#1F6FEB", tabId }).catch(() => {});
  } catch (e) { /* ignore */ }
}

function pruneSeen(url, tabId) {
  try {
    const ck = canonKey(url);
    const tabs = seenTabs.get(ck);
    if (!tabs) return;
    tabs.delete(tabId);
    if (!tabs.size) seenTabs.delete(ck);
  } catch (e) { /* ignore */ }
}

function forgetTab(tabId) {
  tabSessions.delete(tabId);
  tabMedia.delete(tabId);
  tabPlaylist.delete(tabId);
  try {
    for (const [ck, tabs] of seenTabs) {
      tabs.delete(tabId);
      if (!tabs.size) seenTabs.delete(ck);
    }
  } catch (e) { /* ignore */ }
  try { api.action.setBadgeText({ text: "", tabId }).catch(() => {}); } catch (e) {}
}

// Optional playlist metadata has a small per-tab budget. The candidate is
// available immediately even when a signed URL cannot be fetched again.
async function readPlaylistMetadata(url) {
  const res = await fetch(url, {credentials:"include", redirect:"error", signal:AbortSignal.timeout(8000)});
  if (!res.ok || !res.body) return null;
  const reader = res.body.getReader();
  try {
    if (Number(res.headers.get("content-length")) > 1024 * 1024) return null;
    let body = "", size = 0;
    const decoder = new TextDecoder();
    while (true) {
      const {value, done} = await reader.read();
      if (done) break;
      size += value.byteLength;
      if (size > 1024 * 1024) return null;
      body += decoder.decode(value, {stream:true});
    }
    body += decoder.decode();
    if (!/^\s*#EXTM3U(?:\s|$)/.test(body) && !/<(?:\w+:)?MPD[\s>]/.test(body)) return null;
    return {master:/#EXT-X-STREAM-INF:|<(?:\w+:)?MPD[\s>]/.test(body),
      children:DCRank.playlistReferences(body, url)};
  } finally { await reader.cancel(); }
}
function playlistMetadata(session, details) {
  const now = Date.now(), key = JSON.stringify([details.frameId, details.url]);
  for (const [k, value] of session.probes) if (now-value.at >= 15000) session.probes.delete(k);
  if (session.probes.has(key)) return session.probes.get(key).promise;
  if (session.active >= 2 || activePlaylistProbes >= 4 || session.probes.size >= 32) return Promise.resolve(null);
  session.active++;
  activePlaylistProbes++;
  const promise = readPlaylistMetadata(details.url).catch(()=>null).finally(()=>{session.active--;activePlaylistProbes--;});
  session.probes.set(key, {at:now,promise});
  return promise;
}
async function observeMediaResponse(details) {
  try {
    if (details.tabId === undefined || details.tabId < 0 || typeof DCRank === "undefined"
        || !DCRank.mediaResponse(details, 0)) return;
    let session = tabSessions.get(details.tabId);
    if (!session) { session = {probes:new Map(),active:0}; tabSessions.set(details.tabId,session); }
    const cfg = await settings().catch(() => ({ blockHosts:"", minSizeKB:100 }));
    if (tabSessions.get(details.tabId) !== session || hostBlocked(details.url,cfg.blockHosts)) return;
    const candidate = DCRank.mediaResponse(details,cfg.minSizeKB);
    if (!candidate) return;
    const entry = {...candidate, pageUrl:details.documentUrl || null, frameId:details.frameId,
      documentId:details.documentId || null};
    noteMedia(details.tabId,entry);
    if (candidate.kind === "playlist") {
      const metadata = await playlistMetadata(session,details);
      if (metadata && tabSessions.get(details.tabId) === session) noteMedia(details.tabId,{...entry,...metadata});
    }
  } catch (e) { /* observation must never break navigation or takeover */ }
}
try {
  if (chrome.webRequest && chrome.webRequest.onHeadersReceived) {
    chrome.webRequest.onHeadersReceived.addListener(details=>{void observeMediaResponse(details);},
      {urls:["http://*/*", "https://*/*"]}, ["responseHeaders"]);
  }
} catch (e) { /* observation unavailable; ordinary download takeover remains */ }

try {
  if (chrome.tabs && chrome.tabs.onRemoved) {
    chrome.tabs.onRemoved.addListener((tabId) => {
      forgetTab(tabId);
    });
  }
  // New navigation (same tab, new video): drop the previous video's URLs so
  // itag-swaps and menus can never graft stale streams. SPA history swaps
  // (Facebook reels, YouTube shorts) fire onUpdated with a new url but no
  // "loading" status — clear on any url change too, so the next reel can't
  // offer the previous reel's fbcdn link.
  if (chrome.tabs && chrome.tabs.onUpdated) {
    chrome.tabs.onUpdated.addListener((tabId, info) => {
      if (info.status === "loading" || info.url !== undefined) {
        forgetTab(tabId);
      }
    });
  }
} catch (e) { /* ignore */ }

function setBadge() {
  api.action
    .setBadgeText({ text: hostOk ? "" : "!" })
    .catch(() => {});
  if (!hostOk) {
    api.action
      .setBadgeBackgroundColor({ color: "#C42B1C" })
      .catch(() => {});
  }
}

// Ping once at startup so the badge reflects reality.
sendToHostPing();
async function sendToHostPing() {
  try {
    const res = await api.runtime.sendNativeMessage(HOST, { type: "ping" });
    hostOk = !!res && res.type === "pong";
    if (!hostOk) lastHostError = "ping answered, but no pong (old host version?)";
    console.log(
      "[download-craft] native host '" + HOST + "': " +
        (hostOk ? "connected" : "unexpected reply: " + lastHostError)
    );
  } catch (e) {
    hostOk = false;
    lastHostError = (e && e.message) || String(e);
    console.error(
      "[download-craft] native host '" + HOST + "' unreachable:", lastHostError
    );
  }
  setBadge();
  await syncBrowserDownloadUI();
}

chrome.runtime.onInstalled.addListener(() => {
  // removeAll first: re-installs otherwise fail with duplicate-id.
  api.contextMenus.removeAll().then(() => {
    chrome.contextMenus.create({
      id: "send-to-download-craft",
      title: "Download with Download Craft",
      contexts: ["link", "video", "audio"],
    });
  });
  sendToHostPing();
  // Push the current page script into every open tab. Content scripts in
  // already-open tabs would otherwise keep running the pre-update code
  // (and fail once their APIs are gone) until each tab is reloaded by hand.
  refreshOpenTabs();
});

async function refreshOpenTabs() {
  try {
    const tabs = await api.tabs.query({});
    for (const t of tabs) {
      if (t.id === undefined) continue;
      try {
        await api.scripting.executeScript({
          target: { tabId: t.id },
          files: ["rank.js", "content.js"],
        });
      } catch (e) {
        /* chrome:// pages, web store, etc. — not scriptable, skip */
      }
    }
  } catch (e) {
    /* tabs API unavailable */
  }
}

chrome.contextMenus.onClicked.addListener((info, tab) => {
  const url = info.linkUrl || info.srcUrl;
  if (url && /^https?:\/\//i.test(url)) {
    markSeen(url, null);
    const pageUrl = (tab && tab.url) || null;
    sendToHost(url, null, { pageUrl }).then((ok) => {
      hostOk = ok;
      setBadge();
    });
  }
});

// ---- automatic capture ----
async function performBrowserCapture(item) {
  const cfg = await settings();
  const url = item.finalUrl || item.url;
  if (!url || !/^https?:\/\//i.test(url)) return; // blobs/data stay in browser
  if (!cfg.autoCapture) return;
  // DownloadItem omits the original HTTP method. Replaying a form's POST as
  // a GET loses the request body, so leave observed non-GET downloads intact.
  if (nonReplayableDownload(item.url) || nonReplayableDownload(url)) return;
  if (hostBlocked(url, cfg.blockHosts)) return; // P0.1 site filter
  if (freshSeen(url, item.filename)) return; // our own fallback re-download
  // Honor the size filter here too (sniffer-only before): tiny files stay
  // in the browser. Unknown sizes (0/undefined) still capture.
  const minB = Math.max(0, (parseInt(cfg.minSizeKB, 10) || 0)) * 1024;
  const known = item.fileSize || item.totalBytes || 0;
  if (minB > 0 && known > 0 && known < minB) return;
  try {
    await api.downloads.cancel(item.id);
  } catch (e) {
    // Already finished (fast download): cancelling is impossible, and
    // re-sending would deliver a SECOND copy — leave the browser's file.
    return;
  }
  let ok = false;
  try {
    ok = await sendToHost(url, item.filename, {pageUrl:item.referrer || null,referer:item.referrer || null});
  } catch (e) {
    // Cancellation already succeeded: even an unexpected messaging failure
    // must reach browser fallback rather than leaving a cancelled transfer.
    lastHostError = (e && e.message) || "native host handoff failed";
  }
  // Erase the browser entry only once the app confirmed receipt.
  if (ok) {
    try {
      await api.downloads.erase({ id: item.id });
    } catch (e) {
      /* already gone */
    }
  }
  hostOk = ok;
  setBadge();
  if (!ok) {
    try { await syncBrowserDownloadUI(); } catch (e) { /* UI state cannot block recovery */ }
    // Native host missing: hand the file back to the browser so nothing is lost.
    markSeen(url, null);
    chrome.downloads.download({ url, saveAs: false }, () => {
      if (chrome.runtime.lastError) {
        lastFallbackError = chrome.runtime.lastError.message || "browser re-download failed";
        setBadge();
      } else {
        lastFallbackError = "";
      }
    });
  }
}

const nonGetRequests = new Map();
function rememberDownloadRequest(details) {
  if (!details.url || !details.method || String(details.method).toUpperCase() === "GET") return;
  nonGetRequests.set(details.url, Date.now());
  if (nonGetRequests.size > 500) nonGetRequests.delete(nonGetRequests.keys().next().value);
}
function nonReplayableDownload(url) {
  const time = nonGetRequests.get(url);
  if (time === undefined) return false;
  if (Date.now() - time > 60000) { nonGetRequests.delete(url); return false; }
  return true;
}
try {
  if (chrome.webRequest && chrome.webRequest.onBeforeRequest) {
    chrome.webRequest.onBeforeRequest.addListener(rememberDownloadRequest,
      {urls:["http://*/*", "https://*/*"]});
  }
} catch (e) { /* unsupported API; normal direct-link capture still works */ }

const browserCaptures = new Map();
function captureBrowserDownload(item) {
  const previous = browserCaptures.get(item.id);
  if (previous) { Object.assign(previous.item,item); return previous.promise; }
  const state = {item:{...item},promise:null};
  state.promise = performBrowserCapture(state.item).catch(e => {
    console.error("[download-craft] capture failed",e);
  });
  browserCaptures.set(item.id,state);
  if (browserCaptures.size > 200) browserCaptures.delete(browserCaptures.keys().next().value);
  return state.promise;
}
chrome.downloads.onCreated.addListener(item => { void captureBrowserDownload(item); });
if (chrome.downloads.onDeterminingFilename) {
  chrome.downloads.onDeterminingFilename.addListener((item,suggest) => {
    captureBrowserDownload(item).finally(() => { try { suggest(); } catch (e) {} });
    return true;
  });
}


// Firefox supports asynchronous blocking at response headers. Chrome MV3
// uses the shared early-link path plus downloads.onDeterminingFilename above.
async function captureAttachmentResponse(details) {
  try {
    const file = DCRank.attachmentDownload(details);
    if (!file || freshSeen(details.url, file.filename)) return {};
    const cfg = await settings();
    const minBytes = Math.max(0, Number(cfg.minSizeKB) || 0) * 1024;
    if (!cfg.autoCapture || hostBlocked(details.url, cfg.blockHosts)
        || (file.size > 0 && file.size < minBytes)) return {};
    const pageUrl = details.originUrl || details.documentUrl || null;
    const ok = await sendToHost(details.url, file.filename,
      {pageUrl, referer:pageUrl, mime:file.mime, size:file.size || null});
    // If the app declines, continue the original request, including its
    // authentication/session. Do not cancel and recreate this request.
    markSeen(details.url, null);
    hostOk = ok; setBadge();
    return ok ? {cancel:true} : {};
  } catch (e) { return {}; }
}
try {
  if (typeof browser !== "undefined" && browser.runtime
      && typeof browser.runtime.getBrowserInfo === "function"
      && browser.webRequest && browser.webRequest.onHeadersReceived) {
    browser.webRequest.onHeadersReceived.addListener(captureAttachmentResponse,
      {urls:["http://*/*", "https://*/*"], types:["main_frame", "sub_frame", "other"]},
      ["blocking", "responseHeaders"]);
  }
} catch (e) { /* downloads.onCreated remains the fallback */ }

// ---- popup / content-script messages ----
chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  (async () => {
    if (!msg || typeof msg !== "object") return { ok: false };
    const validPosition = p => p && Number.isFinite(p.x) && Number.isFinite(p.y)
      && p.x >= 0 && p.x <= 1 && p.y >= 0 && p.y <= 1;
    if (msg.type === "getVideoBarPosition") {
      const data=await api.storage.local.get("videoBarPosition");
      if (validPosition(data.videoBarPosition)) return {ok:true,position:data.videoBarPosition};
      // Adopt an existing session preference once; new saves survive a full
      // browser restart, without website storage or cross-device syncing.
      const legacy=api.storage.session ? await api.storage.session.get("videoBarPosition") : {};
      if (validPosition(legacy.videoBarPosition)) {
        await api.storage.local.set({videoBarPosition:legacy.videoBarPosition});
        return {ok:true,position:legacy.videoBarPosition};
      }
      return {ok:true,position:null};
    }
    if (msg.type === "setVideoBarPosition") {
      const p=msg.position;
      if(!validPosition(p))return {ok:false};
      await api.storage.local.set({videoBarPosition:{x:p.x,y:p.y}});
      return {ok:true};
    }
    if (msg.type === "download" && msg.url) {
      if (!/^https?:\/\//i.test(msg.url) || msg.url.length > 4000) {
        return { ok: false, error: "only http(s) links up to 4000 chars can be sent" };
      }
      if (msg.ver && msg.ver !== EXT_VER) {
        // Stale tab script (extension reloaded without a tab refresh):
        // push the current script into the tab so the *next* click works.
        try {
          if (sender && sender.tab && sender.tab.id !== undefined &&
              chrome.scripting && chrome.scripting.executeScript) {
            await api.scripting.executeScript({
              target: { tabId: sender.tab.id },
              files: ["rank.js", "content.js"],
            });
            return {
              ok: false,
              error: "page script refreshed itself — click once more",
            };
          }
        } catch (e) {
          /* fall through to the reload hint */
        }
        return {
          ok: false,
          error: `page runs script v${msg.ver}, app expects v${EXT_VER} — reload this tab (Ctrl+R), then click again`,
        };
      }
      markSeen(msg.url, msg.filename);
      try {
        const pageUrl = msg.pageUrl || (sender && sender.tab && sender.tab.url) || null;
        const fromPage = /^https?:\/\//i.test(sender && sender.url || "");
        const ok = await sendToHost(msg.url, msg.filename, {
          _tabId:fromPage ? sender && sender.tab && sender.tab.id : undefined,
          _frameId:fromPage ? sender && sender.frameId : undefined,
          _documentId:fromPage ? sender && sender.documentId : undefined,
          pageUrl,
          referer: msg.referer || pageUrl,
          mime: msg.mime || null,
          size: msg.size || null,
          pairUrl: msg.pairUrl || null,
          pairMime: msg.pairMime || null,
          quality: msg.quality || null,
          dur: msg.dur || null,
          audioExpected: !!msg.audioExpected,
        });
        hostOk = ok;
        setBadge();
        if (ok) return { ok: true };
        return {
          ok: false,
          error: lastHostError ||
            "native host unreachable — reinstall it from Download Craft → Settings → Browser integration, then restart Chrome",
        };
      } catch (e) {
        return { ok: false, error: (e && e.message) || String(e) };
      }
    }
    // Presence pre-check: which urls are already queued (no side effects).
    // Lets menus mark "✓ Queued" rows instead of sending dupes that bounce
    // back as "already in queue" errors.
    if (msg.type === "isQueued" && Array.isArray(msg.urls)) {
      try {
        const urls = msg.urls
          .filter((u) => typeof u === "string" && /^https?:\/\//i.test(u))
          .slice(0, 25);
        if (!urls.length) return { queued: [] };
        const res = await api.runtime.sendNativeMessage(HOST, { type: "query", urls });
        if (res && res.type === "queued" && Array.isArray(res.urls)) {
          return { queued: res.urls };
        }
        return { queued: [] };
      } catch (e) {
        return { queued: [] };
      }
    }
    if (msg.type === "getMedia") {      // Popup passes its tab; content scripts arrive with sender.tab.
      // Extension pages can also have sender.tab/documentId. Their explicit
      // selected-tab query is different from a website content-script query.
      const ownTab = /^https?:\/\//i.test(sender && sender.url || "")
        ? sender && sender.tab && sender.tab.id : undefined;
      const tabId = ownTab !== undefined ? ownTab : msg.tabId;
      const list = (tabId !== undefined && tabMedia.get(tabId)) || [];
      const page = ownTab !== undefined ? sender.url || msg.pageUrl : msg.pageUrl;
      // A content script may read its own frame only. Do not graft another
      // player's streams onto a blob player, or revive a previous SPA page.
      const full = list.slice(-25).filter(m=>
        (ownTab===undefined || sender.frameId===undefined || m.frameId===sender.frameId)
        && (ownTab===undefined || !sender.documentId || !m.documentId || sender.documentId===m.documentId)
        && (!page || !m.pageUrl || m.pageUrl===page));
      // P4 cross-tab hint: how many tabs have seen the same file.
      const annotated = full.map((m) => {
        let tabs = 1;
        try {
          const hit = seenTabs.get(canonKey(m.url));
          if (hit) tabs = Math.max(1, hit.size);
        } catch (e) { /* ignore */ }
        return { ...m, tabs };
      });
      return { items: annotated };
    }
    if (msg.type === "getSizes" && Array.isArray(msg.urls)) {
      // IDM-style file sizes for the quality menu. HEAD first, then a
      // bytes=0-0 Range fallback (media servers often hide length on HEAD).
      // credentials:include sends the browser's own cookies (googlevideo
      // answers 403 to anonymous HEADs but serves logged-in sessions), with
      // the page as referrer. (fetch still cannot set Cookie manually —
      // forbidden header — so off-session hosts may stay unknown; sizes
      // stay best-effort.)
      const seenUrls = new Set();
      const urls = msg.urls
        .filter((u) => typeof u === "string" && /^https?:\/\//i.test(u) && !seenUrls.has(u) && (seenUrls.add(u), true))
        .slice(0, 12);
      const referrer = (typeof msg.referer === "string" && /^https?:\/\//i.test(msg.referer))
        ? msg.referer
        : undefined;
      const out = {};
      await Promise.all(urls.map(async (u) => {
        try {
          if (sizeCache.has(u)) { out[u] = sizeCache.get(u); return; }
          const ctl = new AbortController();
          const timer = setTimeout(() => ctl.abort(), 8000);
          let len = 0;
          try {
            const r = await fetch(u, { method: "HEAD", signal: ctl.signal, credentials: "include", redirect: "follow", ...(referrer ? { referrer, referrerPolicy: "unsafe-url" } : {}) });
            len = parseInt(r.headers.get("content-length") || "0", 10) || 0;
          } finally {
            clearTimeout(timer);
          }
          if (!len) {
            const ctl2 = new AbortController();
            const t2 = setTimeout(() => ctl2.abort(), 8000);
            try {
              const r2 = await fetch(u, { method: "GET", headers: { Range: "bytes=0-0" }, signal: ctl2.signal, credentials: "include", redirect: "follow", ...(referrer ? { referrer, referrerPolicy: "unsafe-url" } : {}) });
              const cr = r2.headers.get("content-range") || "";
              const m = cr.match(/\/(\d+)\s*$/);
              len = m ? parseInt(m[1], 10) || 0 : (parseInt(r2.headers.get("content-length") || "0", 10) || 0);
            } finally {
              clearTimeout(t2);
            }
          }
          out[u] = len;
          sizeCache.set(u, len);
          // Drop the oldest half instead of wiping the cache outright.
          if (sizeCache.size > 200) {
            let n = 0;
            for (const k of sizeCache.keys()) {
              sizeCache.delete(k);
              if (++n >= 100) break;
            }
          }
        } catch (e) {
          out[u] = 0;
        }
      }));
      return { sizes: out };
    }
    if (msg.type === "captureLink") {
      const cfg = await settings();
      if (!cfg.autoCapture || !/^https?:\/\//i.test(msg.url || "") || hostBlocked(msg.url,cfg.blockHosts)) return {ok:false};
      const ok = await sendToHost(msg.url,msg.filename,{pageUrl:msg.pageUrl,referer:msg.pageUrl});
      // A failed direct handoff must replay in Brave without recapture.
      markSeen(msg.url,null);
      return {ok};
    }
    if (msg.type === "getState") {
      // Fresh status on every popup open (the worker only pings at
      // startup, which can predate the app's first run).
      await sendToHostPing();
      const cfg = await settings();
      const downloadUI = await syncBrowserDownloadUI();
      return { hideBrowserUI:!!cfg.hideBrowserUI, downloadUI, autoCapture: !!cfg.autoCapture, minSizeKB: cfg.minSizeKB, blockHosts: cfg.blockHosts, hostOk, hostError: lastHostError || null, fallbackError: lastFallbackError || null };
    }
    if (msg.type === "setAuto" && typeof msg.value === "boolean") {
      await api.storage.local.set({ autoCapture: msg.value });
      await syncBrowserDownloadUI();
      return { ok: true };
    }
    if (msg.type === "setBrowserUI" && typeof msg.value === "boolean") {
      await api.storage.local.set({hideBrowserUI:msg.value});
      await sendToHostPing();
      const state = await syncBrowserDownloadUI();
      return {ok:state.supported && !state.error, ...state};
    }
    if (msg.type === "setFilters") {
      const patch = {};
      let ok = false;
      if (msg.minSizeKB !== undefined) {
        const n = parseInt(msg.minSizeKB, 10);
        if (!Number.isFinite(n) || n < 0 || n > 100000) {
          return { ok: false, error: "min size must be 0–100000 KB" };
        }
        patch.minSizeKB = n;
        ok = true;
      }
      if (typeof msg.blockHosts === "string" && msg.blockHosts.length < 4000) {
        patch.blockHosts = msg.blockHosts;
        ok = true;
      }
      if (!ok) return { ok: false, error: "nothing valid to save" };
      await api.storage.local.set(patch);
      return { ok: true };
    }
    return { ok: false };
  })().then(sendResponse,()=>sendResponse({ok:false}));
  return true; // async response
});
