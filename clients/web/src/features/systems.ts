import { computed, effect, signal } from 'kerfjs';

import type { Api } from '../api';
import { describeRequestFailure } from '../bounded-backoff';
import type { SystemsInspectorProps } from '../components/systems-inspector';
import type { SystemsSidebarProps, SystemsToolCapacityState } from '../components/systems-sidebar';
import type { SystemsViewProps } from '../components/systems-view';
import { createHostCpuFeed, type HostCpuFeed, type HostCpuFeedOptions } from '../host-cpu-feed';
import type { HostHealthSnapshot } from '../host-health';
import type { HostHealthResourceState } from '../host-health-resource';
import { isRemoteProjectId } from '../remote-projects/identity';
import { createSystemsCapacityFeeds, type SystemsCapacityRead } from '../systems-capacity-feed';
import {
  LOCAL_MACHINE_ID,
  machineCpuPercent,
  machineRowDescription,
  selectedSystemsMachine,
  sessionRows,
  type SystemsKnownTerminal,
  type SystemsMachine,
  type SystemsMachineReadings,
  systemsMachines,
  type SystemsOpenProject,
  type SystemsRemoteServer,
  toolCapacityRows,
} from '../systems-model';

/** The reads the Systems screen makes through one open project's path. */
export type SystemsApi = Pick<Api, 'machineCapacity' | 'hostCpu' | 'orchestrationToolCapacity'>;

export interface SystemsDependencies {
  /** Whether the Systems screen shows (the shell mode is `systems`). */
  showing: () => boolean;
  /** Whether the page is visible; a hidden page reads nothing. */
  pageVisible: () => boolean;
  projects: () => readonly SystemsOpenProject[];
  remoteServers: () => readonly SystemsRemoteServer[];
  /** This computer's pushed host health (HS2-DTAVQA). */
  localHealth: () => HostHealthResourceState;
  /** A remote tab's pushed host health (HS2-K4DY5G). */
  remoteHealth: (projectId: string) => HostHealthSnapshot | undefined;
  api: (projectId: string) => SystemsApi | undefined;
  /** Whether an open project is an orchestration project, whose AI tool capacity Systems shows. */
  orchestrationProject: (projectId: string) => boolean;
  /** The terminals this app shows, to name a running session. */
  terminals: () => readonly SystemsKnownTerminal[];
  toolLabel: (tool: string) => string;
  formatTime?: (ms: number) => string;
  /** Why the last disk choice failed (This Computer only). */
  diskMessage?: () => string | undefined;
  cpuOptions?: HostCpuFeedOptions;
  capacityOptions?: Parameters<typeof createSystemsCapacityFeeds>[1];
}

/**
 * The Systems screen (HS2-DR0ZSQ): which machine is selected and the live reads behind it. While the
 * screen shows on a visible page it keeps one capacity long poll per connected machine and the live
 * CPU long poll for the selected one; leaving the screen or hiding the page stops them all. AI Tool
 * Capacity is read once each time the screen opens. Nothing here runs on a timer.
 */
