// HS-06: manifest/segment URL sanitisation (rank.js).
// A manifest is untrusted input: it names the URLs the downloader fetches.
// These tests pin what must be refused and what must survive untouched.
// Run: `node --test manifest.test.js`
const { describe, it } = require("node:test");
const assert = require("node:assert/strict");
const DCRank = require("./rank.js");

const BASE = "https://media.example.test/hls/movie/master.m3u8";

describe("manifest URL scheme safety", () => {
  it("refuses every scheme that is not http or https", () => {
    for (const ref of [
      "file:///etc/passwd",
      "file://media.example.test/seg.ts",
      "data:text/plain;base64,AAAA",
      "data:video/mp4;base64,AAAA",
      "blob:https://media.example.test/1234",
      "javascript:alert(1)",
      "ftp://media.example.test/seg.ts",
      "ftps://media.example.test/seg.ts",
      "ws://media.example.test/seg.ts",
      "chrome-extension://abcdef/seg.ts",
      "about:blank",
      "vbscript:msgbox(1)",
    ]) {
      assert.equal(DCRank.manifestUrl(ref, BASE), null, `must refuse ${ref}`);
    }
  });

  it("keeps http and https references, absolute or relative", () => {
    assert.equal(
      DCRank.manifestUrl("https://media.example.test/hls/seg-1.ts", BASE),
      "https://media.example.test/hls/seg-1.ts"
    );
    assert.equal(
      DCRank.manifestUrl("http://media.example.test/hls/seg-1.ts", BASE),
      "http://media.example.test/hls/seg-1.ts"
    );
    assert.equal(
      DCRank.manifestUrl("seg-1.ts", BASE),
      "https://media.example.test/hls/movie/seg-1.ts"
    );
  });

  it("refuses credentials in the URL", () => {
    assert.equal(DCRank.manifestUrl("https://user:pass@media.example.test/seg.ts", BASE), null);
    assert.equal(DCRank.manifestUrl("//user:pass@media.example.test/seg.ts", BASE), null);
    assert.equal(DCRank.manifestUrl("https://user@media.example.test/seg.ts", BASE), null);
  });

  it("refuses a base that is not an http(s) manifest URL", () => {
    assert.equal(DCRank.manifestUrl("seg-1.ts", "file:///tmp/master.m3u8"), null);
    assert.equal(DCRank.manifestUrl("seg-1.ts", "not a url"), null);
    assert.equal(DCRank.manifestUrl("seg-1.ts", ""), null);
  });
});

describe("manifest URL control characters", () => {
  it("refuses NUL, CR, LF, ESC, DEL and other control bytes", () => {
    for (const ref of [
      "seg\x001.ts",
      "seg\r\n.ts",
      "seg\n.ts",
      "seg\r.ts",
      "seg\x1b[31m.ts",
      "seg\x7f.ts",
      "seg\x08.ts",
      "seg\x0b.ts",
    ]) {
      assert.equal(DCRank.manifestUrl(ref, BASE), null, `must refuse ${JSON.stringify(ref)}`);
    }
  });

  it("refuses percent-encoded control bytes in the path", () => {
    assert.equal(DCRank.manifestUrl("seg%00.ts", BASE), null);
    assert.equal(DCRank.manifestUrl("seg%0a.ts", BASE), null);
    assert.equal(DCRank.manifestUrl("seg%0d%0a.ts", BASE), null);
    assert.equal(DCRank.manifestUrl("seg%1b.ts", BASE), null);
  });

  it("refuses a raw space (an argument separator on any command line)", () => {
    assert.equal(DCRank.manifestUrl("seg 1.ts", BASE), null);
  });

  it("still accepts percent-encoded spaces in a path", () => {
    assert.equal(
      DCRank.manifestUrl("seg%201.ts", BASE),
      "https://media.example.test/hls/movie/seg%201.ts"
    );
  });
});

