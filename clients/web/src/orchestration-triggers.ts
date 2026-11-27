/**
 * HS2-KXKA7X: scheduled and event triggers in the Orchestration view (docs/26). A trigger binds when (a
 * schedule, or an event in one project) to what (apply one plan template, start or pause placement). It lives
 * on this machine, starts as Dry Run, and a person (or an AI session, through the permission card) turns it On
 * after a review. Pure: it parses the server's answers, words them, and maps the setup form to the request
 * and back, so the dialog and its controller stay thin. Unknown codes read as their words.
 */
import {
  newScheduleDraft,
  scheduleCadence,
  type ScheduleFormDraft,
  scheduleInputFromDraft,
} from './command-schedule-form';
import type { Recurrence, ScheduleView } from './command-schedules';

export type TriggerState = 'on' | 'dry_run' | 'paused' | 'needs_review';
export type TriggerEvent = 'ticket_opened' | 'gate_failed' | 'branch_ci_failed' | 'commit_landed';
export type TriggerSourceKind = 'schedule' | TriggerEvent;
export type EventField = 'commit' | 'ci_run' | 'ticket' | 'branch' | 'occurrence' | 'gate';
export type Placement = 'start' | 'pause';
export type Overlap = 'skip' | 'queue_latest';
export type DeliveryOutcome =
  'applied' | 'dry_run' | 'skipped_overlap' | 'queued' | 'already_handled' | 'needs_review' | 'refused';
export type DeliveryKind = 'schedule' | 'ticket_opened' | 'gate_failed' | 'ci_failed' | 'commit_landed' | 'manual';
export type ReviewNeed = 'not_needed' | 'remembered' | 'asks' | 'unchanged';
export type TurnOnNeed = 'person' | 'remembered' | 'asks';

export interface TriggerSchedule {
  time_zone: string;
  local_time: string;
  recurrence: Recurrence;
}

export interface BranchWatch {
  provider?: string;
  repository: string;
  workflow: string;
  branch: string;
}

export type TriggerSource =
  | { kind: 'schedule'; schedule: TriggerSchedule }
  | { kind: 'ticket_opened'; target_id: string; tag?: string; category?: string }
  | { kind: 'gate_failed'; target_id?: string }
  | { kind: 'branch_ci_failed'; target_id: string; watch: BranchWatch }
  | { kind: 'commit_landed'; target_id: string; watch: BranchWatch };

export interface TriggerTemplate {
  template_id: string;
  inputs: Record<string, unknown>;
  event_inputs: Record<string, EventField>;
  tool: string;
  model?: string;
  effort?: string;
}

export interface TriggerAction {
  template?: TriggerTemplate;
  placement?: Placement;
}

export interface WouldRun {
  goal_title?: string;
  steps: number;
  targets: string[];
  asks: string[];
  placement?: Placement;
}

export interface TriggerHistoryEntry {
  delivery_key: string;
  kind: DeliveryKind;
  at_ms: number;
  outcome: DeliveryOutcome;
  count: number;
  evidence: Readonly<Partial<Record<string, string>>>;
  missed: number;
  operation_id?: string;
  goal_slug?: string;
  placement?: Placement;
  reason?: string;
  would?: WouldRun;
}

export interface TriggerView {
  trigger_id: string;
  name: string;
  source: TriggerSource;
  action: TriggerAction;
  overlap: Overlap;
  state: TriggerState;
  paused_from?: TriggerState;
  review?: { role: string; actor_id: string; at_ms: number; template_version?: number };
  pending_on?: { requests: string[]; at_ms: number };
  history: TriggerHistoryEntry[];
  warning?: { code: string; since_ms: number };
  next_runs_ms: number[];
  template_name?: string;
  template_version?: number;
  reviewed: boolean;
  running: boolean;
  running_goal?: string;
  /** The template apply of the last applied delivery, which opened the running goal. */
  active_operation?: string;
  updated_at_ms: number;
}

export interface TriggerProblem {
  code: string;
  at: string;
}

export interface ReviewPermission {
  action: string;
  target_id?: string;
  need: ReviewNeed;
  rule_id?: string;
  tool?: string;
}

export interface TriggerReview {
  problems: TriggerProblem[];
  watches: {
    kind: TriggerSourceKind;
    target_id?: string;
    label?: string;
    branch?: BranchWatch;
    tag?: string;
    category?: string;
  };
  projects: Array<{ target_id: string; label?: string; declared: boolean; steps: number }>;
  perDelivery: { goals: number; steps: number; goal_title?: string; placement?: Placement };
  permissions: ReviewPermission[];
  next: { runs_ms: number[]; event?: string; overlap: Overlap };
  digest: string;
  template_version?: number;
  turn_on: TurnOnNeed;
}

type Raw = Record<string, unknown>;
const record = (value: unknown): Raw =>
  value && typeof value === 'object' && !Array.isArray(value) ? (value as Raw) : {};
const text = (value: unknown): string | undefined => (typeof value === 'string' && value !== '' ? value : undefined);
const texts = (value: unknown): string[] =>
  Array.isArray(value) ? value.filter((entry): entry is string => typeof entry === 'string') : [];
