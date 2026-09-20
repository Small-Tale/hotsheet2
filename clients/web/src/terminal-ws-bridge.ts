import type { IncomingMessage } from 'node:http';
import type { Duplex } from 'node:stream';

import WebSocket,{ WebSocketServer } from 'ws';

import { projectChangeWebSocketUrl,projectTerminalWebSocketUrl } from './project-bridge';

const browserAttach=/^\/__hotsheet\/project-api\/([^/]+)\/terminals\/([^/]+)\/attach$/;
const browserSync=/^\/__hotsheet\/project-api\/([^/]+)\/ws\/sync$/;
type TargetResolver=(projectId:string,terminalId:string)=>string|undefined|Promise<string|undefined>;
type SyncTargetResolver=(projectId:string)=>string|undefined|Promise<string|undefined>;

export async function browserTerminalAttachTarget(requestUrl:string|undefined,resolveTarget:TargetResolver=projectTerminalWebSocketUrl):Promise<string|undefined> {
  if(!requestUrl)return undefined;
  const match=new URL(requestUrl,'http://localhost').pathname.match(browserAttach);
  if(!match)return undefined;
  try{return await resolveTarget(decodeURIComponent(match[1]),decodeURIComponent(match[2]))}catch{return undefined}
}

export async function browserProjectWebSocketTarget(requestUrl:string|undefined,resolveTerminal:TargetResolver=projectTerminalWebSocketUrl,resolveSync:SyncTargetResolver=projectChangeWebSocketUrl):Promise<string|undefined>{
  if(!requestUrl)return undefined;
  const pathname=new URL(requestUrl,'http://localhost').pathname,sync=pathname.match(browserSync);
  if(sync){try{return await resolveSync(decodeURIComponent(sync[1]))}catch{return undefined}}
  return browserTerminalAttachTarget(requestUrl,resolveTerminal);
}

export function installProjectWebSocketBridge(server:{httpServer?:{on(event:'upgrade',listener:(request:IncomingMessage,socket:Duplex,head:Buffer)=>void):unknown}|null},resolveTerminal:TargetResolver=projectTerminalWebSocketUrl,resolveSync:SyncTargetResolver=projectChangeWebSocketUrl):void {
  const browserServer=new WebSocketServer({noServer:true});
  server.httpServer?.on('upgrade',(request,socket,head)=>{
    void browserProjectWebSocketTarget(request.url,resolveTerminal,resolveSync).then(target=>{
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
    }).catch(()=>{socket.destroy()});
  });
}

/** Backward-compatible adapter for existing callers that only customize terminal resolution. */
export function installTerminalWebSocketBridge(server:Parameters<typeof installProjectWebSocketBridge>[0],resolveTarget:TargetResolver=projectTerminalWebSocketUrl):void{
  installProjectWebSocketBridge(server,resolveTarget);
}
