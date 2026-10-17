import { describe, expect, it, vi } from 'vitest';

import type { PollResponse } from './api';
import { CHANGE_MUX_MAX_POLLS } from './change-mux-bridge';
import {
  changeMuxTarget,
  type ChangePollFn,
  ChangePollMultiplexer,
  createChangePollMultiplexers,
} from './change-poll-mux';
import { type ProjectChangeSink, runProjectChangePollTransport } from './project-change-poll';
import { parsePollResponse } from './server-response-parsers';

interface MuxCall {
  body: {
    version: number;
    client: string;
    polls: Array<{ project: string; client?: string; since?: number; timeout_ms: number }>;
  };
  signal: AbortSignal;
  respond(results: Array<{ project: string; client?: string; status?: number; body?: unknown }>): void;
  respondRaw(response: Response): void;
  fail(reason: unknown): void;
}

const page = (cursor: number, kinds: string[] = []): PollResponse => ({
  cursor,
  overflow: false,
  events: kinds.map((kind, index) => ({ kind, store: 's', id: `t${index}`, slug: `T-${index}` })),
});

/** A fake bridge whose answers the test controls one request at a time. */
function fakeBridge() {
  const calls: MuxCall[] = [];
  const fetcher = vi.fn((_input: RequestInfo | URL, init?: RequestInit) => {
    const signal = init!.signal!;
    const body = JSON.parse(init!.body as string) as MuxCall['body'];
    return new Promise<Response>((resolve, reject) => {
      signal.addEventListener('abort', () => {
        const error = new Error('aborted');
        error.name = 'AbortError';
        reject(error);
      });
      calls.push({
        body,
        signal,
        respond: (results) => {
          resolve(
            Response.json({
              version: body.version,
              results: results.map((result) => ({
                project: result.project,
                ...(result.client === undefined ? {} : { client: result.client }),
                status: result.status ?? 200,
                body: result.body ?? page(1),
              })),
            }),
          );
        },
        respondRaw: resolve,
        fail: reject,
      });
    });
  });
  return { calls, fetcher: fetcher as unknown as typeof fetch };
}

const tick = (ms = 2) => new Promise((resolve) => setTimeout(resolve, ms));

function mux(fetcher: typeof fetch, extra: Partial<ConstructorParameters<typeof ChangePollMultiplexer>[0]> = {}) {
  return new ChangePollMultiplexer({
    endpoint: '/__hotsheet/change-mux/poll',
    clientId: 'tab-1',
    parse: parsePollResponse,
    fetch: fetcher,
    coalesceMs: 0,
    now: () => 0,
    ...extra,
  });
}

const neverDirect: ChangePollFn = () => Promise.reject(new Error('direct poll used'));

