// Background worker: per-website overlay positions and the one-time
// migration of the removed overlay-style setting.
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
      if(Array.isArray(key)){
        const out={};
        for(const k of key) if(data[k]!==undefined) out[k]=data[k];
        return out;
      }
      if(key && typeof key==="object"){
        const out={...key};
        for(const k of Object.keys(key)) if(data[k]!==undefined) out[k]=data[k];
        return out;
      }
      return {[key]:data[key]};
    },
    set:async patch=>Object.assign(data,patch),
    remove:async (keys)=>{
      for(const k of (Array.isArray(keys)?keys:[keys])) delete data[k];
    }});
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
  return Object.assign(msg=>new Promise(resolve=>listener(msg,{},resolve)),{context});
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

test('there is one overlay, so there is no style to ask for',async()=>{
  const local={};const send=worker(local);
  // The style messages are gone: the worker does not know them any more.
  assert.deepEqual(plain(await send({type:'getOverlayStyle'})),{ok:false});
  assert.deepEqual(plain(await send({type:'setOverlayStyle',value:'modern'})),{ok:false});
  assert.equal(local.overlayStyle,undefined);
  // And the popup state does not carry one either.
  const res=await send({type:'getState'});
  assert.equal(res.overlayStyle,undefined);
});

test('an older install\'s style keys are cleared once',async()=>{
  const local={overlayStyle:'modern',overlayStyleSource:'extension'};
  const {context}=worker(local);
  // The migration removes the two keys an old install may still hold, so the
  // old choice cannot linger and the bar is what every page shows.
  assert.equal(await vm.runInContext('migrateLegacyOverlayStyle()',context),true);
  assert.equal(local.overlayStyle,undefined);
  assert.equal(local.overlayStyleSource,undefined);
  // A second run has nothing left to do.
  assert.equal(await vm.runInContext('migrateLegacyOverlayStyle()',context),false);
});

test('a clean install has nothing to migrate',async()=>{
  const local={};const {context}=worker(local);
  assert.equal(await vm.runInContext('migrateLegacyOverlayStyle()',context),false);
  assert.equal(local.overlayStyle,undefined);
  assert.equal(local.overlayStyleSource,undefined);
});

test('the migration never touches anything else',async()=>{
  const local={overlayStyle:'classic',videoBarPosition:{x:0.5,y:0.5},appTheme:'dark'};
  const {context}=worker(local);
  assert.equal(await vm.runInContext('migrateLegacyOverlayStyle()',context),true);
  assert.deepEqual(plain(local.videoBarPosition),{x:0.5,y:0.5});
  assert.equal(local.appTheme,'dark');
});
