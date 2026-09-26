import { describe, expect, it, vi } from 'vitest';

import type { Project } from './interactions/types';
import { openProjectFetch, type ProjectOpenResult, restoreRememberedProjects } from './project-startup';

const project = (root: string): Project => ({
  id: root,
  root,
  name: root,
  stores: [],
  apiPath: `/api/${root}`,
  compatibility: { kind: 'compatible', revisionMismatch: false, sourceStale: false, canRestartServer: false },
});
const success = (root: string): ProjectOpenResult => ({ ok: true, project: project(root), providers: [] });
const failure = (error = 'Unavailable'): ProjectOpenResult => ({ ok: false, error });
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}
function callbacks() {
  return {
    wire: vi
      .fn<(root: string, result: Extract<ProjectOpenResult, { ok: true }>, index: number) => Promise<void>>()
      .mockResolvedValue(undefined),
    activate: vi.fn<(project: Project) => Promise<void>>().mockResolvedValue(undefined),
    retainFailure: vi.fn(),
    selectFailure: vi.fn(),
    waitForRetry: vi.fn(async () => undefined),
  };
}

describe('project open preparation', () => {
  it('sends explicit ticket-store selection and prepares descriptors without mutating the returned project', async () => {
    const value = project('alpha');
    const request = vi.fn<typeof fetch>().mockResolvedValue(Response.json(value, { status: 201 }));
    const providers = vi.fn(async () => []);
    expect(await openProjectFetch('alpha', '/tickets', request, providers)).toEqual(success('alpha'));
    expect(request).toHaveBeenCalledWith('/__hotsheet/projects/open', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ root: 'alpha', ticketStore: '/tickets' }),
    });
    expect(providers).toHaveBeenCalledWith(value);
  });
  it('keeps recovery identity local to each concurrently failed request', async () => {
    const recovery = { store: '/a.hs2', expected: { pid: 42, url: 'http://local', started_at: 'now' } };
    const request = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(Response.json({ error: 'Busy', recovery }, { status: 409 }))
      .mockRejectedValueOnce(new Error('Offline'))
      .mockResolvedValueOnce(Response.json({}, { status: 500 }));
    const providers = vi.fn(async () => []);
    expect(
      await Promise.all(['a', 'b', 'c'].map((root) => openProjectFetch(root, undefined, request, providers))),
    ).toEqual([{ ok: false, error: 'Busy', recovery }, failure('Offline'), failure('Could not open project.')]);
    expect(providers).not.toHaveBeenCalled();
  });
  it('retains an opened project when optional provider metadata is unavailable', async () => {
    expect(
      await openProjectFetch(
        'alpha',
        undefined,
        vi.fn<typeof fetch>().mockResolvedValue(Response.json(project('alpha'))),
        async () => {
          throw new Error('Disconnected');
        },
      ),
    ).toEqual(success('alpha'));
  });
  it('turns malformed responses into a typed failure that can be retried', async () => {
    const result = await openProjectFetch(
      'alpha',
      undefined,
      vi.fn<typeof fetch>().mockResolvedValue(new Response('{')),
    );
    expect(result).toMatchObject({ ok: false, error: expect.any(String) });
  });
});

