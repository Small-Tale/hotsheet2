import { describe, expect, it, vi } from 'vitest';

import type { ChangeEvent, PollResponse } from './api';
import { type ChangeStreamWorkerLike, createChangeStreamWorkerHost } from './change-stream-worker-host';
import {
  CHANGE_STREAM_WORKER_PROTOCOL,
  type ChangeStreamDelivery,
  ChangeStreamDeliveryQueue,
  type ChangeStreamMainMessage,
  type ChangeStreamWorkerMessage,
  parseChangeStreamMainMessage,
  parseChangeStreamWorkerMessage,
} from './change-stream-worker-protocol';
import { createChangeStreamWorkerRuntime } from './change-stream-worker-runtime';
import type { ProjectChangeSink, ProjectChangeTransportOptions } from './project-change-poll';

const event = (cursor: number | undefined, kind = 'changed', slug = 'HS2-ONE'): ChangeEvent => ({
  store: 'local',
  kind,
  id: '01',
  slug,
  ...(cursor === undefined ? {} : { cursor }),
});
const page = (cursor: number, events: ChangeEvent[] = [], overflow = false): PollResponse => ({
  cursor,
  events,
  overflow,
});

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
  message(value: unknown) {
    const message = new Event('message') as MessageEvent;
    Object.defineProperty(message, 'data', { value: JSON.stringify(value) });
    this.dispatchEvent(message);
  }
  send(value: string) {
    this.sent.push(value);
  }
  close() {
    if (this.readyState === 3) return;
    this.readyState = 3;
    this.dispatchEvent(new Event('close'));
  }
}

/** A controllable stand-in for the real transport: tests push sink calls directly. */
function fakeTransports() {
  const runs: Array<{ options: ProjectChangeTransportOptions; signal: AbortSignal; sink: ProjectChangeSink }> = [];
  const runTransport = (options: ProjectChangeTransportOptions, signal: AbortSignal) => {
    runs.push({ options, signal, sink: options.sink });
  };
  return { runs, runTransport };
}

const batches = (posted: ChangeStreamWorkerMessage[]) =>
  posted.filter(
    (message): message is Extract<ChangeStreamWorkerMessage, { type: 'batch' }> => message.type === 'batch',
  );

const open = (
  project: string,
  generation: number,
  extra: Partial<Extract<ChangeStreamMainMessage, { type: 'open' }>> = {},
) =>
  ({
    type: 'open',
    project,
    generation,
    apiUrl: `http://host/p/${project}`,
    clientId: 'tab-1',
    ...extra,
  }) satisfies ChangeStreamMainMessage;

