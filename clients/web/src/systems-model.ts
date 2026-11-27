/**
 * The Systems screen's machines and what each one shows (HS2-DR0ZSQ, docs/06-clients.md §6.13).
 * Pure: the runtime passes the open project tabs, the paired remote servers, and each machine's last
 * readings (host health, the hosted capacity snapshot, the live CPU frame); these functions decide
 * the machine list, the rail rows, and the detail's tables. A reading the machine did not give reads
 * as unknown, never as zero.
 */
import type {
  OrchestrationAdmission,
  OrchestrationCapacity,
  OrchestrationDestination,
  OrchestrationSitesView,
  OrchestrationToolCapacity,
} from './api';
import type { HostCpuFrame } from './host-cpu-feed';
import type { HostHealthSnapshot } from './host-health';
import { gpuLabel, systemLabel } from './orchestration-capabilities';
import { admissionValue, formatMegabytes } from './orchestration-settings-model';
import { parseRemoteProjectId, remoteHostLabel } from './remote-projects/identity';
import { hostPlatform } from './stats-this-mac';

/** The id of this computer's entry; a remote's id is `remote:<binding id>`. */
export const LOCAL_MACHINE_ID = 'local';

export type SystemsMachineKind = 'local' | 'remote';

/** Why a machine has no live readings. */
export type SystemsMachineUnavailable = 'no-project' | 'not-connected' | 'revoked' | 'unreachable';

export interface SystemsMachine {
  id: string;
  kind: SystemsMachineKind;
  name: string;
  /** The open project tab whose path reaches this machine; its reads go through that path. */
  projectId?: string;
  /** A remote's binding id (the desktop's remote list). */
  bindingId?: string;
  /**
   * HS2-CMXKK2: a paired remote with no open tab, read through the bridge's machine path for its
   * binding (`/__hotsheet/remotes/{binding}/machine`) instead of a tab.
   */
  direct?: boolean;
  unavailable?: SystemsMachineUnavailable;
  /** HS2-CMXKK2: why a remote read through its binding could not be reached. */
  failure?: string;
}

/**
 * HS2-CMXKK2: a remote read without a tab whose first capacity read and host health both failed is
 * Not reachable, with why; its streams keep retrying with backoff, and the first reading clears it.
 */
export function reachableMachine(machine: SystemsMachine, readings: SystemsMachineReadings, error?: string) {
  if (!machine.direct || !error || readings.capacity || readings.health) return machine;
  return { ...machine, unavailable: 'unreachable' as const, failure: error };
}

/** The key one machine's reads go through: its tab, else its binding's machine path. */
export function machineReadKey(machine: SystemsMachine): string | undefined {
  if (machine.projectId) return machine.projectId;
  return machine.direct && machine.bindingId ? `machine:${machine.bindingId}` : undefined;
}

/** Whether a machine has live readings to show. */
export const machineConnected = (machine: SystemsMachine) => machineReadKey(machine) !== undefined;

/** An open project tab, as the machine list needs it. */
export interface SystemsOpenProject {
  id: string;
  /** A remote tab's machine name (HS2-CW8EY3). */
  remoteHost?: string;
}

/** A paired remote server (the desktop's remote list). */
export interface SystemsRemoteServer {
  id: string;
  label: string;
  origin: string;
  revoked: boolean;
}

export const remoteMachineId = (bindingId: string) => `remote:${bindingId}`;

/**
 * This computer first, then each paired remote server in its listed order, then any remote an open
 * tab reaches that the list does not name (a browser without the desktop's remote list). A machine
 * is read through its first open tab. HS2-CMXKK2: a paired remote without an open tab is read
 * through the bridge's machine path when this host offers one (`direct`), else it is not connected.
 */
