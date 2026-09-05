import '@xterm/xterm/css/xterm.css';

import { FitAddon } from '@xterm/addon-fit';
import { WebglAddon } from '@xterm/addon-webgl';
import { Terminal } from '@xterm/xterm';

import { parseTerminalSizeMessage,terminalReconnectDelay,terminalResizeClaim,terminalViewportScale } from './terminal-viewport';

export function mountTerminalViewportRuntime(element:HTMLElement,{url,viewerId}:{url:string;viewerId:string}):()=>void {
  const background=getComputedStyle(element).getPropertyValue('--hs-terminal-background').trim()||'#000';
  const terminal=new Terminal({cursorBlink:true,convertEol:false,scrollback:5_000,fontFamily:'ui-monospace, SFMono-Regular, Menlo, monospace',fontSize:12,theme:{background}}),fit=new FitAddon();
  terminal.loadAddon(fit);terminal.open(element);
  let webgl:WebglAddon|undefined;
  if(element.classList.contains('terminal-viewport--dedicated'))try{webgl=new WebglAddon();terminal.loadAddon(webgl);element.dataset.renderer='webgl';webgl.onContextLoss(()=>{webgl?.dispose();webgl=undefined;element.dataset.renderer='dom'})}catch{element.dataset.renderer='dom'}else element.dataset.renderer='dom';
  let socket:WebSocket|undefined,reconnect:number|undefined,heartbeat:number|undefined,fitFrame:number|undefined,attempt=0,visible=true,disposed=false,serverSize:{cols:number;rows:number}|undefined;
  const focused=()=>element.contains(document.activeElement);
  const proposed=()=>fit.proposeDimensions()??{cols:terminal.cols,rows:terminal.rows};
  const reconcileScale=()=>{if(!serverSize||!terminal.element)return;const size=proposed(),scale=terminalViewportScale(size.cols,size.rows,serverSize.cols,serverSize.rows),mismatch=scale<1;terminal.element.style.transform=mismatch?`scale(${scale})`:'';terminal.element.style.width=mismatch?`${100/scale}%`:'';terminal.element.style.height=mismatch?`${100/scale}%`:'';element.dataset.scale=String(scale);if(element.dataset.driving==='false'){const label=`Viewing at ${serverSize.cols}×${serverSize.rows} · focus to resize`;element.dataset.viewingLabel=label;element.setAttribute('aria-description',label)}else{delete element.dataset.viewingLabel;element.removeAttribute('aria-description')}};
  const claim=()=>{if(socket?.readyState!==WebSocket.OPEN)return;const size=proposed();socket.send(terminalResizeClaim(viewerId,size.cols,size.rows,focused(),visible))};
  const fitAndClaim=()=>{if(fitFrame!==undefined)window.cancelAnimationFrame(fitFrame);fitFrame=window.requestAnimationFrame(()=>{fitFrame=undefined;if(disposed)return;try{fit.fit()}catch{/* layout can be transiently zero-sized */}reconcileScale();claim()})};
  const connect=()=>{
    if(disposed)return;element.dataset.connection='connecting';const current=new WebSocket(url);socket=current;current.binaryType='arraybuffer';
    current.addEventListener('open',()=>{if(socket!==current)return;attempt=0;element.dataset.connection='connected';claim();heartbeat=window.setInterval(claim,5_000)});
    current.addEventListener('message',event=>{if(socket!==current)return;if(typeof event.data==='string'){const size=parseTerminalSizeMessage(event.data);if(!size)return;serverSize=size.pty_size;terminal.resize(size.pty_size.cols,size.pty_size.rows);element.dataset.driving=String(size.driven_by===viewerId);element.dataset.ptySize=`${size.pty_size.cols}x${size.pty_size.rows}`;queueMicrotask(reconcileScale);return}if(event.data instanceof Blob){void event.data.arrayBuffer().then(value=> { terminal.write(new Uint8Array(value)); });return}if(event.data instanceof ArrayBuffer)terminal.write(new Uint8Array(event.data))});
    const disconnected=()=>{if(socket!==current||disposed)return;if(heartbeat!==undefined)window.clearInterval(heartbeat);heartbeat=undefined;element.dataset.connection='reconnecting';const delay=terminalReconnectDelay(attempt++);reconnect=window.setTimeout(connect,delay)};
    current.addEventListener('close',disconnected);current.addEventListener('error',()=> { current.close(); });
  };
  const resize=new ResizeObserver(fitAndClaim);resize.observe(element);fitAndClaim();
  const intersection=new IntersectionObserver(entries=>{const next=entries[0]?.isIntersecting??false;if(next!==visible){visible=next;claim()} });intersection.observe(element);
  const focus=()=> { claim(); },focusTerminal=()=> { terminal.focus(); };element.addEventListener('click',focusTerminal);element.addEventListener('focusin',focus);element.addEventListener('focusout',focus);
  const input=terminal.onData(value=>{if(socket?.readyState===WebSocket.OPEN)socket.send(value)});
  connect();
  return ()=>{disposed=true;if(reconnect!==undefined)window.clearTimeout(reconnect);if(heartbeat!==undefined)window.clearInterval(heartbeat);if(fitFrame!==undefined)window.cancelAnimationFrame(fitFrame);resize.disconnect();intersection.disconnect();input.dispose();element.removeEventListener('click',focusTerminal);element.removeEventListener('focusin',focus);element.removeEventListener('focusout',focus);socket?.close();terminal.dispose()};
}
