import type {
  DayRule,
  Recurrence,
  RecurrenceKind,
  ScheduleAction,
  ScheduleInput,
  ScheduleView,
} from './command-schedules';

/** The host's floor for sub-day repeats (HS2-29V4ZG); mirrors `MIN_INTERVAL_MINUTES`. */
export const MIN_INTERVAL_MINUTES = 5;

export interface EveryBounds {
  min: number;
  max: number;
  label: string;
  hint?: string;
}

/** The Every field's unit and bounds for one repeat kind, matching host validation. */
export function everyBounds(repeats: Exclude<RecurrenceKind, 'once'>): EveryBounds {
  if (repeats === 'minutely')
    return {
      min: MIN_INTERVAL_MINUTES,
      max: 1440,
      label: 'Every (Minutes)',
      hint: `Minimum ${MIN_INTERVAL_MINUTES} minutes.`,
    };
  if (repeats === 'hourly') return { min: 1, max: 24, label: 'Every (Hours)', hint: '1 to 24 hours.' };
  return { min: 1, max: 365, label: 'Every' };
}

export const isSubDay = (kind: RecurrenceKind) => kind === 'minutely' || kind === 'hourly';

/**
 * Change the repeat kind and keep Every inside the new kind's bounds, so switching from
 * Daily (every 1) to minutes does not submit an interval below the floor.
 */
export function withRepeats(draft: ScheduleFormDraft, repeats: ScheduleFormDraft['repeats']): ScheduleFormDraft {
  const { min, max } = everyBounds(repeats),
    every = Number(draft.every);
  const clamped = Number.isFinite(every) ? Math.min(max, Math.max(min, Math.round(every))) : min;
  return { ...draft, repeats, every: String(repeats === 'minutely' && every < min ? 15 : clamped) };
}

/** The occurrence's local date, plus its time for sub-day kinds. */
export function occurrenceLabel(item: ScheduleView['history'][number]): string {
  return item.occurrence_time ? `${item.occurrence_date} ${item.occurrence_time}` : item.occurrence_date;
}

export interface ScheduleFormDraft {
  scheduleType: 'once' | 'repeating';
  repeats: Exclude<RecurrenceKind, 'once'>;
  date: string;
  startDate: string;
  every: string;
  weekdays: number[];
  month: string;
  on: DayRule['kind'];
  day: string;
  ordinal: string;
  weekday: string;
  selectedDates: string;
  endDate: string;
  count: string;
  includeDates: string;
  excludeDates: string;
  time: string;
  timeZone: string;
  /** `command_default` inherits the command's action and saves no override (HS2-17Y2EC). */
  action: ScheduleAction | typeof COMMAND_DEFAULT_ACTION;
  paused: boolean;
}

/** The schedule Action choice that follows the command's own action (HS2-17Y2EC). */
export const COMMAND_DEFAULT_ACTION = 'command_default';

const listDates = (value: string) =>
  value
    .split(/[\s,]+/)
    .map((part) => part.trim())
    .filter(Boolean);

function localDate() {
  const parts = new Intl.DateTimeFormat('en-CA', { year: 'numeric', month: '2-digit', day: '2-digit' }).formatToParts(
    new Date(),
  );
  const part = (type: string) => parts.find((item) => item.type === type)?.value ?? '';
  return `${part('year')}-${part('month')}-${part('day')}`;
}

export function newScheduleDraft(): ScheduleFormDraft {
  const today = localDate();
  return {
    scheduleType: 'once',
    repeats: 'daily',
    date: today,
    startDate: today,
    every: '1',
    weekdays: [1],
    month: '1',
    on: 'day_of_month',
    day: '1',
    ordinal: '1',
    weekday: '1',
    selectedDates: today,
    endDate: '',
    count: '',
    includeDates: '',
    excludeDates: '',
    time: '09:00',
    timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC',
    action: COMMAND_DEFAULT_ACTION,
    paused: false,
  };
}

export function scheduleDraftFromView(view: ScheduleView): ScheduleFormDraft {
  const draft = newScheduleDraft(),
    schedule = view.schedule,
    recurrence = schedule.recurrence;
  const on = recurrence.on;
  return {
    ...draft,
    scheduleType: recurrence.kind === 'once' ? 'once' : 'repeating',
    repeats: recurrence.kind === 'once' ? draft.repeats : recurrence.kind,
    date: recurrence.date ?? draft.date,
    startDate: recurrence.start_date ?? draft.startDate,
    every: String(recurrence.every ?? 1),
    weekdays: recurrence.weekdays ?? draft.weekdays,
    month: String(recurrence.month ?? 1),
    on: on?.kind ?? draft.on,
    day: String(on?.kind === 'day_of_month' ? on.day : 1),
    ordinal: String(on?.kind === 'weekday_ordinal' ? on.ordinal : 1),
    weekday: String(on?.kind === 'weekday_ordinal' ? on.weekday : 1),
    selectedDates: (recurrence.dates ?? []).join(', '),
    endDate: recurrence.end_date ?? '',
    count: recurrence.count === undefined ? '' : String(recurrence.count),
    includeDates: (recurrence.include_dates ?? []).join(', '),
    excludeDates: (recurrence.exclude_dates ?? []).join(', '),
    time: schedule.local_time,
    timeZone: schedule.time_zone,
    action: schedule.action ?? COMMAND_DEFAULT_ACTION,
    paused: schedule.paused,
  };
}

