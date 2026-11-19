import { signal } from 'kerfjs';

import type { HostHealthSnapshot } from './host-health';
import { createHostHealthResource, type HostHealthResource } from './host-health-resource';
import { isRemoteProjectId } from './remote-projects/identity';

type Transport = Parameters<typeof createHostHealthResource>[0];

/**
 * Host health of cross-server (remote) project tabs (HS2-K4DY5G). The sidebar's disk and memory
 * callouts describe the machine the project runs on, so a remote tab needs its own server's health,
 * read through that tab's bridge path: the local resource reads the Stats project or the first open
 * project and would show this machine's warnings on a remote tab. Each remote project owns a resource
 * fed by its own change stream (`host_health` events and resync). Nothing here reads on a timer.
 */
export function createRemoteHostHealth() {
  const byProject = new Map<string, HostHealthResource>();
  /** Bumped when a resource appears or goes, so a sidebar that read an empty answer re-renders. */
  const version = signal(0);

  /** The resource a remote project's stream feeds, created on first use. */
  function resourceFor(projectId: string, transport: Transport): HostHealthResource {
    let resource = byProject.get(projectId);
    if (!resource) {
      resource = createHostHealthResource(transport);
      byProject.set(projectId, resource);
      version.value += 1;
    }
    return resource;
  }

  /** The remote server's latest snapshot for this remote project; `undefined` before the first read. */
  function snapshot(projectId: string): HostHealthSnapshot | undefined {
    void version.value;
    return byProject.get(projectId)?.state.value.snapshot;
  }

  /** Stop and forget a closed remote project's resource. */
  function drop(projectId: string) {
    const resource = byProject.get(projectId);
    if (!resource) return;
    resource.dispose();
    byProject.delete(projectId);
    version.value += 1;
  }

  function reset() {
    if (byProject.size === 0) return;
    for (const resource of byProject.values()) resource.dispose();
    byProject.clear();
    version.value += 1;
  }

  return { resourceFor, snapshot, drop, reset };
}

export type RemoteHostHealth = ReturnType<typeof createRemoteHostHealth>;

/**
 * The host health a project's sidebar shows: a remote project reads its own server's snapshot and
 * never the local one, every other project the local machine's.
 */
export function projectHostHealth(
  local: { readonly value: HostHealthSnapshot | undefined },
  remote: Pick<RemoteHostHealth, 'snapshot'>,
) {
  return {
    forProject: (projectId: string): HostHealthSnapshot | undefined =>
      isRemoteProjectId(projectId) ? remote.snapshot(projectId) : local.value,
  };
}
