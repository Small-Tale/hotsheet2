import { signal } from 'kerfjs';

import type { LiveStoreState } from './api';
import { createLiveStateStore, type LiveStateStore } from './live-state';
import { parseRemoteProjectId } from './remote-projects/identity';

/**
 * Live state for cross-server (remote) project tabs (HS2-4R99XY). The local store keys snapshots by
 * store id and matches a project by the checkout ids a store links, so it cannot hold a remote tab:
 * the browser's `remote:<binding>:<checkout>` id never equals the remote server's checkout id, and a
 * remote server can host a store whose id the local server also uses. Each remote project therefore
 * owns a store of its own, fed by its own change stream and `GET /live-state` resync, and the Live
 * section reads the stores linked to the remote checkout id. Nothing here reads on a timer.
 */
export function createRemoteLiveStates() {
  const byProject = new Map<string, LiveStateStore>();
  /** Bumped when a store appears or goes, so a sidebar that read an empty answer re-renders. */
  const version = signal(0);

  /** The store a remote project's stream feeds, created on first use. */
  function storeFor(projectId: string): LiveStateStore {
    let store = byProject.get(projectId);
    if (!store) {
      store = createLiveStateStore();
      byProject.set(projectId, store);
      version.value += 1;
    }
    return store;
  }

  /** The remote server's stores linked to this remote project's checkout; empty for any other id. */
  function forProject(projectId: string): LiveStoreState[] {
    void version.value;
    const parsed = parseRemoteProjectId(projectId);
    if (!parsed) return [];
    return byProject.get(projectId)?.forProject(parsed.checkoutId) ?? [];
  }

  /** Forget a closed remote project's snapshots. */
  function drop(projectId: string) {
    if (!byProject.delete(projectId)) return;
    version.value += 1;
  }

  function reset() {
    if (byProject.size === 0) return;
    byProject.clear();
    version.value += 1;
  }

  return { storeFor, forProject, drop, reset };
}

export type RemoteLiveStates = ReturnType<typeof createRemoteLiveStates>;

/**
 * Per-project reads over the local and remote stores: a remote project id reads its own store, every
 * other id the local one.
 */
export function projectLiveStates(
  local: Pick<LiveStateStore, 'forProject'>,
  remote: Pick<RemoteLiveStates, 'forProject'>,
) {
  return {
    forProject: (projectId: string): LiveStoreState[] =>
      parseRemoteProjectId(projectId) ? remote.forProject(projectId) : local.forProject(projectId),
  };
}