const records = (value: unknown): Raw[] => (Array.isArray(value) ? value.map(record) : []);
const numbers = (value: unknown): number[] =>
  Array.isArray(value) ? value.filter((entry): entry is number => typeof entry === 'number') : [];
const num = (value: unknown, fallback = 0): number => (typeof value === 'number' ? value : fallback);
const oneOf = <T extends string>(choices: readonly T[], value: unknown, fallback: T): T =>
  choices.find((choice) => choice === value) ?? fallback;

const STATES: readonly TriggerState[] = ['on', 'dry_run', 'paused', 'needs_review'];
const EVENTS: readonly TriggerEvent[] = ['ticket_opened', 'gate_failed', 'branch_ci_failed', 'commit_landed'];
const FIELDS: readonly EventField[] = ['commit', 'ci_run', 'ticket', 'branch', 'occurrence', 'gate'];
const OUTCOMES: readonly DeliveryOutcome[] = [
  'applied',
  'dry_run',
  'skipped_overlap',
  'queued',
  'already_handled',
  'needs_review',
  'refused',
];
const KINDS: readonly DeliveryKind[] = [
  'schedule',
  'ticket_opened',
  'gate_failed',
  'ci_failed',
  'commit_landed',
  'manual',
];
const NEEDS: readonly ReviewNeed[] = ['not_needed', 'remembered', 'asks', 'unchanged'];
const placement = (value: unknown): Placement | undefined =>
  value === 'start' || value === 'pause' ? value : undefined;
const stringMap = (value: unknown): Record<string, string> =>
  Object.fromEntries(
    Object.entries(record(value)).filter((entry): entry is [string, string] => typeof entry[1] === 'string'),
  );

function parseWatch(value: unknown): BranchWatch {
  const raw = record(value);
  return {
    ...(text(raw.provider) ? { provider: text(raw.provider) } : {}),
    repository: text(raw.repository) ?? '',
    workflow: text(raw.workflow) ?? '',
    branch: text(raw.branch) ?? '',
  };
}

const RECURRENCE_KINDS: readonly Recurrence['kind'][] = [
  'once',
  'minutely',
  'hourly',
  'daily',
  'weekly',
  'monthly',
  'yearly',
  'selected_dates',
];

/** A recurrence as the command-schedule engine stores it; only its kind is required. */
function parseRecurrence(value: unknown): Recurrence {
  const raw = record(value);
  const kind = oneOf(RECURRENCE_KINDS, raw.kind, 'daily');
  return { ...(raw as Partial<Recurrence>), kind };
}

export function parseTriggerSource(value: unknown): TriggerSource | undefined {
  const raw = record(value);
  switch (raw.kind) {
    case 'schedule': {
      const schedule = record(raw.schedule);
      return {
        kind: 'schedule',
        schedule: {
          time_zone: text(schedule.time_zone) ?? 'UTC',
          local_time: text(schedule.local_time) ?? '00:00',
          recurrence: parseRecurrence(schedule.recurrence),
        },
      };
    }
    case 'ticket_opened':
      return {
        kind: 'ticket_opened',
        target_id: text(raw.target_id) ?? '',
        ...(text(raw.tag) ? { tag: text(raw.tag) } : {}),
        ...(text(raw.category) ? { category: text(raw.category) } : {}),
      };
    case 'gate_failed':
      return { kind: 'gate_failed', ...(text(raw.target_id) ? { target_id: text(raw.target_id) } : {}) };
    case 'branch_ci_failed':
    case 'commit_landed':
      return { kind: raw.kind, target_id: text(raw.target_id) ?? '', watch: parseWatch(raw.watch) };
    default:
      return undefined;
  }
}

function parseAction(value: unknown): TriggerAction {
  const raw = record(value),
    template = record(raw.template);
  const id = text(template.template_id);
  return {
    ...(id
      ? {
          template: {
            template_id: id,
            inputs: record(template.inputs),
            event_inputs: Object.fromEntries(
              Object.entries(record(template.event_inputs)).flatMap(([key, field]) => {
                const known = FIELDS.find((candidate) => candidate === field);
                return known ? [[key, known]] : [];
              }),
            ),
            tool: text(template.tool) ?? '',
            ...(text(template.model) ? { model: text(template.model) } : {}),
            ...(text(template.effort) ? { effort: text(template.effort) } : {}),
          },
        }
      : {}),
    ...(placement(raw.placement) ? { placement: placement(raw.placement) } : {}),
  };
}

function parseEntry(value: unknown): TriggerHistoryEntry | undefined {
  const raw = record(value);
  const key = text(raw.delivery_key),
    outcome = OUTCOMES.find((candidate) => candidate === raw.outcome);
  if (!key || !outcome) return undefined;
  const would = raw.would === undefined ? undefined : record(raw.would);
  return {
    delivery_key: key,
    kind: oneOf(KINDS, raw.kind, 'manual'),
    at_ms: num(raw.at_ms),
    outcome,
    count: Math.max(1, num(raw.count, 1)),
    evidence: stringMap(raw.evidence),
    missed: num(raw.missed),
    ...(text(raw.operation_id) ? { operation_id: text(raw.operation_id) } : {}),
    ...(text(raw.goal_slug) ? { goal_slug: text(raw.goal_slug) } : {}),
    ...(placement(raw.placement) ? { placement: placement(raw.placement) } : {}),
    ...(text(raw.reason) ? { reason: text(raw.reason) } : {}),
    ...(would
      ? {
          would: {
            ...(text(would.goal_title) ? { goal_title: text(would.goal_title) } : {}),
            steps: num(would.steps),
            targets: texts(would.targets),
            asks: texts(would.asks),
            ...(placement(would.placement) ? { placement: placement(would.placement) } : {}),
          },
        }
      : {}),
  };
}

