import { describe, expect, it } from 'vitest';

import { newScheduleDraft, scheduleDraftFromView, scheduleInputFromDraft } from './command-schedule-form';
import { parseSchedulePreview, parseScheduleView } from './command-schedules';

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
