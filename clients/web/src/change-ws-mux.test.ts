import { describe, expect, it, vi } from 'vitest';

import {
  CHANGE_WS_MUX_PATH,
  ChangeWebSocketMultiplexer,
  changeWsMuxUrl,
  createChangeWebSocketMultiplexers,
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
      parseChangeWsCommand({ version: 2, type: 'subscribe', project: 'a', client: 'tab', subscription: 1 }),
    ).toMatchObject({
      project: 'a',
    });
    expect(
      parseChangeWsCommand({ version: 1, type: 'subscribe', project: 'a', client: 'tab', subscription: 1 }),
    ).toBeUndefined();
    expect(
      parseChangeWsMessage({ version: 2, type: 'event', client: 'tab', project: 'b', subscription: 2, data: '{}' }),
    ).toMatchObject({
      project: 'b',
    });
    expect(
      parseChangeWsMessage({ version: 2, type: 'event', client: 'tab', project: 'b', subscription: 2, data: 4 }),
    ).toBeUndefined();
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
    sockets[0].message({ version: 2, type: 'subscribed', client: 'tab', project: 'b', subscription: 2 });
    sockets[0].message({ version: 2, type: 'subscribed', client: 'tab', project: 'a', subscription: 1 });
    sockets[0].message({
      version: 2,
      type: 'event',
      client: 'tab',
      project: 'b',
      subscription: 2,
      data: '{"cursor":8}',
    });
    sockets[0].message({
      version: 2,
      type: 'event',
      client: 'tab',
      project: 'a',
      subscription: 1,
      data: '{"cursor":3}',
    });
    a.close();
    expect(JSON.parse(sockets[0].sent.at(-1)!).type).toBe('unsubscribe');
    const c = mux.open('c', 'ws://host/p/c/ws/sync');
    c.addEventListener('open', () => seen.push('c:open'));
    sockets[0].message({ version: 2, type: 'subscribed', client: 'tab', project: 'c', subscription: 3 });
    sockets[0].close();
    expect(seen).toEqual(['b:open', 'a:open', 'b:8', 'a:3', 'c:open', 'b:close']);
    const resumed = mux.open('b', 'ws://host/p/b/ws/sync');
    resumed.addEventListener('open', () => seen.push('b:reopen'));
    expect(sockets).toHaveLength(2);
    sockets[1].open();
    sockets[1].message({ version: 2, type: 'subscribed', client: 'tab', project: 'b', subscription: 4 });
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

  it('uses a direct socket when an older bridge accepts the upgrade but rejects protocol v2', () => {
    const sockets: FakeSocket[] = [];
    const mux = new ChangeWebSocketMultiplexer('ws://host/__hotsheet/change-mux/ws', 'tab', (url) => {
      const socket = new FakeSocket(url);
      sockets.push(socket);
      return socket as unknown as WebSocket;
    });
    const project = mux.open('a', 'ws://host/p/a/ws/sync');
    sockets[0].open();
    expect(JSON.parse(sockets[0].sent[0])).toMatchObject({ version: 2, client: 'tab' });
    sockets[0].close();
    expect(sockets.map((socket) => socket.url)).toEqual([
      'ws://host/__hotsheet/change-mux/ws',
      'ws://host/p/a/ws/sync',
    ]);
    sockets[1].open();
    expect(project.readyState).toBe(1);
    project.close();
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
    sockets[0].message({ version: 2, type: 'subscribed', client: 'tab', project: 'a', subscription: 1 });
    first.close();
    const second = mux.open('a', 'ws://host/p/a/ws/sync');
    const seen: string[] = [];
    second.addEventListener('open', () => seen.push('open'));
    second.addEventListener('message', () => seen.push('event'));
    sockets[1].open();
    sockets[1].message({ version: 2, type: 'subscribed', client: 'tab', project: 'a', subscription: 1 });
    sockets[1].message({ version: 2, type: 'event', client: 'tab', project: 'a', subscription: 1, data: '{}' });
    expect(seen).toEqual([]);
    sockets[1].message({ version: 2, type: 'subscribed', client: 'tab', project: 'a', subscription: 2 });
    sockets[1].message({ version: 2, type: 'event', client: 'tab', project: 'a', subscription: 2, data: '{}' });
    expect(seen).toEqual(['open', 'event']);
    second.close();
  });

  it('shares one browser socket across tabs that opened the same project', () => {
    const sockets: FakeSocket[] = [];
    const muxFor = createChangeWebSocketMultiplexers((url) => {
      const socket = new FakeSocket(url);
      sockets.push(socket);
      return socket as unknown as WebSocket;
    });
    const endpoint = 'http://host/__hotsheet/change-mux/poll';
    const tabA = muxFor(endpoint, 'tab-a').open('same', 'ws://host/p/same/ws/sync?client=tab-a', 'tab-a');
    const tabB = muxFor(endpoint, 'tab-b').open('same', 'ws://host/p/same/ws/sync?client=tab-b', 'tab-b');
    expect(sockets).toHaveLength(1);
    const seen: string[] = [];
    tabA.addEventListener('message', () => seen.push('a'));
    tabB.addEventListener('message', () => seen.push('b'));
    sockets[0].open();
    const commands = sockets[0].sent.map((value) => JSON.parse(value) as { client: string; subscription: number });
    expect(commands.map((item) => item.client)).toEqual(['tab-a', 'tab-b']);
    sockets[0].message({
      version: 2,
      type: 'subscribed',
      client: 'tab-a',
      project: 'same',
      subscription: commands[0].subscription,
    });
    sockets[0].message({
      version: 2,
      type: 'subscribed',
      client: 'tab-b',
      project: 'same',
      subscription: commands[1].subscription,
    });
    sockets[0].message({
      version: 2,
      type: 'event',
      client: 'tab-b',
      project: 'same',
      subscription: commands[1].subscription,
      data: '{}',
    });
    sockets[0].message({
      version: 2,
      type: 'event',
      client: 'tab-a',
      project: 'same',
      subscription: commands[0].subscription,
      data: '{}',
    });
    expect(seen).toEqual(['b', 'a']);
    tabA.close();
    expect(JSON.parse(sockets[0].sent.at(-1)!).client).toBe('tab-a');
    expect(tabB.readyState).toBe(1);
    tabB.close();
    expect(sockets[0].readyState).toBe(3);
  });
});