export function systemsMachines({
  projects,
  remoteServers,
  localName,
  direct = false,
}: {
  projects: readonly SystemsOpenProject[];
  remoteServers: readonly SystemsRemoteServer[];
  /** The local server's host name, once host health has been read. */
  localName?: string;
  /** Whether the bridge reads a paired remote's machine without an open tab (HS2-CMXKK2). */
  direct?: boolean;
}): SystemsMachine[] {
  const local = projects.find((project) => !parseRemoteProjectId(project.id));
  const tabsByBinding = new Map<string, SystemsOpenProject>();
  for (const project of projects) {
    const remote = parseRemoteProjectId(project.id);
    if (remote && !tabsByBinding.has(remote.bindingId)) tabsByBinding.set(remote.bindingId, project);
  }
  const machines: SystemsMachine[] = [
    {
      id: LOCAL_MACHINE_ID,
      kind: 'local',
      name: localName?.trim() || 'This Computer',
      ...(local ? { projectId: local.id } : { unavailable: 'no-project' as const }),
    },
  ];
  const listed = new Set<string>();
  for (const server of remoteServers) {
    if (listed.has(server.id)) continue;
    listed.add(server.id);
    const tab = tabsByBinding.get(server.id);
    machines.push({
      id: remoteMachineId(server.id),
      kind: 'remote',
      name: tab?.remoteHost?.trim() || remoteHostLabel(null, server.origin) || server.label,
      bindingId: server.id,
      ...(server.revoked
        ? { unavailable: 'revoked' as const }
        : tab
          ? { projectId: tab.id }
          : direct
            ? { direct: true }
            : { unavailable: 'not-connected' as const }),
    });
  }
  for (const [bindingId, tab] of tabsByBinding) {
    if (listed.has(bindingId)) continue;
    machines.push({
      id: remoteMachineId(bindingId),
      kind: 'remote',
      name: tab.remoteHost?.trim() || 'Remote Server',
      bindingId,
      projectId: tab.id,
    });
  }
  return machines;
}

/**
 * HS2-JF3EA3: the Systems machine an orchestration binding runs on, for Open in Systems. A binding is
 * the serving tab's: through a local tab, a local destination is This Computer and a remote one is
 * that registration; through a remote tab, a local destination is that remote, and a remote one is a
 * machine this computer may not know, so it opens Systems without choosing.
 */
export function systemsMachineForDestination(
  destination: OrchestrationDestination | null | undefined,
  servingProjectId: string,
): string | undefined {
  if (!destination) return undefined;
  const serving = parseRemoteProjectId(servingProjectId);
  if (destination.kind === 'local') return serving ? remoteMachineId(serving.bindingId) : LOCAL_MACHINE_ID;
  return serving ? undefined : remoteMachineId(destination.registration_id);
}

/** The selected machine, else this computer (a selected remote that went away falls back). */
export function selectedSystemsMachine(machines: readonly SystemsMachine[], selected: string | undefined) {
  return machines.find((machine) => machine.id === selected) ?? machines[0];
}

/** The live readings one machine has, any of which may be missing. */
export interface SystemsMachineReadings {
  health?: HostHealthSnapshot;
  capacity?: OrchestrationCapacity;
  /** The live CPU frame, only for the selected machine while Systems shows. */
  cpu?: HostCpuFrame;
}

/** How many AI sessions run on the machine, or `undefined` while unknown. */
export function aiSessionCount(capacity: OrchestrationCapacity | undefined): number | undefined {
  return capacity?.sessions?.length ?? undefined;
}

const plural = (count: number, one: string, many = `${one}s`) => `${String(count)} ${count === 1 ? one : many}`;

/** The machine's kind in words: the inspector's subtitle and the detail's context line. */
export function machineKindLabel(machine: SystemsMachine): string {
  return machine.kind === 'local' ? 'This Computer' : 'Paired Remote';
}

/** The quiet line under a machine's name in the Machines list. */
export function machineRowDescription(machine: SystemsMachine, readings: SystemsMachineReadings): string {
  if (machine.unavailable === 'revoked') return 'Approval revoked';
  if (machine.unavailable === 'not-connected') return 'Not connected';
  if (machine.unavailable === 'unreachable') return 'Not reachable';
  if (machine.unavailable === 'no-project') return `${machineKindLabel(machine)} · no project open`;
  const sessions = aiSessionCount(readings.capacity);
  return [machineKindLabel(machine), sessions === undefined ? undefined : plural(sessions, 'AI session')]
    .filter(Boolean)
    .join(' · ');
}

/** The CPU busy percent the Machines list's gauge shows: the live frame, else the host-health sample. */
export function machineCpuPercent(readings: SystemsMachineReadings): number | undefined {
  const live = readings.cpu?.status === 'live' ? readings.cpu.busy_percent : null;
  const value = live ?? readings.health?.signals.cpu.busy_percent ?? readings.capacity?.cpu.load_percent ?? null;
  return value === null ? undefined : Math.max(0, Math.min(100, value));
}