describe('change-stream worker protocol (HS2-SH9179)', () => {
  it('accepts only well-formed messages in both directions', () => {
    expect(parseChangeStreamMainMessage({ type: 'hello', protocol: 1 })).toEqual({ type: 'hello', protocol: 1 });
    expect(parseChangeStreamMainMessage(open('a', 1, { resume: { cursor: 4 } }))).toMatchObject({
      resume: { cursor: 4 },
    });
    expect(parseChangeStreamMainMessage(open('a', 1, { resume: {} }))).toMatchObject({ resume: {} });
    expect(parseChangeStreamMainMessage(open('a', 1, { changeMuxUrl: 'http://host/m' }))).toMatchObject({
      changeMuxUrl: 'http://host/m',
    });
    for (const bad of [
      null,
      [],
      { type: 'hello' },
      { type: 'open', project: 'a', generation: -1, apiUrl: 'x', clientId: 'c' },
      { type: 'open', project: 'a', generation: 1, apiUrl: 'x' },
      { ...open('a', 1), webSocketUrl: 4 },
      { ...open('a', 1), changeMuxUrl: 4 },
      { ...open('a', 1), resume: 'yes' },
      { ...open('a', 1), resume: { cursor: 1.5 } },
      { type: 'ack', project: 'a', generation: 1 },
      { type: 'nope', project: 'a', generation: 1 },
    ])
      expect(parseChangeStreamMainMessage(bad)).toBeUndefined();
    expect(parseChangeStreamWorkerMessage({ type: 'ready', protocol: 1 })).toEqual({ type: 'ready', protocol: 1 });
    expect(
      parseChangeStreamWorkerMessage({
        type: 'batch',
        project: 'a',
        generation: 1,
        seq: 1,
        kind: 'resync',
        reason: 'initial',
        cursor: 3,
      }),
    ).toMatchObject({ kind: 'resync', cursor: 3 });
    expect(
      parseChangeStreamWorkerMessage({ type: 'error', project: 'a', generation: 1, message: 'boom' }),
    ).toMatchObject({ message: 'boom' });
    for (const bad of [
      { type: 'ready' },
      { type: 'batch', project: 'a', generation: 1, seq: 1, kind: 'resync', reason: 'whenever', cursor: 3 },
      { type: 'batch', project: 'a', generation: 1, seq: 1, kind: 'deliver', response: { cursor: 1 }, force: false },
      { type: 'batch', project: 'a', generation: 1, seq: 1, kind: 'deliver', response: page(1), force: 'no' },
      { type: 'batch', project: 'a', generation: 1, kind: 'deliver', response: page(1), force: false },
      { type: 'error', project: 'a', generation: 1 },
      { type: 'batch', project: 4, generation: 1 },
    ])
      expect(parseChangeStreamWorkerMessage(bad)).toBeUndefined();
  });

  it('keeps one delivery in flight, coalesces adjacent pages, and ignores stale or repeated acks', () => {
    const sent: Array<[number, ChangeStreamDelivery]> = [];
    const queue = new ChangeStreamDeliveryQueue((seq, delivery) => sent.push([seq, delivery]));
    queue.push({ kind: 'resync', reason: 'initial', cursor: 1 });
    queue.push({ kind: 'deliver', response: page(2, [event(2)]), force: false });
    queue.push({ kind: 'deliver', response: page(3, [event(3, 'activity')]), force: false });
    queue.push({ kind: 'deliver', response: page(4, [], true), force: true });
    expect(sent).toHaveLength(1);
    expect(queue.size).toBe(2);
    queue.ack(7); // unknown seq
    expect(sent).toHaveLength(1);
    queue.ack(1);
    expect(sent[1]).toEqual([
      2,
      {
        kind: 'deliver',
        response: { cursor: 4, overflow: true, events: [event(2), event(3, 'activity')] },
        force: true,
      },
    ]);
    queue.ack(1); // repeated
    expect(sent).toHaveLength(2);
    // A resync is an ordering barrier: pages on either side never merge across it.
    queue.push({ kind: 'deliver', response: page(5, [event(5)]), force: false });
    queue.push({ kind: 'resync', reason: 'reconnect', cursor: 9 });
    queue.push({ kind: 'deliver', response: page(9, [event(undefined, 'announce')]), force: true });
    queue.ack(2);
    queue.ack(3);
    queue.ack(4);
    expect(sent.slice(2).map(([seq, delivery]) => [seq, delivery.kind])).toEqual([
      [3, 'deliver'],
      [4, 'resync'],
      [5, 'deliver'],
    ]);
    queue.ack(5);
    expect(queue.size).toBe(0);
  });

  it('collapses an unbounded backlog into one overflow page with the newest cursor', () => {
    const sent: ChangeStreamDelivery[] = [];
    const queue = new ChangeStreamDeliveryQueue((_seq, delivery) => sent.push(delivery), 3);
    queue.push({ kind: 'deliver', response: page(1, [event(1)]), force: false });
    queue.push({ kind: 'resync', reason: 'overflow', cursor: 1 });
    queue.push({ kind: 'deliver', response: page(3, [event(2), event(3)]), force: true });
    queue.push({ kind: 'deliver', response: page(5, [event(4), event(5)]), force: false });
    queue.ack(1);
    expect(sent[1]).toEqual({ kind: 'deliver', response: { cursor: 5, events: [], overflow: true }, force: true });
    expect(queue.size).toBe(1);
  });
});

