import { describe, expect, it } from 'vitest';

import {
  COMMAND_DEFAULT_ACTION,
  everyBounds,
  isSubDay,
  MIN_INTERVAL_MINUTES,
  newScheduleDraft,
  occurrenceLabel,
  scheduleCadence,
  scheduleDraftFromView,
  scheduleInputFromDraft,
  withRepeats,
  withScheduleField,
} from './command-schedule-form';
import {
  commandClickActionLabel,
  LEGACY_COMMAND_ACTION_LABEL,
  parseSchedulePreview,
  parseScheduleView,
  SCHEDULE_ACTION_LABELS,
  SCHEDULE_ACTIONS,
  scheduleInheritedAction,
} from './command-schedules';
import { CommandScheduleDialog } from './components/command-schedule-dialog';

describe('HS2-17663F command schedule form', () => {
  it('round-trips a paused yearly weekday occurrence and preserves its exact date rule', () => {
    const saved = parseScheduleView({
      schedule: {
        command_id: 'review',
        time_zone: 'Asia/Manila',
        local_time: '09:00',
        action: 'reuse_or_new_ai_session',
        paused: true,
        recurrence: {
          kind: 'yearly',
          start_date: '2027-01-01',
          every: 1,
          month: 2,
          on: { kind: 'weekday_ordinal', ordinal: 3, weekday: 1 },
          end_date: '2030-12-31',
          count: 4,
        },
      },
      next_runs: [],
      history: [{ occurrence_date: '2027-02-15', outcome: 'skipped_open_ticket', ticket_slug: 'HS2-EXAMPLE' }],
    });
    const draft = scheduleDraftFromView(saved);
    expect(draft.paused).toBe(true);
    expect(scheduleInputFromDraft(draft)).toEqual({
      time_zone: 'Asia/Manila',
      local_time: '09:00',
      action: 'reuse_or_new_ai_session',
      paused: true,
      recurrence: {
        kind: 'yearly',
        start_date: '2027-01-01',
        every: 1,
        month: 2,
        on: { kind: 'weekday_ordinal', ordinal: 3, weekday: 1 },
        end_date: '2030-12-31',
        count: 4,
      },
    });
  });

  it('switches between one-time, selected dates, and weekly without carrying hidden fields into the request', () => {
    const draft = newScheduleDraft();
    const once = scheduleInputFromDraft({ ...draft, date: '2027-03-12', time: '08:30' });
    expect(once.recurrence).toEqual({ kind: 'once', date: '2027-03-12' });
    const selected = scheduleInputFromDraft({
      ...draft,
      scheduleType: 'repeating',
      repeats: 'selected_dates',
      selectedDates: '2027-03-12, 2027-04-02',
    });
    expect(selected.recurrence).toEqual({ kind: 'selected_dates', dates: ['2027-03-12', '2027-04-02'] });
    const weekly = scheduleInputFromDraft({
      ...draft,
      scheduleType: 'repeating',
      repeats: 'weekly',
      weekdays: [1, 5],
      endDate: '2027-12-31',
    });
    expect(weekly.recurrence).toMatchObject({ kind: 'weekly', weekdays: [1, 5], end_date: '2027-12-31' });
    expect(weekly.recurrence).not.toHaveProperty('dates');
  });

  it('rejects a drifted preview or schedule response before the form uses it', () => {
    expect(() => parseSchedulePreview({ next_runs: ['2027-02-15T01:00:00Z', 12] })).toThrow();
    expect(() => parseScheduleView({ schedule: { command_id: 'x' }, next_runs: [], history: [] })).toThrow();
  });

  it('HS2-NR0VRF accepts a crash-uncertain occurrence without treating it as a retry', () => {
    const view = parseScheduleView({
      schedule: {
        command_id: 'review',
        time_zone: 'Asia/Manila',
        local_time: '09:00',
        action: 'new_ai_session',
        paused: false,
        recurrence: { kind: 'once', date: '2027-02-15' },
      },
      next_runs: [],
      history: [
        {
          occurrence_date: '2027-02-15',
          outcome: 'delivery_uncertain',
          ticket_id: 'source:ticket',
          turn_completed: false,
        },
      ],
    });
    expect(view.history[0]?.outcome).toBe('delivery_uncertain');
    expect(() => parseScheduleView({ ...view, history: [{ ...view.history[0], turn_completed: 'yes' }] })).toThrow();
  });
});