/** Why a machine shows no readings, as an empty state's title and detail. */
export function machineUnavailableMessage(machine: SystemsMachine): { title: string; detail: string } | undefined {
  if (machine.unavailable === 'revoked')
    return {
      title: `${machine.name} Revoked This Computer`,
      detail: 'Pair this computer with it again to see its readings here.',
    };
  if (machine.unavailable === 'not-connected')
    return {
      title: `${machine.name} Is Not Connected`,
      detail:
        'This app cannot reach it without an open project. Open one of its projects in a tab to see its live readings here.',
    };
  if (machine.unavailable === 'unreachable')
    return {
      title: `${machine.name} Is Not Reachable`,
      detail: `${machine.failure ?? 'It did not answer.'} Check that it is running and that this computer’s approval is current; it is tried again on its own.`,
    };
  if (machine.unavailable === 'no-project')
    return {
      title: 'No Project Is Open on This Computer',
      detail: 'Open a project to see this computer’s live readings here.',
    };
  return undefined;
}

export type SystemsBadgeTone = 'success' | 'warning' | 'danger' | 'neutral';

/** One AI tool on the machine: installed or not, its version, and how many of its sessions run. */
export interface SystemsAiToolRow {
  tool: string;
  label: string;
  description: string;
  status: { label: string; tone: SystemsBadgeTone };
  installed: boolean;
  /** HS2-S1FEYY: whether it is signed in there; absent while the machine cannot tell. */
  signedIn?: boolean;
  sessions: number;
}

/** HS2-S1FEYY: the AI Tools badge: Signed In, Signed Out, Not Installed, or Installed when sign-in is unknown. */
function aiToolStatus(installed: boolean, signedIn: boolean | undefined): SystemsAiToolRow['status'] {
  if (!installed) return { label: 'Not Installed', tone: 'neutral' };
  if (signedIn === true) return { label: 'Signed In', tone: 'success' };
  if (signedIn === false) return { label: 'Signed Out', tone: 'warning' };
  return { label: 'Installed', tone: 'success' };
}

/** The row's quiet line: its sign-in state (when known) and its running sessions. */
function aiToolDescription(installed: boolean, signedIn: boolean | undefined, count: number): string {
  if (!installed) return 'Not installed on this machine';
  const running = count ? `${plural(count, 'session')} running` : 'no sessions running';
  if (signedIn === false) return `Installed, not signed in · sign in on this machine to use it · ${running}`;
  if (signedIn === true) return `Signed in · ${running}`;
  return count ? running : `Installed · ${running}`;
}

/**
 * The machine's AI tools: every tool the capacity snapshot names, installed ones first. Versions and
 * sign-in state (HS2-S1FEYY) come from the capabilities report; sessions are counted by tool.
 * `undefined` while the snapshot is unread.
 */
export function aiToolRows(
  capacity: OrchestrationCapacity | undefined,
  toolLabel: (tool: string) => string = (tool) => tool,
): SystemsAiToolRow[] | undefined {
  if (!capacity) return undefined;
  const reported = capacity.capabilities?.ai_tools ?? [];
  const versions = new Map(reported.map((tool) => [tool.tool, tool.version]));
  const signIns = new Map(reported.map((tool) => [tool.tool, tool.signed_in]));
  const installed = new Map<string, boolean>();
  for (const tool of capacity.tools ?? []) installed.set(tool.tool, tool.installed);
  for (const tool of versions.keys()) if (!installed.has(tool)) installed.set(tool, true);
  const sessions = new Map<string, number>();
  for (const session of capacity.sessions ?? [])
    if (session.tool) sessions.set(session.tool, (sessions.get(session.tool) ?? 0) + 1);
  return [...installed]
    .map(([tool, present]): SystemsAiToolRow => {
      const count = sessions.get(tool) ?? 0;
      const version = versions.get(tool);
      const signedIn = present ? signIns.get(tool) : undefined;
      return {
        tool,
        label: [toolLabel(tool), version].filter(Boolean).join(' '),
        description: aiToolDescription(present, signedIn, count),
        status: aiToolStatus(present, signedIn),
        installed: present,
        ...(signedIn === undefined ? {} : { signedIn }),
        sessions: count,
      };
    })
    .sort((a, b) => Number(b.installed) - Number(a.installed) || a.label.localeCompare(b.label));
}

export interface SystemsValueRow {
  label: string;
  value: string;
}