describe('ChangePollMultiplexer', () => {
  it('shares one request for two tabs on the same project and routes answers by client', async () => {
    const bridge = fakeBridge();
    const m = mux(bridge.fetcher, { sharedAcrossClients: true });
    const a = m.poll('same', neverDirect, 1, undefined, 25_000, 'tab-a');
    const b = m.poll('same', neverDirect, 2, undefined, 25_000, 'tab-b');
    await tick();
    expect(bridge.calls).toHaveLength(1);
    expect(bridge.calls[0].body).toMatchObject({
      version: 2,
      polls: [
        { project: 'same', client: 'tab-a', since: 1 },
        { project: 'same', client: 'tab-b', since: 2 },
      ],
    });
    bridge.calls[0].respond([
      { project: 'same', client: 'tab-b', body: page(3) },
      { project: 'same', client: 'tab-a', body: page(4) },
    ]);
    await expect(a).resolves.toEqual(page(4));
    await expect(b).resolves.toEqual(page(3));
  });

  it('keeps the other tab polling when one tab closes the same project', async () => {
    const bridge = fakeBridge();
    const m = mux(bridge.fetcher, { sharedAcrossClients: true });
    const closing = new AbortController();
    const a = m.poll('same', neverDirect, 1, closing.signal, 25_000, 'tab-a');
    const b = m.poll('same', neverDirect, 2, undefined, 25_000, 'tab-b');
    await tick();
    closing.abort();
    await expect(a).rejects.toMatchObject({ name: 'AbortError' });
    await tick();
    expect(bridge.calls[0].signal.aborted).toBe(true);
    expect(bridge.calls[1].body.polls).toEqual([{ project: 'same', client: 'tab-b', since: 2, timeout_ms: 25_000 }]);
    bridge.calls[1].respond([{ project: 'same', client: 'tab-b', body: page(3) }]);
    await expect(b).resolves.toEqual(page(3));
  });
  it('coalesces concurrent project polls into one request and fans results out by project', async () => {
    const bridge = fakeBridge();
    const m = mux(bridge.fetcher);
    const a = m.poll('a', neverDirect, 3, undefined, 25_000);
    const b = m.poll('b', neverDirect, undefined, undefined, 0);
    await tick();
    expect(bridge.calls).toHaveLength(1);
    expect(bridge.calls[0].body).toMatchObject({
      version: 1,
      client: 'tab-1',
      polls: [
        { project: 'a', since: 3 },
        { project: 'b', timeout_ms: 0 },
      ],
    });
    expect(bridge.calls[0].body.polls[1]).not.toHaveProperty('since');
    bridge.calls[0].respond([
      { project: 'a', body: page(4, ['updated']) },
      { project: 'b', body: page(9) },
    ]);
    await expect(a).resolves.toEqual(page(4, ['updated']));
    await expect(b).resolves.toEqual(page(9));
    expect(m.requests).toBe(1);
  });

  it('keeps unanswered projects pending and reissues them with their remaining deadline', async () => {
    const bridge = fakeBridge();
    let clock = 1_000;
    const m = mux(bridge.fetcher, { now: () => clock });
    const a = m.poll('a', neverDirect, 1, undefined, 25_000);
    void m.poll('b', neverDirect, 2, undefined, 25_000);
    await tick();
    clock += 10_000;
    bridge.calls[0].respond([{ project: 'a', body: page(2, ['created']) }]);
    await expect(a).resolves.toEqual(page(2, ['created']));
    await tick();
    expect(bridge.calls).toHaveLength(2);
    expect(bridge.calls[1].body.polls).toEqual([{ project: 'b', since: 2, timeout_ms: 15_000 }]);
    expect(m.waiting()).toEqual(['b']);
  });

  it('aborts and reissues the in-flight request when another project joins', async () => {
    const bridge = fakeBridge();
    const m = mux(bridge.fetcher);
    const a = m.poll('a', neverDirect, 1);
    await tick();
    const b = m.poll('b', neverDirect, 5);
    await tick();
    expect(bridge.calls).toHaveLength(2);
    expect(bridge.calls[0].signal.aborted).toBe(true);
    expect(bridge.calls[1].body.polls.map((poll) => poll.project)).toEqual(['a', 'b']);
    bridge.calls[1].respond([
      { project: 'a', body: page(2) },
      { project: 'b', body: page(6) },
    ]);
    await expect(a).resolves.toEqual(page(2));
    await expect(b).resolves.toEqual(page(6));
  });

  it('drops a project closed mid-flight without disturbing the others', async () => {
    const bridge = fakeBridge();
    const m = mux(bridge.fetcher);
    const closing = new AbortController();
    const a = m.poll('a', neverDirect, 1, closing.signal);
    const b = m.poll('b', neverDirect, 1);
    await tick();
    closing.abort();
    await expect(a).rejects.toMatchObject({ name: 'AbortError' });
    await tick();
    expect(bridge.calls[0].signal.aborted).toBe(true);
    expect(bridge.calls[1].body.polls.map((poll) => poll.project)).toEqual(['b']);
    bridge.calls[1].respond([{ project: 'b', body: page(3) }]);
    await expect(b).resolves.toEqual(page(3));
  });

  it('closing the only project cancels the request and sends nothing more', async () => {
    const bridge = fakeBridge();
    const m = mux(bridge.fetcher);
    const closing = new AbortController();
    const a = m.poll('a', neverDirect, 1, closing.signal);
    await tick();
    closing.abort();
    await expect(a).rejects.toMatchObject({ name: 'AbortError' });
    await tick(10);
    expect(bridge.calls).toHaveLength(1);
    expect(bridge.calls[0].signal.aborted).toBe(true);
  });

  it('an already-aborted poll never reaches the bridge', async () => {
    const bridge = fakeBridge();
    const m = mux(bridge.fetcher);
    const closed = new AbortController();
    closed.abort();
    await expect(m.poll('a', neverDirect, 1, closed.signal)).rejects.toMatchObject({ name: 'AbortError' });
    await tick();
    expect(bridge.calls).toHaveLength(0);
  });

  it('a newer poll for the same project supersedes the older one', async () => {
    const bridge = fakeBridge();
    const m = mux(bridge.fetcher);
    const first = m.poll('a', neverDirect, 1);
    const second = m.poll('a', neverDirect, 2);
    await expect(first).rejects.toThrow('Superseded');
    await tick();
    expect(bridge.calls[0].body.polls).toEqual([{ project: 'a', since: 2, timeout_ms: 25_000 }]);
    bridge.calls[0].respond([{ project: 'a', body: page(3) }]);
    await expect(second).resolves.toEqual(page(3));
  });

  it('rejects only the project whose upstream poll failed', async () => {
    const bridge = fakeBridge();
    const m = mux(bridge.fetcher);
    const a = m.poll('a', neverDirect, 1);
    const b = m.poll('b', neverDirect, 1);
    await tick();
    bridge.calls[0].respond([
      { project: 'a', status: 404, body: { error: 'Project session is not open.' } },
      { project: 'b', body: page(2) },
    ]);
    await expect(a).rejects.toThrow('Change poll failed with 404.');
    await expect(b).resolves.toEqual(page(2));
    expect(m.mode).toBe('mux');
  });

  it('rejects a project whose answer is not a valid poll page', async () => {
    const bridge = fakeBridge();
    const m = mux(bridge.fetcher);
    const a = m.poll('a', neverDirect, 1);
    await tick();
    bridge.calls[0].respond([{ project: 'a', body: { cursor: 'nope' } }]);
    await expect(a).rejects.toThrow();
  });

  it('a failed request rejects every project it carried, and the next polls recover', async () => {
    const bridge = fakeBridge();
    const m = mux(bridge.fetcher);
    const a = m.poll('a', neverDirect, 1);
    const b = m.poll('b', neverDirect, 1);
    await tick();
    bridge.calls[0].fail(new TypeError('Failed to fetch'));
    await expect(a).rejects.toThrow('Failed to fetch');
    await expect(b).rejects.toThrow('Failed to fetch');
    await tick(10);
    expect(bridge.calls).toHaveLength(1);
    const retried = m.poll('a', neverDirect, 1);
    await tick();
    bridge.calls[1].respond([{ project: 'a', body: page(2) }]);
    await expect(retried).resolves.toEqual(page(2));
    expect(m.mode).toBe('mux');
  });

  it('a 5xx answer rejects the carried projects but keeps multiplexing', async () => {
    const bridge = fakeBridge();
    const m = mux(bridge.fetcher);
    const a = m.poll('a', neverDirect, 1);
    await tick();
    bridge.calls[0].respondRaw(new Response('upstream', { status: 503 }));
    await expect(a).rejects.toThrow('Change multiplex failed with 503.');
    expect(m.mode).toBe('mux');
  });

  it.each([404, 405, 400, 501])(
    'an older bridge answering %i switches every project to direct polls',
    async (status) => {
      const bridge = fakeBridge();
      const onModeChange = vi.fn();
      const m = mux(bridge.fetcher, { onModeChange });
      const direct = vi.fn((since?: number) => Promise.resolve(page((since ?? 0) + 10)));
      const a = m.poll('a', direct, 1);
      const b = m.poll('b', direct, 2);
      await tick();
      bridge.calls[0].respondRaw(new Response('Not Found', { status }));
      await expect(a).resolves.toEqual(page(11));
      await expect(b).resolves.toEqual(page(12));
      expect(m.mode).toBe('direct');
      expect(onModeChange).toHaveBeenCalledWith('direct', expect.stringContaining(String(status)));
      // Later polls go straight to the project, never back to the bridge.
      await expect(m.poll('a', direct, 11)).resolves.toEqual(page(21));
      await tick();
      expect(bridge.calls).toHaveLength(1);
    },
  );

  it.each([
    ['a non-multiplex JSON body', () => Response.json({ cursor: 0, events: [], overflow: false })],
    ['another wire version', () => Response.json({ version: 2, results: [] })],
    ['an empty result list', () => Response.json({ version: 1, results: [] })],
    ['invalid JSON', () => new Response('<html>', { status: 200 })],
  ])('%s switches to direct polls rather than looping', async (_name, response) => {
    const bridge = fakeBridge();
    const m = mux(bridge.fetcher);
    const direct = vi.fn(() => Promise.resolve(page(7)));
    const a = m.poll('a', direct, 1);
    await tick();
    bridge.calls[0].respondRaw(response());
    await expect(a).resolves.toEqual(page(7));
    expect(m.mode).toBe('direct');
    await tick(10);
    expect(bridge.calls).toHaveLength(1);
  });

  it('ignores a late answer for a project that was superseded or closed', async () => {
    const bridge = fakeBridge();
    const m = mux(bridge.fetcher);
    const closing = new AbortController();
    void m.poll('a', neverDirect, 1, closing.signal).catch(() => undefined);
    const b = m.poll('b', neverDirect, 1);
    await tick();
    // The answer and the close race: the answer for the closed project must be dropped.
    bridge.calls[0].respond([
      { project: 'a', body: page(2) },
      { project: 'b', body: page(2) },
    ]);
    closing.abort();
    await expect(b).resolves.toEqual(page(2));
    expect(m.waiting()).toEqual([]);
  });

  it('switches to direct polls when more projects wait than one request may carry', async () => {
    const bridge = fakeBridge();
    const m = mux(bridge.fetcher);
    const direct = vi.fn(() => Promise.resolve(page(1)));
    const polls = Array.from({ length: CHANGE_MUX_MAX_POLLS + 1 }, (_, index) => m.poll(`p${index}`, direct, 0));
    await Promise.all(polls);
    expect(bridge.calls).toHaveLength(0);
    expect(m.mode).toBe('direct');
  });

  it('an idle set of projects holds exactly one request per window', async () => {
    const bridge = fakeBridge();
    const m = mux(bridge.fetcher);
    for (const project of ['a', 'b', 'c', 'd', 'e', 'f', 'g']) void m.poll(project, neverDirect, 1);
    await tick(20);
    expect(bridge.calls).toHaveLength(1);
    expect(bridge.calls[0].body.polls).toHaveLength(7);
  });
});

