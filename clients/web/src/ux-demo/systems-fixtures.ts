import type { OrchestrationCapacity } from '../api';
import type { SystemsInspectorProps } from '../components/systems-inspector';
import type { SystemsSidebarProps, SystemsToolCapacityState } from '../components/systems-sidebar';
import type { SystemsViewProps } from '../components/systems-view';
import type { HostCpuFrame } from '../host-cpu-feed';
import {
  admissionRow,
  machineCpuPercent,
  machineRowDescription,
  placementLimitRows,
  sessionRows,
  type SystemsMachine,
  type SystemsMachineReadings,
  toolCapacityRows,
} from '../systems-model';
import { hostHealthFixture, type HostHealthScenario } from './host-health-fixtures';

/** Synthetic machines for the Systems demos (HS2-DR0ZSQ); no real host or customer data. */
export type SystemsDemoMachine = 'local' | 'build' | 'offline' | 'unreachable' | 'revoked';
export type SystemsDemoScenario = 'ready' | 'loading' | 'failed' | 'stale';

const BINDING = '01JSYNTHREMTE0000000000001';
export const SYSTEMS_DEMO_MACHINES: Readonly<Record<SystemsDemoMachine, SystemsMachine>> = {
  local: { id: 'local', kind: 'local', name: 'Studio', projectId: 'demo-local' },
  build: { id: `remote:${BINDING}`, kind: 'remote', name: 'Build Host', projectId: `remote:${BINDING}:demo` },
  offline: {
    id: 'remote:01JSYNTHREMTE0000000000002',
    kind: 'remote',
    name: 'Travel Laptop',
    unavailable: 'not-connected',
  },
  // HS2-CMXKK2: a paired remote read without a tab whose machine path did not answer.
  unreachable: {
    id: 'remote:01JSYNTHREMTE0000000000004',
    kind: 'remote',
    name: 'Render Box',
    bindingId: '01JSYNTHREMTE0000000000004',
    direct: true,
    unavailable: 'unreachable',
    failure: 'Remote server is unavailable or its approval changed.',
  },
  revoked: { id: 'remote:01JSYNTHREMTE0000000000003', kind: 'remote', name: 'Lab Box', unavailable: 'revoked' },
};

export const systemsDemoToolLabel = (tool: string) =>
  ({ claude: 'Claude', codex: 'Codex', opencode: 'OpenCode' })[tool] ?? tool;

/** A synthetic capacity snapshot: what the machine is and its running sessions. */
export function systemsDemoCapacity(machine: SystemsDemoMachine): OrchestrationCapacity {
  const build = machine === 'build';
  return {
    sampled_at_ms: 1_794_000_000_000,
    memory: { free_mb: build ? 9_216 : 22_630, total_mb: build ? 32_768 : 65_536 },
    cpu: { load_percent: build ? 86 : 38, logical_cpus: build ? 10 : 12 },
    disk: { free_mb: build ? 182_000 : 1_232_000, total_mb: build ? 494_000 : 1_907_000 },
    tools: [
      { tool: 'claude', installed: true, configured: true },
      { tool: 'codex', installed: true, configured: true },
      { tool: 'opencode', installed: !build, configured: false },
    ],
    sessions: build
      ? [
          { session_id: 'b1', tool: 'claude', busy: true },
          { session_id: 'b2', tool: 'codex', busy: false },
        ]
      : [
          { session_id: 't1', tool: 'claude', busy: true },
          { session_id: 't2', tool: 'claude', busy: false },
          { session_id: 't3', tool: 'codex', busy: true },
        ],
    claims: [],
    claim_namespaces: [],
    capabilities: {
      os: build ? 'linux' : 'macos',
      ...(build ? { os_distribution: 'ubuntu', os_version: '24.04' } : { os_version: '15.1' }),
      arch: build ? 'x86_64' : 'arm64',
      cpu_model: build ? 'Example 10-Core Processor' : 'Example M-Series',
      gpu: build ? { present: false } : { present: true, vendors: ['apple'] },
      ai_tools: [
        // HS2-S1FEYY: signed in, signed out, and (Codex on the build host) unknown.
        { tool: 'claude', version: '2.1.0', signed_in: true },
        { tool: 'codex', version: '0.40.0', ...(build ? {} : { signed_in: true }) },
        ...(build ? [] : [{ tool: 'opencode', version: '1.2.0', signed_in: false }]),
      ],
      toolchains: [
        { name: 'node', installed: true, version: '22.10.0' },
        { name: 'cargo', installed: true, version: '1.90.0' },
        { name: 'git', installed: true, version: '2.50.0' },
        { name: 'docker', installed: build, ...(build ? { version: '27.0.0' } : {}) },
        { name: 'xcode', installed: !build, ...(build ? {} : { version: '17.0' }) },
      ],
    },
    version: `demo-${machine}`,
    // HS2-5H8R0N: this computer's owner set every limit; the build host's set none.
    owner_limits: build ? {} : { max_sessions: 10, min_free_ram_mb: 1024, max_cpu_percent: 95, min_free_disk_mb: 5120 },
  };
}

