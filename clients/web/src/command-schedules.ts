/** Checkout-local AI command schedules (HS2-G5J2E7, HS2-17663F). */
export type ScheduleAction = 'create_ticket' | 'new_ai_session' | 'reuse_or_new_ai_session';
export type RecurrenceKind = 'once' | 'daily' | 'weekly' | 'monthly' | 'yearly' | 'selected_dates';
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
  action: ScheduleAction;
  paused: boolean;
  recurrence: Recurrence;
}

export interface ScheduleView {
  schedule: ScheduleInput & { command_id: string };
  next_runs: string[];
  history: Array<{
    occurrence_date: string;
    outcome: 'created' | 'skipped_open_ticket' | 'delivery_pending' | 'delivered' | 'delivery_failed';
    ticket_id?: string;
    ticket_slug?: string;
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
    !['create_ticket', 'new_ai_session', 'reuse_or_new_ai_session'].includes(String(schedule.action)) ||
    typeof schedule.paused !== 'boolean' ||
    !['once', 'daily', 'weekly', 'monthly', 'yearly', 'selected_dates'].includes(String(recurrence.kind)) ||
    (view.unavailable !== undefined && typeof view.unavailable !== 'string')
  )
    throw new Error('Invalid command schedule response');
  const history = view.history;
  if (!Array.isArray(history)) throw new Error('Invalid command schedule history response');
  for (const entry of history) {
    const item = object(entry, 'command schedule history');
    if (
      typeof item.occurrence_date !== 'string' ||
      !['created', 'skipped_open_ticket', 'delivery_pending', 'delivered', 'delivery_failed'].includes(
        String(item.outcome),
      ) ||
      (item.ticket_id !== undefined && typeof item.ticket_id !== 'string') ||
      (item.ticket_slug !== undefined && typeof item.ticket_slug !== 'string')
    )
      throw new Error('Invalid command schedule history response');
  }
  return {
    schedule: schedule as unknown as ScheduleView['schedule'],
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
