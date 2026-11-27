import { signal } from 'kerfjs';
import { describe, expect, it } from 'vitest';

import type { OrchestrationCapacity, OrchestrationToolCapacity } from '../api';
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
  });
  return {
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
});