describe('change-stream worker runtime (HS2-SH9179)', () => {
  it('answers the version handshake and opens one fair transport per project', async () => {
    const posted: ChangeStreamWorkerMessage[] = [];
    const { runs, runTransport } = fakeTransports();
    const runtime = createChangeStreamWorkerRuntime({ post: (message) => posted.push(message), runTransport });
    runtime.handle({ type: 'hello', protocol: 99 });
    expect(posted[0]).toEqual({ type: 'ready', protocol: CHANGE_STREAM_WORKER_PROTOCOL });
    runtime.handle(open('a', 1));
    runtime.handle(open('b', 2, { resume: { cursor: 8 } }));
    runtime.handle('garbage');
    expect(runtime.projects()).toEqual(['a', 'b']);
    expect(runs[1].options.resume).toEqual({ cursor: 8 });
    // A burst on project a, never acknowledged, does not block project b.
    for (let cursor = 1; cursor <= 5; cursor += 1) await runs[0].sink.deliver(page(cursor, [event(cursor)]), false);
    await runs[1].sink.resync('reconnect', 8);
    expect(batches(posted).map((batch) => [batch.project, batch.seq])).toEqual([
      ['a', 1],
      ['b', 1],
    ]);
    runtime.handle({ type: 'ack', project: 'a', generation: 1, seq: 1 });
    const coalesced = batches(posted).at(-1)!;
    expect(coalesced).toMatchObject({ project: 'a', seq: 2, kind: 'deliver' });
    expect(coalesced.kind === 'deliver' && coalesced.response.events.map((item) => item.cursor)).toEqual([2, 3, 4, 5]);
    runs[0].options.onError?.(new Error('socket gap'));
    expect(posted.at(-1)).toEqual({ type: 'error', project: 'a', generation: 1, message: 'socket gap' });
  });

  it('cancels on close, ignores stale generations, and replaces a project on a newer open', async () => {
    const posted: ChangeStreamWorkerMessage[] = [];
    const { runs, runTransport } = fakeTransports();
    const runtime = createChangeStreamWorkerRuntime({ post: (message) => posted.push(message), runTransport });
    runtime.handle(open('a', 3));
    runtime.handle(open('a', 2)); // stale reopen
    expect(runs).toHaveLength(1);
    runtime.handle({ type: 'close', project: 'a', generation: 2 }); // stale close
    expect(runs[0].signal.aborted).toBe(false);
    runtime.handle(open('a', 4));
    expect(runs[0].signal.aborted).toBe(true);
    await runs[0].sink.deliver(page(1, [event(1)]), false);
    runs[0].options.onError?.('late');
    expect(posted).toEqual([]);
    await runs[1].sink.deliver(page(1, [event(1)]), false);
    runtime.handle({ type: 'ack', project: 'a', generation: 3, seq: 1 }); // stale ack
    await runs[1].sink.deliver(page(2, [event(2)]), false);
    expect(batches(posted)).toHaveLength(1);
    runtime.handle({ type: 'close', project: 'a', generation: 4 });
    expect(runs[1].signal.aborted).toBe(true);
    expect(runtime.projects()).toEqual([]);
    runtime.handle(open('b', 5));
    runtime.dispose();
    expect(runs[2].signal.aborted).toBe(true);
  });

  it('runs the real transport: handshake, repeats, out-of-order and gapped cursors, reconnect replay', async () => {
    const posted: ChangeStreamWorkerMessage[] = [];
    const sockets: FakeSocket[] = [];
    const polls: string[] = [];
    const pages = [
      page(10),
      page(10),
      page(12, [event(11), event(12)]),
      // HS2-BX4MCH: the fallback keeps long-polling its retry window after an event page.
      page(12),
      page(14, [event(13), event(14)]),
    ];
    const fetcher = vi.fn(async (url: string | URL | Request) => {
      polls.push(url instanceof Request ? url.url : url.toString());
      const next = pages.shift();
      if (!next) return new Promise<Response>(() => undefined);
      return new Response(JSON.stringify(next), { status: 200 });
    }) as unknown as typeof fetch;
    const runtime = createChangeStreamWorkerRuntime({
      post: (message) => {
        posted.push(message);
        if (message.type === 'batch')
          queueMicrotask(() => {
            runtime.handle({ type: 'ack', project: message.project, generation: message.generation, seq: message.seq });
          });
      },
      fetch: fetcher,
      openWebSocket: (url) => {
        const socket = new FakeSocket(url);
        sockets.push(socket);
        return socket as unknown as WebSocket;
      },
    });
    runtime.handle(open('a', 1, { webSocketUrl: 'ws://host/p/a/ws/sync' }));
    await vi.waitFor(() => {
      expect(sockets).toHaveLength(1);
    });
    expect(polls[0]).toBe('http://host/p/a/ws/poll?timeout_ms=25000&client=tab-1');
    sockets[0].open();
    await vi.waitFor(() => {
      expect(polls).toHaveLength(2);
    });
    expect(polls[1]).toContain('timeout_ms=0&since=10');
    sockets[0].message(event(11)); // live
    sockets[0].message(event(11)); // repeat
    sockets[0].message(event(9)); // out of order (already replayed)
    sockets[0].message(event(undefined, 'announce'));
    sockets[0].message(event(13)); // gap: 12 missing
    await vi.waitFor(() => {
      expect(polls.length).toBeGreaterThanOrEqual(3);
    });
    // The gap closes the socket and replays from the last applied cursor.
    expect(polls[2]).toContain('since=11');
    await vi.waitFor(() => {
      expect(sockets).toHaveLength(2);
    });
    expect(polls[3]).toContain('since=12');
    expect(polls[3]).not.toContain('timeout_ms=0&');
    const delivered = batches(posted).flatMap((batch) =>
      batch.kind === 'deliver'
        ? batch.response.events.map((item) => item.cursor ?? item.kind)
        : [`resync:${batch.reason}`],
    );
    expect(delivered).toEqual(['resync:initial', 11, 'announce', 11, 12]);
    expect(posted.some((message) => message.type === 'error' && message.message.includes('jumped from 11 to 13'))).toBe(
      true,
    );
    sockets[1].open();
    await vi.waitFor(() => {
      expect(polls).toHaveLength(5);
    });
    expect(polls[4]).toContain('timeout_ms=0&since=12');
    runtime.dispose();
    expect(sockets[1].readyState).toBe(3);
  });

  it('shares one multiplexed long-poll across projects whose bridge advertises it (HS2-6W1NNF)', async () => {
    const posted: ChangeStreamWorkerMessage[] = [];
    const requests: Array<{ url: string; body: { client: string; polls: Array<{ project: string }> } }> = [];
    const fetcher = ((url: string, init?: RequestInit) => {
      if (!url.endsWith('/__hotsheet/change-mux/poll')) return Promise.reject(new Error(`direct poll ${url}`));
      const body = JSON.parse(init!.body as string) as (typeof requests)[number]['body'];
      requests.push({ url, body });
      if (requests.length > 1) return new Promise<Response>(() => undefined);
      return Promise.resolve(
        Response.json({
          version: 1,
          results: body.polls.map((poll, index) => ({ project: poll.project, status: 200, body: page(index + 1) })),
        }),
      );
    }) as unknown as typeof fetch;
    const runtime = createChangeStreamWorkerRuntime({ post: (message) => posted.push(message), fetch: fetcher });
    const mux = { changeMuxUrl: 'http://host/__hotsheet/change-mux/poll' };
    runtime.handle(open('a', 1, { apiUrl: 'http://host/__hotsheet/project-api/a', ...mux }));
    runtime.handle(open('b', 1, { apiUrl: 'http://host/__hotsheet/project-api/b', ...mux }));
    await vi.waitFor(() => {
      expect(batches(posted)).toHaveLength(2);
    }, 3_000);
    expect(requests[0].body).toMatchObject({ client: 'tab-1', polls: [{ project: 'a' }, { project: 'b' }] });
    expect(
      batches(posted).map((batch) => [batch.project, batch.kind, 'cursor' in batch ? batch.cursor : undefined]),
    ).toEqual([
      ['a', 'resync', 1],
      ['b', 'resync', 2],
    ]);
    // Both live long-polls then wait in one shared request.
    await vi.waitFor(() => {
      expect(requests).toHaveLength(2);
    });
    expect(requests[1].body.polls.map((poll) => poll.project)).toEqual(['a', 'b']);
    runtime.dispose();
  });

  it('opens one change bridge socket for two project transports and keeps their events isolated (HS2-QJR1K6)', () => {
    const { runs, runTransport } = fakeTransports();
    const sockets: FakeSocket[] = [];
    const runtime = createChangeStreamWorkerRuntime({
      post: () => undefined,
      runTransport,
      openWebSocket: (url) => {
        const socket = new FakeSocket(url);
        sockets.push(socket);
        return socket as unknown as WebSocket;
      },
    });
    const mux = { changeMuxUrl: 'http://host/__hotsheet/change-mux/poll' };
    runtime.handle(open('a', 1, { ...mux, webSocketUrl: 'ws://host/p/a/ws/sync', resume: { cursor: 7 } }));
    runtime.handle(open('b', 1, { ...mux, webSocketUrl: 'ws://host/p/b/ws/sync', resume: { cursor: 19 } }));
    expect(runs.map((run) => run.options.resume?.cursor)).toEqual([7, 19]);
    const a = runs[0].options.openWebSocket!('ws://host/p/a/ws/sync');
    const b = runs[1].options.openWebSocket!('ws://host/p/b/ws/sync');
    expect(sockets.map((socket) => socket.url)).toEqual(['ws://host/__hotsheet/change-mux/ws']);
    const seen: string[] = [];
    a.addEventListener('message', (event) => seen.push(`a:${JSON.parse(event.data as string).cursor}`));
    b.addEventListener('message', (event) => seen.push(`b:${JSON.parse(event.data as string).cursor}`));
    sockets[0].open();
    sockets[0].message({ version: 2, type: 'subscribed', client: 'tab-1', project: 'a', subscription: 1 });
    sockets[0].message({ version: 2, type: 'subscribed', client: 'tab-1', project: 'b', subscription: 2 });
    sockets[0].message({
      version: 2,
      type: 'event',
      client: 'tab-1',
      project: 'b',
      subscription: 2,
      data: '{"cursor":20}',
    });
    sockets[0].message({
      version: 2,
      type: 'event',
      client: 'tab-1',
      project: 'a',
      subscription: 1,
      data: '{"cursor":8}',
    });
    expect(seen).toEqual(['b:20', 'a:8']);
    runtime.handle({ type: 'close', project: 'a', generation: 1 });
    a.close();
    expect(b.readyState).toBe(1);
    runtime.dispose();
  });

  it('keeps a second project live when one multiplexed project overflows its delivery queue (HS2-QJR1K6)', async () => {
    const posted: ChangeStreamWorkerMessage[] = [];
    const { runs, runTransport } = fakeTransports();
    const runtime = createChangeStreamWorkerRuntime({
      post: (message) => posted.push(message),
      runTransport,
      maxQueuedEvents: 2,
    });
    const mux = { changeMuxUrl: 'http://host/__hotsheet/change-mux/poll' };
    runtime.handle(open('a', 1, mux));
    runtime.handle(open('b', 1, mux));
    await runs[0].sink.deliver(page(1, [event(1)]), false);
    await runs[0].sink.deliver(page(2, [event(2)]), false);
    await runs[0].sink.deliver(page(3, [event(3)]), false);
    await runs[0].sink.deliver(page(4, [event(4)]), false);
    await runs[1].sink.deliver(page(20, [event(20, 'changed', 'HS2-B')]), false);
    expect(batches(posted).map((batch) => batch.project)).toEqual(['a', 'b']);
    runtime.handle({ type: 'ack', project: 'a', generation: 1, seq: 1 });
    expect(batches(posted).at(-1)).toMatchObject({
      project: 'a',
      kind: 'deliver',
      response: { cursor: 4, events: [], overflow: true },
    });
    expect(batches(posted)[1]).toMatchObject({ project: 'b', response: { cursor: 20, overflow: false } });
    runtime.dispose();
  });

  it('turns a failed poll into a reconnect resync without a tight request loop', async () => {
    const posted: ChangeStreamWorkerMessage[] = [];
    let calls = 0;
    const fetcher = (async () => {
      calls += 1;
      if (calls === 1) return new Response('{}', { status: 503 });
      if (calls === 2) return new Response(JSON.stringify(page(3)), { status: 200 });
      return new Promise<Response>(() => undefined);
    }) as unknown as typeof fetch;
    const runtime = createChangeStreamWorkerRuntime({ post: (message) => posted.push(message), fetch: fetcher });
    runtime.handle(open('a', 1));
    await vi.waitFor(() => {
      expect(batches(posted)).toHaveLength(1);
    }, 3_000);
    expect(batches(posted)[0]).toMatchObject({ kind: 'resync', reason: 'reconnect', cursor: 3 });
    expect(posted[0]).toMatchObject({ type: 'error', message: 'Change poll failed with 503.' });
    expect(calls).toBe(3);
    runtime.dispose();
  });
});

