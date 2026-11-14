/**
 * Pure helpers behind Project Settings → General → Project Type and Settings → Orchestration
 * (HS2-W27N1D, docs/26 §26.25). The controller and components call these; nothing here reads the
 * network or the DOM.
 */
import type {
  OrchestrationBinding,
  OrchestrationDestination,
  OrchestrationManifest,
  OrchestrationPolicyBody,
  OrchestrationPolicyView,
  OrchestrationResolution,
  OrchestrationSession,
} from './api';
import { parseRemoteProjectId, remoteProjectId } from './remote-projects/identity';

/** What an add asks the server to bind: a checkout here or on a reviewed remote registration. */
export type AddDestination =
  { kind: 'local'; checkout_id: string } | { kind: 'remote'; registration_id: string; checkout_id: string };

/** The machine a project lives on, as the person reads it. */
export const LOCAL_MACHINE_LABEL = 'This Computer';
export const UNKNOWN_REMOTE_LABEL = 'Remote Server';

/** An open tab's project as the add needs it: local tab ids are checkout ids, remote ones encode both. */
export function destinationOf(projectId: string): AddDestination {
  const remote = parseRemoteProjectId(projectId);
  return remote
    ? { kind: 'remote', registration_id: remote.bindingId, checkout_id: remote.checkoutId }
    : { kind: 'local', checkout_id: projectId };
}

/** The open tab that shows a bound project, so its row can show that tab or its sessions. */
export function tabProjectIdOf(destination: OrchestrationDestination): string {
  return destination.kind === 'local'
    ? destination.checkout_id
    : remoteProjectId(destination.registration_id, destination.checkout_id);
}

const placeOf = (destination: AddDestination | OrchestrationDestination) =>
  destination.kind === 'local'
    ? `local:${destination.checkout_id}`
    : `remote:${destination.registration_id}:${destination.checkout_id}`;

export function machineOf(
  destination: AddDestination | OrchestrationDestination,
  remoteLabels: Readonly<Record<string, string>>,
): string {
  return destination.kind === 'local'
    ? LOCAL_MACHINE_LABEL
    : (remoteLabels[destination.registration_id] ?? UNKNOWN_REMOTE_LABEL);
}

export type StatusTone = 'success' | 'warning' | 'danger' | 'neutral';
export interface ConnectionStatus {
  label: string;
  tone: StatusTone;
}

const FAILURE_LABELS: Readonly<Record<string, string>> = {
  unreachable: 'Unreachable',
  unauthorized: 'Not Authorized',
  source_missing: 'Project Missing',
  identity_changed: 'Identity Changed',
  revoked: 'Server Removed',
  missing_credentials: 'Sign-In Needed',
  trust_failed: 'Not Trusted',
  review_required: 'Review Needed',
  disabled_source: 'Source Disabled',
  target_removed: 'Removed',
};

/** The connection state words and tone for a row or the detail. A fresh resolution wins over the cache. */
export function connectionStatus(
  binding: OrchestrationBinding | undefined,
  resolution?: OrchestrationResolution,
): ConnectionStatus {
  if (resolution) {
    if (resolution.usable) return { label: 'Connected', tone: 'success' };
    if (resolution.state === 'unbound') return { label: 'Not Bound', tone: 'warning' };
    if (resolution.state === 'connecting') return { label: 'Connecting', tone: 'neutral' };
    return {
      label: FAILURE_LABELS[resolution.reason ?? resolution.state] ?? 'Unavailable',
      tone: 'danger',
    };
  }
  if (!binding) return { label: 'Checking', tone: 'neutral' };
  if (!binding.destination) return { label: 'Not Bound', tone: 'warning' };
  if (binding.last_failure) return { label: FAILURE_LABELS[binding.last_failure] ?? 'Unavailable', tone: 'danger' };
  return { label: binding.confirmed_at_ms ? 'Bound' : 'Not Confirmed', tone: 'neutral' };
}

