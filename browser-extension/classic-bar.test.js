// The classic overlay bar: the IDM-inspired segments (action, help, close),
// the fit-to-width label and the in-shadow help popover. The content script
// is evaluated in slices, exactly like overlay-style.test.js does, so the
// code under test must not need a browser.
const {test}=require('node:test');
const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync(__dirname+'/content.js','utf8');

const SLICES={
  // The real icons and svgIcon(), so the "?" glyph is a real inline SVG.
  icons:source.slice(source.indexOf('  const SVG_NS ='),source.indexOf('  // ---- overlay style')),
  // The overlay style block: the modern mark and the classic bar.
  bar:source.slice(source.indexOf('  // ---- overlay style'),source.indexOf('  // Theme: tokens')),
  // The shared panel registry and the click-away / Escape closer.
  panels:source.slice(source.indexOf('  function pagePanels()'),source.indexOf('  // A plain styled div')),
  // The saved position and the drag handling.
  position:source.slice(source.indexOf('  // A browser-profile preference'),source.indexOf('  // IDM-style: the circle lives ON the video player')),
  // The width reader and the automatic placement.
  placement:source.slice(source.indexOf('  const dismissedVideos ='),source.indexOf('  function ensureButton(')),
};
for(const [name,code] of Object.entries(SLICES)) {
  assert.ok(code.length>0,"the "+name+" slice is empty");
}

// A tiny DOM: enough for the overlay (shadow root, attributes, events).
function el(tag) {
  const e={
    tagName:String(tag||'div').toUpperCase(),
    style:{},dataset:{},children:[],attributes:{},listeners:{},
    textContent:'',className:'',title:'',hidden:false,
    isConnected:true,parentNode:null,
    setAttribute(k,v){this.attributes[k]=String(v);},
    getAttribute(k){return this.attributes[k]===undefined?null:this.attributes[k];},
    removeAttribute(k){delete this.attributes[k];},
    appendChild(c){this.children.push(c);c.parentNode=this;return c;},
    append(...items){for(const i of items){this.children.push(i);if(i&&typeof i==='object')i.parentNode=this;}},
    attachShadow(){this.shadow=el('shadow-root');return this.shadow;},
    addEventListener(type,fn){(this.listeners[type]=this.listeners[type]||[]).push(fn);},
    remove(){this.isConnected=false;},
    getBoundingClientRect(){return {left:0,top:0,width:240,height:28,right:240,bottom:28};},
    querySelector(){return null;},
    setPointerCapture(){},releasePointerCapture(){},hasPointerCapture(){return false;},
    focus(){},blur(){},
  };
  return e;
}
// Every node below a root, so a test can look for a segment anywhere.
function walk(node,out) {
  out=out||[];
  if(!node||typeof node!=='object')return out;
  out.push(node);
  for(const c of (node.children||[]))walk(c,out);
  return out;
}
function textOf(node) {
  let s=String(node.textContent||"");
  for(const c of (node.children||[]))s+=" "+textOf(c);
  return s;
}

function barHarness(opts) {
  const o=opts||{};
  const innerWidth=o.innerWidth||1280, innerHeight=o.innerHeight||800;
  const measure=o.measure||((s)=>s.length*7);
  const docListeners={},pillHosts=[],sent=[];
  // The bar's drag listens on the window, so the harness has to carry the
  // window's listeners and let an event reach them.
  const winListeners={};
  const fired={click:0,keydown:0};
  const dom={
    createElement:(t)=>{
      if(String(t).toLowerCase()==='canvas') {
        return {getContext:()=>({font:'',measureText:(s)=>({width:measure(s)})})};
      }
      return el(t);
    },
    createElementNS:(ns,t)=>el(t),
    addEventListener(type,fn){(docListeners[type]=docListeners[type]||[]).push(fn);},
    querySelectorAll:(s)=>(s==='[data-dc-pill]'?pillHosts:[]),
    documentElement:{appendChild(e){e.parentNode=this;}},
    fullscreenElement:null,
    getElementById:()=>null,
  };
  const context={
    document:dom,
    window:{innerWidth,innerHeight,addEventListener:(type,fn)=>{(winListeners[type]=winListeners[type]||[]).push(fn);},getComputedStyle:()=>({visibility:'visible'})},
    location:{hostname:"example.com",href:"https://example.com/watch"},
    chrome:{runtime:{lastError:null,sendMessage:(m,cb)=>{sent.push(m);if(cb)cb({ok:true});}}},
  };
  vm.createContext(context);
  vm.runInContext(Object.values(SLICES).join('\n'),context);
  // An event that a handler can stop, so a click can be traced.
  const makeEvent=(type,extra)=>Object.assign({
    type,button:0,isPrimary:true,pointerId:1,clientX:0,clientY:0,shiftKey:false,key:"",
    defaultPrevented:false,stopped:false,
    preventDefault(){this.defaultPrevented=true;},
    stopPropagation(){this.stopped=true;},
    stopImmediatePropagation(){this.stopped=true;},
  },extra||{});
  // A click bubbles up to the document unless a handler stops it.
  const dispatch=(node,type,extra)=>{
    const ev=makeEvent(type,extra);
    if(type==='click'&&typeof node.onclick==='function')node.onclick(ev);
    for(const fn of (node.listeners[type]||[]))fn(ev);
    if(!ev.stopped) {
      // A pointer event bubbles up to the window before it reaches the
      // document. The bar's drag listens there, so the drag keeps working
      // when the pointer leaves the button.
      for(const fn of (winListeners[type]||[]))fn(ev);
    }
    if(!ev.stopped) {
      for(const fn of (docListeners[type]||[])){fired[type]=(fired[type]||0)+1;fn(ev);}
    }
    return ev;
  };
  return {context,dom,docListeners,pillHosts,sent,fired,dispatch,
    page:el('div'),measure};
}

