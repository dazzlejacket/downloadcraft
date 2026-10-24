// Browser test transport only: Node 22+ built-in fetch and WebSocket, no dependency.
const fs=require('node:fs');
const [port,extensionId,adapterPath]=process.argv.slice(2);
async function connect(url) {
  const socket=new WebSocket(url),pending=new Map();let next=0;
  await new Promise((resolve,reject)=>{socket.onopen=resolve;socket.onerror=reject;});
  socket.onmessage=event=>{
    const message=JSON.parse(event.data),task=pending.get(message.id);
    if(task){pending.delete(message.id);message.error?task.reject(Error(message.error.message)):task.resolve(message.result);}
  };
  return {socket,call:(method,params={})=>new Promise((resolve,reject)=>{
    const id=++next;pending.set(id,{resolve,reject});socket.send(JSON.stringify({id,method,params}));
  })};
}
(async()=>{
  const version=await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
  const browser=await connect(version.webSocketDebuggerUrl);
  let page;
  try {
    let targets,target;
    for(let attempt=0;attempt<100;attempt++){
      targets=await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
      target=targets.find(t=>t.url===`chrome-extension://${extensionId}/smoke.html`);
      if(target)break;
      await new Promise(resolve=>setTimeout(resolve,100));
    }
    if(!target)throw Error('Isolated extension test page is unavailable: '+JSON.stringify(targets.map(t=>({type:t.type,url:t.url}))));
    page=await connect(target.webSocketDebuggerUrl);
    // The startup target can precede unpacked-extension registration.
    await page.call('Page.enable');
    await page.call('Page.reload',{ignoreCache:true});
    let ready=false;
    for(let attempt=0;attempt<100;attempt++){
      const state=await page.call('Runtime.evaluate',{returnByValue:true,expression:"Boolean(document.getElementById('result') && document.readyState==='complete')"});
      if(state.result.value){ready=true;break;}
      await new Promise(resolve=>setTimeout(resolve,100));
    }
    if(!ready)throw Error('Reloaded extension fixture page did not become ready');
    // Execute the authored test adapter through DevTools rather than rely
    // on headless startup-page script scheduling. Application scripts still
    // run through the actual installed extension/content-script lifecycle.
    const startup=await page.call('Runtime.evaluate',{returnByValue:true,expression:'document.body?.dataset.step || null'});
    if(!startup.result.value){
      const adapter=await page.call('Runtime.evaluate',{awaitPromise:true,returnByValue:true,expression:fs.readFileSync(adapterPath,'utf8')});
      if(adapter.exceptionDetails)throw Error(JSON.stringify(adapter.exceptionDetails));
    }
    const evaluation=await page.call('Runtime.evaluate',{awaitPromise:true,returnByValue:true,expression:`
      new Promise((resolve,reject)=>{
        const start=Date.now();const tick=()=>{
          const text=document.getElementById('result')?.textContent;
          if(text && text!=='pending')return resolve(text);
          if(Date.now()-start>20000)return reject(Error('Extension smoke page remained pending: '+JSON.stringify({step:document.body.dataset.step,ready:document.readyState,resources:performance.getEntriesByType('resource').map(e=>e.name)})));
          setTimeout(tick,100);
        };tick();
      })`});
    if(evaluation.exceptionDetails)throw Error(JSON.stringify(evaluation.exceptionDetails));
    console.log(evaluation.result.value);
  } finally {
    if(page)page.socket.close();
    await browser.call('Browser.close');browser.socket.close();
  }
})().catch(error=>{console.error(String(error));process.exitCode=1;});