export interface OrchestrationProjectRow {
  targetId: string;
  label: string;
  location: 'local' | 'remote' | 'unbound';
  /** "Local on This Computer", "Remote on Build host", or "Not bound on this computer". */
  where: string;
  connection: ConnectionStatus;
  /** The open tab that shows this project, when it is bound. */
  tabProjectId?: string;
}

/** The orchestrated projects: active declarations with this machine's binding, by label. */
export function projectRows(
  manifest: OrchestrationManifest | undefined,
  bindings: Readonly<Record<string, OrchestrationBinding | undefined>>,
  remoteLabels: Readonly<Record<string, string>>,
): OrchestrationProjectRow[] {
  return (manifest?.targets ?? [])
    .filter((target) => target.state === 'active')
    .map((target) => {
      const binding = bindings[target.target_id],
        destination = binding?.destination ?? undefined;
      return {
        targetId: target.target_id,
        label: target.label,
        location: destination ? destination.kind : binding ? 'unbound' : 'local',
        where: destination
          ? `${destination.kind === 'local' ? 'Local' : 'Remote'} on ${machineOf(destination, remoteLabels)}`
          : binding
            ? 'Not bound on this computer'
            : '',
        connection: connectionStatus(binding),
        tabProjectId: destination ? tabProjectIdOf(destination) : undefined,
      } satisfies OrchestrationProjectRow;
    })
    .sort((left, right) => left.label.localeCompare(right.label));
}

export interface AddChoice {
  /** The open tab's project id. */
  projectId: string;
  name: string;
  location: 'local' | 'remote';
  machine: string;
  destination: AddDestination;
  /** Already orchestrated: shown checked, and choosing it does nothing more. */
  added: boolean;
}

/** The + Add Project menu's Open Projects: every other open tab, with whether it is already added. */
export function addChoices(
  openProjects: readonly { id: string; name: string }[],
  ownerId: string,
  bindings: Readonly<Record<string, OrchestrationBinding | undefined>>,
  manifest: OrchestrationManifest | undefined,
  remoteLabels: Readonly<Record<string, string>>,
): AddChoice[] {
  const active = new Set(
      (manifest?.targets ?? []).filter((target) => target.state === 'active').map((target) => target.target_id),
    ),
    bound = new Set(
      Object.entries(bindings)
        .filter(([target, binding]) => active.has(target) && binding?.destination)
        .map(([, binding]) => placeOf(binding!.destination!)),
    );
  return openProjects
    .filter((project) => project.id !== ownerId)
    .map((project) => {
      const destination = destinationOf(project.id);
      return {
        projectId: project.id,
        name: project.name,
        location: destination.kind,
        machine: machineOf(destination, remoteLabels),
        destination,
        added: bound.has(placeOf(destination)),
      };
    });
}

/** A canonical ULID for an add the person may retry (docs/26 §26.4: retain the id before setup). */
export function newUlid(now = Date.now(), random: (bytes: Uint8Array) => Uint8Array = defaultRandom): string {
  const alphabet = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';
  let time = '';
  let rest = Math.max(0, Math.floor(now));
  for (let index = 0; index < 10; index += 1) {
    time = alphabet[rest % 32] + time;
    rest = Math.floor(rest / 32);
  }
  const bytes = random(new Uint8Array(16));
  let tail = '';
  for (let index = 0; index < 16; index += 1) tail += alphabet[bytes[index] % 32];
  return time + tail;
}

function defaultRandom(bytes: Uint8Array): Uint8Array {
  return globalThis.crypto.getRandomValues(bytes);
}

/* ---- Sessions ---------------------------------------------------------------------------- */

const ROLE_LABELS = { worker: 'Worker', dispatcher: 'Dispatcher', integrator: 'Integrator' } as const;
const STATE_LABELS = {
  requested: 'Starting',
  running: 'Running',
  stopped: 'Stopped',
  failed: 'Failed',
  lost: 'Lost',
} as const;

