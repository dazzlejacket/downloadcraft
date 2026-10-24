const {test}=require('node:test');
const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const content=fs.readFileSync(__dirname+'/content.js','utf8');

function mediaHarness() {
  const calls=[], replies=[];
  const context={Set,JSON,Number,location:{href:'https://example.com/watch'},
    document:{querySelectorAll:()=>[]},EXT_VER:JSON.parse(fs.readFileSync(__dirname+'/manifest.json')).version,
    stripFbSlice:u=>u,unwrapMediaUrl:()=>null,ctxInvalidated:()=>false,toast(){},
    chrome:{runtime:{id:'test',sendMessage:(m,cb)=>{calls.push(m);replies.push(cb);}}}};
  vm.createContext(context);
  vm.runInContext(content.slice(content.indexOf('  const pendingDownloads ='),content.indexOf('  // ---------- naming ----------')),context);
  return {context,calls,replies,send:(quality='720')=>context.sendDownload('https://cdn.example.com/video.mp4','video.mp4','video/mp4',null,null,quality)};
}
test('double video clicks share a handoff, while different quality and later retries remain possible',()=>{
  const h=mediaHarness();h.send();h.send();assert.equal(h.calls.length,1);
  h.send('1080');assert.equal(h.calls.length,2);
  h.replies[0]({ok:true});h.send();assert.equal(h.calls.length,3);
});
test('failed media handoffs release the duplicate guard',()=>{
  const h=mediaHarness();h.send();h.replies[0]({ok:false});h.send();
  assert.equal(h.calls.length,2);
  h.replies[1]({ok:false});
  h.context.chrome.runtime.sendMessage=()=>{throw Error('context invalidated');};
  h.send();
  h.context.chrome.runtime.sendMessage=m=>h.calls.push(m);
  h.send();assert.equal(h.calls.length,3);
});

function linkHarness() {
  let listener;const calls=[],replies=[];
  const context={WeakSet,Set,alive:()=>true,DCRank:{directDownloadLink:()=>true},location:{href:'https://example.com'},
    document:{addEventListener:(type,f)=>listener=f},
    chrome:{runtime:{sendMessage:(m,cb)=>{calls.push(m);replies.push(cb);}}}};
  vm.createContext(context);
  vm.runInContext(content.slice(content.indexOf('  const bypassDownloadLinks ='),content.indexOf('  loadBarPosition();')),context);
  let replayed=0,prevented=0;
  const anchor={href:'https://example.com/file.zip',hasAttribute:()=>true,getAttribute:()=>null,
    click(){const e=event();listener(e);if(!e.prevented)replayed++;}};
  const event=()=>({button:0,isTrusted:true,target:{closest:()=>anchor},preventDefault(){this.prevented=true;prevented++;},stopImmediatePropagation(){}});
  return {calls,replies,click:()=>listener(event()),replayed:()=>replayed,prevented:()=>prevented};
}
test('repeated archive clicks do not open multiple native prompts during handoff',()=>{
  const h=linkHarness();h.click();h.click();assert.equal(h.calls.length,1);assert.equal(h.prevented(),2);
  h.replies[0]({ok:true});h.click();assert.equal(h.calls.length,2);
});
test('failed archive handoff replays the original link exactly once without recapture',()=>{
  const h=linkHarness();h.click();h.click();h.replies[0]({ok:false});
  assert.equal(h.replayed(),1);assert.equal(h.calls.length,1);
  h.click();assert.equal(h.calls.length,2);
});
