const {test}=require('node:test');
const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync(__dirname+'/background.js','utf8');
function worker(local,session={}){
 let listener;
 const chrome={runtime:{onMessage:{addListener:f=>listener=f}}};
 const area=data=>({get:async key=>({[key]:data[key]}),set:async patch=>Object.assign(data,patch)});
 const api={storage:{local:area(local),...(session?{session:area(session)}:{})}};
 const context={chrome,api,Number};vm.createContext(context);
 vm.runInContext(source.slice(source.indexOf('// ---- popup / content-script messages ----')),context);
 return msg=>new Promise(resolve=>listener(msg,{},resolve));
}
test('video-bar position survives worker and full browser restarts in local profile storage',async()=>{
 const local={};let send=worker(local);
 assert.equal((await send({type:'getVideoBarPosition'})).position,null);
 assert.equal((await send({type:'setVideoBarPosition',position:{x:0.25,y:0.75}})).ok,true);
 send=worker(local);
 assert.deepEqual(JSON.parse(JSON.stringify((await send({type:'getVideoBarPosition'})).position)),{x:0.25,y:0.75});
 assert.equal((await worker({})({type:'getVideoBarPosition'})).position,null);
});

test('existing valid session position migrates once, without replacing a newer saved position',async()=>{
 const local={},session={videoBarPosition:{x:.2,y:.8}};
 await worker(local,session)({type:'getVideoBarPosition'});
 assert.deepEqual(local.videoBarPosition,session.videoBarPosition);
 await worker(local)({type:'setVideoBarPosition',position:{x:.4,y:.6}});
 assert.deepEqual(JSON.parse(JSON.stringify((await worker(local,session)({type:'getVideoBarPosition'})).position)),{x:.4,y:.6});
});

test('corrupt stored positions are ignored and older browsers may omit session storage',async()=>{
 const local={videoBarPosition:{x:2,y:0}};
 assert.equal((await worker(local,{videoBarPosition:{x:'bad',y:0}})({type:'getVideoBarPosition'})).position,null);
 assert.equal((await worker({},null)({type:'getVideoBarPosition'})).position,null);
});
test('invalid video-bar coordinates never overwrite the saved profile position',async()=>{
 const local={videoBarPosition:{x:.5,y:.5}},send=worker(local);
 for(const p of [null,{}, {x:-1,y:0}, {x:1,y:Infinity},{x:NaN,y:0},{x:2,y:.2},{x:'0',y:.2}]){
  assert.equal((await send({type:'setVideoBarPosition',position:p})).ok,false);
 }
 assert.deepEqual(local.videoBarPosition,{x:.5,y:.5});
});