test('the classic bar is an action, a help segment and a close segment - no grip',()=>{
  const h=barHarness();
  const bar=h.context.videoDownloadBar("Download this video with Download Craft","video");
  assert.equal(bar.__dcStyle,"classic");
  assert.ok(bar.__dcMain,"the action button");
  assert.ok(bar.__dcHelp,"the help button");
  assert.ok(bar.__dcClose,"the close button");
  // The three segments, in order, and nothing else in the bar.
  const row=walk(bar.shadow).find(n=>n.className==='bar');
  assert.deepEqual(row.children.map(c=>c.className),["download","help","close"]);
  // The old grip is gone: no node and no rule for it.
  assert.equal(walk(bar.shadow).filter(n=>n.className==='grip').length,0);
  const css=walk(bar.shadow).filter(n=>n.tagName==='STYLE').map(n=>n.textContent).join("\n");
  assert.ok(!/\.grip/.test(css),"the grip CSS is gone");
  // The help segment is a real button with an inline SVG, not a web font.
  assert.equal(bar.__dcHelp.tagName,"BUTTON");
  assert.equal(bar.__dcHelp.children.length,1);
  assert.equal(bar.__dcHelp.children[0].tagName,"SVG");
  assert.ok(bar.__dcHelp.children[0].children[0].getAttribute("d").length>0,"the ? glyph is a path");
});

test('the bar says what it downloads: video and audio have their own copy',()=>{
  const h=barHarness();
  const video=h.context.videoDownloadBar("Download this video with Download Craft","video");
  assert.equal(video.__dcMain.children[1].textContent,"Download this video");
  assert.equal(video.__dcMain.getAttribute("aria-label"),"Download this video");
  const audio=h.context.videoDownloadBar("Download this audio with Download Craft","audio");
  assert.equal(audio.__dcMain.children[1].textContent,"Download audio from this page");
  assert.equal(audio.__dcMain.getAttribute("aria-label"),"Download audio from this page");
});

test('the quality-menu button keeps its own label and never the download wording',()=>{
  const h=barHarness();
  const bar=h.context.videoDownloadBar("Choose video quality in Download Craft","video",
    {style:"classic",label:"Choose video quality in Download Craft"});
  assert.equal(bar.__dcMain.children[1].textContent,"Choose video quality in Download Craft");
  assert.equal(bar.__dcMain.getAttribute("aria-label"),"Choose video quality in Download Craft");
  assert.ok(!/download this/i.test(bar.__dcMain.children[1].textContent));
});

test('the close button is always visible and still dismisses this video',()=>{
  const h=barHarness();
  const bar=h.context.videoDownloadBar("Download this video with Download Craft","video");
  assert.equal(bar.__dcClose.hidden,false,"the X is never hidden");
  assert.equal(bar.__dcClose.getAttribute("data-dc-dismiss"),"1");
  assert.equal(bar.__dcClose.getAttribute("aria-label"),"Hide download bar for this video");
  assert.ok(/restore from the extension toolbar/.test(bar.__dcClose.title));
});

test('the width helper clamps to 240..460 and to the viewport',()=>{
  const h=barHarness({innerWidth:1280});
  // A short label still gets the floor.
  assert.equal(h.context.classicBarWidth("Download this video"),240);
  // A longer label is measured, not ellipsised.
  assert.equal(h.context.classicBarWidth("Download audio from this page"),291);
  // A very long label stops at the ceiling.
  assert.equal(h.context.classicBarWidth("x".repeat(200)),460);
  // The viewport is the hard limit.
  const narrow=barHarness({innerWidth:260});
  assert.equal(narrow.context.classicBarWidth("Download audio from this page"),244);
  const tiny=barHarness({innerWidth:200});
  assert.equal(tiny.context.classicBarWidth("Download audio from this page"),184);
});

