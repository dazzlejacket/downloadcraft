// Bandcamp: the track metadata, the stream recognition and the name of the
// file. The helpers in rank.js are pure, so they are tested directly; the
// content script's bar is tested through a slice, exactly like the other
// content-script tests do.
const { describe, it, test } = require("node:test");
const assert = require("node:assert/strict"), fs = require("node:fs"), vm = require("node:vm");
const DCRank = require("./rank.js");

// A realistic slice of Bandcamp's own player metadata: the album's tracks,
// every format's file URL, and the track the player is on.
const TRALBUM = {
  artist: "The Testers",
  album_title: "Second Album",
  current: { id: 222, title: "Second Song" },
  trackinfo: [
    {
      title: "First Song",
      id: 111,
      track_num: 1,
      duration: 181,
      file: {
        "mp3-128": "https://t4.bcbits.com/stream/abc123/mp3-128/111?p=0&ts=1&token=old",
        "mp3-64": "https://t4.bcbits.com/stream/abc123/mp3-64/111?p=0",
      },
    },
    {
      title: "Second Song",
      id: 222,
      track_num: 2,
      duration: 205,
      file: {
        "mp3-128": "https://t4.bcbits.com/stream/abc123/mp3-128/222?p=0&ts=2&token=old",
      },
    },
  ],
};

describe("Bandcamp streams", () => {
  it("recognises Bandcamp's own media streams", () => {
    assert.equal(DCRank.isBandcampStream("https://t4.bcbits.com/stream/abc/mp3-128/111?p=0"), true);
    assert.equal(DCRank.isBandcampStream("https://t4.bcbits.com/stream/abc/flac/222"), true);
    assert.equal(DCRank.isBandcampStream("https://bandcamp.com/stream/abc/mp3-128/1"), true);
  });

  it("is not fooled by another site or by a page URL", () => {
    assert.equal(DCRank.isBandcampStream("https://evil.test/stream/abc/mp3-128/111"), false);
    assert.equal(DCRank.isBandcampStream("https://t4.bcbits.com/track/first-song"), false);
    assert.equal(DCRank.isBandcampStream("not a url"), false);
    // A host that only ends in "bandcamp.com" is not Bandcamp.
    assert.equal(DCRank.isBandcampStream("https://notbandcamp.com/stream/a/mp3-128/1"), false);
  });

  it("reads the format out of the path", () => {
    assert.equal(DCRank.bandcampFormat("https://t4.bcbits.com/stream/a/mp3-128/111"), "mp3");
    assert.equal(DCRank.bandcampFormat("https://t4.bcbits.com/stream/a/flac/111"), "flac");
    assert.equal(DCRank.bandcampFormat("https://t4.bcbits.com/stream/a/m4a/111"), "m4a");
    assert.equal(DCRank.bandcampFormat("https://t4.bcbits.com/track/x"), "");
  });
});