/**
 * The Capabilities table: System, Processor, GPU and Memory. Memory reads free of total from the
 * capacity snapshot; the host platform stands in for System until capabilities are reported.
 */
export function capabilityRows(readings: SystemsMachineReadings): SystemsValueRow[] {
  const { capacity, health } = readings;
  const capabilities = capacity?.capabilities;
  const cores = capacity?.cpu.logical_cpus ?? null;
  const rows: SystemsValueRow[] = [];
  if (capabilities) rows.push({ label: 'System', value: systemLabel(capabilities) });
  else if (health) rows.push({ label: 'System', value: hostPlatform(health) });
  const processor = [capabilities?.cpu_model, cores === null ? undefined : plural(cores, 'core')]
    .filter(Boolean)
    .join(' · ');
  if (processor) rows.push({ label: 'Processor', value: processor });
  if (capabilities) rows.push({ label: 'GPU', value: gpuLabel(capabilities) });
  if (capacity)
    rows.push({
      label: 'Memory',
      value:
        capacity.memory.total_mb === null
          ? formatMegabytes(capacity.memory.free_mb)
          : `${formatMegabytes(capacity.memory.free_mb)} free of ${formatMegabytes(capacity.memory.total_mb)}`,
    });
  return rows;
}

/** The Toolchains table: each installed toolchain and its version; `undefined` while unreported. */
export function toolchainRows(capacity: OrchestrationCapacity | undefined): SystemsValueRow[] | undefined {
  const toolchains = capacity?.capabilities?.toolchains;
  if (!toolchains) return undefined;
  return toolchains
    .filter((toolchain) => toolchain.installed)
    .map((toolchain) => ({ label: toolchain.name, value: toolchain.version ?? 'Installed' }));
}

/** A terminal the workspace knows, used to name a running session. */
export interface SystemsKnownTerminal {
  id: string;
  title?: string;
  projectName: string;
}

export interface SystemsSessionRow {
  id: string;
  title: string;
  description: string;
  busy: boolean;
}

/** The inspector's AI Sessions Here: each running AI session, named by its terminal when it is known. */
export function sessionRows(
  capacity: OrchestrationCapacity | undefined,
  terminals: readonly SystemsKnownTerminal[],
  toolLabel: (tool: string) => string = (tool) => tool,
): SystemsSessionRow[] | undefined {
  if (!capacity?.sessions) return undefined;
  const known = new Map(terminals.map((terminal) => [terminal.id, terminal]));
  return capacity.sessions.map((session) => {
    const terminal = known.get(session.session_id);
    const tool = session.tool ? toolLabel(session.tool) : 'AI session';
    return {
      id: session.session_id,
      title: terminal?.title ? `${terminal.title} · ${tool}` : tool,
      description: terminal?.projectName ?? 'Not shown in an open project tab',
      busy: session.busy,
    };
  });
}

export interface SystemsToolCapacityRow {
  key: string;
  label: string;
  description: string;
  status: { label: string; tone: SystemsBadgeTone };
}

/**
 * AI Tool Capacity (HS2-AHG7HW) across the open orchestration projects: one row per tool and
 * account, the first project's reading winning, since the observed state is the machine's.
 */
export function toolCapacityRows(
  views: readonly OrchestrationToolCapacity[],
  toolLabel: (tool: string) => string = (tool) => tool,
  formatTime: (ms: number) => string = (ms) => new Date(ms).toLocaleTimeString(),
): SystemsToolCapacityRow[] {
  const rows = new Map<string, SystemsToolCapacityRow>();
  for (const view of views) {
    if (!view.enabled) continue;
    for (const row of view.tools) {
      const key = `${row.tool}\u0000${row.account ?? ''}`;
      if (rows.has(key)) continue;
      const budget =
        row.budget_cents_per_day === undefined
          ? undefined
          : `$${String(Math.round((row.spent_cents_today ?? 0) / 100))} of $${String(Math.round(row.budget_cents_per_day / 100))} today`;
      rows.set(key, {
        key,
        label: row.account ? `${toolLabel(row.tool)} · ${row.account}` : toolLabel(row.tool),
        description: [
          row.state === 'limited' && row.until_ms ? `Until ${formatTime(row.until_ms)}` : undefined,
          `Up to ${plural(row.max_sessions, 'session')}`,
          budget,
        ]
          .filter(Boolean)
          .join(' · '),
        status:
          row.state === 'available'
            ? { label: 'Available', tone: 'success' }
            : row.state === 'limited'
              ? { label: 'Limited', tone: 'warning' }
              : row.state === 'degraded'
                ? { label: 'Degraded', tone: 'warning' }
                : { label: 'Unknown', tone: 'neutral' },
      });
    }
  }
  return [...rows.values()];
}