describe('client build announcement on the change bridge (HS2-KPJWBK)', () => {
  it('keeps an announced build on acknowledgements only and tolerates frames without one', () => {
    const base = { version: 2, client: 'tab', project: 'a', subscription: 1 } as const;
    expect(parseChangeWsMessage({ ...base, type: 'subscribed', client_build: 'bundle-a' })).toEqual({
      ...base,
      type: 'subscribed',
      client_build: 'bundle-a',
    });
    expect(parseChangeWsMessage({ ...base, type: 'closed', client_build: 'bundle-a' })).toMatchObject({
      client_build: 'bundle-a',
    });
    // An older host omits it; a malformed value is dropped rather than failing the bridge contract.
    expect(parseChangeWsMessage({ ...base, type: 'subscribed' })).toEqual({ ...base, type: 'subscribed' });
    expect(parseChangeWsMessage({ ...base, type: 'subscribed', client_build: 7 })).toEqual({
      ...base,
      type: 'subscribed',
    });
    expect(parseChangeWsMessage({ ...base, type: 'event', data: '{}', client_build: 'x' })).not.toHaveProperty(
      'client_build',
    );
  });

  it('reports the announced build on every subscription answer, including after a reconnect', () => {
    const sockets: FakeSocket[] = [],
      announced: string[] = [];
    const muxFor = createChangeWebSocketMultiplexers(
      (url) => {
        const socket = new FakeSocket(url);
        sockets.push(socket);
        return socket as unknown as WebSocket;
      },
      (build) => announced.push(build),
    );
    const mux = muxFor('http://host/__hotsheet/change-mux/poll', 'tab');
    const first = mux.open('a', 'ws://host/p/a/ws/sync');
    const opened: string[] = [];
    first.addEventListener('open', () => opened.push('first'));
    sockets[0].open();
    sockets[0].message({
      version: 2,
      type: 'subscribed',
      client: 'tab',
      project: 'a',
      subscription: 1,
      client_build: 'bundle-old',
    });
    // Events never carry it, and an older host's acknowledgement reports nothing.
    sockets[0].message({ version: 2, type: 'event', client: 'tab', project: 'a', subscription: 1, data: '{}' });
    expect(announced).toEqual(['bundle-old']);
    sockets[0].close();
    const resumed = mux.open('a', 'ws://host/p/a/ws/sync');
    resumed.addEventListener('open', () => opened.push('resumed'));
    sockets[1].open();
    sockets[1].message({
      version: 2,
      type: 'subscribed',
      client: 'tab',
      project: 'a',
      subscription: 2,
      client_build: 'bundle-new',
    });
    expect(announced).toEqual(['bundle-old', 'bundle-new']);
    expect(opened).toEqual(['first', 'resumed']);
    // A host whose project session is gone still announces on the closed answer.
    const other = mux.open('b', 'ws://host/p/b/ws/sync');
    sockets[1].message({
      version: 2,
      type: 'closed',
      client: 'tab',
      project: 'b',
      subscription: 3,
      client_build: 'bundle-new',
    });
    expect(announced).toEqual(['bundle-old', 'bundle-new', 'bundle-new']);
    expect(other.readyState).toBe(3);
    resumed.close();
  });
});
