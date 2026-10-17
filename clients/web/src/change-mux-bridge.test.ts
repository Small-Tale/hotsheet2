import { describe, expect, it, vi } from 'vitest';

import {
  CHANGE_MUX_MAX_POLLS,
  CHANGE_MUX_PATH,
  type ChangeMuxForward,
  changeMuxPollPath,
  multiplexChangePolls,
  parseChangeMuxRequest,
} from './change-mux-bridge';
import { createDevApp } from './dev-server';

/** An upstream whose per-project answers the test releases. */
function upstream() {
  const waiting = new Map<string, { path: string; signal: AbortSignal; answer(response: Response): void }>();
  const forward: ChangeMuxForward = (project, path, signal) =>
    new Promise<Response>((resolve, reject) => {
      signal.addEventListener('abort', () => {
        reject(new Error('aborted'));
      });
      waiting.set(project, { path, signal, answer: resolve });
    });
  return { waiting, forward };
}

const page = (cursor: number) => Response.json({ cursor, events: [], overflow: false });
const tick = (ms = 2) => new Promise((resolve) => setTimeout(resolve, ms));

describe('parseChangeMuxRequest', () => {
  it('accepts a valid request and clamps the timeout to the server ceiling', () => {
    expect(
      parseChangeMuxRequest({
        version: 1,
        client: 'tab',
        polls: [
          { project: 'a', since: 3, timeout_ms: 25_000 },
          { project: 'b', timeout_ms: 90_000 },
        ],
      }),
    ).toEqual({
      version: 1,
      client: 'tab',
      polls: [
        { project: 'a', since: 3, timeout_ms: 25_000 },
        { project: 'b', timeout_ms: 30_000 },
      ],
    });
  });

  it.each([
    ['not an object', []],
    ['another version', { version: 3, client: 't', polls: [{ project: 'a', timeout_ms: 0 }] }],
    ['no client', { version: 1, polls: [{ project: 'a', timeout_ms: 0 }] }],
    ['no polls', { version: 1, client: 't', polls: [] }],
    [
      'a duplicate project',
      {
        version: 1,
        client: 't',
        polls: [
          { project: 'a', timeout_ms: 0 },
          { project: 'a', timeout_ms: 0 },
        ],
      },
    ],
    ['a negative cursor', { version: 1, client: 't', polls: [{ project: 'a', since: -1, timeout_ms: 0 }] }],
    ['a fractional timeout', { version: 1, client: 't', polls: [{ project: 'a', timeout_ms: 1.5 }] }],
    ['an empty project', { version: 1, client: 't', polls: [{ project: '', timeout_ms: 0 }] }],
    [
      'too many polls',
      {
        version: 1,
        client: 't',
        polls: Array.from({ length: CHANGE_MUX_MAX_POLLS + 1 }, (_, i) => ({ project: `p${i}`, timeout_ms: 0 })),
      },
    ],
  ])('rejects %s', (_name, body) => {
    expect(typeof parseChangeMuxRequest(body)).toBe('string');
  });

  it('builds the same upstream poll path a direct client sends', () => {
    expect(changeMuxPollPath({ project: 'a', since: 4, timeout_ms: 25_000 }, 'tab 1')).toBe(
      '/ws/poll?timeout_ms=25000&since=4&client=tab%201',
    );
    expect(changeMuxPollPath({ project: 'a', timeout_ms: 0 }, 't')).toBe('/ws/poll?timeout_ms=0&client=t');
  });

  it('accepts two tabs polling the same project in version 2 and returns each client identity', async () => {
    const parsed = parseChangeMuxRequest({
      version: 2,
      client: '',
      polls: [
        { project: 'same', client: 'tab-a', timeout_ms: 0 },
        { project: 'same', client: 'tab-b', timeout_ms: 0 },
      ],
    });
    expect(typeof parsed).toBe('object');
    if (typeof parsed === 'string') throw new Error(parsed);
    const paths: string[] = [];
    const response = await multiplexChangePolls(
      parsed,
      async (_project, path) => {
        paths.push(path);
        return page(1);
      },
      new AbortController().signal,
    );
    expect(paths.sort()).toEqual(['/ws/poll?timeout_ms=0&client=tab-a', '/ws/poll?timeout_ms=0&client=tab-b']);
    expect(response).toMatchObject({
      version: 2,
      results: expect.arrayContaining([
        expect.objectContaining({ project: 'same', client: 'tab-a', status: 200 }),
        expect.objectContaining({ project: 'same', client: 'tab-b', status: 200 }),
      ]),
    });
  });
});

