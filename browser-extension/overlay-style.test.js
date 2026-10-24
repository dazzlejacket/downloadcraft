// Overlay style (classic bar | modern round mark), idle fade and the
// double-click reset. The content script is evaluated in slices, exactly
// like overlay.test.js does, so the code under test must not need a browser.
const {test}=require('node:test');
const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync(__dirname+'/content.js','utf8');

// A tiny DOM: enough for the overlay (shadow root, attributes, events).
function el(tag) {
  const e={
    tagName:String(tag||'div').toUpperCase(),
    style:{},dataset:{},children:[],attributes:{},listeners:{},
    isConnected:true,parentNode:null,
    setAttribute(k,v){this.attributes[k]=String(v);},
    getAttribute(k){return this.attributes[k]===undefined?null:this.attributes[k];},
    appendChild(c){this.children.push(c);c.parentNode=this;return c;},
    append(...items){this.children.push(...items);},
    attachShadow(){this.shadow=el('shadow-root');return this.shadow;},
    addEventListener(type,fn){(this.listeners[type]=this.listeners[type]||[]).push(fn);},
    remove(){this.isConnected=false;},
    getBoundingClientRect(){return {left:0,top:0,width:240,height:28,right:240,bottom:28};},
    querySelector(){return null;},
  };
  return e;
}
const dom={createElement:(t)=>el(t),createElementNS:(ns,t)=>el(t)};

// The overlay style block: style state, the modern button and the bar.
const STYLE_SLICE=source.slice(
  source.indexOf('  // ---- overlay style'),source.indexOf('  // Theme: tokens'));

function barHarness() {
  const context={
    document:dom,
    svgIcon:()=>el('svg'),
    installBarDrag(){},
    ICON_DOWN:'',ICON_CLOSE:'',
    ensureUniversalButtons(){},
    chrome:{runtime:{lastError:null,sendMessage(){}}},
  };
  vm.createContext(context);
  vm.runInContext(STYLE_SLICE,context);
  return context;
}

test('the modern style renders a round mark with an accessible name and no dismiss button',()=>{
  const ctx=barHarness();
  ctx.setOverlayStyle("modern");
  const bar=ctx.videoDownloadBar("Download this video with Download Craft","video");
  assert.equal(bar.__dcStyle,"modern");
  // No X button at all: the modern overlay has no dismiss control.
  assert.equal(bar.__dcClose,null);
  assert.equal(bar.__dcMain.attributes["data-dc-dismiss"],undefined);
  // Accessible name and tooltip.
  assert.equal(bar.__dcMain.attributes["aria-label"],"Download this video");
  assert.ok(bar.__dcMain.title);
  // No text label: the mark is the whole button.
  assert.equal(bar.__dcMain.children.length,1);
  // The DownloadCraft mark, inline: blue ring + blue arrow.
  const svg=bar.__dcMain.children[0];
  assert.equal(svg.getAttribute("viewBox"),"0 0 64 64");
  const ring=svg.children[0],arrow=svg.children[1];
  assert.equal(ring.getAttribute("cx"),"32");
  assert.equal(ring.getAttribute("cy"),"32");
  assert.equal(ring.getAttribute("r"),"26");
  assert.equal(ring.getAttribute("fill"),"none");
  assert.equal(ring.getAttribute("stroke"),"#3b82f6");
  assert.equal(ring.getAttribute("stroke-width"),"8");
  assert.equal(arrow.getAttribute("d"),"M32 16V44M20 33l12 12 12-12");
  assert.equal(arrow.getAttribute("stroke"),"#3b82f6");
  assert.equal(arrow.getAttribute("stroke-linecap"),"round");
});

test('the modern style names an audio overlay "Download this audio"',()=>{
  const ctx=barHarness();
  ctx.setOverlayStyle("modern");
  const bar=ctx.videoDownloadBar("Download audio","audio");
  assert.equal(bar.__dcMain.attributes["aria-label"],"Download this audio");
  assert.equal(bar.__dcClose,null);
});

test('the classic style renders the bar with its label and its X button',()=>{
  const ctx=barHarness();
  ctx.setOverlayStyle("classic");
  const bar=ctx.videoDownloadBar("Download this video with Download Craft","video");
  assert.equal(bar.__dcStyle,"classic");
  assert.equal(bar.__dcMain.attributes["aria-label"],"Download this video");
  assert.equal(bar.__dcMain.children[1].textContent,"Download this video");
  assert.ok(bar.__dcClose);
  assert.equal(bar.__dcClose.attributes["aria-label"],"Hide download bar for this video");
  assert.equal(bar.__dcClose.attributes["data-dc-dismiss"],"1");
});

test('an unknown or missing style falls back to the classic bar',()=>{
  const ctx=barHarness();
  ctx.setOverlayStyle("neon");
  assert.equal(ctx.currentOverlayStyle(),"classic");
  const bar=ctx.videoDownloadBar("Download this video","video");
  assert.equal(bar.__dcStyle,"classic");
  assert.ok(bar.__dcClose);
  // A page that never heard of the setting keeps the classic bar too.
  const fresh=barHarness();
  assert.equal(fresh.currentOverlayStyle(),"classic");
  assert.equal(fresh.videoDownloadBar("Download this video","video").__dcStyle,"classic");
});

// ---- the overlay on a page: idle fade and double click ----

const PILL_SLICE=source.slice(
  source.indexOf('  function currentSite()'),source.indexOf('  function isFacebookPage('));

function player(tagName='AUDIO',src='https://example.com/song.mp3') {
  return {tagName,src,currentSrc:src,controls:true,duration:120,querySelector:()=>null,
    getAttribute:()=>null,querySelectorAll:()=>[],
    getBoundingClientRect:()=>({width:300,height:54,top:0,bottom:54,left:0,right:300}),
    closest:()=>null};
}

