import { computed, effect, signal } from 'kerfjs';

import type { Api, OrchestrationPolicyBody, OrchestrationSitesView } from '../api';
import { describeRequestFailure } from '../bounded-backoff';
import type { SystemsInspectorProps } from '../components/systems-inspector';
import type { SystemsSidebarProps, SystemsToolCapacityState } from '../components/systems-sidebar';
import type { SystemsViewProps } from '../components/systems-view';
import { createHostCpuFeed, type HostCpuFeed, type HostCpuFeedOptions } from '../host-cpu-feed';
import type { HostHealthSnapshot } from '../host-health';
import type { HostHealthResourceState } from '../host-health-resource';
import { admissionTools } from '../orchestration-settings-model';
import { isRemoteProjectId } from '../remote-projects/identity';
import { createSystemsCapacityFeeds, type SystemsCapacityRead } from '../systems-capacity-feed';
import {
  admissionRow,
  LOCAL_MACHINE_ID,
  machineCpuPercent,
  machineRowDescription,
  placementLimitRows,
  selectedSystemsMachine,
  sessionRows,
  sitesOnMachine,
  type SystemsAdmissionState,
  type SystemsKnownTerminal,
  type SystemsMachine,
  type SystemsMachineReadings,
  systemsMachines,
  type SystemsOpenProject,
  type SystemsRemoteServer,
  toolCapacityRows,
} from '../systems-model';

/** The reads the Systems screen makes through one open project's path. */
export type SystemsApi = Pick<
  Api,
  | 'machineCapacity'
  | 'hostCpu'
  | 'orchestrationToolCapacity'
  | 'orchestrationSites'
  | 'orchestrationPolicy'
  | 'orchestrationAdmission'
>;

/** HS2-5H8R0N: what Systems read of one orchestration project: its sites and its policy. */
interface SystemsPlacement {
  sites: OrchestrationSitesView;
  policy: OrchestrationPolicyBody | null;
}

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
  /** The AI tools this app knows, for a policy that lists none (Auto), as the inspector asks (HS2-5H8R0N). */
  knownTools?: () => readonly string[];
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
  // HS2-5H8R0N: each open orchestration project's sites and policy, read once per opening, and the
  // selected machine's admission answers, asked again only when its capacity version changes.
  const placements = signal<ReadonlyMap<string, SystemsPlacement>>(new Map());
  const admissions = signal<ReadonlyMap<string, SystemsAdmissionState>>(new Map());

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

  /** The open local orchestration projects, whose sites and admission Systems shows. */
  const owners = computed(() =>
    dependencies
      .projects()
      .filter((project) => !isRemoteProjectId(project.id) && dependencies.orchestrationProject(project.id))
      .map((project) => project.id),
  );

  let placementRead = 0;
  /** HS2-5H8R0N: read each orchestration project's sites and policy; one that fails is left out. */
  async function refreshPlacements(ids: readonly string[]) {
    const request = ++placementRead;
    const results = await Promise.all(
      ids.map(async (owner): Promise<[string, SystemsPlacement] | undefined> => {
        const api = dependencies.api(owner);
        if (!api) return undefined;
        try {
          const [sites, policy] = await Promise.all([api.orchestrationSites(owner), api.orchestrationPolicy(owner)]);
          return [owner, { sites, policy: policy.policy }];
        } catch {
          return undefined;
        }
      }),
    );
    if (request !== placementRead) return;
    placements.value = new Map(results.filter((entry) => entry !== undefined));
  }

  let admissionRead = 0;
  /** HS2-5H8R0N: ask each placed site on the machine whether its sessions may start there now. */
  async function refreshAdmissions(machineId: string) {
    const request = ++admissionRead;
    const sites = [...placements.peek()].flatMap(([owner, placement]) =>
      sitesOnMachine(owner, placement.sites, machineId).map((site) => ({ site, policy: placement.policy })),
    );
    const key = (owner: string, targetId: string) => `${owner}\u0000${targetId}`;
    const previous = admissions.peek();
    admissions.value = new Map(
      sites.map(({ site }) => [
        key(site.owner, site.targetId),
        previous.get(key(site.owner, site.targetId)) ?? { status: 'loading' as const },
      ]),
    );
    const answers = await Promise.all(
      sites.map(async ({ site, policy }): Promise<[string, SystemsAdmissionState]> => {
        const api = dependencies.api(site.owner);
        const tools = admissionTools(policy, site.targetId, dependencies.knownTools?.() ?? []);
        try {
          if (!api) throw new Error('This project is no longer open.');
          const answered = await Promise.all(
            tools.map((tool) => api.orchestrationAdmission(site.owner, site.targetId, tool)),
          );
          return [key(site.owner, site.targetId), { status: 'ready', admissions: answered }];
        } catch (reason) {
          return [key(site.owner, site.targetId), { status: 'error', message: describeRequestFailure(reason) }];
        }
      }),
    );
    if (request !== admissionRead) return;
    admissions.value = new Map(answers);
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
  // HS2-5H8R0N: the orchestration projects' sites and policies are read on opening, and again only
  // when the set of open orchestration projects changes while the screen shows.
  let placedFor: string | undefined;
  const disposePlacements = effect(() => {
    if (!active.value) {
      placedFor = undefined;
      return;
    }
    const ids = owners.value;
    const signature = ids.join('\u0000');
    if (signature === placedFor) return;
    placedFor = signature;
    queueMicrotask(() => void refreshPlacements(ids));
  });
  // Admission is asked for the selected machine when it, the placements, or its capacity version
  // change: a version changes exactly when the machine's room does (HS2-P5GTSW).
  let admittedFor: string | undefined;
  const disposeAdmissions = effect(() => {
    if (!active.value) {
      admittedFor = undefined;
      return;
    }
    const machine = current.value;
    const version = feeds.states.value.get(machine.id)?.capacity?.version ?? '';
    const placed = placements.value;
    if (machine.unavailable || placed.size === 0) return;
    const signature = `${machine.id}\u0000${version}\u0000${[...placed.keys()].join(',')}`;
    if (signature === admittedFor) return;
    admittedFor = signature;
    queueMicrotask(() => void refreshAdmissions(machine.id));
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
    const answered = admissions.value;
    const placed = machine.unavailable
      ? []
      : [...placements.value].flatMap(([owner, placement]) => sitesOnMachine(owner, placement.sites, machine.id));
    const limits = placementLimitRows(capacity.capacity);
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
        admission: placed.map((site) =>
          admissionRow(site, answered.get(`${site.owner}\u0000${site.targetId}`), dependencies.toolLabel),
        ),
        orchestrating: owners.value.length > 0,
        ...(limits ? { limits } : {}),
        capacityRead: Boolean(capacity.capacity),
      },
    };
  }

  function dispose() {
    disposeFeeds();
    disposeCpu();
    disposeToolCapacity();
    disposePlacements();
    disposeAdmissions();
    feeds.stop();
    cpu.peek()?.feed.stop();
  }

  return { selected, machines, select, view, refreshToolCapacity, feeds, placements, admissions, dispose };
}

export type SystemsController = ReturnType<typeof createSystemsController>;
