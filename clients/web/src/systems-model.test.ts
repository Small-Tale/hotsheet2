import { describe, expect, it } from 'vitest';

import type {
  OrchestrationAdmission,
  OrchestrationCapacity,
  OrchestrationSitesView,
  OrchestrationSiteView,
  OrchestrationToolCapacity,
} from './api';
import {
  admissionRow,
  aiSessionCount,
  aiToolRows,
  capabilityRows,
  LOCAL_MACHINE_ID,
  machineCpuPercent,
  machineKindLabel,
  machineRowDescription,
  machineUnavailableMessage,
  placementLimitRows,
  remoteMachineId,
  selectedSystemsMachine,
  sessionRows,
  sitesOnMachine,
  systemsMachineForDestination,
  systemsMachines,
  toolCapacityRows,
  toolchainRows,
} from './systems-model';
import { hostHealthFixture } from './ux-demo/host-health-fixtures';
import { systemsDemoCapacity, systemsDemoCpu } from './ux-demo/systems-fixtures';

const A = '01JSYNTHREMTE0000000000001',
  B = '01JSYNTHREMTE0000000000002',
  C = '01JSYNTHREMTE0000000000003';
const remoteTab = (binding: string, checkout = 'c1', remoteHost?: string) => ({
  id: `remote:${binding}:${checkout}`,
  ...(remoteHost ? { remoteHost } : {}),
});
const server = (id: string, origin: string, revoked = false) => ({ id, label: `Label ${id}`, origin, revoked });

describe('HS2-DR0ZSQ systemsMachines', () => {
  it('lists this computer first, then each paired remote in order, read through its first open tab', () => {
    const machines = systemsMachines({
      projects: [remoteTab(A, 'x', 'Build Host'), { id: 'local-1' }, remoteTab(A, 'y'), { id: 'local-2' }],
      remoteServers: [server(A, 'https://build.example.test:7400'), server(B, 'https://laptop.example.test')],
      localName: 'Studio',
    });
    expect(machines).toEqual([
      { id: LOCAL_MACHINE_ID, kind: 'local', name: 'Studio', projectId: 'local-1' },
      { id: remoteMachineId(A), kind: 'remote', name: 'Build Host', projectId: `remote:${A}:x` },
      { id: remoteMachineId(B), kind: 'remote', name: 'laptop.example.test', unavailable: 'not-connected' },
    ]);
  });

  it('marks a revoked remote even while a tab is open, and adds remotes the list does not name', () => {
    const machines = systemsMachines({
      projects: [remoteTab(A), remoteTab(C, 'z', 'Lab')],
      remoteServers: [server(A, 'https://a.example.test', true)],
    });
    expect(machines.map((machine) => [machine.id, machine.name, machine.unavailable, machine.projectId])).toEqual([
      ['local', 'This Computer', 'no-project', undefined],
      [remoteMachineId(A), 'a.example.test', 'revoked', undefined],
      [remoteMachineId(C), 'Lab', undefined, `remote:${C}:z`],
    ]);
  });

  it('names an unnamed unlisted remote and lists a server once even when repeated', () => {
    const machines = systemsMachines({
      projects: [remoteTab(B)],
      remoteServers: [server(A, 'https://a.example.test'), server(A, 'https://a.example.test')],
      localName: '  ',
    });
    expect(machines.map((machine) => machine.name)).toEqual(['This Computer', 'a.example.test', 'Remote Server']);
  });

  it('falls back to this computer when the selected machine went away, and back again when it returns', () => {
    const both = systemsMachines({ projects: [{ id: 'l' }, remoteTab(A)], remoteServers: [] });
    expect(selectedSystemsMachine(both, remoteMachineId(A)).id).toBe(remoteMachineId(A));
    const local = systemsMachines({ projects: [{ id: 'l' }], remoteServers: [] });
    expect(selectedSystemsMachine(local, remoteMachineId(A)).id).toBe(LOCAL_MACHINE_ID);
    expect(selectedSystemsMachine(local, undefined).id).toBe(LOCAL_MACHINE_ID);
    expect(selectedSystemsMachine(both, remoteMachineId(A)).id).toBe(remoteMachineId(A));
  });
});

