// The caption URL the extension sends must stay YouTube's caption endpoint:
// it carries the video's id, so the app has to recognise it as a caption
// FILE. Resolved as a page, it would download the video the captions belong
// to instead of the captions.
const vm=require('node:vm');
const fs=require('node:fs');
const assert=require('node:assert');
const test=require('node:test');

const source=fs.readFileSync(__dirname+'/content.js','utf8');
const start=source.indexOf('// ---- YouTube caption tracks ----');
assert.ok(start>0,'the content script must have the caption helper');
const slice=source.slice(start,source.indexOf('// ---- end YouTube caption tracks ----'));
const context={};
vm.createContext(context);
vm.runInContext(slice,context);
const tracks=context.ytCaptionTracks;

const player=(captionTracks)=>({
  captions:{playerCaptionsTracklistRenderer:{captionTracks}},
});

test('a caption track becomes a plain WebVTT URL',()=>{
  const subs=tracks(player([
    {baseUrl:'https://www.youtube.com/api/timedtext?v=abc123&lang=en&expire=1&signature=s',
     name:{simpleText:'English'},languageCode:'en'},
  ]));
  assert.equal(subs.length,1);
  assert.equal(subs[0].lang,'en');
  assert.equal(subs[0].label,'English');
  // The format is asked for explicitly, so the app gets a .vtt file.
  assert.match(subs[0].url,/[?&]fmt=vtt$/);
  // The endpoint is kept: the app tells a caption file from a video page by
  // this path.
  assert.match(subs[0].url,/^https:\/\/www\.youtube\.com\/api\/timedtext\?/);
  // The video's id travels with it - which is exactly why the app must not
  // treat this URL as a page.
  assert.match(subs[0].url,/[?&]v=abc123/);
});

test('an auto-generated caption is labelled, and a foreign format is left alone',()=>{
  const subs=tracks(player([
    {baseUrl:'https://www.youtube.com/api/timedtext?v=x&lang=en&kind=asr',
     name:{simpleText:'English (auto-generated)'},languageCode:'en',kind:'asr'},
    {baseUrl:'https://www.youtube.com/api/timedtext?v=x&lang=de&fmt=srt',
     name:{simpleText:'Deutsch'},languageCode:'de'},
  ]));
  assert.equal(subs.length,2);
  assert.equal(subs[0].label,'English (auto)');
  // A URL that already names its own format is not asked for another one.
  assert.match(subs[1].url,/[?&]fmt=srt$/);
  assert.equal(subs[1].label,'Deutsch');
});

test('a track without a usable URL is not offered',()=>{
  assert.deepEqual( tracks(player([{baseUrl:'javascript:alert(1)',name:{simpleText:'x'}}])),[]);
  assert.deepEqual( tracks(player([{name:{simpleText:'x'}}])),[]);
  assert.deepEqual( tracks({}),[]);
  assert.deepEqual( tracks(null),[]);
});

 test('caption chip uses the selected SRT format and preserves signed parameters',()=>{
  const raw='https://www.youtube.com/api/timedtext?v=x&lang=de&fmt=srt&signature=Ab%2FC&name=CC';
  const sb=tracks(player([{baseUrl:raw,languageCode:'de'}]))[0];
  assert.equal(sb.url,raw);
  assert.equal(sb.format,'srt');
  assert.match(source,/sb\.format === "srt" \? "application\/x-subrip"/);
  assert.match(source,/sb\.format \|\| "vtt"/);
});