export function parseTriggerView(value: unknown): TriggerView | undefined {
  const raw = record(value);
  const id = text(raw.trigger_id),
    name = text(raw.name),
    source = parseTriggerSource(raw.source);
  if (!id || !name || !source) return undefined;
  const review = raw.review === undefined ? undefined : record(raw.review);
  const pending = raw.pending_on === undefined ? undefined : record(raw.pending_on);
  const warning = raw.warning === undefined ? undefined : record(raw.warning);
  return {
    trigger_id: id,
    name,
    source,
    action: parseAction(raw.action),
    overlap: raw.overlap === 'queue_latest' ? 'queue_latest' : 'skip',
    state: oneOf(STATES, raw.state, 'paused'),
    ...(STATES.includes(raw.paused_from as TriggerState) ? { paused_from: raw.paused_from as TriggerState } : {}),
    ...(review
      ? {
          review: {
            role: text(review.role) ?? 'human',
            actor_id: text(review.actor_id) ?? '',
            at_ms: num(review.at_ms),
            ...(typeof review.template_version === 'number' ? { template_version: review.template_version } : {}),
          },
        }
      : {}),
    ...(pending ? { pending_on: { requests: texts(pending.requests), at_ms: num(pending.at_ms) } } : {}),
    history: records(raw.history).flatMap((entry) => {
      const parsed = parseEntry(entry);
      return parsed ? [parsed] : [];
    }),
    ...(warning && text(warning.code)
      ? { warning: { code: text(warning.code)!, since_ms: num(warning.since_ms) } }
      : {}),
    next_runs_ms: numbers(raw.next_runs_ms),
    ...(text(raw.template_name) ? { template_name: text(raw.template_name) } : {}),
    ...(typeof raw.template_version === 'number' ? { template_version: raw.template_version } : {}),
    reviewed: raw.reviewed === true,
    running: raw.running === true,
    ...(text(raw.running_goal) ? { running_goal: text(raw.running_goal) } : {}),
    ...(text(record(raw.active).operation_id) ? { active_operation: text(record(raw.active).operation_id) } : {}),
    updated_at_ms: num(raw.updated_at_ms),
  };
}

export function parseTriggerList(value: unknown): TriggerView[] {
  return records(record(value).triggers).flatMap((entry) => {
    const parsed = parseTriggerView(entry);
    return parsed ? [parsed] : [];
  });
}

export function parseTriggerReview(value: unknown): TriggerReview {
  const raw = record(value),
    scope = record(raw.scope),
    watches = record(scope.watches),
    per = record(scope.per_delivery),
    next = record(raw.next);
  const kind = oneOf<TriggerSourceKind>(['schedule', ...EVENTS], watches.kind, 'schedule');
  return {
    problems: records(raw.problems).flatMap((problem) => {
      const code = text(problem.code);
      return code ? [{ code, at: text(problem.at) ?? '' }] : [];
    }),
    watches: {
      kind,
      ...(text(watches.target_id) ? { target_id: text(watches.target_id) } : {}),
      ...(text(watches.label) ? { label: text(watches.label) } : {}),
      ...(watches.branch ? { branch: parseWatch(watches.branch) } : {}),
      ...(text(watches.tag) ? { tag: text(watches.tag) } : {}),
      ...(text(watches.category) ? { category: text(watches.category) } : {}),
    },
    projects: records(scope.projects).flatMap((project) => {
      const id = text(project.target_id);
      return id
        ? [
            {
              target_id: id,
              ...(text(project.label) ? { label: text(project.label) } : {}),
              declared: project.declared === true,
              steps: num(project.steps),
            },
          ]
        : [];
    }),
    perDelivery: {
      goals: num(per.goals),
      steps: num(per.steps),
      ...(text(per.goal_title) ? { goal_title: text(per.goal_title) } : {}),
      ...(placement(per.placement) ? { placement: placement(per.placement) } : {}),
    },
    permissions: records(raw.permissions).flatMap((row) => {
      const action = text(row.action);
      return action
        ? [
            {
              action,
              ...(text(row.target_id) ? { target_id: text(row.target_id) } : {}),
              need: oneOf(NEEDS, row.need, 'asks'),
              ...(text(row.rule_id) ? { rule_id: text(row.rule_id) } : {}),
              ...(text(row.tool) ? { tool: text(row.tool) } : {}),
            },
          ]
        : [];
    }),
    next: {
      runs_ms: numbers(next.runs_ms),
      ...(text(next.event) ? { event: text(next.event) } : {}),
      overlap: next.overlap === 'queue_latest' ? 'queue_latest' : 'skip',
    },
    digest: text(raw.digest) ?? '',
    ...(typeof raw.template_version === 'number' ? { template_version: raw.template_version } : {}),
    turn_on: oneOf<TurnOnNeed>(['person', 'remembered', 'asks'], raw.turn_on, 'asks'),
  };
}