describe('HS2-DR0ZSQ machine rows', () => {
  const local = { id: 'local', kind: 'local' as const, name: 'Studio', projectId: 'p' };
  const remote = { id: 'remote:x', kind: 'remote' as const, name: 'Build', projectId: 'r' };

  it('describes each machine by kind and running AI sessions, or why it has no readings', () => {
    expect(machineRowDescription(local, {})).toBe('This Computer');
    expect(machineRowDescription(local, { capacity: systemsDemoCapacity('local') })).toBe(
      'This Computer · 3 AI sessions',
    );
    expect(machineRowDescription(remote, { capacity: systemsDemoCapacity('build') })).toBe(
      'Paired Remote · 2 AI sessions',
    );
    const one = { ...systemsDemoCapacity('build'), sessions: [{ session_id: 's', tool: 'claude', busy: true }] };
    expect(machineRowDescription(remote, { capacity: one })).toBe('Paired Remote · 1 AI session');
    expect(machineRowDescription({ ...remote, unavailable: 'not-connected' }, {})).toBe('Not connected');
    expect(machineRowDescription({ ...remote, unavailable: 'revoked' }, {})).toBe('Approval revoked');
    expect(machineRowDescription({ ...local, unavailable: 'no-project' }, {})).toBe('This Computer · no project open');
    expect(machineKindLabel(remote)).toBe('Paired Remote');
  });

  it('counts sessions only when the machine reported them', () => {
    expect(aiSessionCount(undefined)).toBeUndefined();
    expect(aiSessionCount({ ...systemsDemoCapacity('local'), sessions: null })).toBeUndefined();
    expect(aiSessionCount({ ...systemsDemoCapacity('local'), sessions: [] })).toBe(0);
  });

  it('reads CPU from the live frame, then host health, then capacity, clamped to the scale', () => {
    const health = hostHealthFixture('normal');
    expect(machineCpuPercent({ cpu: systemsDemoCpu('build'), health })).toBe(86);
    expect(machineCpuPercent({ cpu: { ...systemsDemoCpu('build'), status: 'starting' }, health })).toBe(
      health.signals.cpu.busy_percent,
    );
    expect(
      machineCpuPercent({ capacity: { ...systemsDemoCapacity('build'), cpu: { load_percent: 140, logical_cpus: 2 } } }),
    ).toBe(100);
    expect(machineCpuPercent({})).toBeUndefined();
    const unknown = { ...health, signals: { ...health.signals, cpu: { ...health.signals.cpu, busy_percent: null } } };
    expect(machineCpuPercent({ health: unknown })).toBeUndefined();
  });

  it('explains each reason a machine has no readings and says nothing for a connected one', () => {
    expect(machineUnavailableMessage(local)).toBeUndefined();
    expect(machineUnavailableMessage({ ...remote, unavailable: 'not-connected' })?.title).toBe(
      'Build Is Not Connected',
    );
    expect(machineUnavailableMessage({ ...remote, unavailable: 'revoked' })?.title).toBe('Build Revoked This Computer');
    expect(machineUnavailableMessage({ ...local, unavailable: 'no-project' })?.detail).toContain('Open a project');
  });
});

