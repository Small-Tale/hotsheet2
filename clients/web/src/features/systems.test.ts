import { signal } from 'kerfjs';
import { describe, expect, it } from 'vitest';

import type {
  OrchestrationAdmission,
  OrchestrationCapacity,
  OrchestrationPolicyView,
  OrchestrationSitesView,
  OrchestrationToolCapacity,
} from '../api';
import type { HostCpuFrame } from '../host-cpu-feed';
import type { HostHealthResourceState } from '../host-health-resource';
import { hostHealthFixture } from '../ux-demo/host-health-fixtures';
import { systemsDemoCapacity, systemsDemoCpu } from '../ux-demo/systems-fixtures';
import { createSystemsController, type SystemsApi } from './systems';

const BINDING = '01JSYNTHREMTE0000000000001';
const REMOTE_TAB = `remote:${BINDING}:c1`;
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

/** Every read the controller makes, held open until the test answers. */
function harness({ orchestration = ['local-1'] }: { orchestration?: string[] } = {}) {
  const capacityReads: {
    projectId: string;
    after?: string;
    signal: AbortSignal;
    resolve: (v: OrchestrationCapacity) => void;
  }[] = [];
  const cpuReads: { projectId: string; after?: number; signal: AbortSignal; resolve: (v: HostCpuFrame) => void }[] = [];
  const toolReads: {
    projectId: string;
    resolve: (v: OrchestrationToolCapacity) => void;
    reject: (e: unknown) => void;
  }[] = [];
  // HS2-5H8R0N: one project placed on this computer and a site on the paired remote; its policy
  // lets Codex and Claude run. Each admission answer is held until the test answers it.
  const placementReads: string[] = [];
  const sites: OrchestrationSitesView = {
    revision: 'r1',
    projects: [
      {
        project_id: 'P1',
        label: 'Widgets',
        sites: [
          {
            target_id: 'P1',
            label: 'Widgets',
            project: true,
            destination: { kind: 'local', server_id: 's1', checkout_id: 'local-1', checkout_authority: 'a1' },
            confirmed_at_ms: null,
            last_failure: null,
            stale: false,
          },
          {
            target_id: 'S2',
            label: 'Build clone',
            project: false,
            destination: {
              kind: 'remote',
              registration_id: BINDING,
              identity: { server_id: 's2' },
              checkout_id: 'c1',
              checkout_authority: 'a2',
            },
            confirmed_at_ms: null,
            last_failure: null,
            stale: false,
          },
        ],
      },
    ],
  };
  const policy: OrchestrationPolicyView = {
    enabled: true,
    revision: 'p1',
    policy: { tools: [{ tool: 'codex' }, { tool: 'claude' }] },
  };
  const admissionReads: {
    target: string;
    tool: string;
    resolve: (v: OrchestrationAdmission) => void;
    reject: (e: unknown) => void;
  }[] = [];
  const api = (projectId: string): SystemsApi => ({
    machineCapacity: (_owner: string, after: string | undefined, abort?: AbortSignal) =>
      new Promise<OrchestrationCapacity>((resolve) => {
        capacityReads.push({ projectId, after, signal: abort!, resolve });
      }),
    hostCpu: (after: number | undefined, abort: AbortSignal) =>
      new Promise<HostCpuFrame>((resolve) => {
        cpuReads.push({ projectId, after, signal: abort, resolve });
      }),
    orchestrationToolCapacity: () =>
      new Promise<OrchestrationToolCapacity>((resolve, reject) => {
        toolReads.push({ projectId, resolve, reject });
      }),
    orchestrationSites: (owner: string) => {
      placementReads.push(`sites:${owner}`);
      return Promise.resolve(sites);
    },
    orchestrationPolicy: (owner: string) => {
      placementReads.push(`policy:${owner}`);
      return Promise.resolve(policy);
    },
    orchestrationAdmission: (_owner: string, target: string, tool: string) =>
      new Promise<OrchestrationAdmission>((resolve, reject) => {
        admissionReads.push({ target, tool, resolve, reject });
      }),
  });
  const showing = signal(false),
    visible = signal(true),
    projects = signal<{ id: string; remoteHost?: string }[]>([
      { id: 'local-1' },
      { id: REMOTE_TAB, remoteHost: 'Build Host' },
    ]),
    remoteServers = signal([{ id: BINDING, label: 'Build', origin: 'https://build.example.test', revoked: false }]),
    localHealth = signal<HostHealthResourceState>({ snapshot: hostHealthFixture('normal'), loading: false }),
    remoteHealth = signal<Record<string, ReturnType<typeof hostHealthFixture>>>({});
  const controller = createSystemsController({
    showing: () => showing.value,
    pageVisible: () => visible.value,
    projects: () => projects.value,
    remoteServers: () => remoteServers.value,
    localHealth: () => localHealth.value,
    remoteHealth: (projectId) => remoteHealth.value[projectId],
    api: (projectId) => (projects.peek().some((project) => project.id === projectId) ? api(projectId) : undefined),
    orchestrationProject: (projectId) => orchestration.includes(projectId),
    terminals: () => [{ id: 't1', title: 'Worker', projectName: 'Hot Sheet 2' }],
    toolLabel: (tool) => tool.toUpperCase(),
    formatTime: () => 'soon',
    cpuOptions: { wait: () => Promise.resolve() },
    capacityOptions: { delay: () => Promise.resolve(), now: () => 0 },
    knownTools: () => ['claude', 'codex'],
  });
  return {
    placementReads,
    admissionReads,
    controller,
    showing,
    visible,
    projects,
    remoteServers,
    localHealth,
    remoteHealth,
    capacityReads,
    cpuReads,
    toolReads,
  };
}