test('the bar, the drag maths and the automatic placement share one width',()=>{
  const h=barHarness({innerWidth:1280});
  // A label that needs more than the floor, so a hard-coded 240 would show.
  const bar=h.context.videoDownloadBar("Download this audio with Download Craft","audio");
  const width=h.context.classicBarWidth(bar.__dcLabelText);
  assert.equal(width,291);
  assert.notEqual(width,240);
  assert.equal(bar.style.width,width+"px","the bar is built with the shared width");
  // The automatic placement reads the same number.
  const video={tagName:"VIDEO",isConnected:true,controls:true,
    getBoundingClientRect:()=>({width:640,height:360,left:60,right:700,top:20,bottom:380})};
  bar.__dcVideo=video;
  h.pillHosts.push(bar);
  h.context.syncDismissControls();
  assert.equal(bar.style.width,width+"px");
  assert.equal(bar.style.left,(700-width-8)+"px","the placement uses the shared width");
  // The drag maths uses the same number, so the bar never jumps.
  bar.getBoundingClientRect=()=>({left:100,top:100,width,height:28,right:100+width,bottom:128});
  h.dispatch(bar.__dcMain,"pointerdown",{clientX:100,clientY:100});
  h.dispatch(bar.__dcMain,"pointermove",{clientX:140,clientY:100});
  assert.equal(vm.runInContext("barPosition.x",h.context),(140-8)/Math.max(1,1280-width-16));
  assert.equal(bar.style.width,width+"px","the width survives the drag");
});

test('the help popover explains the capture and never links out',()=>{
  const h=barHarness();
  const bar=h.context.videoDownloadBar("Download this video with Download Craft","video");
  const panel=bar.__dcHelpPanel;
  assert.ok(panel,"the popover");
  assert.equal(panel.getAttribute("data-dc-panel"),"help");
  assert.equal(panel.style.display,"none","it starts closed");
  // It lives in the bar's own shadow root.
  assert.ok(bar.shadow.children.includes(panel));
  assert.equal(bar.__dcHelp.getAttribute("aria-label"),"What this button downloads");
  assert.ok(bar.__dcHelp.title);
  // The three bullets.
  const items=walk(panel).filter(n=>n.tagName==="LI");
  assert.equal(items.length,3);
  assert.match(items[0].textContent,/cookies and User-Agent/);
  assert.match(items[1].textContent,/quality is chosen in Download Craft/);
  assert.match(items[2].textContent,/reports it and stops/);
  // No external address anywhere in the popover.
  const everything=walk(panel).map(n=>textOf(n)+" "+JSON.stringify(n.attributes)).join(" ");
  assert.ok(!/https?:\/\//i.test(everything),"the popover never points outside");
  assert.ok(!/<a\b/i.test(everything),"the popover has no link");
});

test('the popover opens under the bar and closes on Escape and on a click away',()=>{
  const h=barHarness();
  const bar=h.context.videoDownloadBar("Download this video with Download Craft","video");
  const panel=bar.__dcHelpPanel;
  // The "?" opens it, and that click never reaches the page.
  h.dispatch(bar.__dcHelp,"click");
  assert.equal(panel.style.display,"block");
  assert.equal(bar.__dcHelp.getAttribute("aria-expanded"),"true");
  assert.equal(h.fired.click,0,"the opening click does not reach the document");
  // A click inside the popover does not reach the page either.
  h.dispatch(panel,"click");
  assert.equal(h.fired.click,0);
  assert.equal(panel.style.display,"block");
  // A click away closes it (the shared closer hides every panel).
  h.dispatch(h.page,"click");
  assert.equal(h.fired.click,1);
  assert.equal(panel.style.display,"none");
  // Escape closes it too.
  h.dispatch(bar.__dcHelp,"click");
  assert.equal(panel.style.display,"block");
  h.dispatch(bar.__dcHelp,"keydown",{key:"Escape"});
  assert.equal(panel.style.display,"none");
});

test('the modern overlay is untouched: its own mark, no help and no close',()=>{
  const h=barHarness();
  h.context.setOverlayStyle("modern");
  const bar=h.context.videoDownloadBar("Download this video with Download Craft","video");
  assert.equal(bar.__dcStyle,"modern");
  assert.equal(bar.__dcClose,null,"the modern overlay has no dismiss button");
  assert.equal(bar.__dcHelp,undefined,"the modern overlay has no help segment");
  assert.equal(bar.__dcHelpPanel,undefined);
  // Only the mark is in the button.
  assert.equal(bar.__dcMain.children.length,1);
  const svg=bar.__dcMain.children[0];
  assert.equal(svg.getAttribute("viewBox"),"0 0 64 64");
  assert.equal(svg.children[0].getAttribute("stroke"),"#3b82f6");
  // The modern overlay keeps its own wording.
  const audio=h.context.videoDownloadBar("Download this audio with Download Craft","audio");
  assert.equal(audio.__dcMain.getAttribute("aria-label"),"Download this audio");
});