describe('HS2-29V4ZG sub-day command schedules', () => {
  const hourly = {
    schedule: {
      command_id: 'cleanup',
      time_zone: 'UTC',
      local_time: '08:30',
      action: 'create_ticket',
      paused: false,
      recurrence: { kind: 'hourly', start_date: '2027-01-01', every: 2, exclude_dates: ['2027-01-02'] },
    },
    next_runs: ['2027-01-01T10:30:00Z'],
    history: [
      { occurrence_date: '2027-01-01', occurrence_time: '08:30', outcome: 'created', ticket_slug: 'HS2-A' },
      { occurrence_date: '2027-01-01', occurrence_time: '10:30', outcome: 'skipped_open_ticket', ticket_slug: 'HS2-A' },
    ],
  };

  it('round-trips an hourly schedule and labels its occurrences with their time', () => {
    const view = parseScheduleView(hourly);
    const draft = scheduleDraftFromView(view);
    expect(draft).toMatchObject({ scheduleType: 'repeating', repeats: 'hourly', every: '2', time: '08:30' });
    expect(scheduleInputFromDraft(draft).recurrence).toEqual({
      kind: 'hourly',
      start_date: '2027-01-01',
      every: 2,
      exclude_dates: ['2027-01-02'],
    });
    expect(view.history.map(occurrenceLabel)).toEqual(['2027-01-01 08:30', '2027-01-01 10:30']);
    expect(occurrenceLabel({ occurrence_date: '2027-01-01', outcome: 'created' })).toBe('2027-01-01');
    expect(scheduleCadence(view)).toBe('Every 2 hours from 08:30 UTC');
    expect(scheduleCadence({ ...view, schedule: { ...view.schedule, recurrence: { kind: 'hourly', every: 1 } } })).toBe(
      'Hourly from 08:30 UTC',
    );
    expect(
      scheduleCadence({ ...view, schedule: { ...view.schedule, recurrence: { kind: 'minutely', every: 15 } } }),
    ).toBe('Every 15 minutes from 08:30 UTC');
  });

  it('shows the minimum interval and keeps Every inside each kind when switching', () => {
    expect(everyBounds('minutely')).toEqual({
      min: MIN_INTERVAL_MINUTES,
      max: 1440,
      label: 'Every (Minutes)',
      hint: 'Minimum 5 minutes.',
    });
    expect(everyBounds('hourly')).toMatchObject({ min: 1, max: 24 });
    expect(everyBounds('daily')).toEqual({ min: 1, max: 365, label: 'Every' });
    const daily = { ...newScheduleDraft(), scheduleType: 'repeating' as const };
    const minutes = withRepeats(daily, 'minutely');
    expect(minutes).toMatchObject({ repeats: 'minutely', every: '15' });
    expect(withRepeats({ ...minutes, every: '90' }, 'hourly').every).toBe('24');
    expect(withRepeats({ ...minutes, every: '30' }, 'minutely').every).toBe('30');
    expect(withRepeats({ ...minutes, every: '' }, 'daily').every).toBe('1');
    expect(withRepeats({ ...minutes, every: 'x' }, 'hourly').every).toBe('1');
    expect(isSubDay('minutely') && isSubDay('hourly') && !isSubDay('daily')).toBe(true);
    expect(scheduleInputFromDraft(minutes).recurrence).toEqual({
      kind: 'minutely',
      start_date: daily.startDate,
      every: 15,
    });
  });

  it('rejects a drifted occurrence time or recurrence kind', () => {
    expect(() =>
      parseScheduleView({
        ...hourly,
        history: [{ occurrence_date: '2027-01-01', occurrence_time: 830, outcome: 'created' }],
      }),
    ).toThrow();
    expect(() =>
      parseScheduleView({ ...hourly, schedule: { ...hourly.schedule, recurrence: { kind: 'secondly' } } }),
    ).toThrow();
  });
});

