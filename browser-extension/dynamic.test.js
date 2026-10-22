const {test}=require('node:test');
const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync(__dirname+'/content.js','utf8');
function harness(){
  let observer,options,timer,metadata,alive=true,refreshes=0,disconnects=0;
  const context={document:{documentElement:{},addEventListener:(name,f)=>{if(name==='loadedmetadata')metadata=f;}},
    MutationObserver:class {constructor(f){observer=f;}observe(root,o){options=o;}disconnect(){disconnects++;}},
    alive:()=>alive,__dcObservers:[],collectLinks:()=>refreshes++,ensureUniversalButtons(){},
    setTimeout:f=>{timer=f;return 1;},clearTimeout:()=>{timer=null;}};
  vm.createContext(context);
  const start=source.indexOf('    let moTimer');
  assert.ok(start>=0);
  vm.runInContext(source.slice(start,source.indexOf('  } catch (e) { /* observer optional */ }',start)),context);
  return {observe:records=>observer(records),metadata:event=>metadata(event),options,
    flush(){const f=timer;timer=null;if(f)f();},count:()=>refreshes,stale(){alive=false;},disconnects:()=>disconnects};
}
test('media source/control attribute changes and selected-source metadata trigger one debounced refresh',()=>{
  const h=harness();assert.ok(h.options.attributes);assert.ok(h.options.attributeFilter.includes('src'));
  h.observe([{type:'attributes',target:{tagName:'VIDEO'}}]);
  h.observe([{type:'attributes',target:{tagName:'SOURCE'}}]);
  h.metadata({target:{tagName:'AUDIO'}});h.flush();assert.equal(h.count(),1);
});
test('unrelated image attributes and superseded content scripts do not refresh media',()=>{
  const h=harness();h.observe([{type:'attributes',target:{tagName:'IMG'}}]);h.flush();assert.equal(h.count(),0);
  h.observe([{type:'childList',target:{tagName:'DIV'}}]);h.stale();h.flush();assert.equal(h.count(),0);
  h.observe([{type:'childList'}]);assert.equal(h.disconnects(),1);
});