/* ---- Admission by Project and Placement Limits (HS2-5H8R0N) ------------------------------------- */

/** One orchestrated project's site placed on a machine, read through its owning orchestration project. */
export interface SystemsPlacedSite {
  owner: string;
  targetId: string;
  label: string;
}

/**
 * The sites of an orchestration project (`owner`, a local tab) that run on `machineId`, from its sites
 * view (HS2-SHH7WW): each site's destination names its machine (`systemsMachineForDestination`). A
 * home site reads as its project; another site adds its own label.
 */
export function sitesOnMachine(owner: string, view: OrchestrationSitesView, machineId: string): SystemsPlacedSite[] {
  return view.projects.flatMap((project) =>
    project.sites
      .filter((site) => systemsMachineForDestination(site.destination, owner) === machineId)
      .map((site) => ({
        owner,
        targetId: site.target_id,
        label: site.project || site.label === project.label ? project.label : `${project.label} · ${site.label}`,
      })),
  );
}

/** What Systems last read of one placed site's admission. */
export type SystemsAdmissionState =
  | { status: 'loading' }
  | { status: 'ready'; admissions: readonly OrchestrationAdmission[] }
  | { status: 'error'; message: string };

export interface SystemsAdmissionRow {
  key: string;
  label: string;
  status: { label: string; tone: SystemsBadgeTone };
  description?: string;
}

/**
 * One Admission by Project row: the best answer as its badge ("Can start N now", the reduced count, or
 * Refused), each tool's answer in the quiet line, and "No AI tool allowed here" when the policy lets
 * no tool run there. The same answers as the orchestration inspector's Admission (HS2-JF3EA3).
 */
export function admissionRow(
  site: SystemsPlacedSite,
  state: SystemsAdmissionState | undefined,
  toolLabel: (tool: string) => string = (tool) => tool,
): SystemsAdmissionRow {
  const key = `${site.owner}\u0000${site.targetId}`;
  if (!state || state.status === 'loading')
    return { key, label: site.label, status: { label: 'Checking', tone: 'neutral' } };
  if (state.status === 'error')
    return {
      key,
      label: site.label,
      status: { label: 'Unknown', tone: 'neutral' },
      description: `Admission is not available. ${state.message}`,
    };
  if (state.admissions.length === 0)
    return { key, label: site.label, status: { label: 'No AI tool allowed here', tone: 'neutral' } };
  const best = [...state.admissions].sort((a, b) => b.admitted - a.admitted)[0];
  const status =
    best.admitted === 0
      ? { label: 'Refused', tone: 'warning' as const }
      : best.outcome === 'accepted'
        ? { label: `Can start ${String(best.admitted)} now`, tone: 'success' as const }
        : { label: `Can start ${String(best.admitted)} of ${String(best.requested)}`, tone: 'warning' as const };
  return {
    key,
    label: site.label,
    status,
    description: state.admissions
      .map((admission) => `${toolLabel(admission.tool)}: ${admissionValue(admission)}`)
      .join(' · '),
  };
}

/**
 * The Placement Limits table: the machine owner's four limits (`orchestration admission set`). An unset
 * one leaves each project's policy value, so it says so. `undefined` while the snapshot is unread or the
 * machine does not report its limits (an older host).
 */
export function placementLimitRows(capacity: OrchestrationCapacity | undefined): SystemsValueRow[] | undefined {
  const limits = capacity?.owner_limits;
  if (!limits) return undefined;
  const policy = 'Set by each project';
  return [
    {
      label: 'Sessions per Machine',
      value: limits.max_sessions === undefined ? policy : String(limits.max_sessions),
    },
    {
      label: 'Free Memory Floor',
      value: limits.min_free_ram_mb === undefined ? policy : formatMegabytes(limits.min_free_ram_mb),
    },
    {
      label: 'CPU Ceiling',
      value: limits.max_cpu_percent === undefined ? policy : `${String(limits.max_cpu_percent)}%`,
    },
    {
      label: 'Free Disk Floor',
      value: limits.min_free_disk_mb === undefined ? policy : formatMegabytes(limits.min_free_disk_mb),
    },
  ];
}