export function scheduleInputFromDraft(draft: ScheduleFormDraft): ScheduleInput {
  let recurrence: Recurrence;
  if (draft.scheduleType === 'once') recurrence = { kind: 'once', date: draft.date };
  else if (draft.repeats === 'selected_dates')
    recurrence = { kind: 'selected_dates', dates: listDates(draft.selectedDates) };
  else {
    recurrence = {
      kind: draft.repeats,
      start_date: draft.startDate,
      every: Number(draft.every),
      ...(draft.endDate ? { end_date: draft.endDate } : {}),
      ...(draft.count ? { count: Number(draft.count) } : {}),
      ...(draft.includeDates ? { include_dates: listDates(draft.includeDates) } : {}),
      ...(draft.excludeDates ? { exclude_dates: listDates(draft.excludeDates) } : {}),
    };
    if (draft.repeats === 'weekly') recurrence.weekdays = draft.weekdays;
    if (draft.repeats === 'monthly' || draft.repeats === 'yearly') {
      recurrence.on =
        draft.on === 'last_day'
          ? { kind: 'last_day' }
          : draft.on === 'day_of_month'
            ? { kind: 'day_of_month', day: Number(draft.day) }
            : { kind: 'weekday_ordinal', ordinal: Number(draft.ordinal), weekday: Number(draft.weekday) };
      if (draft.repeats === 'yearly') recurrence.month = Number(draft.month);
    }
  }
  return {
    time_zone: draft.timeZone,
    local_time: draft.time,
    ...(draft.action === COMMAND_DEFAULT_ACTION ? {} : { action: draft.action }),
    paused: draft.paused,
    recurrence,
  };
}

const WEEKDAY_NAMES = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday'];
const ORDINAL_NAMES: Record<number, string> = {
  1: 'first',
  2: 'second',
  3: 'third',
  4: 'fourth',
  5: 'fifth',
  [-1]: 'last',
};

/** Compact, readable cadence for the command row (HS2-17663F). */
export function scheduleCadence(view: ScheduleView): string {
  const { recurrence, local_time: time, time_zone: zone, paused } = view.schedule;
  if (paused) return 'Paused';
  let cadence: string;
  switch (recurrence.kind) {
    case 'minutely':
      return `Every ${recurrence.every ?? MIN_INTERVAL_MINUTES} minutes from ${time} ${zone}`;
    case 'hourly':
      return `${recurrence.every === 1 ? 'Hourly' : `Every ${recurrence.every ?? 1} hours`} from ${time} ${zone}`;
    case 'once':
      cadence = recurrence.date ?? 'One time';
      break;
    case 'daily':
      cadence = recurrence.every === 1 ? 'Daily' : `Every ${recurrence.every ?? 1} days`;
      break;
    case 'weekly': {
      const days = (recurrence.weekdays ?? []).map((day) => WEEKDAY_NAMES[day - 1] ?? '').filter(Boolean);
      cadence = `Weekly ${days.join(', ')}`;
      break;
    }
    case 'monthly':
    case 'yearly': {
      const on = recurrence.on;
      const day =
        on?.kind === 'last_day'
          ? 'last day'
          : on?.kind === 'day_of_month'
            ? `day ${on.day}`
            : on?.kind === 'weekday_ordinal'
              ? `${ORDINAL_NAMES[on.ordinal] ?? on.ordinal} ${WEEKDAY_NAMES[on.weekday - 1] ?? ''}`
              : '';
      const month =
        recurrence.kind === 'yearly'
          ? new Intl.DateTimeFormat('en', { month: 'long' }).format(
              new Date(Date.UTC(2024, (recurrence.month ?? 1) - 1, 1)),
            )
          : '';
      cadence = `${recurrence.kind === 'monthly' ? 'Monthly' : `Yearly ${month}`} on ${day}`;
      break;
    }
    case 'selected_dates':
      cadence = `${recurrence.dates?.length ?? 0} selected dates`;
      break;
  }
  return `${cadence} at ${time} ${zone}`;
}

export function scheduleNextRun(view: ScheduleView): string | undefined {
  const instant = view.next_runs[0];
  if (!instant) return undefined;
  try {
    return new Intl.DateTimeFormat('en', {
      timeZone: view.schedule.time_zone,
      month: 'short',
      day: 'numeric',
      year: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
      hourCycle: 'h23',
    }).format(new Date(instant));
  } catch {
    return instant;
  }
}