describe('HS2-DR0ZSQ detail tables', () => {
  const label = (tool: string) => ({ claude: 'Claude', codex: 'Codex', opencode: 'OpenCode' })[tool] ?? tool;

  it('lists AI tools installed first with versions and running sessions; unread is undefined', () => {
    expect(aiToolRows(undefined)).toBeUndefined();
    const rows = aiToolRows(systemsDemoCapacity('build'), label)!;
    expect(rows.map((row) => [row.label, row.description, row.status.label])).toEqual([
      ['Claude 2.1.0', 'Signed in · 1 session running', 'Signed In'],
      ['Codex 0.40.0', '1 session running', 'Installed'],
      ['OpenCode', 'Not installed on this machine', 'Not Installed'],
    ]);
    const idle = aiToolRows({ ...systemsDemoCapacity('local'), sessions: [] }, label)!;
    expect(idle.map((row) => [row.description, row.status, row.signedIn])).toEqual([
      ['Signed in · no sessions running', { label: 'Signed In', tone: 'success' }, true],
      ['Signed in · no sessions running', { label: 'Signed In', tone: 'success' }, true],
      [
        'Installed, not signed in · sign in on this machine to use it · no sessions running',
        { label: 'Signed Out', tone: 'warning' },
        false,
      ],
    ]);
  });

  it('HS2-S1FEYY reads sign-in from capabilities: unknown stays Installed, a missing tool never reads signed in', () => {
    const base = systemsDemoCapacity('local');
    const reported = (signIns: Record<string, boolean | undefined>, installed = true): OrchestrationCapacity => ({
      ...base,
      sessions: [],
      tools: [{ tool: 'codex', installed, configured: true }],
      capabilities: {
        ...base.capabilities!,
        ai_tools: Object.entries(signIns).map(([tool, signed_in]) =>
          signed_in === undefined ? { tool } : { tool, signed_in },
        ),
      },
    });
    const one = (capacity: OrchestrationCapacity) => {
      const [row] = aiToolRows(capacity)!;
      return [row.status.label, row.signedIn, row.description];
    };
    expect(one(reported({ codex: undefined }))).toEqual(['Installed', undefined, 'Installed · no sessions running']);
    expect(one(reported({ codex: true }))).toEqual(['Signed In', true, 'Signed in · no sessions running']);
    // Signing out and back in, as successive snapshots report it.
    expect(one(reported({ codex: false }))[0]).toBe('Signed Out');
    expect(one(reported({ codex: true }))[0]).toBe('Signed In');
    // A tool the machine says is not installed ignores a stale sign-in report.
    expect(one(reported({ codex: true }, false))).toEqual([
      'Not Installed',
      undefined,
      'Not installed on this machine',
    ]);
    // An older host without capabilities: Installed.
    expect(aiToolRows({ ...reported({}), capabilities: undefined })![0].status.label).toBe('Installed');
  });

  it('adds a tool the capabilities name that the tool list omits, and ignores sessions without a tool', () => {
    const capacity: OrchestrationCapacity = {
      ...systemsDemoCapacity('local'),
      tools: null,
      sessions: [
        { session_id: 'a', tool: null, busy: true },
        { session_id: 'b', tool: 'claude', busy: true },
      ],
    };
    const rows = aiToolRows(capacity)!;
    expect(rows.map((row) => [row.tool, row.sessions, row.installed])).toEqual([
      ['claude', 1, true],
      ['codex', 0, true],
      ['opencode', 0, true],
    ]);
  });

  it('builds Capabilities from capabilities and memory, with the host platform before they arrive', () => {
    expect(capabilityRows({ capacity: systemsDemoCapacity('build') })).toEqual([
      { label: 'System', value: 'Linux (ubuntu 24.04), x86_64' },
      { label: 'Processor', value: 'Example 10-Core Processor · 10 cores' },
      { label: 'GPU', value: 'None' },
      { label: 'Memory', value: '9.0 GB free of 32.0 GB' },
    ]);
    expect(capabilityRows({ health: hostHealthFixture('normal') })).toEqual([
      { label: 'System', value: 'macOS · aarch64' },
    ]);
    const bare: OrchestrationCapacity = {
      ...systemsDemoCapacity('local'),
      capabilities: undefined,
      cpu: { load_percent: null, logical_cpus: 1 },
      memory: { free_mb: 512, total_mb: null },
    };
    expect(capabilityRows({ capacity: bare })).toEqual([
      { label: 'Processor', value: '1 core' },
      { label: 'Memory', value: '512 MB' },
    ]);
    expect(capabilityRows({})).toEqual([]);
  });

  it('lists installed toolchains only; unreported is undefined', () => {
    expect(toolchainRows(undefined)).toBeUndefined();
    expect(
      toolchainRows({ ...systemsDemoCapacity('local'), capabilities: { os: 'linux', arch: 'x' } }),
    ).toBeUndefined();
    expect(toolchainRows(systemsDemoCapacity('build'))?.map((row) => row.label)).toEqual([
      'node',
      'cargo',
      'git',
      'docker',
    ]);
    const unversioned = {
      ...systemsDemoCapacity('build'),
      capabilities: { os: 'linux' as const, arch: 'x', toolchains: [{ name: 'go', installed: true }] },
    };
    expect(toolchainRows(unversioned)).toEqual([{ label: 'go', value: 'Installed' }]);
  });

  it('names each running session by its terminal when known', () => {
    expect(sessionRows(undefined, [])).toBeUndefined();
    expect(sessionRows({ ...systemsDemoCapacity('local'), sessions: null }, [])).toBeUndefined();
    const rows = sessionRows(
      systemsDemoCapacity('local'),
      [{ id: 't1', title: 'Worker', projectName: 'Hot Sheet 2' }],
      label,
    )!;
    expect(rows).toEqual([
      { id: 't1', title: 'Worker · Claude', description: 'Hot Sheet 2', busy: true },
      { id: 't2', title: 'Claude', description: 'Not shown in an open project tab', busy: false },
      { id: 't3', title: 'Codex', description: 'Not shown in an open project tab', busy: true },
    ]);
    const anonymous = sessionRows(
      { ...systemsDemoCapacity('local'), sessions: [{ session_id: 'x', tool: null, busy: false }] },
      [],
    );
    expect(anonymous?.[0].title).toBe('AI session');
  });

  it('merges AI Tool Capacity across projects by tool and account, first reading winning', () => {
    const view = (tools: OrchestrationToolCapacity['tools'], enabled = true): OrchestrationToolCapacity => ({
      enabled,
      now_ms: 0,
      tools,
    });
    const rows = toolCapacityRows(
      [
        view([
          {
            tool: 'claude',
            account: 'Work',
            state: 'available',
            verified: true,
            max_sessions: 20,
            weight: 1,
            budget_cents_per_day: 4000,
            spent_cents_today: 1400,
          },
          { tool: 'codex', state: 'limited', until_ms: 5, verified: true, max_sessions: 1, weight: 1 },
        ]),
        view([
          { tool: 'claude', account: 'Work', state: 'unknown', verified: false, max_sessions: 2, weight: 1 },
          { tool: 'claude', state: 'degraded', verified: false, max_sessions: 2, weight: 1 },
        ]),
        view([{ tool: 'gone', state: 'available', verified: true, max_sessions: 1, weight: 1 }], false),
      ],
      label,
      () => 'soon',
    );
    expect(rows.map((row) => [row.label, row.description, row.status.label])).toEqual([
      ['Claude · Work', 'Up to 20 sessions · $14 of $40 today', 'Available'],
      ['Codex', 'Until soon · Up to 1 session', 'Limited'],
      ['Claude', 'Up to 2 sessions', 'Degraded'],
    ]);
    expect(
      toolCapacityRows([view([{ tool: 'x', state: 'unknown', verified: false, max_sessions: 3, weight: 1 }])])[0]
        .status,
    ).toEqual({ label: 'Unknown', tone: 'neutral' });
  });
});

