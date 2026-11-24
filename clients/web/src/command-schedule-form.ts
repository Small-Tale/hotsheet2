import type {
  DayRule,
  Recurrence,
  RecurrenceKind,
  ScheduleAction,
  ScheduleInput,
  ScheduleView,
} from './command-schedules';

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
  action: ScheduleAction;
  paused: boolean;
}

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
    action: 'create_ticket',
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
    action: schedule.action,
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
    action: draft.action,
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