export function sessionTitle(session: OrchestrationSession, toolLabel: (tool: string) => string): string {
  return session.label ?? `${toolLabel(session.tool)} ${ROLE_LABELS[session.role]}`;
}

export function sessionStatus(session: OrchestrationSession): ConnectionStatus {
  if (session.owner === 'human') return { label: 'Taken Over', tone: 'neutral' };
  if (session.paused) return { label: 'Paused', tone: 'warning' };
  return {
    label: STATE_LABELS[session.state],
    tone: session.state === 'running' ? 'success' : session.state === 'requested' ? 'neutral' : 'danger',
  };
}

/** Megabytes as the person reads them. */
export function formatMegabytes(value: number | null | undefined): string {
  if (value === null || value === undefined) return 'Unknown';
  return value >= 1024 ? `${(value / 1024).toFixed(1)} GB` : `${String(value)} MB`;
}

/* ---- Policy, limits only (HS2-XHQ1EH) ---------------------------------------------------- */

/** The defaults the server applies to an absent member (docs/26 §26.8). */
export const POLICY_DEFAULTS = {
  machine: { max_sessions: 2, min_free_ram_mb: 2048, max_cpu_percent: 90, min_free_disk_mb: 5120 },
  tool: { weight: 50 },
  wind_down: { idle_minutes: 15, keep_warm: 0 },
} as const;

export interface PolicyToolDraft {
  tool: string;
  /** Empty: the orchestrator does not start this tool. */
  max_sessions: string;
  weight: string;
}

export interface PolicyDraft {
  automatic: boolean;
  machine: { max_sessions: string; min_free_ram_mb: string; max_cpu_percent: string; min_free_disk_mb: string };
  tools: PolicyToolDraft[];
  idle_minutes: string;
  keep_warm: string;
  build_ahead: boolean;
}

export const MACHINE_FIELDS = ['max_sessions', 'min_free_ram_mb', 'max_cpu_percent', 'min_free_disk_mb'] as const;
export type MachineField = (typeof MACHINE_FIELDS)[number];

/** A form draft from the stored policy. Every AI tool the app knows gets a row, configured or not. */
export function policyDraftFrom(view: OrchestrationPolicyView | undefined, knownTools: readonly string[]): PolicyDraft {
  const body: OrchestrationPolicyBody = view?.policy ?? {},
    machine = { ...POLICY_DEFAULTS.machine, ...body.machine },
    configured = body.tools ?? [],
    names = [
      ...configured.map((tool) => tool.tool),
      ...knownTools.filter((tool) => !configured.some((item) => item.tool === tool)),
    ];
  return {
    automatic: body.placement?.automatic ?? false,
    machine: {
      max_sessions: String(machine.max_sessions),
      min_free_ram_mb: String(machine.min_free_ram_mb),
      max_cpu_percent: String(machine.max_cpu_percent),
      min_free_disk_mb: String(machine.min_free_disk_mb),
    },
    tools: names.map((name) => {
      const tool = configured.find((item) => item.tool === name);
      return {
        tool: name,
        max_sessions: tool ? String(tool.max_sessions) : '',
        weight: String(tool?.weight ?? POLICY_DEFAULTS.tool.weight),
      };
    }),
    idle_minutes: String(body.wind_down?.idle_minutes ?? POLICY_DEFAULTS.wind_down.idle_minutes),
    keep_warm: String(body.wind_down?.keep_warm ?? POLICY_DEFAULTS.wind_down.keep_warm),
    build_ahead: body.speculation?.allowed ?? false,
  };
}

