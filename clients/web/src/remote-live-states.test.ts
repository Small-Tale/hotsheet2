import { effect } from 'kerfjs';
import { describe, expect, it } from 'vitest';

import type { LiveStoreState } from './api';
import { createLiveStateStore } from './live-state';
import { createRemoteLiveStates, projectLiveStates } from './remote-live-states';
import { remoteProjectId } from './remote-projects/identity';

const BINDING = '01M7S5JGSGWFWB3A2WHJ0P2NTC';
const OTHER_BINDING = '01M7S5JH74D4GDMHHFPTRD300Q';
const REMOTE = remoteProjectId(BINDING, 'hotsheet2-ab12cd');

/** A snapshot with only the fields the store keys on; the shape is covered by live-state.test.ts. */
function snapshot(overrides: Partial<LiveStoreState>): LiveStoreState {
  return { store: 's1', checkouts: ['hotsheet2-ab12cd'], epoch_ms: 1, revision: 1, ...overrides } as LiveStoreState;
}

describe('remote project live states (HS2-4R99XY)', () => {
  it('reads a remote project by the remote checkout id, which the browser id never equals', () => {
    const remote = createRemoteLiveStates();
    remote.storeFor(REMOTE).apply(snapshot({ store: 'remote-store' }));
    // The local store matches `checkouts.includes(projectId)`, so the browser id finds nothing there.
    const local = createLiveStateStore();
    local.apply(snapshot({ store: 'remote-store' }));
    expect(local.forProject(REMOTE)).toEqual([]);
    expect(remote.forProject(REMOTE).map((state) => state.store)).toEqual(['remote-store']);
  });

  it('keeps a remote store apart from a local store with the same store id', () => {
    const local = createLiveStateStore();
    const remote = createRemoteLiveStates();
    local.apply(snapshot({ epoch_ms: 5, revision: 9, state: 'stopped', checkouts: ['demo-ab12'] }));
    remote.storeFor(REMOTE).apply(snapshot({ epoch_ms: 2, revision: 1, state: 'active' }));
    const states = projectLiveStates(local, remote);
    expect(states.forProject('demo-ab12').map((state) => state.state)).toEqual(['stopped']);
    expect(states.forProject(REMOTE).map((state) => state.state)).toEqual(['active']);
  });

  it('keeps two remote projects on different servers apart', () => {
    const remote = createRemoteLiveStates();
    const other = remoteProjectId(OTHER_BINDING, 'hotsheet2-ab12cd');
    remote.storeFor(REMOTE).apply(snapshot({ state: 'active' }));
    remote.storeFor(other).apply(snapshot({ state: 'halted' }));
    expect(remote.forProject(REMOTE).map((state) => state.state)).toEqual(['active']);
    expect(remote.forProject(other).map((state) => state.state)).toEqual(['halted']);
  });

  it('answers only the checkout the remote project names, not the remote server’s other stores', () => {
    const remote = createRemoteLiveStates();
    const store = remote.storeFor(REMOTE);
    store.apply(snapshot({ store: 'mine' }));
    store.apply(snapshot({ store: 'theirs', checkouts: ['other-ff00ff'] }));
    expect(remote.forProject(REMOTE).map((state) => state.store)).toEqual(['mine']);
  });

  it('is empty for a local id, an unopened remote project, and a dropped one', () => {
    const remote = createRemoteLiveStates();
    expect(remote.forProject('demo-ab12')).toEqual([]);
    expect(remote.forProject(REMOTE)).toEqual([]);
    remote.storeFor(REMOTE).apply(snapshot({}));
    remote.drop(REMOTE);
    expect(remote.forProject(REMOTE)).toEqual([]);
    expect(() => {
      remote.drop(REMOTE);
    }).not.toThrow();
  });

  it('refills after a drop and after a reset', () => {
    const remote = createRemoteLiveStates();
    remote.storeFor(REMOTE).apply(snapshot({ revision: 3 }));
    remote.reset();
    expect(remote.forProject(REMOTE)).toEqual([]);
    remote.storeFor(REMOTE).apply(snapshot({ revision: 1 }));
    expect(remote.forProject(REMOTE)).toHaveLength(1);
  });

  it('re-renders a reader that looked before the stream created the store', () => {
    const remote = createRemoteLiveStates();
    const seen: number[] = [];
    const stop = effect(() => {
      seen.push(remote.forProject(REMOTE).length);
    });
    remote.storeFor(REMOTE).apply(snapshot({}));
    stop();
    expect(seen).toEqual([0, 0, 1]);
  });

  it('reuses one store per remote project', () => {
    const remote = createRemoteLiveStates();
    expect(remote.storeFor(REMOTE)).toBe(remote.storeFor(REMOTE));
  });
});
