/**
 * HS2-J89JC7: the orchestration timeline read model, as `GET /checkouts/{owner}/orchestration/timeline`
 * answers it (docs/26 and the OpenAPI `OrchestrationTimeline` schema). Members the Rust DTO skips when
 * unset are optional here and never `null`.
 */
import { ServerResponseShapeError } from './server-response-parsers';

export type TimelinePhase = 'no_projects' | 'no_plan' | 'proposed' | 'running' | 'paused';
export type WaveKind = 'done' | 'working_now' | 'next' | 'then' | 'later' | 'planned';
export type EstimateBasis = 'started' | 'measured' | 'default';
export type TimelineItemState =
  | 'running'
  | 'provisional'
  | 'ready'
  | 'waits_for_prerequisite'
  | 'waits_for_permission'
  | 'waits_for_decision'
  | 'done';
export type TargetLocation = 'local' | 'remote' | 'unbound';
/**
 * HS2-XYAHB4: no placement actor is needed; HS2-J3ZF27: a Software project cannot start. HS2-D9KP86: no
 * saved policy never blocks Start (`default_policy`).
 */
export type StartBlocked = 'no_projects' | 'no_plan' | 'project_software' | 'running';

export interface TimelineTarget {
  target_id: string;
  label: string;
  location: TargetLocation;
  machine_label?: string;
  checkout_id?: string;
}

export interface TimelineRun {
  run_id: string;
  state: string;
  worker_id?: string;
  lease_expires_at_ms?: number;
  paused: boolean;
  cancel_requested: boolean;
  unreachable_since_ms?: number;
  last_confirmed_at_ms?: number;
  created_at_ms: number;
  finished_at_ms?: number;
}

export interface TimelinePermission {
  /** An orchestrator permission request (HS2-XHQ1EH). */
  kind: 'permission';
  request_id: string;
  action: string;
  operation_id?: string;
  requested_at_ms?: number;
}

export interface TimelineWaitsOn {
  target_id: string;
  connection_id: string;
  native_id: string;
  slug?: string;
  step_id?: string;
  reason: string;
}

export interface TimelinePlacement {
  at_ms: number;
  decision: 'start' | 'keep' | 'unplaced';
  reason?: string;
  tool?: string;
  model?: string;
  session_id?: string;
  outcome?: string;
}

export interface TimelineItem {
  step_id: string;
  goal: { connection_id: string; native_id: string };
  goal_slug?: string;
  goal_title?: string;
  target: { target_id: string; connection_id: string; native_id: string };
  slug?: string;
  title?: string;
  status?: string;
  priority?: string;
  category?: string;
  /**
   * HS2-6GE7WM: the declared target (site) the work runs on: its run's site, which the scheduler may
   * choose among the project's sites, or before a run the site the step names. `project_label`,
   * `location` and `machine_label` describe it. Absent from a server older than this field.
   */
  site_id?: string;
  project_label: string;
  location: TargetLocation;
  machine_label?: string;
  tool?: string;
  model?: string;
  session_id?: string;
  session_state?: string;
  state: TimelineItemState;
  level: string;
  reasons?: string[];
  waits_on?: TimelineWaitsOn[];
  run?: TimelineRun;
  permission?: TimelinePermission;
  decision?: { note_id?: string };
  placement?: TimelinePlacement;
  started_at_ms?: number;
  finished_at_ms?: number;
  estimated_start_ms?: number;
  /** HS2-H5H9QP: the ledger record of work started ahead of a pending gate (§26.22). */
  provisional?: TimelineProvisional;
}

/** HS2-H5H9QP: provisional work on an item: its ledger state and the gates it rests on. */
export interface TimelineProvisional {
  /** `provisional`, `confirmed`, `at_risk` or `reopened`. */
  state: string;
  gates: Array<{ target_id: string; connection_id: string; native_id: string; slug?: string; result: string }>;
}

export interface TimelineWave {
  kind: WaveKind;
  ordinal: number;
  estimated_start_ms?: number;
  estimate?: EstimateBasis;
  items: TimelineItem[];
}

export interface OrchestrationTimeline {
  version: string;
  now_ms: number;
  declared: boolean;
  phase: TimelinePhase;
  automatic: boolean;
  /**
   * HS2-R2N33M: placement runs because the policy leaves it Auto ("On once a plan exists"), not because a
   * person pressed Start Orchestration. The header says sessions will start and offers Pause.
   */
  automatic_by_default?: boolean;
  /**
   * HS2-38R8J2: placement was never turned on for this project, so work started by hand under the plan reads
   * Not Started rather than Paused. Absent once placement was ever on.
   */
  never_started?: boolean;
  kick_off: {
    can_start: boolean;
    start_blocked?: StartBlocked;
    /** HS2-D9KP86: no policy is saved, so Start Orchestration first saves the default policy. */
    default_policy?: boolean;
    can_pause: boolean;
    policy_revision?: string;
  };
  running_since_ms?: number;
  sessions: number;
  targets: TimelineTarget[];
  waves: TimelineWave[];
  counts: Record<string, number>;
  wave_minutes: number;
  estimate: EstimateBasis;
}

/** HS2-S3FKJ7: one target ticket's line, read from the target project's side. */
export interface OrchestrationTargetLine {
  owner_checkout_id: string;
  /** The orchestration project's name. */
  owner_label: string;
  /** The owner's plan was never started, so the line reads as a dry run. */
  proposed: boolean;
  /** Other plan steps that also work this ticket. */
  also_planned?: number;
  /** HS2-KB2Y51: the server id of the machine that pushed this line, when another machine's plan works it. */
  observed_from?: string;
  item: TimelineItem;
}