/** One form control's change. Field names are the controls' `name` attributes. */
export function changePolicyDraft(draft: PolicyDraft, field: string, value: string | boolean): PolicyDraft {
  if (field === 'automatic' && typeof value === 'boolean') return { ...draft, automatic: value };
  if (field === 'build_ahead' && typeof value === 'boolean') return { ...draft, build_ahead: value };
  if (typeof value !== 'string') return draft;
  if (field === 'idle_minutes' || field === 'keep_warm') return { ...draft, [field]: value };
  const machineField = /^machine\.(.+)$/u.exec(field)?.[1];
  if (machineField && (MACHINE_FIELDS as readonly string[]).includes(machineField))
    return { ...draft, machine: { ...draft.machine, [machineField]: value } };
  const tool = /^tool\.(.+)\.(max_sessions|weight)$/u.exec(field);
  if (tool)
    return {
      ...draft,
      tools: draft.tools.map((item) => (item.tool === tool[1] ? { ...item, [tool[2]]: value } : item)),
    };
  return draft;
}

const LABELS: Readonly<Record<string, string>> = {
  max_sessions: 'Sessions per Machine',
  min_free_ram_mb: 'Free Memory Floor',
  max_cpu_percent: 'CPU Ceiling',
  min_free_disk_mb: 'Free Disk Floor',
  idle_minutes: 'Wind Down After Idle',
  keep_warm: 'Idle Sessions Kept per Project',
};

function whole(value: string, label: string, minimum: number, maximum?: number): number | string {
  const trimmed = value.trim();
  if (!/^\d+$/u.test(trimmed)) return `${label} must be a whole number.`;
  const number = Number(trimmed);
  if (number < minimum || (maximum !== undefined && number > maximum))
    return maximum === undefined
      ? `${label} must be at least ${String(minimum)}.`
      : `${label} must be ${String(minimum)} to ${String(maximum)}.`;
  return number;
}

/**
 * The policy to store: the stored body with only the limits replaced. Targets, accounts and every
 * other member round-trip unchanged, so this form never edits what it does not show. Retired members
 * (`targets[].actions`, `approval`) pass through untouched; the server ignores and drops them
 * (HS2-XHQ1EH).
 */
export function policyFromDraft(
  view: OrchestrationPolicyView | undefined,
  draft: PolicyDraft,
): { policy: OrchestrationPolicyBody } | { error: string } {
  const body: OrchestrationPolicyBody = structuredClone(view?.policy ?? {});
  const machine: Record<string, number> = {};
  for (const field of MACHINE_FIELDS) {
    const parsed = whole(
      draft.machine[field],
      LABELS[field],
      field === 'max_sessions' || field === 'max_cpu_percent' ? 1 : 0,
      field === 'max_sessions' ? 64 : field === 'max_cpu_percent' ? 100 : undefined,
    );
    if (typeof parsed === 'string') return { error: parsed };
    machine[field] = parsed;
  }
  const idle = whole(draft.idle_minutes, LABELS.idle_minutes, 1, 1440),
    warm = whole(draft.keep_warm, LABELS.keep_warm, 0, 64);
  if (typeof idle === 'string') return { error: idle };
  if (typeof warm === 'string') return { error: warm };
  const stored = body.tools ?? [],
    tools: NonNullable<OrchestrationPolicyBody['tools']> = [];
  for (const row of draft.tools) {
    if (!row.max_sessions.trim()) continue;
    const sessions = whole(row.max_sessions, `${row.tool} sessions`, 1, 64),
      weight = whole(row.weight, `${row.tool} weight`, 1, 100);
    if (typeof sessions === 'string') return { error: sessions };
    if (typeof weight === 'string') return { error: weight };
    const previous = stored.find((tool) => tool.tool === row.tool);
    tools.push({ ...previous, tool: row.tool, max_sessions: sessions, weight });
  }
  body.machine = { ...body.machine, ...machine };
  body.tools = tools;
  body.wind_down = { ...body.wind_down, idle_minutes: idle, keep_warm: warm };
  body.speculation = { ...body.speculation, allowed: draft.build_ahead };
  body.placement = { ...body.placement, automatic: draft.automatic };
  return { policy: body };
}
