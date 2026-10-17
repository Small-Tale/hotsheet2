import { describe, expect, it } from 'vitest';

import { createChangeStreamSharedHub } from './change-stream-shared-hub';
import type { ProjectChangeTransportOptions } from './project-change-poll';

class FakePort extends EventTarget {
  readonly sent: unknown[] = [];
  closed = false;
  postMessage(message: unknown): void {
    this.sent.push(message);
  }
  start(): void {}
  close(): void {
    this.closed = true;
  }
  incoming(message: unknown): void {
    this.dispatchEvent(new MessageEvent('message', { data: message }));
  }
}

class FakeSocket extends EventTarget {
  readyState = 0;
  readonly sent: string[] = [];
  send(message: string): void {
    this.sent.push(message);
  }
  open(): void {
    this.readyState = 1;
    this.dispatchEvent(new Event('open'));
  }
  message(message: unknown): void {
    this.dispatchEvent(new MessageEvent('message', { data: JSON.stringify(message) }));
  }
  close(): void {
    this.readyState = 3;
    this.dispatchEvent(new Event('close'));
  }
}

describe('SharedWorker change-stream port hub', () => {
  it('shares one bridge socket while same-project tabs retain separate presence and disposal', () => {
    let now = 0;
    const sockets: FakeSocket[] = [];
    const runs: Array<{ options: ProjectChangeTransportOptions; signal: AbortSignal }> = [];
    const hub = createChangeStreamSharedHub({
      now: () => now,
      idleMs: 60,
      openWebSocket: () => {
        const socket = new FakeSocket();
        sockets.push(socket);
        return socket as unknown as WebSocket;
      },
      runTransport: (options, signal) => {
        runs.push({ options, signal });
      },
    });
    const a = new FakePort(),
      b = new FakePort();
    hub.connect(a);
    hub.connect(b);
    a.incoming({ type: 'hello', protocol: 1 });
    b.incoming({ type: 'hello', protocol: 1 });
    const open = (clientId: string) => ({
      type: 'open',
      project: 'same',
      generation: 1,
      apiUrl: 'http://host/__hotsheet/project-api/same',
      webSocketUrl: `ws://host/__hotsheet/project-api/same/ws/sync?client=${clientId}`,
      changeMuxUrl: 'http://host/__hotsheet/change-mux/poll',
      clientId,
    });
    a.incoming(open('tab-a'));
    b.incoming(open('tab-b'));
    expect(runs).toHaveLength(2);
    const first = runs[0].options.openWebSocket!('ws://host/a');
    const second = runs[1].options.openWebSocket!('ws://host/b');
    expect(sockets).toHaveLength(1);
    const seen: string[] = [];
    first.addEventListener('message', () => seen.push('a'));
    second.addEventListener('message', () => seen.push('b'));
    sockets[0].open();
    const commands = sockets[0].sent.map((raw) => JSON.parse(raw) as { client: string; subscription: number });
    expect(commands.map((command) => command.client)).toEqual(['tab-a', 'tab-b']);
    for (const command of commands)
      sockets[0].message({
        version: 2,
        type: 'subscribed',
        client: command.client,
        project: 'same',
        subscription: command.subscription,
      });
    sockets[0].message({
      version: 2,
      type: 'event',
      client: 'tab-b',
      project: 'same',
      subscription: commands[1].subscription,
      data: '{}',
    });
    expect(seen).toEqual(['b']);
    a.incoming({ type: 'dispose' });
    expect(runs[0].signal.aborted).toBe(true);
    expect(runs[1].signal.aborted).toBe(false);
    expect(b.closed).toBe(false);
    expect(sockets[0].readyState).toBe(1);
    now = 61;
    hub.sweep();
    expect(runs[1].signal.aborted).toBe(true);
    expect(b.closed).toBe(true);
    expect(hub.portCount()).toBe(0);
  });
});