/** The list badge of a trigger: On, Dry Run, Paused or Review (the template changed). */
export function stateBadge(state: TriggerState): {
  label: string;
  tone: 'success' | 'info' | 'neutral' | 'warning';
} {
  switch (state) {
    case 'on':
      return { label: 'On', tone: 'success' };
    case 'dry_run':
      return { label: 'Dry Run', tone: 'info' };
    case 'paused':
      return { label: 'Paused', tone: 'neutral' };
    case 'needs_review':
      return { label: 'Review', tone: 'warning' };
    default:
      return state satisfies never;
  }
}

/** A project's name, from the declared projects; its id when it is not declared. */
type Labels = Readonly<Record<string, string>>;
const named = (labels: Labels, target: string | undefined) => (target ? (labels[target] ?? target) : 'a project');

/** The command-schedule view a schedule reads as, so the cadence is worded as command schedules word it. */
function asScheduleView(schedule: TriggerSchedule): ScheduleView {
  return {
    schedule: {
      command_id: '',
      time_zone: schedule.time_zone,
      local_time: schedule.local_time,
      paused: false,
      recurrence: schedule.recurrence,
    },
    next_runs: [],
    history: [],
  };
}

/** What a trigger watches, in words: "Every 4 hours from 02:00 UTC", "When API's CI fails on main". */
export function sourceText(source: TriggerSource, labels: Labels): string {
  switch (source.kind) {
    case 'schedule':
      return scheduleCadence(asScheduleView(source.schedule));
    case 'ticket_opened': {
      const filters = [
        source.tag ? `tagged ${source.tag}` : '',
        source.category ? `of category ${source.category}` : '',
      ]
        .filter(Boolean)
        .join(' ');
      return `When a ticket ${filters ? `${filters} ` : ''}opens in ${named(labels, source.target_id)}`;
    }
    case 'gate_failed':
      return source.target_id
        ? `When a run’s CI gate fails in ${named(labels, source.target_id)}`
        : 'When a run’s CI gate fails';
    case 'branch_ci_failed':
      return `When ${named(labels, source.target_id)}’s CI fails on ${source.watch.branch}`;
    case 'commit_landed':
      return `When a commit lands on ${named(labels, source.target_id)} ${source.watch.branch}`;
    default:
      return source satisfies never;
  }
}

/** What one delivery runs, in words. */
export function actionText(action: TriggerAction, templateName: string | undefined): string {
  const parts: string[] = [];
  if (action.template) parts.push(`Applies ${templateName ?? 'a template'}`);
  if (action.placement === 'start') parts.push('starts orchestration');
  if (action.placement === 'pause') parts.push('pauses orchestration');
  const joined = parts.join(' and ');
  return joined ? `${joined.charAt(0).toUpperCase()}${joined.slice(1)}` : 'Nothing';
}

/** A time a person reads: "Today 14:31", "Yesterday 22:00", or a short date. */
export function whenText(atMs: number, now: number = Date.now(), zone?: string): string {
  const options = zone ? { timeZone: zone } : {};
  const day = (ms: number) => new Intl.DateTimeFormat('en-CA', { ...options, dateStyle: 'short' }).format(new Date(ms));
  const time = new Intl.DateTimeFormat('en', {
    ...options,
    hour: '2-digit',
    minute: '2-digit',
    hourCycle: 'h23',
  }).format(new Date(atMs));
  if (day(atMs) === day(now)) return `Today ${time}`;
  if (day(atMs) === day(now - 86_400_000)) return `Yesterday ${time}`;
  return `${new Intl.DateTimeFormat('en', { ...options, month: 'short', day: 'numeric' }).format(new Date(atMs))} ${time}`;
}

const short = (commit: string | undefined) => (commit ? commit.slice(0, 7) : undefined);

/** The event a delivery came from, in words. */
function eventText(entry: TriggerHistoryEntry): string | undefined {
  const commit = short(entry.evidence.commit),
    run = entry.evidence.ci_run?.split('.')[0];
  switch (entry.kind) {
    case 'ci_failed':
      return `CI failed on ${commit ?? 'a commit'}${run ? ` (run ${run})` : ''}`;
    case 'commit_landed':
      return `${commit ?? 'A commit'} landed`;
    case 'gate_failed':
      return `A run’s CI gate failed${commit ? ` on ${commit}` : ''}`;
    case 'ticket_opened':
      return `${entry.evidence.slug ?? 'A ticket'} opened`;
    case 'manual':
      return 'Run by hand';
    case 'schedule':
      return undefined;
    default:
      return entry.kind satisfies never;
  }
}

/** A refusal or wait code in words. */
export function reasonText(reason: string | undefined): string | undefined {
  if (!reason) return undefined;
  if (reason === 'permission_required') return 'waits for permission';
  if (reason === 'template_planning') return undefined;
  if (reason === 'template_changed') return 'its template changed';
  return reason.replaceAll('_', ' ');
}