export interface OrchestrationTargetLines {
  version: string;
  /** An orchestration project on this machine binds the project at all. */
  orchestrated: boolean;
  lines: OrchestrationTargetLine[];
}

const PHASES: readonly TimelinePhase[] = ['no_projects', 'no_plan', 'proposed', 'running', 'paused'];
const WAVES: readonly WaveKind[] = ['done', 'working_now', 'next', 'then', 'later', 'planned'];
const STATES: readonly TimelineItemState[] = [
  'running',
  'provisional',
  'ready',
  'waits_for_prerequisite',
  'waits_for_permission',
  'waits_for_decision',
  'done',
];
const LOCATIONS: readonly TargetLocation[] = ['local', 'remote', 'unbound'];

function fail(): never {
  throw new ServerResponseShapeError('orchestration timeline');
}
function record(value: unknown): Record<string, unknown> {
  return value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : fail();
}
function text(value: unknown): string {
  return typeof value === 'string' ? value : fail();
}
function count(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : fail();
}
function one<T extends string>(value: unknown, choices: readonly T[]): T {
  return typeof value === 'string' && (choices as readonly string[]).includes(value) ? (value as T) : fail();
}
/** An optional member: absent, or the stated type (never `null`). */
function optional<T>(value: unknown, parse: (value: unknown) => T): T | undefined {
  return value === undefined ? undefined : parse(value);
}
function list<T>(value: unknown, parse: (value: unknown) => T): T[] {
  return Array.isArray(value) ? value.map(parse) : fail();
}

function item(value: unknown): TimelineItem {
  const row = record(value),
    goal = record(row.goal),
    target = record(row.target);
  for (const key of ['goal_slug', 'goal_title', 'slug', 'title', 'status', 'priority', 'category', 'tool'] as const)
    optional(row[key], text);
  optional(row.reasons, (reasons) => list(reasons, text));
  optional(row.waits_on, (waits) =>
    list(waits, (wait) => {
      const entry = record(wait);
      text(entry.target_id);
      text(entry.native_id);
      text(entry.reason);
      return entry;
    }),
  );
  optional(row.run, (run) => {
    const entry = record(run);
    text(entry.run_id);
    text(entry.state);
    count(entry.created_at_ms);
    return entry;
  });
  optional(row.permission, (permission) => {
    const entry = record(permission);
    one(entry.kind, ['permission']);
    text(entry.request_id);
    text(entry.action);
    return entry;
  });
  for (const key of ['started_at_ms', 'finished_at_ms', 'estimated_start_ms'] as const) optional(row[key], count);
  optional(row.provisional, (provisional) => {
    const entry = record(provisional);
    text(entry.state);
    list(entry.gates, (gate) => {
      const found = record(gate);
      text(found.native_id);
      text(found.result);
      optional(found.slug, text);
      return found;
    });
    return entry;
  });
  text(row.step_id);
  text(goal.native_id);
  text(target.target_id);
  text(target.native_id);
  text(row.project_label);
  one(row.location, LOCATIONS);
  one(row.state, STATES);
  text(row.level);
  return row as unknown as TimelineItem;
}

/** Validate the timeline's shape; a drifted response throws instead of rendering half a view. */
export function parseOrchestrationTimeline(value: unknown): OrchestrationTimeline {
  const view = record(value),
    kickOff = record(view.kick_off);
  text(view.version);
  count(view.now_ms);
  if (typeof view.declared !== 'boolean' || typeof view.automatic !== 'boolean') fail();
  if (view.automatic_by_default !== undefined && typeof view.automatic_by_default !== 'boolean') fail();
  if (view.never_started !== undefined && typeof view.never_started !== 'boolean') fail();
  one(view.phase, PHASES);
  if (typeof kickOff.can_start !== 'boolean' || typeof kickOff.can_pause !== 'boolean') fail();
  if (kickOff.default_policy !== undefined && typeof kickOff.default_policy !== 'boolean') fail();
  optional(view.running_since_ms, count);
  count(view.sessions);
  list(view.targets, (target) => {
    const entry = record(target);
    text(entry.target_id);
    text(entry.label);
    one(entry.location, LOCATIONS);
    return entry;
  });
  list(view.waves, (wave) => {
    const entry = record(wave);
    one(entry.kind, WAVES);
    count(entry.ordinal);
    optional(entry.estimated_start_ms, count);
    list(entry.items, item);
    return entry;
  });
  record(view.counts);
  count(view.wave_minutes);
  return view as unknown as OrchestrationTimeline;
}

/** HS2-S3FKJ7: validate a target project's orchestration lines; a drifted response throws. */
export function parseOrchestrationTargetLines(value: unknown): OrchestrationTargetLines {
  const view = record(value);
  text(view.version);
  if (typeof view.orchestrated !== 'boolean') fail();
  list(view.lines, (line) => {
    const entry = record(line);
    text(entry.owner_checkout_id);
    text(entry.owner_label);
    if (typeof entry.proposed !== 'boolean') fail();
    optional(entry.also_planned, count);
    optional(entry.observed_from, text);
    item(entry.item);
    return entry;
  });
  return view as unknown as OrchestrationTargetLines;
}

/** An orchestration answer the inspector reads a few members of (gates, run events, control results). */
export function parseOrchestrationRecord(value: unknown): Record<string, unknown> {
  return record(value);
}

/** An orchestration list answer (validation evidence). */
export function parseOrchestrationList(value: unknown): Array<Record<string, unknown>> {
  return list(value, record);
}