describe('HS2-17Y2EC schedule action override', () => {
  const base = {
    command_id: 'review',
    time_zone: 'UTC',
    local_time: '09:00',
    paused: false,
    recurrence: { kind: 'once', date: '2027-03-12' },
  };

  it('defaults a new schedule to the command default and saves no override', () => {
    const draft = newScheduleDraft();
    expect(draft.action).toBe(COMMAND_DEFAULT_ACTION);
    expect(scheduleInputFromDraft(draft)).not.toHaveProperty('action');
    expect(scheduleInputFromDraft({ ...draft, action: 'new_ai_session' }).action).toBe('new_ai_session');
  });

  it('loads an inheriting schedule as the command default and a legacy one as its explicit override', () => {
    const inheriting = parseScheduleView({
      schedule: base,
      effective_action: 'reuse_or_new_ai_session',
      next_runs: [],
      history: [],
    });
    expect(inheriting.effective_action).toBe('reuse_or_new_ai_session');
    expect(scheduleDraftFromView(inheriting).action).toBe(COMMAND_DEFAULT_ACTION);
    expect(scheduleInputFromDraft(scheduleDraftFromView(inheriting))).not.toHaveProperty('action');
    // A record stored before HS2-17Y2EC always carried an action: it stays an explicit choice.
    const legacy = parseScheduleView({ schedule: { ...base, action: 'create_ticket' }, next_runs: [], history: [] });
    expect(legacy.effective_action).toBeUndefined();
    expect(scheduleDraftFromView(legacy).action).toBe('create_ticket');
    expect(scheduleInputFromDraft(scheduleDraftFromView(legacy)).action).toBe('create_ticket');
  });

  it('rejects unknown actions in either the override or the effective action', () => {
    expect(() => parseScheduleView({ schedule: { ...base, action: 'run' }, next_runs: [], history: [] })).toThrow();
    expect(() => parseScheduleView({ schedule: base, effective_action: 'run', next_runs: [], history: [] })).toThrow();
  });

  it('names the click and inherited actions from one label table', () => {
    expect(commandClickActionLabel(undefined)).toBe(LEGACY_COMMAND_ACTION_LABEL);
    for (const action of SCHEDULE_ACTIONS) expect(commandClickActionLabel(action)).toBe(SCHEDULE_ACTION_LABELS[action]);
    // Schedules never sent the legacy idle-session signal, so an unset command inherits ticket-only.
    expect(scheduleInheritedAction(undefined)).toBe('create_ticket');
    expect(scheduleInheritedAction('new_ai_session')).toBe('new_ai_session');
  });

  it('offers Use Command Default naming the inherited action in the shipped dialog', () => {
    const markup = String(
      CommandScheduleDialog({ commandTitle: 'Review', commandAction: 'new_ai_session', draft: newScheduleDraft() }),
    );
    expect(markup).toContain('Use Command Default (Auto-Start in a New AI Session)');
    for (const action of SCHEDULE_ACTIONS) expect(markup).toContain(SCHEDULE_ACTION_LABELS[action]);
    expect(String(CommandScheduleDialog({ draft: newScheduleDraft() }))).toContain(
      'Use Command Default (Create Ticket Only)',
    );
  });
});

describe('HS2-SEEMXR one field change for both schedule forms', () => {
  it('applies each recurrence field and ignores unknown fields and out-of-set choices', () => {
    const draft = { ...newScheduleDraft(), scheduleType: 'repeating' as const, repeats: 'daily' as const };
    expect(withScheduleField(draft, 'scheduleType', 'once').scheduleType).toBe('once');
    expect(withScheduleField(draft, 'scheduleType', 'sometimes')).toBe(draft);
    expect(withScheduleField(draft, 'repeats', 'minutely')).toMatchObject({ repeats: 'minutely', every: '15' });
    expect(withScheduleField(draft, 'repeats', 'fortnightly')).toBe(draft);
    expect(withScheduleField(draft, 'weekdays', ['7', '1', '1', '9']).weekdays).toEqual([1, 7]);
    expect(withScheduleField(draft, 'weekdays', '3').weekdays).toEqual([3]);
    expect(withScheduleField(draft, 'weekdays', undefined).weekdays).toEqual([]);
    expect(withScheduleField(draft, 'on', 'last_day').on).toBe('last_day');
    expect(withScheduleField(draft, 'on', 'first_day')).toBe(draft);
    for (const field of [
      'date',
      'startDate',
      'every',
      'month',
      'day',
      'ordinal',
      'weekday',
      'selectedDates',
      'endDate',
      'count',
      'includeDates',
      'excludeDates',
      'time',
      'timeZone',
    ] as const)
      expect(withScheduleField(draft, field, 'x')[field], field).toBe('x');
    expect(withScheduleField(draft, 'action', 'run')).toBe(draft);
    expect(withScheduleField(draft, 'paused', true)).toBe(draft);
    expect(withScheduleField(draft, 'bogus', 'x')).toBe(draft);
    // Edit, reset to a new draft, edit again: nothing of the first edit is left.
    const edited = withScheduleField(withScheduleField(draft, 'repeats', 'weekly'), 'weekdays', ['2', '4']);
    const again = withScheduleField(newScheduleDraft(), 'endDate', '2027-01-01');
    expect(edited.weekdays).toEqual([2, 4]);
    expect(again.weekdays).toEqual([1]);
    expect(again.repeats).toBe('daily');
  });
});