describe('HS2-DR0ZSQ Systems controller', () => {
  it('reads nothing until Systems shows, then one capacity stream per connected machine and the selected CPU', async () => {
    const h = harness();
    await settle();
    expect([h.capacityReads, h.cpuReads, h.toolReads].map((reads) => reads.length)).toEqual([0, 0, 0]);
    h.showing.value = true;
    await settle();
    expect(h.capacityReads.map((read) => read.projectId)).toEqual(['local-1', REMOTE_TAB]);
    expect(h.cpuReads.map((read) => read.projectId)).toEqual(['local-1']);
    expect(h.toolReads.map((read) => read.projectId)).toEqual(['local-1']);
    h.controller.dispose();
  });

  it('stops every read when the page hides or the screen leaves, and resumes on return', async () => {
    const h = harness();
    h.showing.value = true;
    await settle();
    h.visible.value = false;
    expect(h.capacityReads.every((read) => read.signal.aborted)).toBe(true);
    expect(h.cpuReads.every((read) => read.signal.aborted)).toBe(true);
    h.visible.value = true;
    await settle();
    expect(h.capacityReads.filter((read) => !read.signal.aborted)).toHaveLength(2);
    h.showing.value = false;
    expect(h.capacityReads.every((read) => read.signal.aborted)).toBe(true);
    expect(h.cpuReads.every((read) => read.signal.aborted)).toBe(true);
    // Each opening reads AI Tool Capacity once more; hiding and showing the page does not.
    expect(h.toolReads).toHaveLength(2);
    h.controller.dispose();
  });

  it('moves the live CPU feed to the selected machine with a fresh sequence, and back', async () => {
    const h = harness();
    h.showing.value = true;
    await settle();
    h.cpuReads[0].resolve(systemsDemoCpu('local'));
    await settle();
    expect(h.cpuReads[1].after).toBe(7);
    h.controller.select(`remote:${BINDING}`);
    await settle();
    expect(h.cpuReads[1].signal.aborted).toBe(true);
    const remote = h.cpuReads.at(-1)!;
    expect([remote.projectId, remote.after]).toEqual([REMOTE_TAB, undefined]);
    h.controller.select('local');
    await settle();
    expect(remote.signal.aborted).toBe(true);
    expect(h.cpuReads.at(-1)?.projectId).toBe('local-1');
    // A machine without an open project has no CPU feed.
    h.remoteServers.value = [
      ...h.remoteServers.value,
      { id: '01JSYNTHREMTE0000000000002', label: 'Off', origin: 'https://off.example.test', revoked: false },
    ];
    const reads = h.cpuReads.length;
    h.controller.select('remote:01JSYNTHREMTE0000000000002');
    await settle();
    expect(h.cpuReads.at(-1)?.signal.aborted).toBe(true);
    expect(h.cpuReads).toHaveLength(reads);
    h.controller.dispose();
  });

  it('builds the three regions from each machine’s readings', async () => {
    const h = harness();
    h.showing.value = true;
    await settle();
    h.capacityReads[0].resolve(systemsDemoCapacity('local'));
    h.cpuReads[0].resolve(systemsDemoCpu('local'));
    h.toolReads[0].resolve({
      enabled: true,
      now_ms: 0,
      tools: [{ tool: 'claude', state: 'available', verified: true, max_sessions: 2, weight: 1 }],
    });
    await settle();
    const view = h.controller.view();
    expect(view.sidebar.machines.map((row) => [row.machine.name, row.description, row.cpu])).toEqual([
      ['studio', 'This Computer · 3 AI sessions', 38],
      ['Build Host', 'Paired Remote', undefined],
    ]);
    expect(view.sidebar.toolCapacity).toMatchObject({ status: 'ready', rows: [{ label: 'CLAUDE' }] });
    expect(view.workspace).toMatchObject({ machine: { id: 'local' }, liveCpu: { seq: 7 } });
    expect(view.inspector.sessions?.[0]).toEqual({
      id: 't1',
      title: 'Worker · CLAUDE',
      description: 'Hot Sheet 2',
      busy: true,
    });
    // The remote reads its own tab's host health; until it arrives the cards wait.
    h.controller.select(`remote:${BINDING}`);
    expect(h.controller.view().workspace.health).toEqual({ loading: true });
    expect(h.controller.view().inspector.sessions).toBeUndefined();
    h.remoteHealth.value = { [REMOTE_TAB]: hostHealthFixture('crossed') };
    expect(h.controller.view().workspace.health.snapshot?.level).toBe(hostHealthFixture('crossed').level);
    h.controller.dispose();
  });

  it('says when no orchestration project is open, and reports a failed tool capacity read', async () => {
    const none = harness({ orchestration: [] });
    none.showing.value = true;
    await settle();
    expect(none.toolReads).toHaveLength(0);
    expect(none.controller.view().sidebar.toolCapacity).toEqual({ status: 'none' });
    none.controller.dispose();
    const failing = harness();
    failing.showing.value = true;
    await settle();
    expect(failing.controller.view().sidebar.toolCapacity).toEqual({ status: 'loading' });
    failing.toolReads[0].reject(new Error('refused'));
    await settle();
    expect(failing.controller.view().sidebar.toolCapacity).toEqual({ status: 'error', message: 'refused' });
    failing.controller.dispose();
  });

  it('restarts a machine’s stream when its tab changes and drops one whose last tab closed', async () => {
    const h = harness();
    h.showing.value = true;
    await settle();
    const first = h.capacityReads.find((read) => read.projectId === REMOTE_TAB)!;
    h.projects.value = [{ id: 'local-1' }];
    await settle();
    expect(first.signal.aborted).toBe(true);
    expect(h.controller.view().sidebar.machines[1]).toMatchObject({
      machine: { unavailable: 'not-connected' },
      description: 'Not connected',
    });
    h.projects.value = [{ id: 'local-1' }, { id: `remote:${BINDING}:c2` }];
    await settle();
    expect(h.capacityReads.at(-1)?.projectId).toBe(`remote:${BINDING}:c2`);
    h.controller.dispose();
    expect(h.capacityReads.every((read) => read.signal.aborted)).toBe(true);
  });

  it('HS2-5H8R0N asks admission for the projects placed on the selected machine, again only when its room changes', async () => {
    const h = harness();
    h.showing.value = true;
    await settle();
    await settle();
    // The orchestration project's sites and policy are read once per opening.
    expect(h.placementReads).toEqual(['sites:local-1', 'policy:local-1']);
    // This computer runs the home site: its policy's two tools are asked about.
    expect(h.admissionReads.map((read) => [read.target, read.tool])).toEqual([
      ['P1', 'codex'],
      ['P1', 'claude'],
    ]);
    expect(h.controller.view().inspector.admission).toEqual([
      { key: 'local-1\u0000P1', label: 'Widgets', status: { label: 'Checking', tone: 'neutral' } },
    ]);
    const answer = (outcome: OrchestrationAdmission['outcome'], admitted: number, reason: string | null = null) => ({
      outcome,
      tool: '',
      requested: 1,
      admitted,
      reason,
    });
    h.admissionReads[0].resolve({ ...answer('accepted', 1), tool: 'codex' });
    h.admissionReads[1].resolve({ ...answer('refused', 0, 'tool_not_installed'), tool: 'claude' });
    await settle();
    expect(h.controller.view().inspector.admission).toEqual([
      {
        key: 'local-1\u0000P1',
        label: 'Widgets',
        status: { label: 'Can start 1 now', tone: 'success' },
        description: 'CODEX: Can start 1 now · CLAUDE: Refused · not installed there',
      },
    ]);
    expect(h.controller.view().inspector.orchestrating).toBe(true);
    // The capacity stream's first answer gives a version: admission is asked again once.
    h.capacityReads[0].resolve({ ...systemsDemoCapacity('local'), version: 'v1', owner_limits: { max_sessions: 3 } });
    await settle();
    await settle();
    expect(h.admissionReads).toHaveLength(4);
    expect(h.controller.view().inspector.limits?.[0]).toEqual({ label: 'Sessions per Machine', value: '3' });
    expect(h.controller.view().inspector.capacityRead).toBe(true);
    // The same version again (a long poll timing out) asks nothing.
    const next = h.capacityReads.find((read) => read.projectId === 'local-1' && read.after === 'v1')!;
    next.resolve({ ...systemsDemoCapacity('local'), version: 'v1' });
    await settle();
    await settle();
    expect(h.admissionReads).toHaveLength(4);
    // A failed answer keeps the row and says why.
    h.admissionReads[2].reject(new Error('refused by the gate'));
    h.admissionReads[3].resolve({ ...answer('accepted', 1), tool: 'claude' });
    await settle();
    expect(h.controller.view().inspector.admission?.[0]).toMatchObject({
      status: { label: 'Unknown' },
      description: 'Admission is not available. refused by the gate',
    });
    // The paired remote runs the other site, named with its project.
    h.controller.select(`remote:${BINDING}`);
    await settle();
    expect(h.admissionReads.slice(4).map((read) => read.target)).toEqual(['S2', 'S2']);
    expect(h.controller.view().inspector.admission?.map((row) => row.label)).toEqual(['Widgets · Build clone']);
    // Leaving reads nothing more; opening again re-reads the placements.
    h.showing.value = false;
    await settle();
    const asked = h.admissionReads.length;
    h.showing.value = true;
    await settle();
    await settle();
    expect(h.placementReads).toHaveLength(4);
    expect(h.admissionReads.length).toBeGreaterThan(asked);
    h.controller.dispose();
  });

  it('HS2-5H8R0N shows no admission without an orchestration project, and none for a machine without readings', async () => {
    const none = harness({ orchestration: [] });
    none.showing.value = true;
    await settle();
    await settle();
    expect(none.placementReads).toEqual([]);
    expect(none.admissionReads).toEqual([]);
    expect(none.controller.view().inspector).toMatchObject({
      admission: [],
      orchestrating: false,
      capacityRead: false,
    });
    none.controller.dispose();
    const h = harness();
    h.showing.value = true;
    await settle();
    await settle();
    h.remoteServers.value = [
      ...h.remoteServers.value,
      { id: '01JSYNTHREMTE0000000000002', label: 'Off', origin: 'https://off.example.test', revoked: false },
    ];
    const asked = h.admissionReads.length;
    h.controller.select('remote:01JSYNTHREMTE0000000000002');
    await settle();
    expect(h.admissionReads).toHaveLength(asked);
    expect(h.controller.view().inspector.admission).toEqual([]);
    h.controller.dispose();
  });
});