describe('HS2-JF3EA3 systemsMachineForDestination', () => {
  const local = { kind: 'local' as const, server_id: 's', checkout_id: 'c', checkout_authority: 'a' };
  const remote = {
    kind: 'remote' as const,
    registration_id: A,
    identity: { server_id: 'r' },
    checkout_id: 'c',
    checkout_authority: 'a',
  };
  it('maps a binding to the machine it runs on, as the serving tab sees it', () => {
    expect(systemsMachineForDestination(local, 'local-checkout')).toBe(LOCAL_MACHINE_ID);
    expect(systemsMachineForDestination(remote, 'local-checkout')).toBe(remoteMachineId(A));
    expect(systemsMachineForDestination(local, `remote:${B}:c1`)).toBe(remoteMachineId(B));
    expect(systemsMachineForDestination(remote, `remote:${B}:c1`)).toBeUndefined();
    expect(systemsMachineForDestination(null, 'local-checkout')).toBeUndefined();
    expect(systemsMachineForDestination(undefined, 'local-checkout')).toBeUndefined();
  });
});

describe('HS2-5H8R0N Admission by Project and Placement Limits', () => {
  const BIND = '01JSYNTHREMTE0000000000001';
  const site = (target: string, destination: OrchestrationSiteView['destination'], project = true, label = 'Widgets') =>
    ({
      target_id: target,
      label,
      project,
      destination,
      confirmed_at_ms: null,
      last_failure: null,
      stale: false,
    }) satisfies OrchestrationSiteView;
  const view: OrchestrationSitesView = {
    revision: null,
    projects: [
      {
        project_id: 'P1',
        label: 'Widgets',
        sites: [
          site('P1', { kind: 'local', server_id: 's1', checkout_id: 'c1', checkout_authority: 'a1' }),
          site(
            'S2',
            {
              kind: 'remote',
              registration_id: BIND,
              identity: { server_id: 's2' },
              checkout_id: 'c2',
              checkout_authority: 'a2',
            },
            false,
            'Build clone',
          ),
          site('S3', null, false, 'Unbound'),
        ],
      },
    ],
  };

  it('places each site on the machine its destination names, through a local owner and a remote one', () => {
    expect(sitesOnMachine('owner', view, 'local')).toEqual([{ owner: 'owner', targetId: 'P1', label: 'Widgets' }]);
    expect(sitesOnMachine('owner', view, `remote:${BIND}`)).toEqual([
      { owner: 'owner', targetId: 'S2', label: 'Widgets · Build clone' },
    ]);
    // Through a remote owner tab, a local destination is that remote; an unbound site is nowhere.
    expect(sitesOnMachine(`remote:${BIND}:c9`, view, `remote:${BIND}`).map((entry) => entry.targetId)).toEqual(['P1']);
    expect(sitesOnMachine('owner', { revision: null, projects: [] }, 'local')).toEqual([]);
  });

  it('reads each admission state as one row: checking, unknown, no tool, refused, reduced, accepted', () => {
    const placed = { owner: 'o', targetId: 'T', label: 'Widgets' };
    const ask = (
      outcome: OrchestrationAdmission['outcome'],
      admitted: number,
      tool = 'codex',
      reason: string | null = null,
    ): OrchestrationAdmission => ({ outcome, tool, requested: 2, admitted, reason });
    expect(admissionRow(placed, undefined).status.label).toBe('Checking');
    expect(admissionRow(placed, { status: 'loading' }).status).toEqual({ label: 'Checking', tone: 'neutral' });
    expect(admissionRow(placed, { status: 'error', message: 'Gone.' })).toMatchObject({
      status: { label: 'Unknown' },
      description: 'Admission is not available. Gone.',
    });
    expect(admissionRow(placed, { status: 'ready', admissions: [] }).status.label).toBe('No AI tool allowed here');
    expect(
      admissionRow(placed, { status: 'ready', admissions: [ask('refused', 0, 'codex', 'below_ram_floor')] }),
    ).toMatchObject({
      status: { label: 'Refused', tone: 'warning' },
      description: 'codex: Refused · memory below the floor',
    });
    expect(
      admissionRow(placed, { status: 'ready', admissions: [ask('reduced', 1, 'codex', 'session_cap_reached')] }).status,
    ).toEqual({ label: 'Can start 1 of 2', tone: 'warning' });
    // The best tool decides the badge; every tool is in the quiet line.
    expect(
      admissionRow(placed, { status: 'ready', admissions: [ask('refused', 0, 'claude'), ask('accepted', 2)] }, (tool) =>
        tool.toUpperCase(),
      ),
    ).toEqual({
      key: 'o\u0000T',
      label: 'Widgets',
      status: { label: 'Can start 2 now', tone: 'success' },
      description: 'CLAUDE: Refused · CODEX: Can start 2 now',
    });
  });

  it('lists the owner’s four limits, unset ones left to each project, and nothing from an older host', () => {
    const base = systemsDemoCapacity('local');
    expect(placementLimitRows(undefined)).toBeUndefined();
    expect(placementLimitRows({ ...base, owner_limits: undefined })).toBeUndefined();
    expect(placementLimitRows(base)).toEqual([
      { label: 'Sessions per Machine', value: '10' },
      { label: 'Free Memory Floor', value: '1.0 GB' },
      { label: 'CPU Ceiling', value: '95%' },
      { label: 'Free Disk Floor', value: '5.0 GB' },
    ]);
    expect(placementLimitRows({ ...base, owner_limits: { max_sessions: 0, min_free_disk_mb: 512 } })).toEqual([
      { label: 'Sessions per Machine', value: '0' },
      { label: 'Free Memory Floor', value: 'Set by each project' },
      { label: 'CPU Ceiling', value: 'Set by each project' },
      { label: 'Free Disk Floor', value: '512 MB' },
    ]);
  });
});