/** HS2-5H8R0N: synthetic orchestrated projects placed on this computer, with their admission answers. */
const DEMO_ADMISSION = [
  admissionRow(
    { owner: 'demo-local', targetId: 'P1', label: 'Hot Sheet 2' },
    {
      status: 'ready',
      admissions: [
        { outcome: 'accepted', tool: 'claude', requested: 1, admitted: 1, reason: null },
        { outcome: 'accepted', tool: 'codex', requested: 1, admitted: 1, reason: null },
      ],
    },
    systemsDemoToolLabel,
  ),
  admissionRow(
    { owner: 'demo-local', targetId: 'P2', label: 'Website' },
    {
      status: 'ready',
      admissions: [{ outcome: 'refused', tool: 'claude', requested: 1, admitted: 0, reason: 'session_cap_reached' }],
    },
    systemsDemoToolLabel,
  ),
  admissionRow(
    { owner: 'demo-local', targetId: 'P3', label: 'Internal API' },
    { status: 'ready', admissions: [] },
    systemsDemoToolLabel,
  ),
];

/** A synthetic live CPU frame. */
export function systemsDemoCpu(machine: SystemsDemoMachine): HostCpuFrame {
  const busy = machine === 'build' ? 86 : 38;
  return {
    seq: 7,
    status: 'live',
    sampled_at: '2026-11-27T18:47:00Z',
    busy_percent: busy,
    core_percent: Array.from({ length: machine === 'build' ? 10 : 12 }, (_, index) => (busy + index * 7) % 100),
    interval_ms: 2_000,
  };
}

const DEMO_TERMINALS = [
  { id: 't1', title: 'Worker', projectName: 'Hot Sheet 2' },
  { id: 't3', title: 'Integrator', projectName: 'Hot Sheet 2' },
];

const HEALTH: Partial<Record<SystemsDemoMachine, HostHealthScenario>> = { local: 'normal', build: 'crossed' };

function readings(machine: SystemsDemoMachine, scenario: SystemsDemoScenario): SystemsMachineReadings {
  if (SYSTEMS_DEMO_MACHINES[machine].unavailable) return {};
  const health = hostHealthFixture(HEALTH[machine] ?? 'normal');
  return {
    health: { ...health, host: { ...health.host, name: SYSTEMS_DEMO_MACHINES[machine].name } },
    ...(scenario === 'loading' || scenario === 'failed' ? {} : { capacity: systemsDemoCapacity(machine) }),
    cpu: systemsDemoCpu(machine),
  };
}

/** The workspace's props for one machine and reading state. */
export function systemsDemoView(machine: SystemsDemoMachine, scenario: SystemsDemoScenario): SystemsViewProps {
  const read = readings(machine, scenario);
  return {
    machine: SYSTEMS_DEMO_MACHINES[machine],
    health: read.health ? { snapshot: read.health, loading: false } : { loading: scenario === 'loading' },
    ...(read.cpu ? { liveCpu: read.cpu } : {}),
    capacity: {
      ...(read.capacity ? { capacity: read.capacity } : {}),
      ...(scenario === 'failed' || scenario === 'stale' ? { error: 'The Hot Sheet server could not be reached.' } : {}),
    },
    toolLabel: systemsDemoToolLabel,
  };
}

const DEMO_TOOL_CAPACITY: SystemsToolCapacityState = {
  status: 'ready',
  rows: toolCapacityRows(
    [
      {
        enabled: true,
        now_ms: 1_794_000_000_000,
        tools: [
          {
            tool: 'claude',
            account: 'Work',
            state: 'available',
            verified: true,
            max_sessions: 20,
            weight: 1,
            budget_cents_per_day: 4_000,
            spent_cents_today: 1_400,
          },
          {
            tool: 'claude',
            account: 'Personal',
            state: 'limited',
            until_ms: 1_794_003_600_000,
            verified: true,
            max_sessions: 4,
            weight: 1,
          },
          { tool: 'codex', account: 'Work', state: 'available', verified: true, max_sessions: 10, weight: 1 },
        ],
      },
    ],
    systemsDemoToolLabel,
    () => '3:40 PM',
  ),
};

/** The left rail's props: every demo machine, the chosen one selected. */
export function systemsDemoSidebar(
  selected: SystemsDemoMachine,
  scenario: SystemsDemoScenario,
  toolCapacity: SystemsToolCapacityState = DEMO_TOOL_CAPACITY,
): SystemsSidebarProps {
  return {
    machines: (Object.keys(SYSTEMS_DEMO_MACHINES) as SystemsDemoMachine[]).map((key) => {
      const machine = SYSTEMS_DEMO_MACHINES[key];
      const read = readings(key, scenario);
      const cpu = machine.unavailable ? undefined : machineCpuPercent(read);
      return {
        machine,
        description: machineRowDescription(machine, read),
        ...(cpu === undefined ? {} : { cpu }),
      };
    }),
    selectedId: SYSTEMS_DEMO_MACHINES[selected].id,
    toolCapacity,
  };
}

/** The right rail's props for one machine. */
export function systemsDemoInspector(
  machine: SystemsDemoMachine,
  scenario: SystemsDemoScenario,
): SystemsInspectorProps {
  const read = readings(machine, scenario);
  const sessions = sessionRows(read.capacity, DEMO_TERMINALS, systemsDemoToolLabel);
  const limits = placementLimitRows(read.capacity);
  return {
    machine: SYSTEMS_DEMO_MACHINES[machine],
    ...(sessions ? { sessions } : {}),
    ...(scenario === 'failed' ? { error: 'The Hot Sheet server could not be reached.' } : {}),
    admission: machine === 'local' && read.capacity ? DEMO_ADMISSION : [],
    orchestrating: true,
    ...(limits ? { limits } : {}),
    capacityRead: Boolean(read.capacity),
  };
}