/** One Recent Runs (or Would Have Run) row: a time and what happened. */
export function historyRow(
  entry: TriggerHistoryEntry,
  now: number = Date.now(),
  labels: Labels = {},
  dryRunView = true,
): { label: string; value: string } {
  const label = whenText(entry.at_ms, now),
    event = eventText(entry),
    reason = reasonText(entry.reason);
  const lead = event ? `${event} · ` : '';
  const missed =
    entry.missed > 0
      ? ` · missed ${entry.missed === 1 ? '1 run' : `${entry.missed} runs`} while this computer was off; ran once on return`
      : '';
  switch (entry.outcome) {
    case 'applied': {
      const parts = ['Ran'];
      if (entry.goal_slug) parts.push(`goal ${entry.goal_slug}`);
      if (entry.placement === 'start') parts.push('started orchestration');
      if (entry.placement === 'pause') parts.push('paused orchestration');
      if (reason) parts.push(reason);
      return { label, value: `${lead}${parts.join(' · ')}${missed}` };
    }
    case 'dry_run': {
      const would = entry.would;
      const parts: string[] = [];
      if (would?.goal_title)
        parts.push(
          `would open “${would.goal_title}”${would.steps ? ` with ${would.steps === 1 ? '1 step' : `${would.steps} steps`}` : ''}`,
        );
      if (would?.placement === 'start') parts.push('would start orchestration');
      if (would?.placement === 'pause') parts.push('would pause orchestration');
      if (would && would.asks.length > 0)
        parts.push(`would ask in ${would.asks.map((target) => named(labels, target)).join(', ')}`);
      if (reason) parts.push(reason);
      // Outside a dry run, an earlier dry-run entry says it was one.
      const prefix = dryRunView ? '' : 'Dry run · ';
      return { label, value: `${prefix}${lead}${parts.length > 0 ? parts.join(' · ') : 'would run'}${missed}` };
    }
    case 'skipped_overlap':
      return {
        label,
        value: `Skipped · the last run was still working${entry.count > 1 ? ` (${entry.count} skips)` : ''}`,
      };
    case 'queued':
      return { label, value: `${lead}Runs once the current run finishes` };
    case 'already_handled':
      return {
        label,
        value: `Same event reported again · already handled${entry.count > 1 ? ` (${entry.count} times)` : ''}`,
      };
    case 'needs_review':
      return { label, value: `Waits for a review · ${reason ?? 'its template changed'}` };
    case 'refused':
      return { label, value: `${lead}Could not run · ${reason ?? 'refused'}` };
    default:
      return entry.outcome satisfies never;
  }
}

/** Recent Runs, newest first. */
export function historyRows(view: TriggerView, now: number = Date.now(), labels: Labels = {}) {
  const dry = view.state === 'dry_run';
  return [...view.history].reverse().map((entry) => {
    // The running delivery's apply may have opened its goal since it was recorded (a permission answered).
    const current =
      entry.outcome === 'applied' &&
      !entry.goal_slug &&
      view.running_goal !== undefined &&
      entry.operation_id !== undefined &&
      entry.operation_id === view.active_operation
        ? { ...entry, goal_slug: view.running_goal, reason: undefined }
        : entry;
    return { ...historyRow(current, now, labels, dry), outcome: entry.outcome, key: entry.delivery_key };
  });
}

/**
 * The overlap banner: shown while the last delivery was skipped because the run it started is still working,
 * as "Skipped 2 Runs: The Last Run Is Still Working" with the goal to open.
 */
export function overlapBanner(view: TriggerView): { title: string; detail: string; goal?: string } | undefined {
  const last = view.history.at(-1);
  if (!last || last.outcome !== 'skipped_overlap' || !view.running) return undefined;
  const goal = view.running_goal;
  return {
    title: `Skipped ${last.count === 1 ? '1 Run' : `${last.count} Runs`}: The Last Run Is Still Working`,
    detail: `A trigger never overlaps its own runs. ${
      goal ? `${goal} is still open` : 'The last run is still working'
    }; the next run starts once it finishes${goal ? ` or you close ${goal}` : ''}.`,
    ...(goal ? { goal } : {}),
  };
}

/** The Dry Run and Review banners, with the Turn On… they offer. */
export function stateBanner(
  view: TriggerView,
): { tone: 'info' | 'warning'; title: string; detail: string; turnOn: boolean } | undefined {
  if (view.pending_on)
    return {
      tone: 'info',
      title: 'Waiting for Permission',
      detail: 'An AI session asked to turn this trigger on. It turns on once a person allows it.',
      turnOn: false,
    };
  if (view.state === 'dry_run') {
    const events = view.history.filter((entry) => entry.outcome === 'dry_run').length;
    return {
      tone: 'info',
      title: 'Dry Run',
      detail: `${events === 1 ? '1 event' : `${events} events`} so far. Each shows what would have happened. Nothing was created and nobody was asked.`,
      turnOn: true,
    };
  }
  if (view.state === 'needs_review')
    return {
      tone: 'warning',
      title: 'Needs Review',
      detail: 'Its template changed since it was turned on. It runs again once you review it and turn it on.',
      turnOn: true,
    };
  return undefined;
}

