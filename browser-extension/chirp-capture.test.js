// The CHIRP capture: a link on chirpmyradio.com that points at
// archive.chirpmyradio.com - a DIFFERENT host. The capture must carry the
// cookies the browser holds for the DOWNLOAD host (the server's browser
// check cookie lives there) and the browser's own User-Agent, or the app
// talks to the server as a stranger and gets the "Just a moment..." page.
const { it } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const R = require('./rank.js');

const source = fs.readFileSync(path.join(__dirname, 'background.js'), 'utf8');
const block = source.slice(
  source.indexOf('async function sendToHost('),
  source.indexOf('// ---- IDM-style network sniffer'),
);

const PAGE = 'https://chirpmyradio.com/projects/chirp/wiki/Download';
const FILE = 'https://archive.chirpmyradio.com/download?stream=next';
const UA =
  'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36';

function harness(cookiesByHost) {
  const calls = [];
  const asked = [];
  const api = {
    runtime: {
      sendNativeMessage: async (host, msg) => {
        calls.push(msg);
        return { type: 'ok', queued: true };
      },
    },
  };
  // Stands in for chrome.cookies.getAll({url}): the browser answers with the
  // cookies of THAT url's host, nothing else.
  const cookiesFor = async (url) => {
    asked.push(url);
    const host = new URL(url).hostname;
    return cookiesByHost[host] || null;
  };
  const make = new Function(
    'DCRank',
    'tabMedia',
    'api',
    'cookiesFor',
    'stripFbSlice',
    'HOST',
    'console',
    'navigator',
    `let lastHostError=''; ${block}; return { sendToHost, error: () => lastHostError };`,
  );
  const result = make(
    R,
    new Map(),
    api,
    cookiesFor,
    (u) => u,
    'test-host',
    { error() {} },
    { userAgent: UA },
  );
  return { ...result, calls, asked };
}

it('carries the cookies of the download host, not of the page host', async () => {
  const h = harness({
    'chirpmyradio.com': 'session=page-only',
    'archive.chirpmyradio.com': 'cf_clearance=abc123; __cf_bm=xyz',
  });
  assert.equal(await h.sendToHost(FILE, null, { pageUrl: PAGE }), true);
  assert.equal(h.calls.length, 1);
  assert.equal(h.calls[0].url, FILE);
  assert.equal(
    h.calls[0].cookie,
    'cf_clearance=abc123; __cf_bm=xyz',
    'the browser check cookie of the download host must travel with the capture',
  );
  assert.deepEqual(h.asked, [FILE], 'only the download host may be asked for cookies');
});

it('sends the browser User-Agent with the capture', async () => {
  const h = harness({ 'archive.chirpmyradio.com': 'cf_clearance=abc123' });
  assert.equal(await h.sendToHost(FILE, null, { pageUrl: PAGE }), true);
  assert.equal(h.calls[0].userAgent, UA);
  assert.equal(h.calls[0].type, 'download');
});

it('never lends the page cookies to an unrelated host', async () => {
  // The archive host holds no cookie: the page's session cookie must not be
  // sent to a third-party host (only the known platform hosts may borrow).
  const h = harness({ 'chirpmyradio.com': 'session=page-only' });
  assert.equal(await h.sendToHost(FILE, null, { pageUrl: PAGE }), true);
  assert.equal(h.calls[0].cookie, null);
  assert.deepEqual(h.asked, [FILE]);
});

it('keeps the protocol shape the app expects', async () => {
  const h = harness({ 'archive.chirpmyradio.com': 'cf_clearance=abc123' });
  assert.equal(await h.sendToHost(FILE, 'chirp.exe', { pageUrl: PAGE }), true);
  const msg = h.calls[0];
  for (const key of ['type', 'url', 'filename', 'pageUrl', 'referer', 'cookie', 'mime']) {
    assert.ok(key in msg, `the native-host payload must keep "${key}"`);
  }
  assert.equal(msg.referer, PAGE);
  assert.equal(msg.filename, 'chirp.exe');
});