describe('multiplexChangePolls', () => {
  const request = (...projects: string[]) => ({
    version: 1,
    client: 'tab',
    polls: projects.map((project, index) => ({ project, since: index, timeout_ms: 25_000 })),
  });

  it('answers with the first completion plus stragglers inside the grace window, then aborts the rest', async () => {
    const fake = upstream();
    const answer = multiplexChangePolls(request('a', 'b', 'c'), fake.forward, new AbortController().signal, 30);
    await tick();
    expect(fake.waiting.get('b')!.path).toBe('/ws/poll?timeout_ms=25000&since=1&client=tab');
    fake.waiting.get('a')!.answer(page(5));
    await tick(5);
    fake.waiting.get('b')!.answer(page(7));
    const result = await answer;
    expect(result.version).toBe(1);
    expect(result.results).toEqual([
      { project: 'a', status: 200, body: { cursor: 5, events: [], overflow: false } },
      { project: 'b', status: 200, body: { cursor: 7, events: [], overflow: false } },
    ]);
    expect(fake.waiting.get('c')!.signal.aborted).toBe(true);
  });

  it('returns as soon as every poll completed, without waiting out the grace window', async () => {
    const fake = upstream();
    const started = Date.now();
    const answer = multiplexChangePolls(request('a'), fake.forward, new AbortController().signal, 5_000);
    await tick();
    fake.waiting.get('a')!.answer(page(1));
    await answer;
    expect(Date.now() - started).toBeLessThan(1_000);
  });

  it('passes an upstream error status and body through for that project only', async () => {
    const fake = upstream();
    const answer = multiplexChangePolls(request('a', 'b'), fake.forward, new AbortController().signal, 20);
    await tick();
    fake.waiting.get('a')!.answer(Response.json({ error: 'Project session is not open.' }, { status: 404 }));
    fake.waiting.get('b')!.answer(new Response('not json', { status: 502 }));
    const result = await answer;
    expect(result.results).toEqual([
      { project: 'a', status: 404, body: { error: 'Project session is not open.' } },
      { project: 'b', status: 502, body: { error: 'not json' } },
    ]);
  });

  it('reports a forward that throws as a 502 for that project', async () => {
    const forward: ChangeMuxForward = () => Promise.reject(new Error('connect ECONNREFUSED'));
    const result = await multiplexChangePolls(request('a'), forward, new AbortController().signal, 5);
    expect(result.results).toEqual([{ project: 'a', status: 502, body: { error: 'connect ECONNREFUSED' } }]);
  });

  it('a browser disconnect cancels every upstream poll', async () => {
    const fake = upstream();
    const browser = new AbortController();
    const answer = multiplexChangePolls(request('a', 'b'), fake.forward, browser.signal, 5);
    await tick();
    browser.abort();
    const result = await answer;
    expect(result.results).toEqual([]);
    expect(fake.waiting.get('a')!.signal.aborted).toBe(true);
    expect(fake.waiting.get('b')!.signal.aborted).toBe(true);
  });

  it('an already-disconnected browser forwards nothing live', async () => {
    const forward = vi.fn<ChangeMuxForward>((_project, _path, signal) =>
      signal.aborted ? Promise.reject(new Error('aborted')) : Promise.resolve(page(1)),
    );
    const browser = new AbortController();
    browser.abort();
    const result = await multiplexChangePolls(request('a'), forward, browser.signal, 5);
    expect(result.results).toEqual([]);
  });
});

describe('dev bridge endpoint', () => {
  const app = (forward: ChangeMuxForward, dev = true) =>
    createDevApp(
      dev,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      forward,
    );
  const post = (body: string) =>
    new Request(`http://localhost${CHANGE_MUX_PATH}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body,
    });

  it('multiplexes project polls through the injected forwarder', async () => {
    const forward = vi.fn<ChangeMuxForward>(() => Promise.resolve(page(3)));
    const response = await app(forward).fetch(
      post(JSON.stringify({ version: 1, client: 'tab', polls: [{ project: 'p1', since: 2, timeout_ms: 0 }] })),
    );
    expect(response.status).toBe(200);
    expect(await response.json()).toEqual({
      version: 1,
      results: [{ project: 'p1', status: 200, body: { cursor: 3, events: [], overflow: false } }],
    });
    expect(forward).toHaveBeenCalledWith('p1', '/ws/poll?timeout_ms=0&since=2&client=tab', expect.any(AbortSignal));
  });

  it('rejects a malformed request with 400', async () => {
    const forward = vi.fn<ChangeMuxForward>();
    expect((await app(forward).fetch(post('{'))).status).toBe(400);
    expect((await app(forward).fetch(post(JSON.stringify({ version: 9 })))).status).toBe(400);
    expect(forward).not.toHaveBeenCalled();
  });

  it('is absent outside the local bridge, like the project API', async () => {
    const response = await app(vi.fn<ChangeMuxForward>(), false).fetch(
      post(JSON.stringify({ version: 1, client: 't', polls: [{ project: 'p', timeout_ms: 0 }] })),
    );
    expect(response.status).toBe(404);
  });
});