/** A source warning (an unreachable project, a watch without credentials), in words. */
export function warningText(view: TriggerView): string | undefined {
  return view.warning
    ? `It cannot watch its source right now (${view.warning.code.replaceAll('_', ' ')}). It keeps trying; there is no failover.`
    : undefined;
}

/** The Status rows: state, what runs, and the next times or the event. */
export function statusRows(view: TriggerView, labels: Labels, now: number = Date.now(), zone?: string) {
  const state =
    view.state === 'on'
      ? `On${view.review ? ` · reviewed ${view.review.role === 'ai' ? 'and allowed' : 'by you'} ${whenText(view.review.at_ms, now, zone)}` : ''}`
      : stateBadge(view.state).label;
  const rows = [
    { label: 'State', value: state },
    { label: 'Runs', value: actionText(view.action, view.template_name) },
  ];
  if (view.source.kind === 'schedule') {
    const next = view.next_runs_ms.map((at) => whenText(at, now, zone)).join(' · ');
    rows.push({
      label: 'Next',
      value: next
        ? `${next}${view.running && view.running_goal ? `, if ${view.running_goal} has finished` : ''}`
        : view.state === 'paused'
          ? 'Not while paused'
          : 'No further runs',
    });
  } else rows.push({ label: 'Watches', value: sourceText(view.source, labels) });
  return rows;
}

const PERMISSION_TEXT: Readonly<Record<ReviewNeed, string>> = {
  not_needed: 'Never asks',
  remembered: 'Allowed by a remembered rule',
  asks: 'Asks through the permission prompt',
  unchanged: 'Unchanged',
};

/** The review's Scope rows. */
export function scopeRows(review: TriggerReview, labels: Labels, ownerName: string) {
  const watched = review.watches;
  const watches =
    watched.kind === 'schedule'
      ? 'A schedule on this computer'
      : watched.branch
        ? `${watched.label ?? named(labels, watched.target_id)} · ${watched.branch.provider ?? 'GitHub Actions'} ${watched.branch.workflow} on ${watched.branch.branch}`
        : watched.kind === 'ticket_opened'
          ? `Tickets opening in ${watched.label ?? named(labels, watched.target_id)}${watched.tag ? ` tagged ${watched.tag}` : ''}${watched.category ? ` of category ${watched.category}` : ''}`
          : `Failed CI gates of runs${watched.target_id ? ` in ${watched.label ?? named(labels, watched.target_id)}` : ''}`;
  const projects = review.projects.map((project) => project.label ?? project.target_id);
  const changes = [
    ...(review.perDelivery.goals > 0 ? [`${ownerName} (goal ticket)`] : []),
    ...(projects.length > 0 ? [`${projects.join(', ')} tickets`] : []),
  ];
  const per: string[] = [];
  if (review.perDelivery.goals > 0)
    per.push(`1 goal, ${review.perDelivery.steps === 1 ? '1 step' : `${review.perDelivery.steps} steps`}`);
  if (review.perDelivery.placement === 'start') per.push('starts orchestration');
  if (review.perDelivery.placement === 'pause') per.push('pauses orchestration');
  return [
    { label: 'Watches', value: watches },
    { label: 'Changes', value: changes.length > 0 ? `${changes.join(' and ')}; no other project` : 'No tickets' },
    { label: review.watches.kind === 'schedule' ? 'Per Run' : 'Per Event', value: per.join(', ') || 'Nothing' },
  ];
}

/** The review's Permissions rows: watching never asks, writes ask or are remembered, push stays unchanged. */
export function permissionRows(review: TriggerReview, labels: Labels) {
  return review.permissions.map((row) => {
    const where = row.target_id ? named(labels, row.target_id) : undefined;
    switch (row.action) {
      case 'watch':
        return {
          label: where ? `Watch ${where}` : 'Watch',
          value: `${PERMISSION_TEXT.not_needed}${where ? `; adding ${where} was the consent` : ''}`,
        };
      case 'tickets_write':
        return { label: `Create Tickets in ${where ?? 'the Project'}`, value: PERMISSION_TEXT[row.need] };
      case 'session_start':
        return {
          label: `Start Sessions in ${where ?? 'the Project'}`,
          value: `${PERMISSION_TEXT[row.need]}${row.tool ? ` (${row.tool})` : ''}`,
        };
      case 'placement_start':
        return { label: 'Start Orchestration', value: 'Allowed by turning the trigger on' };
      case 'placement_pause':
        return { label: 'Pause Orchestration', value: 'Allowed by turning the trigger on' };
      case 'push_deploy':
        return { label: 'Push and Deploy', value: 'Unchanged: each project’s integrator and push policy' };
      default:
        return { label: row.action.replaceAll('_', ' '), value: PERMISSION_TEXT[row.need] };
    }
  });
}

