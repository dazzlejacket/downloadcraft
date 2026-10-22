const {test}=require('node:test');
const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const R=require('./rank.js');
const source=fs.readFileSync(__dirname+'/background.js','utf8');
function harness(options={}) {
  const listeners={},requests=[];
  const event=name=>({addListener:f=>{listeners[name]=f;}});
  const context={Map,Set,URL,Date,Number,Promise,TextDecoder,AbortSignal,DCRank:R,
    stripFbSlice:u=>u,hostBlocked:()=>false,settings:options.settings || (async()=>({minSizeKB:0})),
    fetch:options.fetch || (async url=>{requests.push(url);return new Response('#EXTM3U\n');}),
    chrome:{runtime:{onMessage:event('message')},storage:{local:{set(){}}},webRequest:{onHeadersReceived:event('headers')},
      tabs:{onUpdated:event('updated'),onRemoved:event('removed')}},
    api:{action:{setBadgeText:async()=>{},setBadgeBackgroundColor:async()=>{}}}};
  vm.createContext(context);
  vm.runInContext(source.slice(source.indexOf('const tabSessions ='),source.indexOf('function setBadge()')),context);
  vm.runInContext(source.slice(source.indexOf('// ---- popup / content-script messages ----')),context);
  return {context,listeners,requests,list:()=>vm.runInContext('tabMedia.get(7) || []',context),
    send:(msg,sender={})=>new Promise(resolve=>listeners.message(msg,sender,resolve)),
    run:async details=>{listeners.headers(details);await new Promise(setImmediate);}};
}
function response(url='https://cdn.example.test/movie.mp4',changes={}) {
  return {url,tabId:7,frameId:2,documentUrl:'https://player.example.test/embed',type:'media',method:'GET',statusCode:200,
    responseHeaders:[{name:'Content-Type',value:'video/mp4'},{name:'Content-Length',value:'2097152'}],...changes};
}
test('actual network observer records iframe media and rejects binary/HTML/error responses',async()=>{
  const h=harness();await h.run(response(undefined,{type:'sub_frame'}));
  assert.equal(h.list().length,1);assert.equal(h.list()[0].frameId,2);
  for(const changes of [{url:'https://cdn.example.test/runtime.wasm',responseHeaders:[{name:'content-type',value:'application/octet-stream'}]},
    {url:'https://cdn.example.test/error.mp4',statusCode:403},{url:'https://cdn.example.test/html.mp4',responseHeaders:[{name:'content-type',value:'text/html'}]}])
    await h.run(response(undefined,changes));
  assert.equal(h.list().length,1);assert.equal(h.requests.length,0);
});
test('opaque HLS MIME reaches optional manifest enrichment and retains the exact signed URL',async()=>{
  const h=harness(),url='https://cdn.example.test/stream?token=exact';
  await h.run(response(url,{responseHeaders:[{name:'content-type',value:'audio/x-mpegurl'}]}));
  assert.deepEqual(h.requests,[url]);assert.equal(h.list()[0].url,url);assert.equal(h.list()[0].kind,'playlist');
});

test('navigation during settings or manifest reads cannot restore old media',async()=>{
  let releaseSettings;const h=harness({settings:()=>new Promise(resolve=>{releaseSettings=resolve;})});
  const pending=h.context.observeMediaResponse(response());
  h.listeners.updated(7,{status:'loading'});releaseSettings({minSizeKB:0});await pending;
  assert.equal(h.list().length,0);
  let releaseFetch;const next=harness({fetch:()=>new Promise(resolve=>{releaseFetch=resolve;})});
  const task=next.context.observeMediaResponse(response('https://cdn.example.test/master.m3u8'));
  await new Promise(setImmediate);assert.equal(next.list().length,1);
  next.listeners.removed(7);releaseFetch(new Response('#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nv.m3u8\n'));
  await task;assert.equal(next.list().length,0);
});

test('repeated responses share one probe and refresh enriched metadata without duplicate rows',async()=>{
  const h=harness();const item=response('https://cdn.example.test/master.m3u8');
  await Promise.all([h.context.observeMediaResponse(item),h.context.observeMediaResponse(item)]);
  assert.equal(h.requests.length,1);assert.equal(h.list().length,1);
  await h.context.observeMediaResponse({...item,responseHeaders:[{name:'content-type',value:'application/vnd.apple.mpegurl'},{name:'content-length',value:'999'}]});
  assert.equal(h.requests.length,1);assert.equal(h.list()[0].size,999);assert.equal(h.list()[0].kind,'playlist');
});

test('manifest probes are limited while all original candidates remain visible',async()=>{
  const requests=[],release=[];
  const h=harness({fetch:url=>{requests.push(url);return new Promise(resolve=>release.push(resolve));}});
  const tasks=[1,2,3].map(n=>h.context.observeMediaResponse(response(`https://cdn.example.test/${n}.m3u8`)));
  await new Promise(setImmediate);assert.equal(requests.length,2);assert.equal(h.list().length,3);
  for(const resolve of release)resolve(new Response('#EXTM3U\n'));
  await Promise.all(tasks);
});

test('oversized and failed metadata probes retain the candidate and never invent children',async()=>{
  for(const make of [()=>new Response('x'.repeat(1024*1024+1)),()=>new Response('#EXTM3U',{status:403}),()=>Promise.reject(Error('expired'))]){
    const h=harness({fetch:make});await h.context.observeMediaResponse(response('https://cdn.example.test/master.m3u8?token=exact'));
    assert.equal(h.list().length,1);assert.equal(h.list()[0].children,undefined);
  }
});

test('identical resource URLs in separate frames retain separate provenance',async()=>{
  const h=harness();await h.run(response());await h.run(response(undefined,{frameId:3}));
  assert.equal(h.list().length,2);assert.notEqual(h.list()[0].frameId,h.list()[1].frameId);
});

test('content queries are scoped to the actual sender tab/frame/document and never fall back to a stale page',async()=>{
  const h=harness();await h.run(response(undefined,{documentId:'doc-a'}));
  await h.run(response('https://cdn.example.test/other.mp4',{frameId:3,documentUrl:'https://player.example.test/other'}));
  const sender={tab:{id:7},frameId:2,documentId:'doc-a',url:'https://player.example.test/embed'};
  assert.equal((await h.send({type:'getMedia',tabId:999},sender)).items.length,1);
  assert.equal((await h.send({type:'getMedia'},{...sender,documentId:'doc-b'})).items.length,0);
  assert.equal((await h.send({type:'getMedia'},{...sender,url:'https://player.example.test/new'})).items.length,0);
  assert.equal((await h.send({type:'getMedia',tabId:7})).items.length,2);
  assert.equal((await h.send({type:'getMedia',tabId:7},{tab:{id:99},frameId:0,documentId:'extension-doc',url:'chrome-extension://test/popup.html'})).items.length,2);
});

module.exports={harness,response};
