import { once } from 'node:events';
import { createServer } from 'node:http';

import { afterEach,describe,expect,it } from 'vitest';
import WebSocket,{ WebSocketServer } from 'ws';

import { browserTerminalAttachTarget,installTerminalWebSocketBridge } from './terminal-ws-bridge';

const servers:Array<{close(callback:()=>void):void}>=[];
afterEach(async()=>{await Promise.all(servers.splice(0).map(server=>new Promise<void>(resolve=>{server.close(()=>{resolve()})})))});
const listen=async(server:ReturnType<typeof createServer>)=>{server.listen(0,'127.0.0.1');await once(server,'listening');const address=server.address();if(!address||typeof address==='string')throw new Error('missing test address');return address.port};

describe('terminal WebSocket bridge',()=>{
  it('parses only credential-free project terminal attach paths',()=>{
    const resolve=(project:string,terminal:string)=>`${project}:${terminal}`;
    expect(browserTerminalAttachTarget('/__hotsheet/project-api/project%20one/terminals/codex%2Fmain/attach',resolve)).toBe('project one:codex/main');
    expect(browserTerminalAttachTarget('/__hotsheet/project-api/project/terminals',resolve)).toBeUndefined();
  });

  it('forwards text and binary frames across an actual WebSocket upgrade',async()=>{
    const upstreamHttp=createServer(),upstream=new WebSocketServer({server:upstreamHttp});servers.push(upstream,upstreamHttp);upstream.on('connection',socket=>{socket.on('message',(data,binary)=>{socket.send(data,{binary})})});
    const upstreamPort=await listen(upstreamHttp),bridgeHttp=createServer();servers.push(bridgeHttp);installTerminalWebSocketBridge({httpServer:bridgeHttp},()=>`ws://127.0.0.1:${upstreamPort}/terminals/test/attach?secret=server-only`);const bridgePort=await listen(bridgeHttp);
    const client=new WebSocket(`ws://127.0.0.1:${bridgePort}/__hotsheet/project-api/project/terminals/test/attach`);await once(client,'open');client.send('hello');const [text]=await once(client,'message');expect(text.toString()).toBe('hello');client.send(Uint8Array.from([1,2,3]));const [binary]=await once(client,'message');expect([...binary as Buffer]).toEqual([1,2,3]);client.close();await once(client,'close');
  });
});