/** The review's Next Actions rows. */
export function nextRows(review: TriggerReview, now: number = Date.now(), zone?: string) {
  const rows: Array<{ label: string; value: string }> = [];
  if (review.next.runs_ms.length > 0)
    rows.push({ label: 'Next Runs', value: review.next.runs_ms.map((at) => whenText(at, now, zone)).join(' · ') });
  if (review.next.event)
    rows.push({
      label: 'Each Event',
      value: `${review.perDelivery.goal_title ? `Opens “${review.perDelivery.goal_title}”` : 'Runs the action'}; the scheduler places its steps within the Orchestration settings`,
    });
  rows.push({ label: 'Duplicates', value: 'The same event or time is handled once' });
  rows.push({
    label: 'While Running',
    value:
      review.next.overlap === 'queue_latest'
        ? 'The latest event waits and runs once the current run finishes'
        : 'Later events are skipped and recorded while a run is working',
  });
  if (review.turn_on === 'asks')
    rows.push({ label: 'Turning On', value: 'An AI session’s turn on asks through the permission prompt' });
  return rows;
}

const PROBLEMS: Readonly<Record<string, string>> = {
  name_invalid: 'Name the trigger',
  time_zone_invalid: 'Choose a time zone',
  local_time_invalid: 'Choose a time',
  recurrence_invalid: 'Check the schedule; it repeats at most every 5 minutes',
  target_invalid: 'Choose a project',
  target_unknown: 'The project is not part of this orchestration project',
  tag_invalid: 'Check the tag',
  category_invalid: 'Check the category',
  watch_invalid: 'Fill in the repository, workflow and branch without spaces',
  action_empty: 'Choose a template or a placement change to run',
  template_invalid: 'Choose a template',
  template_unknown: 'The template no longer exists or was retired',
  tool_invalid: 'Choose the AI tool the planner runs in',
  inputs_too_many: 'Use at most 16 inputs',
  input_invalid: 'An input name is not valid',
  input_twice: 'An input is both fixed and filled from the event',
  event_input_unavailable: 'An input is filled from a value this event does not carry',
};

export function triggerProblemText(problem: TriggerProblem): string {
  return `${PROBLEMS[problem.code] ?? problem.code.replaceAll('_', ' ')}.`;
}

/** The event values a source's deliveries carry, as the server's `event_fields`. */
export function eventFields(kind: TriggerSourceKind): EventField[] {
  switch (kind) {
    case 'schedule':
      return ['occurrence'];
    case 'ticket_opened':
      return ['ticket'];
    case 'gate_failed':
      return ['commit', 'gate', 'ticket'];
    case 'branch_ci_failed':
      return ['commit', 'ci_run', 'branch'];
    case 'commit_landed':
      return ['commit', 'branch'];
    default:
      return kind satisfies never;
  }
}

export const EVENT_FIELD_LABELS: Readonly<Record<EventField, string>> = {
  commit: 'Commit',
  ci_run: 'CI Run',
  ticket: 'Ticket',
  branch: 'Branch',
  occurrence: 'Scheduled Time',
  gate: 'Gate',
};

export const EVENT_LABELS: Readonly<Record<TriggerEvent, string>> = {
  ticket_opened: 'A Ticket Opens',
  gate_failed: 'A Run’s CI Gate Fails',
  branch_ci_failed: 'CI Fails on a Branch',
  commit_landed: 'A Commit Lands on a Branch',
};

/** The setup form of New Trigger… and Edit…. */
export interface TriggerForm {
  name: string;
  when: 'schedule' | 'event';
  schedule: ScheduleFormDraft;
  target: string;
  event: TriggerEvent;
  tag: string;
  category: string;
  provider: string;
  repository: string;
  workflow: string;
  branch: string;
  /** The template to apply; empty for none. */
  template: string;
  tool: string;
  /** Template input key → the event value it is filled from; absent keeps the template's default. */
  eventInputs: Readonly<Partial<Record<string, EventField>>>;
  placement: '' | Placement;
  start: 'dry_run' | 'paused';
  overlap: Overlap;
}

export function newTriggerForm(defaults: { tool?: string; target?: string } = {}): TriggerForm {
  const schedule = newScheduleDraft();
  return {
    name: '',
    when: 'schedule',
    schedule: { ...schedule, scheduleType: 'repeating', repeats: 'daily' },
    target: defaults.target ?? '',
    event: 'branch_ci_failed',
    tag: '',
    category: '',
    provider: '',
    repository: '',
    workflow: '',
    branch: 'main',
    template: '',
    tool: defaults.tool ?? '',
    eventInputs: {},
    placement: '',
    start: 'dry_run',
    overlap: 'skip',
  };
}

const trimmed = (value: string) => value.trim();

/** The source the form describes. */
export function formSource(form: TriggerForm): TriggerSource {
  if (form.when === 'schedule') {
    const input = scheduleInputFromDraft(form.schedule);
    return {
      kind: 'schedule',
      schedule: { time_zone: input.time_zone, local_time: input.local_time, recurrence: input.recurrence },
    };
  }
  const target = trimmed(form.target);
  switch (form.event) {
    case 'ticket_opened':
      return {
        kind: 'ticket_opened',
        target_id: target,
        ...(trimmed(form.tag) ? { tag: trimmed(form.tag) } : {}),
        ...(trimmed(form.category) ? { category: trimmed(form.category) } : {}),
      };
    case 'gate_failed':
      return { kind: 'gate_failed', ...(target ? { target_id: target } : {}) };
    case 'branch_ci_failed':
    case 'commit_landed':
      return {
        kind: form.event,
        target_id: target,
        watch: {
          ...(trimmed(form.provider) ? { provider: trimmed(form.provider) } : {}),
          repository: trimmed(form.repository),
          workflow: trimmed(form.workflow),
          branch: trimmed(form.branch),
        },
      };
    default:
      return form.event satisfies never;
  }
}

