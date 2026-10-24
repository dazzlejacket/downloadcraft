// Exercise the actual native-host handoff with browser APIs stubbed.
const { it } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const R = require('./rank.js');
const source = fs.readFileSync(path.join(__dirname, 'background.js'), 'utf8');
const block = source.slice(source.indexOf('async function sendToHost('), source.indexOf('// ---- IDM-style network sniffer'));
const fragment = 'https://vod.vimeocdn.com/v2/range/prot/token/video.mp4?range=3938712-4982381';
const player = 'https://player.vimeo.com/video/123?h=unlisted';
function harness(capture, framePage, capturedUrl = fragment, reply = {type:'ok'}) {
  const calls = [], cookies = [];
  const api = {
    tabs: { sendMessage: async (tab, msg, opts) => {
      assert.equal(tab, 7); assert.equal(opts.frameId, 4);
      return { pageUrl: framePage };
    } },
    runtime: { sendNativeMessage: async (host, msg) => { calls.push(msg); return reply; } },
  };
  const make = new Function('DCRank', 'tabMedia', 'mediaAuth', 'api', 'cookiesFor', 'stripFbSlice', 'HOST', 'console',
    `let lastHostError=''; ${block}; return { sendToHost, error: () => lastHostError };`);
  const result = make(R, new Map([[7, capture ? [{url:capturedUrl, ...capture}] : []]]), new Map(), api,
    async url => { cookies.push(url); return 'session=example'; }, u => u, 'test-host', {error(){}});
  return {...result, calls, cookies};
}
it('routes a signed fragment through its captured Vimeo player and discards fragment metadata', async () => {
  const h = harness({pageUrl:player});
  assert.equal(await h.sendToHost(fragment, 'video.mp4', {pageUrl:'https://example.com/watch', size:1043670, pairUrl:'wrong-part', cookie:'wrong-domain'}), true);
  assert.equal(h.calls[0].url, player);
  assert.equal(h.calls[0].size, null);
  assert.equal(h.calls[0].pairUrl, null);
  assert.equal(h.calls[0].cookie, 'session=example');
  assert.deepEqual(h.cookies, [player]);
});
it('obtains the exact requesting iframe URL when Chromium omits documentUrl', async () => {
  const h = harness({frameId:4}, player);
  assert.equal(await h.sendToHost(fragment, 'video.mp4', {pageUrl:'https://example.com/watch'}), true);
  assert.equal(h.calls[0].url, player);
});
it('refuses unresolved fragments without starting a native download', async () => {
  const h = harness(null);
  assert.equal(await h.sendToHost(fragment, 'video.mp4', {pageUrl:'https://example.com/feed'}), false);
  assert.equal(h.calls.length, 0);
  assert.match(h.error(), /Vimeo playback fragment/);
});
it('leaves ordinary full-file MP4 handoffs unchanged', async () => {
  const h = harness(null);
  const url = 'https://vod.vimeocdn.com/full.mp4';
  assert.equal(await h.sendToHost(url, 'video.mp4', {size:12345}), true);
  assert.equal(h.calls[0].url, url);
  assert.equal(h.calls[0].size, 12345);
});

it('requires download acceptance rather than a ping or malformed response', async () => {
  for (const reply of [null, {}, {type:'pong'}, {type:'error',error:'queue write failed'}, {type:'ok',queued:false}]) {
    const h = harness(null, null, fragment, reply);
    assert.equal(await h.sendToHost('https://example.com/file.mp4', 'file.mp4', {}), false);
    assert.ok(h.error());
    if (reply && reply.error) assert.equal(h.error(), reply.error);
  }
});

it('routes captured Vimeo HLS manifests through the player, not video-only HLS assembly', async () => {
  const manifest = 'https://vod-adaptive-ak.vimeocdn.com/video/master.m3u8?token=signed';
  const h = harness({pageUrl:player}, null, manifest);
  assert.equal(await h.sendToHost(manifest, 'video.mp4', {size:100}), true);
  assert.equal(h.calls[0].url, player);
  assert.equal(h.calls[0].size, null);
});
it('preserves a complete Vimeo manifest when no player can be recovered', async () => {
  const manifest = 'https://vod-adaptive-ak.vimeocdn.com/video/master.mpd?token=signed';
  const h = harness(null);
  assert.equal(await h.sendToHost(manifest, 'video.mp4', {pageUrl:'https://example.com/watch', size:100}), true);
  assert.equal(h.calls[0].url, manifest);
  assert.equal(h.calls[0].size, null);
});
it('rejects standalone adaptive MP4 legs without an exact player', async () => {
  const h = harness(null);
  assert.equal(await h.sendToHost('https://vod-adaptive-ak.vimeocdn.com/video.mp4', 'video.mp4', {}), false);
  assert.equal(h.calls.length, 0);
});

it('ambiguous shared fragment URLs do not borrow an arbitrary iframe player',async()=>{
  const calls=[];
  const make=new Function('DCRank','tabMedia','api','cookiesFor','stripFbSlice','HOST','console',
    `let lastHostError=''; ${block}; return sendToHost;`);
  const send=make(R,new Map([[7,[{url:fragment,frameId:1,pageUrl:'https://player.vimeo.com/video/111'},
    {url:fragment,frameId:2,pageUrl:'https://player.vimeo.com/video/222'}]]]),
    {runtime:{sendNativeMessage:async(h,m)=>{calls.push(m);return {type:'ok'};}}},async()=>null,u=>u,'test',{error(){}});
  assert.equal(await send(fragment,'video.mp4',{}),false);assert.equal(calls.length,0);
  assert.equal(await send(fragment,'video.mp4',{_tabId:7,_frameId:2}),true);
  assert.equal(calls[0].url,'https://player.vimeo.com/video/222');
  assert.equal(calls[0]._frameId,undefined);
});
