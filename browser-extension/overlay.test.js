const {test}=require('node:test');
const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const R=require('./rank.js');
const source=fs.readFileSync(__dirname+'/content.js','utf8');
function harness(media) {
  const pills=[],sends=[];
  const element=()=>({isConnected:true,dataset:{},style:{},setAttribute(){},remove(){this.isConnected=false;}});
  const context={alive:()=>true,syncDismissControls(){},ytVideoId:()=>null,GEN:1,dismissedVideos:new Set(),
    dismissalKey:v=>v.src,window:{getComputedStyle:v=>({visibility:v.visibility || 'visible'})},
    document:{querySelectorAll:s=>s==='[data-dc-pill]'?pills:media,documentElement:{appendChild(){}}},
    videoDownloadBar:(title,kind)=>{const p=element();p.kind=kind;p.title=title;pills.push(p);return p;},
    addDismissControl(){},isFacebookPage:()=>false,mediaBase:()=> 'song',
    outExtFor:u=>/\.(\w+)(?:\?|$)/.exec(u)?.[0].split('?')[0] || '',mimeExt:()=>'',
    sendDownload:(...args)=>sends.push(args)};
  vm.createContext(context);
  vm.runInContext(source.slice(source.indexOf('  function mediaUrlOf('),source.indexOf('  function isFacebookPage(')),context);
  return {context,pills,sends,refresh:()=>context.ensureUniversalButtons()};
}
function player(tagName='AUDIO',src='https://example.com/song.mp3') {
  return {tagName,src,controls:true,duration:120,querySelector:()=>null,getAttribute:()=>null,
    getBoundingClientRect:()=>({width:300,height:54}),closest:()=>null};
}
test('visible native audio receives an audio bar and keeps its audio filename and MIME',()=>{
  for(const [ext,mime] of [['mp3','audio/mpeg'],['ogg','audio/ogg'],['m4a','audio/mp4']]){
    const v=player('AUDIO','https://example.com/song.'+ext),h=harness([v]);h.refresh();
    assert.equal(h.pills.length,1);assert.equal(h.pills[0].kind,'audio');
    h.pills[0].onclick({stopPropagation(){},preventDefault(){}});
    assert.equal(h.sends[0][0],v.src);assert.equal(h.sends[0][1],'song.'+ext);assert.equal(h.sends[0][2],mime);
    h.refresh();assert.equal(h.pills.length,1);
  }
});
test('hidden audio, blob audio and tiny video previews do not get new bars',()=>{
  const hidden=player();hidden.visibility='hidden';
  const blob=player('AUDIO','blob:example');
  const tiny=player('VIDEO','https://example.com/preview.mp4');tiny.getBoundingClientRect=()=>({width:80,height:40});
  const h=harness([hidden,blob,tiny]);h.refresh();assert.equal(h.pills.length,0);
});
test('a reused player removes an old bar when its new source was already dismissed',()=>{
  const v=player(),h=harness([v]);h.refresh();const old=h.pills[0];
  v.src='https://example.com/other.mp3';h.context.dismissedVideos.add(v.src);h.refresh();
  assert.equal(old.isConnected,false);assert.equal(v.__dcBtn,null);
});
test('audio MP4 never borrows video duration or audio expectations from a different player',()=>{
  const result=R.playerEvidence([{src:'https://example.com/other.mp4',duration:800,mozHasAudio:true}],
    'https://example.com/song.mp4','audio/mp4','https://example.com/page',120);
  assert.equal(result.dur,120);assert.equal(result.audioExpected,false);
});

test('toolbar action, close control and accessible labels identify audio',()=>{
  const element=()=>({style:{},children:[],attributes:{},setAttribute(k,v){this.attributes[k]=v;},
    appendChild(e){this.children.push(e);},append(...items){this.children.push(...items);},attachShadow(){return element();}});
  const context={document:{createElement:element},svgIcon:element,installBarDrag(){},ICON_DOWN:'',ICON_CLOSE:''};
  vm.createContext(context);
  vm.runInContext(source.slice(source.indexOf('  function videoDownloadBar('),source.indexOf('  // Theme: tokens')),context);
  const bar=context.videoDownloadBar('Download audio','audio');
  assert.equal(bar.__dcMain.attributes['aria-label'],'Download this audio');
  assert.equal(bar.__dcMain.children[1].textContent,'Download this audio');
  assert.equal(bar.__dcClose.attributes['aria-label'],'Hide download bar for this audio');
});

test('audio bars stay outside native controls and disappear for unrelated fullscreen content',()=>{
  const video={...player(),isConnected:true,getBoundingClientRect:()=>({width:300,height:54,top:100,bottom:154,left:30,right:330})};
  const host={__dcVideo:video,style:{},remove(){this.removed=true;}};
  const root={appendChild(el){el.parentNode=this;}};
  const context={Set,activeBarDrag:null,applySavedBarPosition:()=>false,
    document:{documentElement:root,fullscreenElement:null,querySelectorAll:s=>s==='[data-dc-pill]'?[host]:[],addEventListener(){}},
    window:{innerWidth:1000,innerHeight:800,getComputedStyle:()=>({visibility:'visible'}),addEventListener(){}}};
  vm.createContext(context);
  vm.runInContext(source.slice(source.indexOf('  const dismissedVideos ='),source.indexOf('  function ensureButton(')),context);
  context.syncDismissControls();assert.equal(host.style.top,'68px');assert.equal(host.style.display,'block');
  context.document.fullscreenElement={contains:()=>false};context.syncDismissControls();
  assert.equal(host.style.display,'none');assert.equal(host.parentNode,root);
  context.document.fullscreenElement=null;video.controls=false;context.syncDismissControls();
  assert.equal(host.removed,true);
});