describe('multiplexed transports', () => {
  it('two projects keep independent cursors, replay, and sinks over one shared request', async () => {
    const bridge = fakeBridge();
    const m = mux(bridge.fetcher, { now: () => 0 });
    const seen: Record<string, Array<[string, number]>> = { a: [], b: [] };
    const sink = (project: 'a' | 'b'): ProjectChangeSink => ({
      resync: (reason, cursor) => {
        seen[project].push([reason, cursor]);
        return Promise.resolve();
      },
      deliver: (response) => {
        seen[project].push(['deliver', response.cursor]);
        return Promise.resolve();
      },
    });
    const controller = new AbortController();
    for (const project of ['a', 'b'] as const)
      runProjectChangePollTransport(
        { client: m.client(project, { pollEvents: neverDirect }), sink: sink(project), retryMs: 1, maxRetryMs: 4 },
        controller.signal,
      );
    await tick();
    // Handshakes ride one request.
    expect(bridge.calls).toHaveLength(1);
    expect(bridge.calls[0].body.polls.map((poll) => [poll.project, poll.since])).toEqual([
      ['a', undefined],
      ['b', undefined],
    ]);
    bridge.calls[0].respond([
      { project: 'a', body: page(10) },
      { project: 'b', body: page(50) },
    ]);
    await tick(10);
    const live = bridge.calls.at(-1)!;
    expect(live.body.polls).toEqual([
      { project: 'a', since: 10, timeout_ms: 25_000 },
      { project: 'b', since: 50, timeout_ms: 25_000 },
    ]);
    // An event for one project delivers there only; the other keeps waiting at its cursor.
    live.respond([{ project: 'b', body: page(51, ['updated']) }]);
    await tick(10);
    expect(seen.b).toEqual([
      ['initial', 50],
      ['deliver', 51],
    ]);
    expect(seen.a).toEqual([['initial', 10]]);
    const next = bridge.calls.at(-1)!;
    expect(next.body.polls).toEqual([
      { project: 'a', since: 10, timeout_ms: 25_000 },
      { project: 'b', since: 51, timeout_ms: 25_000 },
    ]);
    // A failed shared request makes both transports reconnect with a fresh handshake.
    next.fail(new TypeError('Failed to fetch'));
    await tick(20);
    const reconnect = bridge.calls.at(-1)!;
    expect(reconnect.body.polls.map((poll) => poll.since)).toEqual([undefined, undefined]);
    reconnect.respond([
      { project: 'a', body: page(12) },
      { project: 'b', body: page(51) },
    ]);
    await tick(10);
    expect(seen.a.slice(1)).toEqual([
      ['reconnect', 12],
      ['deliver', 12],
    ]);
    controller.abort();
    await tick(10);
    expect(bridge.calls.at(-1)!.signal.aborted).toBe(true);
  });
});