describe("Bandcamp metadata", () => {
  it("reads the album's tracks and the track the player is on", () => {
    const bc = DCRank.bandcampParse(JSON.stringify(TRALBUM));
    assert.equal(bc.artist, "The Testers");
    assert.equal(bc.album, "Second Album");
    assert.equal(bc.tracks.length, 2);
    assert.equal(bc.currentId, 222);
    assert.equal(bc.currentTitle, "Second Song");
    // Every format's URL is kept.
    assert.equal(bc.tracks[0].urls.length, 2);
    assert.equal(bc.tracks[0].duration, 181);
  });

  it("reads the escaped form the page puts in an attribute", () => {
    const escaped = JSON.stringify(TRALBUM)
      .replace(/&/g, "&amp;").replace(/"/g, "&quot;")
      .replace(/</g, "&lt;").replace(/>/g, "&gt;");
    const bc = DCRank.bandcampParse(escaped);
    assert.equal(bc.tracks.length, 2);
    assert.equal(bc.tracks[0].title, "First Song");
  });

  it("reads the older form, where the page assigns the object", () => {
    const bc = DCRank.bandcampParse("TralbumData = " + JSON.stringify(TRALBUM) + ";");
    assert.equal(bc.tracks.length, 2);
    assert.equal(bc.artist, "The Testers");
  });

  it("is empty for a blob it cannot read, and never guesses", () => {
    assert.equal(DCRank.bandcampParse(""), null);
    assert.equal(DCRank.bandcampParse("not json at all"), null);
    assert.equal(DCRank.bandcampParse("{broken"), null);
    assert.equal(DCRank.bandcampParse("[]"), null);
  });

  it("matches a stream to its own track, not to the album", () => {
    const bc = DCRank.bandcampParse(JSON.stringify(TRALBUM));
    const t = DCRank.bandcampTrackFor("https://t4.bcbits.com/stream/abc123/mp3-128/111?p=0&ts=9&token=new", bc);
    assert.ok(t);
    assert.equal(t.title, "First Song");
    // The token in the query changes; the path is what identifies the track.
    assert.equal(DCRank.bandcampTrackFor("https://t4.bcbits.com/stream/abc123/mp3-128/111?token=brand-new", bc).title, "First Song");
    // A stream nobody knows has no track.
    assert.equal(DCRank.bandcampTrackFor("https://t4.bcbits.com/stream/other/mp3-128/999", bc), null);
  });

  it("falls back to the track the player is on", () => {
    const bc = DCRank.bandcampParse(JSON.stringify(TRALBUM));
    const t = DCRank.bandcampCurrentTrack(bc);
    assert.ok(t);
    assert.equal(t.title, "Second Song");
  });

  it("names the file after the artist and the track", () => {
    const bc = DCRank.bandcampParse(JSON.stringify(TRALBUM));
    const t = DCRank.bandcampTrackFor("https://t4.bcbits.com/stream/abc123/mp3-128/111", bc);
    assert.equal(DCRank.bandcampFilename(t, ".mp3"), "The Testers - First Song.mp3");
    // A track without an artist keeps the track's name.
    const noArtist = { title: "Untitled", artist: "" };
    assert.equal(DCRank.bandcampFilename(noArtist, "mp3"), "Untitled.mp3");
    // No title, no name: nothing is invented.
    assert.equal(DCRank.bandcampFilename({ title: "", artist: "X" }, ".mp3"), "");
    assert.equal(DCRank.bandcampFilename(null, ".mp3"), "");
  });
});

describe("audio file names", () => {
  it("gives every audio MIME its own extension", () => {
    assert.equal(DCRank.audioExtForMime("audio/mpeg"), ".mp3");
    assert.equal(DCRank.audioExtForMime("audio/mp3"), ".mp3");
    assert.equal(DCRank.audioExtForMime("audio/mp4"), ".m4a");
    assert.equal(DCRank.audioExtForMime("audio/aac"), ".aac");
    assert.equal(DCRank.audioExtForMime("audio/flac"), ".flac");
    assert.equal(DCRank.audioExtForMime("audio/ogg"), ".ogg");
    assert.equal(DCRank.audioExtForMime("audio/opus"), ".opus");
    assert.equal(DCRank.audioExtForMime("audio/wav"), ".wav");
    assert.equal(DCRank.audioExtForMime("audio/webm"), ".weba");
    assert.equal(DCRank.audioExtForMime("video/mp4"), "");
    assert.equal(DCRank.audioExtForMime(""), "");
  });
});

describe("the audio category", () => {
  it("knows an audio file from its MIME", () => {
    assert.equal(DCRank.isAudioMedia("https://h/stream/1", "audio/mpeg"), true);
    assert.equal(DCRank.isAudioMedia("https://h/stream/1", "audio/flac; charset=x"), true);
    assert.equal(DCRank.isAudioMedia("https://h/v.mp4", "video/mp4"), false);
    assert.equal(DCRank.isAudioMedia("https://h/v.mp4", "text/html"), false);
  });

  it("knows an audio file from its own extension when the server names no type", () => {
    assert.equal(DCRank.isAudioMedia("https://h/song.mp3", ""), true);
    assert.equal(DCRank.isAudioMedia("https://h/song.flac", "application/octet-stream"), true);
    assert.equal(DCRank.isAudioMedia("https://h/clip.mp4", "application/octet-stream"), false);
    assert.equal(DCRank.isAudioMedia("https://h/clip.mp4", ""), false);
  });

  it("puts every audio file in the Audio category", () => {
    for (const n of ["a.mp3", "a.m4a", "a.aac", "a.flac", "a.ogg", "a.oga",
      "a.opus", "a.wav", "a.aiff", "a.weba", "a.mka"]) {
      assert.equal(DCRank.categoryOf(n), "Audio", n + " is audio");
    }
    assert.equal(DCRank.categoryOf("The Testers - First Song.mp3"), "Audio");
  });

  it("leaves the other categories where they were", () => {
    assert.equal(DCRank.categoryOf("a.mp4"), "Video");
    assert.equal(DCRank.categoryOf("a.png"), "Image");
    assert.equal(DCRank.categoryOf("a.pdf"), "Document");
    assert.equal(DCRank.categoryOf("a.zip"), "Compressed");
    assert.equal(DCRank.categoryOf("a.exe"), "Apps");
    assert.equal(DCRank.categoryOf("a.bin"), "Other");
    assert.equal(DCRank.categoryOf("noextension"), "Other");
  });
});

describe("the response classifier", () => {
  it("takes a Bandcamp stream even when the answer names no MIME", () => {
    const r = DCRank.mediaResponse({
      url: "https://t4.bcbits.com/stream/abc/mp3-128/111?p=0&ts=1&token=x",
      type: "xmlhttprequest",
      statusCode: 200,
      method: "GET",
      responseHeaders: [{ name: "content-type", value: "application/octet-stream" }],
    }, 0);
    assert.ok(r, "the stream is media");
    assert.equal(r.mime, "audio/mpeg", "the path says what the file is");
  });

  it("still refuses a Bandcamp page that answers with HTML", () => {
    const r = DCRank.mediaResponse({
      url: "https://t4.bcbits.com/stream/abc/mp3-128/111",
      type: "xmlhttprequest",
      statusCode: 200,
      method: "GET",
      responseHeaders: [{ name: "content-type", value: "text/html" }],
    }, 0);
    assert.equal(r, null);
  });
});

// ---- the content script's Bandcamp bar ----

const source = fs.readFileSync(__dirname + "/content.js", "utf8");
const BANDCAMP_SLICE = source.slice(
  source.indexOf("  // ---- Bandcamp ----"),
  source.indexOf("  // Popup peek:"));
assert.ok(BANDCAMP_SLICE.length > 0, "the Bandcamp slice is empty");

// A tiny DOM: the page's metadata, the player box and the bar's host.
function harness(opts) {
  const o = opts || {};
  const sent = [];
  const created = [];
  const tralbum = o.tralbum === null ? "" : JSON.stringify(o.tralbum || TRALBUM);
  const escaped = o.escaped
    ? tralbum.replace(/&/g, "&amp;").replace(/"/g, "&quot;")
    : tralbum;
  const scripts = o.assigned
    ? [{ textContent: "TralbumData = " + tralbum + ";" }]
    : [];
  const tralbumEl = {
    getAttribute: (k) => (k === "data-tralbum" ? escaped : null),
  };
  const playerBox = { left: 300, top: 120, right: 460, bottom: 160, width: 160, height: 40 };
  const playerEl = { getBoundingClientRect: () => playerBox };
  const doc = {
    querySelector: (s) => {
      if (s === "script[data-tralbum]") return tralbumEl;
      if (s === "#bcPlayer") return playerEl;
      return null;
    },
    querySelectorAll: (s) => (s === "script" ? scripts : []),
    documentElement: { appendChild: (el) => { created.push(el); } },
  };
  const bar = {
    style: {},
    attributes: {},
    children: [],
    // The bar's own segments, as the real factory hands them over.
    __dcMain: { onclick: null },
    __dcClose: { onclick: null },
    setAttribute(k, v) { this.attributes[k] = String(v); },
    getAttribute(k) { return this.attributes[k]; },
    remove() { this.removed = true; },
  };
  const context = {
    DCRank,
    document: doc,
    window: { innerWidth: 1280, innerHeight: 800 },
    overlaysVisible: true,
    videoDownloadBar: () => bar,
    barHostWidth: () => 240,
    sendDownload: (...args) => sent.push(args),
  };
  vm.createContext(context);
  vm.runInContext(BANDCAMP_SLICE, context);
  return { context, sent, bar, created };
}

test("a fetched Bandcamp track gets a bar that downloads the real file", () => {
  const h = harness({});
  h.context.checkBandcamp([
    { url: "https://t4.bcbits.com/stream/abc123/mp3-128/111?p=0&token=x", mime: "audio/mpeg", size: 3000000 },
  ]);
  const bar = h.created[0];
  assert.ok(bar, "the page gets a bar");
  assert.equal(bar.getAttribute("data-dc-bandcamp"), "1");
  // The bar is not a pill: there is no media element to sit on.
  assert.equal(bar.getAttribute("data-dc-pill"), undefined);
  // The bar sits on the player's own box.
  assert.equal(bar.style.left, "300px");
  assert.equal(bar.style.top, "88px");
  // The download carries the track's own name and the real stream.
  h.bar.__dcMain.onclick({ stopPropagation() {}, preventDefault() {} });
  assert.equal(h.sent.length, 1);
  assert.equal(h.sent[0][0], "https://t4.bcbits.com/stream/abc123/mp3-128/111?p=0&token=x");
  assert.equal(h.sent[0][1], "The Testers - First Song.mp3");
  assert.equal(h.sent[0][2], "audio/mpeg");
});

test("the bar follows the track the player is on", () => {
  const h = harness({});
  h.context.checkBandcamp([
    { url: "https://t4.bcbits.com/stream/abc123/mp3-128/222?p=0", mime: "audio/mpeg" },
  ]);
  h.bar.__dcMain.onclick({ stopPropagation() {}, preventDefault() {} });
  assert.equal(h.sent[0][1], "The Testers - Second Song.mp3");
});

test("the metadata is also read from the escaped and from the assigned form", () => {
  for (const opts of [{ escaped: true }, { assigned: true }]) {
    const h = harness(opts);
    h.context.checkBandcamp([
      { url: "https://t4.bcbits.com/stream/abc123/mp3-128/111", mime: "audio/mpeg" },
    ]);
    assert.ok(h.created[0], "the bar is up");
    h.bar.__dcMain.onclick({ stopPropagation() {}, preventDefault() {} });
    assert.equal(h.sent[0][1], "The Testers - First Song.mp3");
  }
});

test("a stream with no metadata gets no bar and no invented name", () => {
  const h = harness({ tralbum: null });
  h.context.checkBandcamp([
    { url: "https://t4.bcbits.com/stream/abc123/mp3-128/111", mime: "audio/mpeg" },
  ]);
  assert.equal(h.created.length, 0, "nothing is invented without the page's own names");
  assert.equal(h.sent.length, 0);
});

test("another site's media is never offered as a Bandcamp track", () => {
  const h = harness({});
  h.context.checkBandcamp([
    { url: "https://example.com/stream/abc/mp3-128/111", mime: "audio/mpeg" },
  ]);
  assert.equal(h.created.length, 0);
});

test("the X on the bar dismisses that stream for good", () => {
  const h = harness({});
  const url = "https://t4.bcbits.com/stream/abc123/mp3-128/111?p=0";
  h.context.checkBandcamp([{ url, mime: "audio/mpeg" }]);
  assert.equal(h.created.length, 1);
  h.bar.__dcClose.onclick({ preventDefault() {}, stopPropagation() {} });
  assert.ok(h.bar.removed, "the bar is gone");
  // The same stream is not offered again.
  h.context.checkBandcamp([{ url, mime: "audio/mpeg" }]);
  assert.equal(h.created.length, 1);
});

test("hidden buttons mean no bandcamp bar either", () => {
  const h = harness({});
  h.context.overlaysVisible = false;
  h.context.checkBandcamp([
    { url: "https://t4.bcbits.com/stream/abc123/mp3-128/111", mime: "audio/mpeg" },
  ]);
  assert.equal(h.created.length, 0);
});
