/** Checkout-local AI command schedules (HS2-G5J2E7, HS2-17663F, HS2-29V4ZG). */
export type ScheduleAction = 'create_ticket' | 'new_ai_session' | 'reuse_or_new_ai_session';
export const SCHEDULE_ACTIONS: readonly ScheduleAction[] = [
  'create_ticket',
  'new_ai_session',
  'reuse_or_new_ai_session',
];

export function isScheduleAction(value: unknown): value is ScheduleAction {
  return SCHEDULE_ACTIONS.includes(value as ScheduleAction);
}

/** One label per action, shared by the command editor, schedule dialog, and sidebar (HS2-17Y2EC). */
export const SCHEDULE_ACTION_LABELS: Record<ScheduleAction, string> = {
  create_ticket: 'Create Ticket Only',
  new_ai_session: 'Auto-Start in a New AI Session',
  reuse_or_new_ai_session: 'Reuse or Start an AI Session',
};

/** A command without an action keeps the original click: queue and signal an idle session (HS2-17Y2EC). */
export const LEGACY_COMMAND_ACTION_LABEL = 'Create Ticket and Notify an Idle AI Session';

/** What clicking an AI command does, for its editor and sidebar tooltip (HS2-17Y2EC). */
export const commandClickActionLabel = (action: ScheduleAction | undefined) =>
  action ? SCHEDULE_ACTION_LABELS[action] : LEGACY_COMMAND_ACTION_LABEL;

/** What a schedule inheriting this command's action does; schedules never signaled, so unset is ticket-only. */
export const scheduleInheritedAction = (commandAction: ScheduleAction | undefined): ScheduleAction =>
  commandAction ?? 'create_ticket';
/**
 * The commands an active schedule runs: a saved schedule that is not paused (HS2-MA31ZN). A paused
 * schedule never runs, so its command counts as unscheduled.
 */
export function activelyScheduledCommandIds(views: readonly ScheduleView[]): Set<string> {
  return new Set(views.filter((view) => !view.schedule.paused).map((view) => view.schedule.command_id));
}

/**
 * The commands the project sidebar shows (HS2-MA31ZN): an AI command with an active schedule is hidden
 * unless its `show_when_scheduled` option is on. Order and every other command are unchanged.
 */
export function sidebarCommandDefinitions<T extends { id: string; kind?: string; show_when_scheduled?: boolean }>(
  definitions: readonly T[],
  views: readonly ScheduleView[],
): T[] {
  const scheduled = activelyScheduledCommandIds(views);
  return definitions.filter(
    (command) => command.kind !== 'ai' || command.show_when_scheduled === true || !scheduled.has(command.id),
  );
}

export type RecurrenceKind =
  'once' | 'minutely' | 'hourly' | 'daily' | 'weekly' | 'monthly' | 'yearly' | 'selected_dates';
const RECURRENCE_KINDS: readonly RecurrenceKind[] = [
  'once',
  'minutely',
  'hourly',
  'daily',
  'weekly',
  'monthly',
  'yearly',
  'selected_dates',
];
export type DayRule =
  | { kind: 'day_of_month'; day: number }
  | { kind: 'last_day' }
  | { kind: 'weekday_ordinal'; ordinal: number; weekday: number };

export interface Recurrence {
  kind: RecurrenceKind;
  date?: string;
  start_date?: string;
  every?: number;
  weekdays?: number[];
  on?: DayRule;
  month?: number;
  dates?: string[];
  end_date?: string;
  count?: number;
  include_dates?: string[];
  exclude_dates?: string[];
}

export interface ScheduleInput {
  time_zone: string;
  local_time: string;
  /** Absent inherits the command's action (HS2-17Y2EC). */
  action?: ScheduleAction;
  paused: boolean;
  recurrence: Recurrence;
}