describe('helpers', () => {
  it('derives the multiplex endpoint and project from a bridge API path', () => {
    expect(changeMuxTarget('/__hotsheet/project-api/proj%2D1', '/__hotsheet/change-mux/poll')).toEqual({
      endpoint: '/__hotsheet/change-mux/poll',
      project: 'proj-1',
    });
    expect(changeMuxTarget('http://h:1/base/__hotsheet/project-api/p/', '/m')).toEqual({
      endpoint: 'http://h:1/base/m',
      project: 'p',
    });
    expect(changeMuxTarget('/api', '/m')).toBeUndefined();
    expect(changeMuxTarget('/__hotsheet/project-api/%E0%A4%A', '/m')).toBeUndefined();
  });

  it('shares one multiplexer per endpoint and client', () => {
    const multiplexerFor = createChangePollMultiplexers({ parse: parsePollResponse });
    expect(multiplexerFor('/m', 'tab')).toBe(multiplexerFor('/m', 'tab'));
    expect(multiplexerFor('/m', 'tab')).not.toBe(multiplexerFor('/m', 'other'));
    expect(multiplexerFor('/n', 'tab')).not.toBe(multiplexerFor('/m', 'tab'));
  });

  it('wraps a client without hiding its other members', async () => {
    const bridge = fakeBridge();
    const m = mux(bridge.fetcher);
    const direct = { pollEvents: neverDirect, changeWebSocketUrl: () => 'ws://x/ws/sync' };
    const wrapped = m.client('a', direct);
    expect(wrapped.changeWebSocketUrl()).toBe('ws://x/ws/sync');
    const polled = wrapped.pollEvents(4);
    await tick();
    expect(bridge.calls[0].body.polls[0]).toMatchObject({ project: 'a', since: 4 });
    bridge.calls[0].respond([{ project: 'a', body: page(5) }]);
    await expect(polled).resolves.toEqual(page(5));
  });
});