/** An in-memory worker: the host talks to a real runtime driven by controllable fake transports. */
function inMemoryWorker(runTransport: ReturnType<typeof fakeTransports>['runTransport'], protocol?: number) {
  const listeners = new Map<string, Array<(event: Event) => void>>();
  const received: ChangeStreamMainMessage[] = [];
  let terminated = false;
  const emit = (type: string, data?: unknown) => {
    const message = new Event(type) as MessageEvent;
    Object.defineProperty(message, 'data', { value: data });
    for (const listener of listeners.get(type) ?? []) listener(message);
  };
  const runtime = createChangeStreamWorkerRuntime({
    post: (message) => {
      if (terminated) return;
      queueMicrotask(() => {
        emit('message', protocol !== undefined && message.type === 'ready' ? { ...message, protocol } : message);
      });
    },
    runTransport,
  });
  const worker: ChangeStreamWorkerLike = {
    postMessage: (message) => {
      received.push(message);
      if (!terminated) runtime.handle(structuredClone(message));
    },
    addEventListener: (type, listener) => {
      listeners.set(type, [...(listeners.get(type) ?? []), listener]);
    },
    terminate: () => {
      terminated = true;
      runtime.dispose();
    },
  };
  return {
    worker,
    received,
    crash: () => {
      emit('error');
    },
    isTerminated: () => terminated,
  };
}

