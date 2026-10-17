import { describe, expect, it, vi } from 'vitest';

import {
  CHANGE_WS_MUX_PATH,
  ChangeWebSocketMultiplexer,
  changeWsMuxUrl,
  parseChangeWsCommand,
  parseChangeWsMessage,
} from './change-ws-mux';

class FakeSocket extends EventTarget {
  readyState = 0;
  readonly sent: string[] = [];
  constructor(readonly url: string) {
    super();
  }
  open() {
    this.readyState = 1;
    this.dispatchEvent(new Event('open'));
  }
  send(data: string) {
    this.sent.push(data);
  }
  message(data: unknown) {
    this.dispatchEvent(new MessageEvent('message', { data: JSON.stringify(data) }));
  }
  close() {
    if (this.readyState === 3) return;
    this.readyState = 3;
    this.dispatchEvent(new Event('close'));
  }
}

describe('change WebSocket multiplexing (HS2-QJR1K6)', () => {
  it('validates versioned frames and derives a credential-free bridge URL', () => {
    expect(changeWsMuxUrl('https://host/__hotsheet/change-mux/poll')).toBe(`wss://host${CHANGE_WS_MUX_PATH}`);
    expect(
      parseChangeWsCommand({ version: 1, type: 'subscribe', project: 'a', client: 'tab', subscription: 1 }),
    ).toMatchObject({
      project: 'a',
    });
    expect(
      parseChangeWsCommand({ version: 2, type: 'subscribe', project: 'a', client: 'tab', subscription: 1 }),
    ).toBeUndefined();
    expect(
      parseChangeWsMessage({ version: 1, type: 'event', project: 'b', subscription: 2, data: '{}' }),
    ).toMatchObject({
      project: 'b',
    });
    expect(parseChangeWsMessage({ version: 1, type: 'event', project: 'b', subscription: 2, data: 4 })).toBeUndefined();
  });

  it('shares one socket while projects arrive, leave, reconnect, and receive interleaved events', () => {
    const sockets: FakeSocket[] = [];
    const mux = new ChangeWebSocketMultiplexer('ws://host/__hotsheet/change-mux/ws', 'tab', (url) => {
      const socket = new FakeSocket(url);
      sockets.push(socket);
      return socket as unknown as WebSocket;
    });
    const a = mux.open('a', 'ws://host/p/a/ws/sync');
    const b = mux.open('b', 'ws://host/p/b/ws/sync');
    const seen: string[] = [];
    a.addEventListener('open', () => seen.push('a:open'));
    b.addEventListener('open', () => seen.push('b:open'));
    a.addEventListener('message', (event) => seen.push(`a:${JSON.parse(event.data as string).cursor}`));
    b.addEventListener('message', (event) => seen.push(`b:${JSON.parse(event.data as string).cursor}`));
    b.addEventListener('close', () => seen.push('b:close'));
    expect(sockets).toHaveLength(1);
    sockets[0].open();
    expect(sockets[0].sent.map((value) => JSON.parse(value).project)).toEqual(['a', 'b']);
    sockets[0].message({ version: 1, type: 'subscribed', project: 'b', subscription: 2 });
    sockets[0].message({ version: 1, type: 'subscribed', project: 'a', subscription: 1 });
    sockets[0].message({ version: 1, type: 'event', project: 'b', subscription: 2, data: '{"cursor":8}' });
    sockets[0].message({ version: 1, type: 'event', project: 'a', subscription: 1, data: '{"cursor":3}' });
    a.close();
    expect(JSON.parse(sockets[0].sent.at(-1)!).type).toBe('unsubscribe');
    const c = mux.open('c', 'ws://host/p/c/ws/sync');
    c.addEventListener('open', () => seen.push('c:open'));
    sockets[0].message({ version: 1, type: 'subscribed', project: 'c', subscription: 3 });
    sockets[0].close();
    expect(seen).toEqual(['b:open', 'a:open', 'b:8', 'a:3', 'c:open', 'b:close']);
    const resumed = mux.open('b', 'ws://host/p/b/ws/sync');
    resumed.addEventListener('open', () => seen.push('b:reopen'));
    expect(sockets).toHaveLength(2);
    sockets[1].open();
    sockets[1].message({ version: 1, type: 'subscribed', project: 'b', subscription: 4 });
    expect(seen.at(-1)).toBe('b:reopen');
    c.close();
    resumed.close();
  });

  it('uses per-project sockets when an older bridge rejects the multiplex endpoint', () => {
    const sockets: FakeSocket[] = [];
    const mux = new ChangeWebSocketMultiplexer('ws://host/__hotsheet/change-mux/ws', 'tab', (url) => {
      const socket = new FakeSocket(url);
      sockets.push(socket);
      return socket as unknown as WebSocket;
    });
    const a = mux.open('a', 'ws://host/p/a/ws/sync');
    const b = mux.open('b', 'ws://host/p/b/ws/sync');
    const seen: string[] = [];
    a.addEventListener('open', () => seen.push('a'));
    b.addEventListener('open', () => seen.push('b'));
    sockets[0].close();
    expect(sockets.map((socket) => socket.url)).toEqual([
      'ws://host/__hotsheet/change-mux/ws',
      'ws://host/p/a/ws/sync',
      'ws://host/p/b/ws/sync',
    ]);
    sockets[1].open();
    sockets[2].open();
    expect(seen).toEqual(['a', 'b']);
    a.close();
    b.close();
  });

  it('falls back if an older bridge leaves the unknown upgrade pending', async () => {
    const sockets: FakeSocket[] = [];
    const mux = new ChangeWebSocketMultiplexer(
      'ws://host/__hotsheet/change-mux/ws',
      'tab',
      (url) => {
        const socket = new FakeSocket(url);
        sockets.push(socket);
        return socket as unknown as WebSocket;
      },
      5,
    );
    const project = mux.open('a', 'ws://host/p/a/ws/sync');
    await vi.waitFor(() => {
      expect(sockets).toHaveLength(2);
    });
    expect(sockets[0].readyState).toBe(3);
    expect(sockets[1].url).toBe('ws://host/p/a/ws/sync');
    project.close();
  });

  it('ignores stale acknowledgements and events after a project is replaced', () => {
    const sockets: FakeSocket[] = [];
    const mux = new ChangeWebSocketMultiplexer('ws://host/__hotsheet/change-mux/ws', 'tab', (url) => {
      const socket = new FakeSocket(url);
      sockets.push(socket);
      return socket as unknown as WebSocket;
    });
    const first = mux.open('a', 'ws://host/p/a/ws/sync');
    sockets[0].open();
    sockets[0].message({ version: 1, type: 'subscribed', project: 'a', subscription: 1 });
    first.close();
    const second = mux.open('a', 'ws://host/p/a/ws/sync');
    const seen: string[] = [];
    second.addEventListener('open', () => seen.push('open'));
    second.addEventListener('message', () => seen.push('event'));
    sockets[1].open();
    sockets[1].message({ version: 1, type: 'subscribed', project: 'a', subscription: 1 });
    sockets[1].message({ version: 1, type: 'event', project: 'a', subscription: 1, data: '{}' });
    expect(seen).toEqual([]);
    sockets[1].message({ version: 1, type: 'subscribed', project: 'a', subscription: 2 });
    sockets[1].message({ version: 1, type: 'event', project: 'a', subscription: 2, data: '{}' });
    expect(seen).toEqual(['open', 'event']);
    second.close();
  });
});
