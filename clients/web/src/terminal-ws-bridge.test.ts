import { once } from 'node:events';
import { createServer } from 'node:http';

import { afterEach,describe,expect,it,vi } from 'vitest';
import WebSocket,{ WebSocketServer } from 'ws';

import { browserProjectWebSocketTarget,browserTerminalAttachTarget,installProjectWebSocketBridge,installTerminalWebSocketBridge } from './terminal-ws-bridge';

const servers:Array<{close(callback:()=>void):void}>=[];
afterEach(async()=>{await Promise.all(servers.splice(0).map(server=>new Promise<void>(resolve=>{server.close(()=>{resolve()})})))});
const listen=async(server:ReturnType<typeof createServer>)=>{server.listen(0,'127.0.0.1');await once(server,'listening');const address=server.address();if(!address||typeof address==='string')throw new Error('missing test address');return address.port};

describe('terminal WebSocket bridge',()=>{
  it('parses only credential-free project terminal attach paths',async()=>{
    const resolve=(project:string,terminal:string)=>`${project}:${terminal}`;
    await expect(browserTerminalAttachTarget('/__hotsheet/project-api/project%20one/terminals/codex%2Fmain/attach',resolve)).resolves.toBe('project one:codex/main');
    await expect(browserTerminalAttachTarget('/__hotsheet/project-api/project/terminals',resolve)).resolves.toBeUndefined();
  });

  it('awaits asynchronous supervision before resolving a reconnect target',async()=>{
    const resolve=async(project:string,terminal:string)=>{await Promise.resolve();return`${project}:${terminal}`};
    await expect(browserTerminalAttachTarget('/__hotsheet/project-api/project/terminals/main/attach',resolve)).resolves.toBe('project:main');
  });

  it('routes the credential-free project sync path separately from terminal attaches',async()=>{
    const terminal=vi.fn(async(project:string,id:string)=>`terminal:${project}:${id}`),sync=vi.fn(async(project:string)=>`sync:${project}`);
    await expect(browserProjectWebSocketTarget('/__hotsheet/project-api/project%20one/ws/sync',terminal,sync)).resolves.toBe('sync:project one');
    await expect(browserProjectWebSocketTarget('/__hotsheet/project-api/project%20one/terminals/main/attach',terminal,sync)).resolves.toBe('terminal:project one:main');
    expect(sync).toHaveBeenCalledOnce();
  });

  it('forwards text and binary frames across an actual WebSocket upgrade',async()=>{
    const upstreamHttp=createServer(),upstream=new WebSocketServer({server:upstreamHttp});servers.push(upstream,upstreamHttp);upstream.on('connection',socket=>{socket.on('message',(data,binary)=>{socket.send(data,{binary})})});
    const upstreamPort=await listen(upstreamHttp),bridgeHttp=createServer();servers.push(bridgeHttp);installTerminalWebSocketBridge({httpServer:bridgeHttp},()=>`ws://127.0.0.1:${upstreamPort}/terminals/test/attach?secret=server-only`);const bridgePort=await listen(bridgeHttp);
    const client=new WebSocket(`ws://127.0.0.1:${bridgePort}/__hotsheet/project-api/project/terminals/test/attach`);await once(client,'open');client.send('hello');const [text]=await once(client,'message');expect(text.toString()).toBe('hello');client.send(Uint8Array.from([1,2,3]));const [binary]=await once(client,'message');expect([...binary as Buffer]).toEqual([1,2,3]);client.close();await once(client,'close');
  });

  it('forwards live project change frames without exposing the upstream secret',async()=>{
    const upstreamHttp=createServer(),upstream=new WebSocketServer({server:upstreamHttp});servers.push(upstream,upstreamHttp);
    let upstreamUrl='';upstream.on('connection',(socket,request)=>{upstreamUrl=request.url??'';socket.send(JSON.stringify({cursor:1,store:'local',kind:'created',id:'01',slug:'HS2-ONE'}))});
    const upstreamPort=await listen(upstreamHttp),bridgeHttp=createServer();servers.push(bridgeHttp);
    installProjectWebSocketBridge({httpServer:bridgeHttp},async()=>undefined,()=>`ws://127.0.0.1:${upstreamPort}/ws/sync?secret=server-only`);
    const bridgePort=await listen(bridgeHttp),browserUrl=`ws://127.0.0.1:${bridgePort}/__hotsheet/project-api/project/ws/sync`;
    expect(browserUrl).not.toContain('secret');
    const client=new WebSocket(browserUrl);const [frame]=await once(client,'message');
    expect(JSON.parse(frame.toString())).toMatchObject({cursor:1,kind:'created'});
    expect(upstreamUrl).toBe('/ws/sync?secret=server-only');
    client.close();await once(client,'close');
  });
});