const consumerOptions = () => {
  const log: string[] = [];
  return {
    log,
    options: {
      client: { pollEvents: vi.fn() },
      refresh: vi.fn(async () => {
        log.push('refresh');
      }),
      onResync: vi.fn((reason: string) => {
        log.push(`resync:${reason}`);
      }),
      onEvents: vi.fn(async (response: PollResponse) => {
        log.push(`events:${response.events.map((item) => item.cursor).join(',')}`);
      }),
      onError: vi.fn(),
    },
  };
};

describe('change-stream worker host (HS2-SH9179)', () => {
  it('applies deliveries in order per project through the main-thread consumer and acknowledges them', async () => {
    const transports = fakeTransports();
    const memory = inMemoryWorker(transports.runTransport);
    const host = createChangeStreamWorkerHost({ createWorker: () => memory.worker, clientId: 'tab-1' });
    const a = consumerOptions(),
      b = consumerOptions();
    host.start('a', a.options, { apiUrl: 'http://host/p/a', webSocketUrl: 'ws://host/p/a/ws/sync' });
    host.start('b', b.options, { apiUrl: 'http://host/p/b' });
    expect(memory.received.map((message) => message.type)).toEqual(['hello', 'open', 'open']);
    await transports.runs[0].sink.resync('initial', 4);
    await transports.runs[0].sink.deliver(page(5, [event(5)]), false);
    await transports.runs[0].sink.deliver(page(6, [event(6, 'activity')]), false);
    await transports.runs[1].sink.deliver(page(2, [event(2)], true), false);
    await vi.waitFor(() => {
      expect(a.log).toEqual(['resync:initial', 'events:5,6', 'refresh']);
    });
    expect(b.log).toEqual(['resync:overflow', 'events:2', 'refresh']);
    transports.runs[0].options.onError?.(new Error('lost'));
    await vi.waitFor(() => {
      expect(a.options.onError).toHaveBeenCalled();
    });
    expect(host.mode()).toBe('worker');
    host.dispose();
    expect(memory.isTerminated()).toBe(true);
  });

  it('stops a project: closes it in the worker and drops late deliveries', async () => {
    const transports = fakeTransports();
    const memory = inMemoryWorker(transports.runTransport);
    const host = createChangeStreamWorkerHost({ createWorker: () => memory.worker, clientId: 'tab-1' });
    const a = consumerOptions();
    const stop = host.start('a', a.options, { apiUrl: 'http://host/p/a' });
    stop();
    stop();
    expect(memory.received.at(-1)).toMatchObject({ type: 'close', project: 'a' });
    expect(transports.runs[0].signal.aborted).toBe(true);
    await transports.runs[0].sink.deliver(page(1, [event(1)]), false);
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(a.log).toEqual([]);
    host.dispose();
  });

  it('restarts a crashed worker and replays every project from its last applied cursor', async () => {
    const transports = fakeTransports();
    const workers: Array<ReturnType<typeof inMemoryWorker>> = [];
    const host = createChangeStreamWorkerHost({
      createWorker: () => {
        workers.push(inMemoryWorker(transports.runTransport));
        return workers.at(-1)!.worker;
      },
      clientId: 'tab-1',
      restartDelayMs: 5,
    });
    const a = consumerOptions(),
      b = consumerOptions();
    host.start('a', a.options, { apiUrl: 'http://host/p/a' });
    host.start('b', b.options, { apiUrl: 'http://host/p/b' });
    await transports.runs[0].sink.resync('initial', 4);
    await transports.runs[0].sink.deliver(page(6, [event(5), event(6)]), false);
    await vi.waitFor(() => {
      expect(a.log).toContain('events:5,6');
    });
    const generation = host.generation('a');
    workers[0].crash();
    expect(workers[0].isTerminated()).toBe(true);
    await vi.waitFor(() => {
      expect(workers).toHaveLength(2);
    });
    const reopened = workers[1].received.filter((message) => message.type === 'open');
    expect(reopened).toEqual([
      expect.objectContaining({ project: 'a', resume: { cursor: 6 } }),
      // b never applied anything: it reconciles with a fresh handshake as a reconnect.
      expect.objectContaining({ project: 'b', resume: {} }),
    ]);
    expect(host.generation('a')).toBeGreaterThan(generation!);
    // Replay overlaps what was applied before the crash; the repeated events are dropped.
    await transports.runs[2].sink.deliver(page(7, [event(6), event(7)]), false);
    await vi.waitFor(() => {
      expect(a.log).toContain('events:7');
    });
    expect(a.log.filter((line) => line.includes('6'))).toEqual(['events:5,6']);
    // A message from the old generation (crashed worker) is ignored.
    await transports.runs[0].sink.deliver(page(9, [event(9)]), false);
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(a.log.some((line) => line.includes('9'))).toBe(false);
    host.dispose();
  });

  it('falls back to the main-thread stream on crash loops, version skew, a missing worker, or no ready', async () => {
    const startFallback = vi.fn(() => vi.fn());
    const transports = fakeTransports();
    const workers: Array<ReturnType<typeof inMemoryWorker>> = [];
    let now = 0;
    const modes: string[] = [];
    const looping = createChangeStreamWorkerHost({
      createWorker: () => {
        workers.push(inMemoryWorker(transports.runTransport));
        return workers.at(-1)!.worker;
      },
      clientId: 'tab-1',
      restartDelayMs: 1,
      maxRestarts: 1,
      startFallback,
      now: () => now,
      onModeChange: (mode) => modes.push(mode),
    });
    const a = consumerOptions();
    looping.start('a', a.options, { apiUrl: 'http://host/p/a' });
    await transports.runs[0].sink.resync('initial', 3);
    await vi.waitFor(() => {
      expect(a.log).toEqual(['resync:initial']);
    });
    workers[0].crash();
    await vi.waitFor(() => {
      expect(workers).toHaveLength(2);
    });
    now = 10;
    workers[1].crash();
    await vi.waitFor(() => {
      expect(startFallback).toHaveBeenCalledTimes(1);
    });
    expect(startFallback.mock.calls[0]).toEqual([expect.objectContaining({ resume: { cursor: 3 } })]);
    expect(looping.mode()).toBe('fallback');
    expect(modes).toEqual(['fallback']);
    // Later projects open directly on the main thread.
    looping.start('b', consumerOptions().options, { apiUrl: 'http://host/p/b' });
    expect(startFallback).toHaveBeenCalledTimes(2);
    looping.dispose();

    const skewed = inMemoryWorker(fakeTransports().runTransport, CHANGE_STREAM_WORKER_PROTOCOL + 1);
    const skewHost = createChangeStreamWorkerHost({ createWorker: () => skewed.worker, clientId: 't', startFallback });
    skewHost.start('a', consumerOptions().options, { apiUrl: 'http://host/p/a' });
    await vi.waitFor(() => {
      expect(skewHost.mode()).toBe('fallback');
    });
    expect(skewed.isTerminated()).toBe(true);
    skewHost.dispose();

    const missing = createChangeStreamWorkerHost({
      createWorker: () => {
        throw new Error('no workers');
      },
      clientId: 't',
      startFallback,
    });
    missing.start('a', consumerOptions().options, { apiUrl: 'http://host/p/a' });
    await vi.waitFor(() => {
      expect(missing.mode()).toBe('fallback');
    });
    missing.dispose();

    const terminate = vi.fn();
    const silent: ChangeStreamWorkerLike = { postMessage: vi.fn(), addEventListener: vi.fn(), terminate };
    const silentHost = createChangeStreamWorkerHost({
      createWorker: () => silent,
      clientId: 't',
      readyTimeoutMs: 5,
      startFallback,
    });
    silentHost.start('a', consumerOptions().options, { apiUrl: 'http://host/p/a' });
    await vi.waitFor(() => {
      expect(silentHost.mode()).toBe('fallback');
    });
    expect(terminate).toHaveBeenCalled();
    silentHost.dispose();
  });
});