/**
 * The definition the form describes, as `set` and `review` take it. Event inputs the source cannot fill are left
 * out, so switching the event never sends a mapping the server refuses.
 */
export function formToSpec(form: TriggerForm, fixedInputs: Readonly<Record<string, unknown>> = {}) {
  const source = formSource(form);
  const available = new Set(eventFields(source.kind));
  const eventInputs = Object.fromEntries(
    Object.entries(form.eventInputs).filter(
      (entry): entry is [string, EventField] => entry[1] !== undefined && available.has(entry[1]),
    ),
  );
  const inputs = Object.fromEntries(
    Object.entries(fixedInputs).filter(([key, value]) => !(key in eventInputs) && value !== '' && value !== undefined),
  );
  return {
    name: trimmed(form.name),
    source,
    action: {
      ...(form.template
        ? {
            template: {
              template_id: form.template,
              tool: trimmed(form.tool),
              ...(Object.keys(inputs).length > 0 ? { inputs } : {}),
              ...(Object.keys(eventInputs).length > 0 ? { event_inputs: eventInputs } : {}),
            },
          }
        : {}),
      ...(form.placement ? { placement: form.placement } : {}),
    },
    overlap: form.overlap,
  };
}

/** The `set` body: the definition, the trigger's id when editing, and how a new one starts. */
export function formToRequest(
  form: TriggerForm,
  triggerId?: string,
  fixedInputs: Readonly<Record<string, unknown>> = {},
) {
  return { ...(triggerId ? { trigger_id: triggerId } : {}), ...formToSpec(form, fixedInputs), start: form.start };
}

/** The form for Edit…: the saved definition, with the command-schedule draft for a schedule. */
export function formFromTrigger(view: TriggerView): TriggerForm {
  const form = newTriggerForm({ tool: view.action.template?.tool });
  const base: TriggerForm = {
    ...form,
    name: view.name,
    template: view.action.template?.template_id ?? '',
    eventInputs: view.action.template?.event_inputs ?? {},
    placement: view.action.placement ?? '',
    start: view.state === 'paused' ? 'paused' : 'dry_run',
    overlap: view.overlap,
  };
  const source = view.source;
  switch (source.kind) {
    case 'schedule': {
      const recurrence = source.schedule.recurrence;
      const draft = form.schedule;
      return {
        ...base,
        when: 'schedule',
        schedule: {
          ...draft,
          scheduleType: recurrence.kind === 'once' ? 'once' : 'repeating',
          repeats: recurrence.kind === 'once' ? draft.repeats : recurrence.kind,
          date: recurrence.date ?? draft.date,
          startDate: recurrence.start_date ?? draft.startDate,
          every: String(recurrence.every ?? 1),
          weekdays: recurrence.weekdays ?? draft.weekdays,
          selectedDates: (recurrence.dates ?? []).join(', '),
          time: source.schedule.local_time,
          timeZone: source.schedule.time_zone,
        },
      };
    }
    case 'ticket_opened':
      return {
        ...base,
        when: 'event',
        event: 'ticket_opened',
        target: source.target_id,
        tag: source.tag ?? '',
        category: source.category ?? '',
      };
    case 'gate_failed':
      return { ...base, when: 'event', event: 'gate_failed', target: source.target_id ?? '' };
    case 'branch_ci_failed':
    case 'commit_landed':
      return {
        ...base,
        when: 'event',
        event: source.kind,
        target: source.target_id,
        provider: source.watch.provider ?? '',
        repository: source.watch.repository,
        workflow: source.watch.workflow,
        branch: source.watch.branch,
      };
    default:
      return source satisfies never;
  }
}

/** Why Save is disabled before the server is asked, or undefined. */
export function formProblem(form: TriggerForm): string | undefined {
  if (trimmed(form.name) === '') return 'Name the trigger';
  if (form.when === 'event') {
    if (form.event !== 'gate_failed' && trimmed(form.target) === '') return 'Choose a project';
    if (
      (form.event === 'branch_ci_failed' || form.event === 'commit_landed') &&
      [form.repository, form.workflow, form.branch].some((value) => trimmed(value) === '')
    )
      return 'Fill in the repository, workflow and branch';
  }
  if (!form.template && !form.placement) return 'Choose a template or a placement change to run';
  if (form.template && trimmed(form.tool) === '') return 'Choose the AI tool the planner runs in';
  return undefined;
}

/** A fresh operation id for a manual run (`fire`); kept while the same run is retried. */
export function newFireOperationId(random: () => string = () => crypto.randomUUID().replaceAll('-', '')): string {
  return `fire-${random()}`.slice(0, 140);
}
