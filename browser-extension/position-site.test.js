// Background worker: per-website overlay positions and the overlay style
// shared with the desktop app.
const {test}=require('node:test');
const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync(__dirname+'/background.js','utf8');

function worker(local,session,native,extra) {
  let listener;
  const chrome={runtime:{onMessage:{addListener:f=>listener=f}}};
  // chrome.storage.local.get() with an object returns the defaults for the
  // keys that are not stored yet.
  const area=data=>({
    get:async (key)=>{
      if(key && typeof key==="object"){
        const out={...key};
        for(const k of Object.keys(key)) if(data[k]!==undefined) out[k]=data[k];
        return out;
      }
      return {[key]:data[key]};
    },
    set:async patch=>Object.assign(data,patch)});
  const api={storage:{local:area(local),...(session?{session:area(session)}:{})},
    runtime:{sendNativeMessage:native||(async()=>{throw new Error("no native host");})}};
  const context={chrome,api,Number,HOST:"com.download_craft",
    // The parts of the worker that live above the message marker.
    hostOk:true,lastHostError:"",lastFallbackError:"",
    settings:async()=>({autoCapture:true,hideBrowserUI:false,minSizeKB:100,blockHosts:""}),
    syncBrowserDownloadUI:async()=>({supported:true,hidden:false}),
    sendToHostPing:async()=>{},
    ...(extra||{})};
  vm.createContext(context);
  vm.runInContext(source.slice(source.indexOf('// ---- popup / content-script messages ----')),context);
  return msg=>new Promise(resolve=>listener(msg,{},resolve));
}
const tick=()=>new Promise(r=>setTimeout(r,0));
// Values that cross the vm realm boundary are compared as plain data.
const plain=v=>JSON.parse(JSON.stringify(v));

test('a position saved for one website does not leak into another one',async()=>{
  const local={};const send=worker(local);
  assert.equal((await send({type:'setVideoBarPosition',site:'example.com',position:{x:0.25,y:0.75}})).ok,true);
  assert.deepEqual(plain((await send({type:'getVideoBarPosition',site:'example.com'})).position),{x:0.25,y:0.75});
  assert.equal((await send({type:'getVideoBarPosition',site:'other.com'})).position,null);
  assert.deepEqual(plain(local.videoBarPositions),{'example.com':{x:0.25,y:0.75}});
});

test('a website without its own position falls back to the legacy global',async()=>{
  const local={videoBarPosition:{x:0.5,y:0.5}};const send=worker(local);
  assert.deepEqual(plain((await send({type:'getVideoBarPosition',site:'other.com'})).position),{x:0.5,y:0.5});
});

test('a reset website returns null even when the legacy global exists',async()=>{
  const local={videoBarPosition:{x:0.5,y:0.5}};const send=worker(local);
  assert.equal((await send({type:'resetVideoBarPosition',site:'example.com'})).ok,true);
  assert.equal((await send({type:'getVideoBarPosition',site:'example.com'})).position,null);
  // Other websites keep the legacy position.
  assert.deepEqual(plain((await send({type:'getVideoBarPosition',site:'other.com'})).position),{x:0.5,y:0.5});
  assert.deepEqual(plain(local.videoBarPositions),{'example.com':{auto:true}});
});

test('a reset without a website is refused',async()=>{
  const local={};const send=worker(local);
  assert.equal((await send({type:'resetVideoBarPosition'})).ok,false);
  assert.equal(local.videoBarPositions,undefined);
});

test('the legacy path without a website keeps using the global key',async()=>{
  const local={};const send=worker(local);
  assert.equal((await send({type:'getVideoBarPosition'})).position,null);
  assert.equal((await send({type:'setVideoBarPosition',position:{x:0.25,y:0.75}})).ok,true);
  assert.deepEqual(plain(local.videoBarPosition),{x:0.25,y:0.75});
  assert.deepEqual(plain((await send({type:'getVideoBarPosition'})).position),{x:0.25,y:0.75});
  // The per-website map stays untouched by the legacy path.
  assert.equal(local.videoBarPositions,undefined);
});

