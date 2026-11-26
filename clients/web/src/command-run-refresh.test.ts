import { describe, expect, it } from 'vitest';

import { createLocalRunStarts, mergeRefreshedCommandRuns } from './command-run-refresh';

interface Run {
  id: string;
  state: string;
}

/** The runtime's refresh shape: capture, hold the read, then merge into whatever is listed now. */
function refreshWith(starts: ReturnType<typeof createLocalRunStarts>, listed: { value: Run[] }) {
  const capture = starts.capture();
  let answer: (runs: Run[]) => void = () => undefined;
  const read = new Promise<Run[]>((resolve) => {
    answer = resolve;
  });
  const settled = read.then((server) => {
    listed.value = mergeRefreshedCommandRuns(server, listed.value, capture);
    starts.release(capture);
  });
  return {
    release: async (server: Run[]) => {
      answer(server);
      await settled;
    },
  };
}

function start(starts: ReturnType<typeof createLocalRunStarts>, listed: { value: Run[] }, run: Run) {
  starts.record(run.id);
  listed.value = [run, ...listed.value.filter((item) => item.id !== run.id)];
}

describe('command runs refresh (HS2-YA6D31)', () => {
  it('keeps a run started while the command-runs read was held', async () => {
    const starts = createLocalRunStarts();
    const listed = { value: [{ id: 'old', state: 'succeeded' }] };
    const refresh = refreshWith(starts, listed);
    start(starts, listed, { id: 'new', state: 'running' });
    await refresh.release([{ id: 'old', state: 'succeeded' }]);
    expect(listed.value.map((run) => run.id)).toEqual(['new', 'old']);
  });

  it('takes the server state when the server already lists the started run', async () => {
    const starts = createLocalRunStarts();
    const listed = { value: [] as Run[] };
    const refresh = refreshWith(starts, listed);
    start(starts, listed, { id: 'new', state: 'running' });
    await refresh.release([{ id: 'new', state: 'succeeded' }]);
    expect(listed.value).toEqual([{ id: 'new', state: 'succeeded' }]);
  });

  it('lets the server drop runs that were not started during the read', async () => {
    const starts = createLocalRunStarts();
    const listed = { value: [{ id: 'gone', state: 'succeeded' }] };
    start(starts, listed, { id: 'before', state: 'running' });
    const refresh = refreshWith(starts, listed);
    await refresh.release([{ id: 'other', state: 'running' }]);
    expect(listed.value.map((run) => run.id)).toEqual(['other']);
  });

  it('records a start into every overlapping refresh and forgets it once each settles', async () => {
    const starts = createLocalRunStarts();
    const listed = { value: [] as Run[] };
    const first = refreshWith(starts, listed);
    const second = refreshWith(starts, listed);
    start(starts, listed, { id: 'new', state: 'running' });
    await second.release([]);
    expect(listed.value.map((run) => run.id)).toEqual(['new']);
    await first.release([]);
    expect(listed.value.map((run) => run.id)).toEqual(['new']);
    // A later refresh that the start did not overlap follows the server.
    const later = refreshWith(starts, listed);
    await later.release([]);
    expect(listed.value).toEqual([]);
  });

  it('returns a copy of the server list when nothing started', () => {
    const server = [{ id: 'a', state: 'running' }];
    const merged = mergeRefreshedCommandRuns(server, [], new Set());
    expect(merged).toEqual(server);
    expect(merged).not.toBe(server);
  });
});