// An injectable clock: the suite must not wait for real seconds.
function clock() {
  let now=0;
  const timers=[];
  return {
    setTimeout:(fn,ms)=>{const id=timers.length+1;timers.push({id,fn,at:now+(ms||0)});return id;},
    clearTimeout:(id)=>{const t=timers.find(t=>t.id===id);if(t)t.cancelled=true;},
    advance:(ms)=>{
      now+=ms;
      for(const t of timers.slice()){
        if(!t.cancelled&&t.at<=now){t.cancelled=true;t.fn();}
      }
    },
  };
}

function pageHarness(media) {
  const hosts=[],sends=[],messages=[];
  const tick=clock();
  const context={
    Set,alive:()=>true,syncDismissControls(){},ytVideoId:()=>null,GEN:1,
    dismissedVideos:new Set(),
    DCRank:{videoDismissKey:(page,src,id)=>id?`youtube:${id}`:String(src||"")},
    activeBarDrag:null,barPosition:null,positionRevision:0,
    validBarPosition:p=>!!(p&&Number.isFinite(p.x)&&Number.isFinite(p.y)
      &&p.x>=0&&p.x<=1&&p.y>=0&&p.y<=1),
    window:{getComputedStyle:v=>({visibility:v.visibility||'visible'}),innerWidth:1000,innerHeight:800,
      addEventListener(){}},
    document:{querySelectorAll:s=>{
        if(s==='[data-dc-pill]')return hosts.filter(h=>h.getAttribute("data-dc-pill")==="1");
        if(s==='video, audio[controls]')return media;
        return [];
      },
      documentElement:{appendChild(el){if(!hosts.includes(el))hosts.push(el);}},
      createElement:t=>el(t),createElementNS:(ns,t)=>el(t),
      addEventListener(){}},
    addDismissControl(){},isFacebookPage:()=>false,mediaBase:()=>'song',
    outExtFor:u=>/\.(\w+)(?:\?|$)/.exec(u)?.[0].split('?')[0]||'',mimeExt:()=>'',
    sendDownload:(...args)=>sends.push(args),
    location:{hostname:"example.com",href:"https://example.com/watch"},
    chrome:{runtime:{lastError:null,sendMessage:(msg,cb)=>{messages.push(msg);if(cb)cb({ok:true});}}},
    setTimeout:tick.setTimeout,clearTimeout:tick.clearTimeout,
  };
  vm.createContext(context);
  vm.runInContext(PILL_SLICE,context);
  const pills=()=>hosts.filter(h=>h.getAttribute("data-dc-pill")==="1");
  return {context,pills,sends,messages,tick};
}

test('the overlay fades to 30 % after the idle interval and wakes up on interaction',()=>{
  const h=pageHarness([player()]);
  h.context.ensureUniversalButtons();
  const pill=h.pills()[0];
  assert.ok(pill,"the page gets an overlay");
  assert.equal(pill.style.opacity,"1");
  assert.ok(/opacity/.test(pill.style.transition));
  // A faded overlay must stay clickable.
  assert.notEqual(pill.style.pointerEvents,"none");
  h.tick.advance(9999);
  assert.equal(pill.style.opacity,"1");
  h.tick.advance(1);
  assert.equal(pill.style.opacity,"0.3");
  // Any interaction restores it and restarts the timer.
  pill.listeners["mouseenter"][0]();
  assert.equal(pill.style.opacity,"1");
  h.tick.advance(10000);
  assert.equal(pill.style.opacity,"0.3");
  pill.listeners["pointerdown"][0]();
  assert.equal(pill.style.opacity,"1");
});

test('the modern round overlay fades the same way',()=>{
  const h=pageHarness([player()]);
  h.context.setOverlayStyle("modern");
  h.context.ensureUniversalButtons();
  const pill=h.pills()[0];
  assert.ok(pill);
  assert.equal(pill.__dcStyle,"modern");
  assert.equal(pill.style.opacity,"1");
  h.tick.advance(10000);
  assert.equal(pill.style.opacity,"0.3");
  pill.listeners["mouseover"][0]();
  assert.equal(pill.style.opacity,"1");
});

test('a single click downloads only after the double-click window',()=>{
  const h=pageHarness([player()]);
  h.context.ensureUniversalButtons();
  const pill=h.pills()[0];
  pill.onclick({type:"click",stopPropagation(){},preventDefault(){}});
  assert.equal(h.sends.length,0,"the download waits for the double-click window");
  h.tick.advance(249);
  assert.equal(h.sends.length,0);
  h.tick.advance(1);
  assert.equal(h.sends.length,1);
  assert.equal(h.sends[0][0],"https://example.com/song.mp3");
});

test('a double click resets the saved position and downloads nothing',()=>{
  const h=pageHarness([player()]);
  h.context.ensureUniversalButtons();
  const pill=h.pills()[0];
  const click={type:"click",stopPropagation(){},preventDefault(){}};
  pill.onclick(click);
  pill.onclick(click);
  h.tick.advance(500);
  assert.equal(h.sends.length,0,"a double click must not download");
  const resets=h.messages.filter(m=>m&&m.type==="resetVideoBarPosition");
  assert.equal(resets.length,1);
  assert.equal(resets[0].site,"example.com");
});

test('a direct (non-event) click keeps the legacy immediate download',()=>{
  const h=pageHarness([player()]);
  h.context.ensureUniversalButtons();
  h.pills()[0].onclick({stopPropagation(){},preventDefault(){}});
  assert.equal(h.sends.length,1);
  assert.equal(h.sends[0][0],"https://example.com/song.mp3");
});
