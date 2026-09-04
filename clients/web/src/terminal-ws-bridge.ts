import type { IncomingMessage } from 'node:http';
import type { Duplex } from 'node:stream';

import WebSocket,{ WebSocketServer } from 'ws';

import { projectTerminalWebSocketUrl } from './project-bridge';

const browserAttach=/^\/__hotsheet\/project-api\/([^/]+)\/terminals\/([^/]+)\/attach$/;
type TargetResolver=(projectId:string,terminalId:string)=>string|undefined;

export function browserTerminalAttachTarget(requestUrl:string|undefined,resolveTarget:TargetResolver=projectTerminalWebSocketUrl):string|undefined {
  if(!requestUrl)return undefined;
  const match=new URL(requestUrl,'http://localhost').pathname.match(browserAttach);
  if(!match)return undefined;
  try{return resolveTarget(decodeURIComponent(match[1]),decodeURIComponent(match[2]))}catch{return undefined}
}

export function installTerminalWebSocketBridge(server:{httpServer?:{on(event:'upgrade',listener:(request:IncomingMessage,socket:Duplex,head:Buffer)=>void):unknown}|null},resolveTarget:TargetResolver=projectTerminalWebSocketUrl):void {
  const browserServer=new WebSocketServer({noServer:true});
  server.httpServer?.on('upgrade',(request,socket,head)=>{
    const target=browserTerminalAttachTarget(request.url,resolveTarget);
    if(!target)return;
    browserServer.handleUpgrade(request,socket,head,browser=>{
      const upstream=new WebSocket(target),pending:Array<{data:WebSocket.RawData;binary:boolean}>=[];
      browser.on('message',(data,binary)=>{if(upstream.readyState===WebSocket.OPEN)upstream.send(data,{binary});else pending.push({data,binary})});
      upstream.on('open',()=>{for(const frame of pending.splice(0))upstream.send(frame.data,{binary:frame.binary})});
      upstream.on('message',(data,binary)=>{if(browser.readyState===WebSocket.OPEN)browser.send(data,{binary})});
      const closeBrowser=()=>{if(browser.readyState===WebSocket.OPEN)browser.close()};
      const closeUpstream=()=>{if(upstream.readyState===WebSocket.OPEN||upstream.readyState===WebSocket.CONNECTING)upstream.close()};
      upstream.on('close',closeBrowser);upstream.on('error',closeBrowser);browser.on('close',closeUpstream);browser.on('error',closeUpstream);
    });
  });
}