test('the legacy session migration still works for the legacy path',async()=>{
  const local={},session={videoBarPosition:{x:.2,y:.8}};
  await worker(local,session)({type:'getVideoBarPosition'});
  assert.deepEqual(plain(local.videoBarPosition),plain(session.videoBarPosition));
});

test('invalid positions never overwrite a stored position',async()=>{
  const local={videoBarPositions:{'example.com':{x:0.5,y:0.5}},videoBarPosition:{x:0.5,y:0.5}};
  const send=worker(local);
  for(const p of [null,{}, {x:-1,y:0}, {x:1,y:Infinity},{x:NaN,y:0},{x:2,y:.2},{x:'0',y:.2}]){
    assert.equal((await send({type:'setVideoBarPosition',site:'example.com',position:p})).ok,false);
    assert.equal((await send({type:'setVideoBarPosition',position:p})).ok,false);
  }
  assert.deepEqual(plain(local.videoBarPositions['example.com']),{x:0.5,y:0.5});
  assert.deepEqual(plain(local.videoBarPosition),{x:0.5,y:0.5});
});

test('the overlay style falls back to the classic bar',async()=>{
  const local={};const send=worker(local);
  assert.deepEqual(plain(await send({type:'getOverlayStyle'})),{ok:true,style:'classic'});
  assert.equal((await send({type:'setOverlayStyle',value:'neon'})).ok,false);
  assert.equal(local.overlayStyle,undefined);
});

test('the extension choice is stored, owns the value and is pushed to the app',async()=>{
  const local={},calls=[];
  const send=worker(local,null,async(host,msg)=>{calls.push(msg);return {type:'ok'};});
  assert.equal((await send({type:'setOverlayStyle',value:'modern'})).ok,true);
  assert.equal(local.overlayStyle,'modern');
  assert.equal(local.overlayStyleSource,'extension');
  assert.deepEqual(plain(calls.filter(c=>c.type==='setOverlayStyle')),[{type:'setOverlayStyle',style:'modern'}]);
  assert.equal((await send({type:'getOverlayStyle'})).style,'modern');
});

test('the app value wins while the app owns the setting',async()=>{
  const local={overlayStyle:'classic',overlayStyleSource:'app'};
  const send=worker(local,null,async(host,msg)=>{
    if(msg.type==='prefs')return {type:'prefs',overlayStyle:'modern',overlayStyleSource:'app'};
    return {type:'ok'};
  });
  await send({type:'getOverlayStyle'});
  await tick();
  assert.equal(local.overlayStyle,'modern');
  assert.equal(local.overlayStyleSource,'app');
});

test('a choice made in the extension is never clobbered by the app',async()=>{
  const local={overlayStyle:'modern',overlayStyleSource:'extension'};
  const send=worker(local,null,async(host,msg)=>{
    if(msg.type==='prefs')return {type:'prefs',overlayStyle:'classic',overlayStyleSource:'app'};
    return {type:'ok'};
  });
  await send({type:'getOverlayStyle'});
  await tick();
  assert.equal(local.overlayStyle,'modern');
  assert.equal(local.overlayStyleSource,'extension');
});

test('an invalid app answer changes nothing',async()=>{
  const local={overlayStyle:'classic',overlayStyleSource:'app'};
  const send=worker(local,null,async(host,msg)=>{
    if(msg.type==='prefs')return {type:'prefs',overlayStyle:'neon'};
    return {type:'ok'};
  });
  await send({type:'getOverlayStyle'});
  await tick();
  assert.equal(local.overlayStyle,'classic');
});

test('a missing desktop app never breaks the extension',async()=>{
  const local={};const send=worker(local,null,async()=>{throw new Error('no such native application');});
  assert.equal((await send({type:'getOverlayStyle'})).style,'classic');
  assert.equal((await send({type:'setOverlayStyle',value:'modern'})).ok,true);
  assert.equal(local.overlayStyle,'modern');
});

test('the popup state carries the overlay style',async()=>{
  const local={overlayStyle:'modern',overlayStyleSource:'app'};
  const send=worker(local,null,async()=>({type:'pong'}));
  const res=await send({type:'getState'});
  assert.equal(res.overlayStyle,'modern');
});
