const {test}=require('node:test');
const assert=require('node:assert/strict');
const R=require('./rank.js');
const fixtures=require('./fixtures/detection-responses.json');
const fs=require('node:fs');
for(const f of fixtures)test('response evidence: '+f.name,()=>{
  const entry=R.mediaResponse({url:f.url,type:f.type,method:'GET',statusCode:f.status,
    responseHeaders:[{name:'Content-Type',value:f.mime}]},0);
  assert.equal(entry?.kind || null,f.expected);
  if(entry)assert.equal(entry.url,f.url);
});
test('zero size threshold is honored and partial responses use whole-file size',()=>{
  const details={url:'https://media.example.test/file.mp4',type:'media',method:'GET',statusCode:206,
    responseHeaders:[{name:'Content-Type',value:'video/mp4'},{name:'Content-Length',value:'512'},
      {name:'Content-Range',value:'bytes 0-511/2097152'}]};
  assert.equal(R.mediaResponse(details,100).size,2097152);
  details.responseHeaders.pop();
  assert.equal(R.mediaResponse(details,100),null);
  assert.equal(R.mediaResponse(details,0).size,512);
});
test('invalid URLs, credentials, unsupported schemes and non-GET observations are ignored',()=>{
  const base={type:'media',method:'GET',statusCode:200,responseHeaders:[{name:'content-type',value:'video/mp4'}]};
  for(const url of ['http://','blob:https://example.test/x','https://user:pass@example.test/v.mp4'])
    assert.equal(R.mediaResponse({...base,url},0),null);
  assert.equal(R.mediaResponse({...base,url:'https://example.test/v.mp4',method:'POST'},0),null);
});

test('original non-DRM HLS fixture retains alternate audio and quality references exactly',()=>{
  const text=fs.readFileSync(__dirname+'/fixtures/multitrack-master.m3u8','utf8');
  const root='https://media.example.test/movie/master.m3u8?token=parent';
  const refs=R.playlistReferences(text,root);
  assert.deepEqual(refs,['https://media.example.test/movie/audio/en.m3u8?token=a%2Fb,c',
    'https://media.example.test/movie/audio/fr.m3u8?token=second',
    'https://media.example.test/movie/720/index.m3u8?token=quality720',
    'https://media.example.test/movie/1080/index.m3u8?token=quality1080']);
  const master={url:root,children:refs,frameId:2,master:true};
  assert.equal(R.completePlaylist({url:refs[0],frameId:2},[master]),master);
});

test('manifest evidence ignores keys, arbitrary text, credential URLs and unbounded references',()=>{
  const base='https://media.example.test/master.m3u8';
  assert.deepEqual(R.playlistReferences('not a manifest\nmovie.mp4',base),[]);
  assert.deepEqual(R.playlistReferences('#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI="key.bin"\nhttps://u:p@media.example.test/file\nseg-1.ts',base),['https://media.example.test/seg-1.ts']);
  assert.equal(R.playlistReferences('#EXTM3U\n'+Array.from({length:600},(_,n)=>n+'.ts').join('\n'),base).length,512);
});

test('one unrelated master is not evidence that a fragment belongs to it',()=>{
  const part={url:'https://media.example.test/seg-1.m4s',frameId:2};
  assert.equal(R.completePlaylist(part,[{url:'https://media.example.test/ad.m3u8',frameId:2,master:true}]),null);
});

test('shared children and cyclic metadata cannot choose an arbitrary master',()=>{
  const child={url:'https://media.example.test/720.m3u8',frameId:2};
  const a={url:'https://media.example.test/a.m3u8',frameId:2,children:[child.url]};
  const b={url:'https://media.example.test/b.m3u8',frameId:2,children:[child.url]};
  assert.equal(R.completePlaylist(child,[a,b]),child);
  b.children=[a.url];assert.equal(R.completePlaylist(child,[a,b]),b);
  a.children.push(b.url);assert.equal(R.completePlaylist(child,[a,b]),child);
});
