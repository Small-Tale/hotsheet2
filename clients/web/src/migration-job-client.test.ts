import { describe, expect, it, vi } from 'vitest';

import { MigrationJobClient } from './migration-job-client';
import type { MigrationJob } from './migration-progress';

const fixture = (root: string, revision = 1): MigrationJob => ({
  id: root,
  attempt: `${root}-first`,
  revision,
  projectId: root,
  root,
  store: `${root}.hs2`,
  sourceIdentity: 'source',
  kind: 'import',
  status: 'running',
  progress: { version: 1, phase: 'copy_database', completed: 1, total: 2, unit: 'bytes' },
  warnings: [],
  ownerPid: 0,
  updatedAt: '',
});

const requestUrl = (input: Parameters<typeof fetch>[0]) => (input instanceof Request ? input.url : input.toString());

describe('project-owned migration subscriptions', () => {
  it('updates the owning project, ignores stale attempts, and reconnects without timed state polling', async () => {
    const received: MigrationJob[] = [],
      finished = vi.fn<(job: MigrationJob) => Promise<void>>(async () => {}),
      errors = vi.fn();
    const snapshots = new Map([
      ['/a', fixture('/a')],
      ['/b', fixture('/b')],
    ]);
    const waits = new Map<string, (job: MigrationJob) => void>();
    const request = vi.fn<typeof fetch>(async (input, init) => {
      const url = new URL(requestUrl(input), 'http://local');
      if (init?.method === 'POST')
        return Response.json(
          snapshots.get(JSON.parse(typeof init.body === 'string' ? init.body : '{}').root as string),
          { status: 202 },
        );
      const root = url.searchParams.get('root')!;
      if (!url.searchParams.has('after')) return Response.json({ job: snapshots.get(root) });
      return new Promise<Response>((resolveResponse, reject) => {
        waits.set(root, (job) => {
          resolveResponse(Response.json({ job }));
        });
        init?.signal?.addEventListener(
          'abort',
          () => {
            reject(new Error('Disconnected'));
          },
          { once: true },
        );
      });
    });
    const client = new MigrationJobClient((job) => received.push(job), finished, errors, request);
    await client.join('/a');
    await client.join('/b');
    await vi.waitFor(() => {
      expect(waits.size).toBe(2);
    });
    expect(request.mock.calls.filter(([url]) => requestUrl(url).includes('after=')).length).toBe(2);
    waits.get('/a')!({ ...fixture('/a', 2), status: 'succeeded' });
    await vi.waitFor(() => {
      expect(finished).toHaveBeenCalledTimes(1);
    });
    expect(finished.mock.calls[0][0].root).toBe('/a');
    expect(received.at(-1)?.root).toBe('/a');
    expect([...received].reverse().find((job) => job.root === '/b')?.status).toBe('running');
    const next = { ...fixture('/a', 4), attempt: 'retry' };
    snapshots.set('/a', next);
    await client.start({ projectId: '/a', root: '/a', location: '/a.hs2', kind: 'import', retryAttempt: '/a-first' });
    await vi.waitFor(() => {
      expect(request.mock.calls.some(([url]) => requestUrl(url).endsWith('after=4'))).toBe(true);
    });
    waits.get('/a')!({ ...fixture('/a', 3), status: 'succeeded' });
    await vi.waitFor(() => {
      expect(errors).toHaveBeenCalledWith('/a', expect.stringContaining('outdated'));
      expect(request.mock.calls.filter(([url]) => requestUrl(url).endsWith('after=4')).length).toBe(1);
    });
    expect(received.at(-1)?.attempt).toBe('retry');
    expect(finished).toHaveBeenCalledTimes(1);
    client.disconnect('/a');
    client.disconnect('/b');
    expect(errors).toHaveBeenCalledTimes(1);
    snapshots.set('/a', { ...next, revision: 5, status: 'failed', error: 'Disk full' });
    await client.join('/a');
    expect(received.at(-1)).toMatchObject({ attempt: 'retry', status: 'failed', error: 'Disk full' });
  });
});

it.each(['idle', 'version', 'authentication'])(
  'stops an invalid %s response without a tight watch loop',
  async (mode) => {
    const errors = vi.fn(),
      job = fixture('/a');
    const request = vi.fn<typeof fetch>(async (input) => {
      if (!requestUrl(input).includes('after=')) return Response.json({ job });
      if (mode === 'authentication') return Response.json({ error: 'Unauthorized' }, { status: 401 });
      return Response.json({
        job: mode === 'version' ? { ...job, revision: 2, progress: { version: 2, phase: 'copy' } } : job,
      });
    });
    const client = new MigrationJobClient(
      () => {},
      async () => {},
      errors,
      request,
    );
    await client.join('/a');
    await vi.waitFor(() => {
      expect(errors).toHaveBeenCalledTimes(1);
    });
    expect(request).toHaveBeenCalledTimes(2);
    client.disconnect('/a');
  },
);

it('retries a failed authoritative completion refresh when the same terminal job is rejoined', async () => {
  const job = { ...fixture('/a'), status: 'succeeded' as const };
  const finished = vi.fn().mockRejectedValueOnce(new Error('Offline')).mockResolvedValue(undefined);
  const errors = vi.fn();
  const client = new MigrationJobClient(
    () => {},
    finished,
    errors,
    async () => Response.json({ job }),
  );
  await client.join('/a');
  await vi.waitFor(() => {
    expect(errors).toHaveBeenCalledTimes(1);
  });
  await client.join('/a');
  await vi.waitFor(() => {
    expect(finished).toHaveBeenCalledTimes(2);
  });
  await client.join('/a');
  expect(finished).toHaveBeenCalledTimes(2);
});