describe('remembered project startup', () => {
  it('starts every unique fetch together, then presents the active project before the others finish (HS2-X74D4B)', async () => {
    const pending = new Map(['alpha', 'beta', 'gamma'].map((root) => [root, deferred<ProjectOpenResult>()]));
    const fetch = vi.fn((root: string) => pending.get(root)!.promise);
    const state = callbacks(),
      activeReady = vi.fn();
    const restored = restoreRememberedProjects({
      ...state,
      roots: ['alpha', 'beta', 'alpha', 'gamma'],
      activeRoot: 'beta',
      fetch,
      activeReady,
    });
    expect(fetch.mock.calls.map(([root]) => root)).toEqual(['alpha', 'beta', 'gamma']);
    // gamma finishing first changes nothing; the active beta registers and activates alone.
    pending.get('gamma')!.resolve(success('gamma'));
    await Promise.resolve();
    expect(state.wire).not.toHaveBeenCalled();
    pending.get('beta')!.resolve(success('beta'));
    await vi.waitFor(() => {
      expect(activeReady).toHaveBeenCalledTimes(1);
    });
    expect(state.wire.mock.calls.map(([root, , index]) => [root, index])).toEqual([['beta', 1]]);
    expect(state.activate).toHaveBeenCalledExactlyOnceWith(project('beta'));
    // The slow alpha still blocks the background registration, which then follows remembered order.
    pending.get('alpha')!.resolve(success('alpha'));
    await restored;
    expect(state.wire.mock.calls.map(([root, , index]) => [root, index])).toEqual([
      ['beta', 1],
      ['alpha', 0],
      ['gamma', 2],
    ]);
    expect(state.activate).toHaveBeenCalledTimes(1);
    expect(activeReady).toHaveBeenCalledTimes(1);
    expect(state.waitForRetry).not.toHaveBeenCalled();
  });
  it('falls back to the full pass when the active first attempt fails, announcing readiness once at the end', async () => {
    const state = callbacks(),
      activeReady = vi.fn(),
      attempts = new Map<string, number>();
    await restoreRememberedProjects({
      ...state,
      roots: ['alpha', 'beta'],
      activeRoot: 'beta',
      activeReady,
      fetch: async (root) => {
        attempts.set(root, (attempts.get(root) ?? 0) + 1);
        return root === 'beta' && attempts.get(root) === 1 ? failure(root) : success(root);
      },
    });
    expect(state.wire.mock.calls.map(([root, , index]) => [root, index])).toEqual([
      ['alpha', 0],
      ['beta', 1],
    ]);
    expect(state.activate).toHaveBeenCalledExactlyOnceWith(project('beta'));
    expect(activeReady).toHaveBeenCalledTimes(1);
    expect(state.activate.mock.invocationCallOrder[0]).toBeLessThan(activeReady.mock.invocationCallOrder[0]);
  });
  it('announces readiness after selecting a failure and after an empty restore', async () => {
    const state = callbacks(),
      activeReady = vi.fn();
    await restoreRememberedProjects({
      ...state,
      roots: ['alpha'],
      activeRoot: 'alpha',
      activeReady,
      fetch: async () => failure(),
    });
    expect(state.selectFailure).toHaveBeenCalledExactlyOnceWith('alpha');
    expect(activeReady).toHaveBeenCalledTimes(1);
    await restoreRememberedProjects({ ...state, roots: [], activeReady, fetch: async () => failure() });
    expect(activeReady).toHaveBeenCalledTimes(2);
  });
  it('retries only failed roots together, preserving original order and active choice after recovery', async () => {
    const state = callbacks(),
      attempts = new Map<string, number>();
    const retry = new Map(['alpha', 'gamma'].map((root) => [root, deferred<ProjectOpenResult>()]));
    const fetch = vi.fn((root: string) => {
      const attempt = (attempts.get(root) ?? 0) + 1;
      attempts.set(root, attempt);
      return root === 'beta'
        ? Promise.resolve(success(root))
        : attempt === 1
          ? Promise.resolve(failure(root))
          : retry.get(root)!.promise;
    });
    const restored = restoreRememberedProjects({
      ...state,
      roots: ['alpha', 'beta', 'gamma'],
      activeRoot: 'alpha',
      fetch,
    });
    await vi.waitFor(() => {
      expect(fetch).toHaveBeenCalledTimes(5);
    });
    expect(attempts).toEqual(
      new Map([
        ['alpha', 2],
        ['beta', 1],
        ['gamma', 2],
      ]),
    );
    expect(state.wire).not.toHaveBeenCalled();
    retry.get('gamma')!.resolve(success('gamma'));
    retry.get('alpha')!.resolve(success('alpha'));
    await restored;
    expect(state.wire.mock.calls.map(([root]) => root)).toEqual(['alpha', 'beta', 'gamma']);
    expect(state.activate).toHaveBeenCalledExactlyOnceWith(project('alpha'));
    expect(state.waitForRetry).toHaveBeenCalledTimes(1);
  });
  it('keeps the remembered active failure selected without activating healthy tabs or leaking recovery identities', async () => {
    const state = callbacks();
    await restoreRememberedProjects({
      ...state,
      roots: ['alpha', 'beta'],
      activeRoot: 'beta',
      fetch: async (root) => (root === 'alpha' ? success(root) : failure('Server unavailable')),
    });
    expect(state.wire).toHaveBeenCalledTimes(1);
    expect(state.activate).not.toHaveBeenCalled();
    expect(state.selectFailure).toHaveBeenCalledExactlyOnceWith('beta');
    expect(state.retainFailure.mock.calls).toEqual([
      ['beta', failure('Server unavailable')],
      ['beta', failure('Server unavailable')],
    ]);
  });
  it.each([undefined, 'missing'])(
    'uses the last successful remembered project if active root is %s',
    async (activeRoot) => {
      const state = callbacks();
      await restoreRememberedProjects({
        ...state,
        roots: ['alpha', 'beta', 'gamma'],
        activeRoot,
        fetch: async (root) => (root === 'gamma' ? failure() : success(root)),
      });
      expect(state.activate).toHaveBeenCalledExactlyOnceWith(project('beta'));
      expect(state.selectFailure).not.toHaveBeenCalled();
    },
  );
  it('retains all failures after one bounded retry and chooses the first when no active root exists', async () => {
    const state = callbacks(),
      fetch = vi.fn(async () => failure());
    await restoreRememberedProjects({ ...state, roots: ['alpha', 'beta'], fetch });
    expect(fetch).toHaveBeenCalledTimes(4);
    expect(state.waitForRetry).toHaveBeenCalledTimes(1);
    expect(state.wire).not.toHaveBeenCalled();
    expect(state.activate).not.toHaveBeenCalled();
    expect(state.selectFailure).toHaveBeenCalledExactlyOnceWith('alpha');
  });
  it('does not retain invocation state across empty, all-failed, empty, and refilled sessions', async () => {
    const state = callbacks(),
      fetch = vi.fn(async (root: string) => failure(root));
    for (const roots of [[], ['alpha'], []]) await restoreRememberedProjects({ ...state, roots, fetch });
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(state.wire).not.toHaveBeenCalled();
    fetch.mockImplementation(async (root) => success(root));
    await restoreRememberedProjects({ ...state, roots: ['beta', 'beta'], activeRoot: 'beta', fetch });
    expect(fetch).toHaveBeenCalledTimes(3);
    expect(state.wire).toHaveBeenCalledExactlyOnceWith('beta', success('beta'), 0);
    expect(state.activate).toHaveBeenCalledExactlyOnceWith(project('beta'));
  });
  it('matches a canonical active root when the remembered open request used a checkout alias', async () => {
    const state = callbacks();
    await restoreRememberedProjects({
      ...state,
      roots: ['alias', 'beta'],
      activeRoot: 'alpha',
      fetch: async (root) => success(root === 'alias' ? 'alpha' : root),
    });
    expect(state.activate).toHaveBeenCalledExactlyOnceWith(project('alpha'));
  });
  it('allows aliases of the same project to clear their own failure while selecting only one project', async () => {
    const state = callbacks(),
      attempts = new Map<string, number>();
    await restoreRememberedProjects({
      ...state,
      roots: ['alias', 'alpha'],
      activeRoot: 'alias',
      fetch: async (root) => {
        attempts.set(root, (attempts.get(root) ?? 0) + 1);
        return root === 'alias' && attempts.get(root) === 1 ? failure() : success('alpha');
      },
    });
    expect(state.wire.mock.calls.map(([root]) => root)).toEqual(['alias', 'alpha']);
    expect(state.activate).toHaveBeenCalledExactlyOnceWith(project('alpha'));
  });
});