export interface ScheduleView {
  schedule: ScheduleInput & { command_id: string };
  /** The override, else the command's action, else ticket-only; absent while unavailable (HS2-17Y2EC). */
  effective_action?: ScheduleAction;
  next_runs: string[];
  history: Array<{
    occurrence_date: string;
    /** Local HH:mm of a sub-day occurrence (HS2-29V4ZG). */
    occurrence_time?: string;
    outcome:
      | 'created'
      | 'skipped_open_ticket'
      | 'delivery_pending'
      | 'delivery_dispatching'
      | 'delivered'
      | 'delivery_failed'
      | 'delivery_uncertain';
    ticket_id?: string;
    ticket_slug?: string;
    turn_completed?: boolean;
    delivery_retry?: {
      failure_count: number;
      /** A failed occurrence with no deadline has stopped retrying. */
      retry_after_ms?: number;
    };
  }>;
  unavailable?: string;
}

function object(value: unknown, label: string): Record<string, unknown> {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) throw new Error(`Invalid ${label} response`);
  return value as Record<string, unknown>;
}

function strings(value: unknown, label: string): string[] {
  if (!Array.isArray(value) || !value.every((item) => typeof item === 'string'))
    throw new Error(`Invalid ${label} response`);
  return value;
}

export function parseScheduleView(value: unknown): ScheduleView {
  const view = object(value, 'command schedule');
  const schedule = object(view.schedule, 'command schedule');
  const recurrence = object(schedule.recurrence, 'command recurrence');
  if (
    typeof schedule.command_id !== 'string' ||
    typeof schedule.time_zone !== 'string' ||
    typeof schedule.local_time !== 'string' ||
    (schedule.action !== undefined && !isScheduleAction(schedule.action)) ||
    (view.effective_action !== undefined && !isScheduleAction(view.effective_action)) ||
    typeof schedule.paused !== 'boolean' ||
    !RECURRENCE_KINDS.includes(recurrence.kind as RecurrenceKind) ||
    (view.unavailable !== undefined && typeof view.unavailable !== 'string')
  )
    throw new Error('Invalid command schedule response');
  const history = view.history;
  if (!Array.isArray(history)) throw new Error('Invalid command schedule history response');
  for (const entry of history) {
    const item = object(entry, 'command schedule history');
    const retry = item.delivery_retry === undefined ? undefined : object(item.delivery_retry, 'command delivery retry');
    if (
      typeof item.occurrence_date !== 'string' ||
      (item.occurrence_time !== undefined && typeof item.occurrence_time !== 'string') ||
      ![
        'created',
        'skipped_open_ticket',
        'delivery_pending',
        'delivery_dispatching',
        'delivered',
        'delivery_failed',
        'delivery_uncertain',
      ].includes(String(item.outcome)) ||
      (item.ticket_id !== undefined && typeof item.ticket_id !== 'string') ||
      (item.ticket_slug !== undefined && typeof item.ticket_slug !== 'string') ||
      (item.turn_completed !== undefined && typeof item.turn_completed !== 'boolean') ||
      (retry !== undefined &&
        (!Number.isSafeInteger(retry.failure_count) ||
          (retry.retry_after_ms !== undefined && !Number.isSafeInteger(retry.retry_after_ms))))
    )
      throw new Error('Invalid command schedule history response');
  }
  return {
    schedule: schedule as unknown as ScheduleView['schedule'],
    ...(view.effective_action === undefined ? {} : { effective_action: view.effective_action }),
    next_runs: strings(view.next_runs, 'command schedule preview'),
    history: history as ScheduleView['history'],
    ...(view.unavailable === undefined ? {} : { unavailable: view.unavailable }),
  };
}

export function parseScheduleViews(value: unknown): ScheduleView[] {
  if (!Array.isArray(value)) throw new Error('Invalid command schedules response');
  return value.map(parseScheduleView);
}

export function parseSchedulePreview(value: unknown): { next_runs: string[] } {
  return { next_runs: strings(object(value, 'command schedule preview').next_runs, 'command schedule preview') };
}