export function createSystemsController(dependencies: SystemsDependencies) {
  const selected = signal<string>(LOCAL_MACHINE_ID);
  const toolCapacity = signal<SystemsToolCapacityState>({ status: 'none' });
  const cpu = signal<{ machineId: string; projectId: string; feed: HostCpuFeed } | undefined>(undefined);
  const read: SystemsCapacityRead = (projectId, after, abort) => {
    const api = dependencies.api(projectId);
    if (!api) return Promise.reject(new Error('This project is no longer open.'));
    return api.machineCapacity(projectId, after, abort);
  };
  const feeds = createSystemsCapacityFeeds(read, dependencies.capacityOptions);

  const machines = computed<SystemsMachine[]>(() =>
    systemsMachines({
      projects: dependencies.projects(),
      remoteServers: dependencies.remoteServers(),
      localName: dependencies.localHealth().snapshot?.host.name,
    }),
  );
  const active = computed(() => dependencies.showing() && dependencies.pageVisible());
  const current = computed(() => selectedSystemsMachine(machines.value, selected.value));

  function healthFor(machine: SystemsMachine): HostHealthResourceState {
    if (machine.kind === 'local') return dependencies.localHealth();
    const snapshot = machine.projectId ? dependencies.remoteHealth(machine.projectId) : undefined;
    return snapshot ? { snapshot, loading: false } : { loading: Boolean(machine.projectId) };
  }

  function readingsFor(machine: SystemsMachine): SystemsMachineReadings {
    const frame = cpu.value?.machineId === machine.id ? cpu.value.feed.state.value.frame : undefined;
    return {
      health: healthFor(machine).snapshot,
      capacity: feeds.states.value.get(machine.id)?.capacity,
      ...(frame ? { cpu: frame } : {}),
    };
  }

  let toolCapacityRead = 0;
  async function refreshToolCapacity() {
    const request = ++toolCapacityRead;
    const owners = dependencies
      .projects()
      .filter((project) => !isRemoteProjectId(project.id) && dependencies.orchestrationProject(project.id));
    if (owners.length === 0) {
      toolCapacity.value = { status: 'none' };
      return;
    }
    if (toolCapacity.peek().status !== 'ready') toolCapacity.value = { status: 'loading' };
    const results = await Promise.allSettled(
      owners.map((owner) => {
        const api = dependencies.api(owner.id);
        return api
          ? api.orchestrationToolCapacity(owner.id)
          : Promise.reject(new Error('This project is no longer open.'));
      }),
    );
    if (request !== toolCapacityRead) return;
    const views = results.flatMap((result) => (result.status === 'fulfilled' ? [result.value] : []));
    const failure = results.find((result): result is PromiseRejectedResult => result.status === 'rejected');
    toolCapacity.value =
      views.length === 0 && failure
        ? { status: 'error', message: describeRequestFailure(failure.reason) }
        : { status: 'ready', rows: toolCapacityRows(views, dependencies.toolLabel, dependencies.formatTime) };
  }

  // One capacity stream per connected machine while the screen shows.
  const disposeFeeds = effect(() => {
    if (!active.value) {
      feeds.stop();
      return;
    }
    feeds.sync(
      machines.value.flatMap((machine) =>
        machine.projectId ? [{ machineId: machine.id, projectId: machine.projectId }] : [],
      ),
    );
  });
  // The live CPU long poll follows the selected machine; another machine starts a fresh feed, since
  // a frame sequence belongs to one server.
  const disposeCpu = effect(() => {
    const machine = current.value;
    const running = cpu.peek();
    if (!active.value || !machine.projectId) {
      running?.feed.stop();
      return;
    }
    if (running && running.machineId === machine.id && running.projectId === machine.projectId) {
      running.feed.start();
      return;
    }
    running?.feed.stop();
    const projectId = machine.projectId;
    const feed = createHostCpuFeed(() => {
      const api = dependencies.api(projectId);
      if (!api) throw new Error('This project is no longer open.');
      return api;
    }, dependencies.cpuOptions);
    cpu.value = { machineId: machine.id, projectId, feed };
    feed.start();
  });
  // Opening the screen reads AI Tool Capacity once; nothing re-reads it on a timer.
  let wasActive = false;
  const disposeToolCapacity = effect(() => {
    const now = active.value;
    if (now && !wasActive) queueMicrotask(() => void refreshToolCapacity());
    wasActive = now;
  });

  function select(machineId: string) {
    selected.value = machineId;
  }

  /** The three regions' props, read inside the shell's render so they follow every reading. */
  function view(): { sidebar: SystemsSidebarProps; workspace: SystemsViewProps; inspector: SystemsInspectorProps } {
    const machine = current.value;
    const capacity = feeds.states.value.get(machine.id) ?? {};
    const frame = cpu.value?.machineId === machine.id ? cpu.value.feed.state.value.frame : undefined;
    const sessions = sessionRows(capacity.capacity, dependencies.terminals(), dependencies.toolLabel);
    return {
      sidebar: {
        machines: machines.value.map((row) => {
          const readings = readingsFor(row);
          const percent = row.unavailable ? undefined : machineCpuPercent(readings);
          return {
            machine: row,
            description: machineRowDescription(row, readings),
            ...(percent === undefined ? {} : { cpu: percent }),
          };
        }),
        selectedId: machine.id,
        toolCapacity: toolCapacity.value,
      },
      workspace: {
        machine,
        health: healthFor(machine),
        ...(frame ? { liveCpu: frame } : {}),
        capacity,
        ...(dependencies.diskMessage?.() ? { diskMessage: dependencies.diskMessage() } : {}),
        toolLabel: dependencies.toolLabel,
      },
      inspector: {
        machine,
        ...(sessions ? { sessions } : {}),
        ...(capacity.error ? { error: capacity.error } : {}),
      },
    };
  }

  function dispose() {
    disposeFeeds();
    disposeCpu();
    disposeToolCapacity();
    feeds.stop();
    cpu.peek()?.feed.stop();
  }

  return { selected, machines, select, view, refreshToolCapacity, feeds, dispose };
}

export type SystemsController = ReturnType<typeof createSystemsController>;