describe("manifest URL traversal safety", () => {
  it("refuses UNC paths and backslash traversal", () => {
    for (const ref of [
      "\\\\evil.test/share/seg.ts",
      "\\\\evil.test\\share\\seg.ts",
      "..\\..\\..\\..\\etc\\passwd",
      "..\\seg.ts",
    ]) {
      assert.equal(DCRank.manifestUrl(ref, BASE), null, `must refuse ${JSON.stringify(ref)}`);
    }
  });

  it("refuses a climb above the origin root", () => {
    const shallow = "https://media.example.test/master.m3u8";
    assert.equal(DCRank.manifestUrl("../../../../etc/passwd", shallow), null);
    assert.equal(DCRank.manifestUrl("../../../etc/passwd", shallow), null);
    assert.equal(
      DCRank.manifestUrl("../../../../etc/passwd", BASE),
      null
    );
  });

  it("refuses encoded dot-dot and encoded separators in the path", () => {
    for (const ref of [
      "%2e%2e/%2e%2e/etc/passwd",
      ".%2e/%2e./etc/passwd",
      "%2E%2E/etc/passwd",
      "..%2f..%2fetc/passwd",
      "%2e%2e%2fetc",
      "a%5cb.ts",
      "%252e%252e/etc/passwd",
    ]) {
      assert.equal(DCRank.manifestUrl(ref, BASE), null, `must refuse ${ref}`);
    }
  });

  it("keeps legitimate relative references that climb inside the tree", () => {
    assert.equal(
      DCRank.manifestUrl("../seg-1.ts", BASE),
      "https://media.example.test/hls/seg-1.ts"
    );
    assert.equal(
      DCRank.manifestUrl("../../seg-1.ts", BASE),
      "https://media.example.test/seg-1.ts"
    );
    assert.equal(
      DCRank.manifestUrl("../seg-1.ts", "https://media.example.test/master.m3u8"),
      "https://media.example.test/seg-1.ts"
    );
    assert.equal(
      DCRank.manifestUrl("./seg-1.ts", BASE),
      "https://media.example.test/hls/movie/seg-1.ts"
    );
  });
});

describe("signed and relative manifest URLs survive untouched", () => {
  it("preserves signed query strings byte for byte", () => {
    const cases = [
      ["audio/en.m3u8?token=a%2Fb,c", "https://media.example.test/hls/movie/audio/en.m3u8?token=a%2Fb,c"],
      ["v.m3u8?token=two", "https://media.example.test/hls/movie/v.m3u8?token=two"],
      [
        "seg.ts?Policy=abc%2Fdef&Signature=x%3D%3D",
        "https://media.example.test/hls/movie/seg.ts?Policy=abc%2Fdef&Signature=x%3D%3D",
      ],
      [
        "https://cdn.other.test/x.m3u8?hmac=abc%2Fdef&exp=1",
        "https://cdn.other.test/x.m3u8?hmac=abc%2Fdef&exp=1",
      ],
    ];
    for (const [ref, want] of cases) {
      assert.equal(DCRank.manifestUrl(ref, BASE), want, `must keep ${ref} exact`);
    }
  });

  it("keeps a percent-encoded dot inside a query string", () => {
    assert.equal(
      DCRank.manifestUrl("seg.ts?data=a%2Eb", BASE),
      "https://media.example.test/hls/movie/seg.ts?data=a%2Eb"
    );
  });
});

describe("playlist reference extraction", () => {
  it("drops unsafe references and keeps the safe ones", () => {
    const manifest = [
      "#EXTM3U",
      "#EXT-X-STREAM-INF:BANDWIDTH=1",
      "good.m3u8?token=one",
      "file:///etc/passwd",
      "../../../../etc/passwd",
      "seg\x00.ts",
      "https://user:pass@media.example.test/secret.m3u8",
      "also-good.m3u8",
    ].join("\n");
    assert.deepEqual(DCRank.playlistReferences(manifest, BASE), [
      "https://media.example.test/hls/movie/good.m3u8?token=one",
      "https://media.example.test/hls/movie/also-good.m3u8",
    ]);
  });

  it("validates EXT-X-MEDIA, I-FRAME-STREAM-INF and MAP URIs too", () => {
    const manifest = [
      "#EXTM3U",
      '#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="a",URI="audio/en.m3u8?token=a"',
      '#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="b",URI="file:///etc/passwd"',
      "#EXT-X-I-FRAME-STREAM-INF:BANDWIDTH=1,URI=\"iframes.m3u8\"",
      '#EXT-X-MAP:URI="init.mp4"',
      '#EXT-X-MAP:URI="..%2f..%2finit.mp4"',
    ].join("\n");
    assert.deepEqual(DCRank.playlistReferences(manifest, BASE), [
      "https://media.example.test/hls/movie/audio/en.m3u8?token=a",
      "https://media.example.test/hls/movie/iframes.m3u8",
      "https://media.example.test/hls/movie/init.mp4",
    ]);
  });

  it("never offers the AES key as a downloadable reference", () => {
    const manifest = '#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI="key.bin"\nseg-1.ts';
    assert.deepEqual(DCRank.playlistReferences(manifest, BASE), [
      "https://media.example.test/hls/movie/seg-1.ts",
    ]);
  });

  it("keeps the reference cap and the manifest byte cap", () => {
    const many = "#EXTM3U\n" + Array.from({ length: 600 }, (_, n) => n + ".ts").join("\n");
    assert.equal(DCRank.playlistReferences(many, BASE).length, 512);

    const huge = "#EXTM3U\n" + "x".repeat(1024 * 1024) + ".ts";
    assert.deepEqual(DCRank.playlistReferences(huge, BASE), []);
  });

  it("still ignores a body that is not a manifest", () => {
    assert.deepEqual(DCRank.playlistReferences("not a manifest\nmovie.mp4", BASE), []);
    assert.deepEqual(DCRank.playlistReferences("", BASE), []);
  });
});
