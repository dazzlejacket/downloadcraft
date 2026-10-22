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
    chrome:{storage:{local:{set(){}}},webRequest:{onHeadersReceived:event('headers')},
      tabs:{onUpdated:event('updated'),onRemoved:event('removed')}},
    api:{action:{setBadgeText:async()=>{},setBadgeBackgroundColor:async()=>{}}}};
  vm.createContext(context);
  vm.runInContext(source.slice(source.indexOf('const tabMedia ='),source.indexOf('function setBadge()')),context);
  return {context,listeners,requests,list:()=>vm.runInContext('tabMedia.get(7) || []',context),
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

module.exports={harness,response};
