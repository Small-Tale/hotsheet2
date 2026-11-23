import { describe, expect, it, vi } from 'vitest';

import { DAY_SECONDS, type SessionLifetime } from '../remote-session-lifetime';
import {
  approvalReminderText,
  BROWSER_APPROVAL_SCOPE,
  createRemoteApprovalReminders,
  sessionLifetimeOf,
} from './remote-approval-reminders';

const RENEWED = 5_000_000;
const lifetime = (renewed = RENEWED): SessionLifetime => ({
  renewed_at: renewed,
  expires_at: renewed + 90 * DAY_SECONDS,
  idle_expires_at: renewed + 200 * DAY_SECONDS,
  renewal_requested_at: null,
});

function setup() {
  let now = RENEWED;
  const timers: Array<{ at: number; run: () => void }> = [];
  const store = new Map<string, string>();
  const requestRenewal = vi.fn<(scope: string) => Promise<SessionLifetime | undefined>>(async () => undefined);
  const reminders = createRemoteApprovalReminders({
    requestRenewal,
    now: () => now,
    storage: { getItem: (key) => store.get(key) ?? null, setItem: (key, value) => store.set(key, value) },
    schedule: (run, ms) => {
      const timer = { at: now + ms / 1000, run };
      timers.push(timer);
      return () => {
        timers.splice(timers.indexOf(timer), 1);
      };
    },
  });
  return {
    reminders,
    requestRenewal,
    store,
    timers,
    /** Move the wall clock; only due local timers run, and none issues a request. */
    advanceTo: (day: number) => {
      now = RENEWED + day * DAY_SECONDS;
      for (const timer of [...timers].sort((a, b) => a.at - b.at))
        if (timer.at <= now) {
          timers.splice(timers.indexOf(timer), 1);
          timer.run();
        }
    },
  };
}

describe('HS2-4B0KSD remote approval reminders', () => {
  it('shows each reminder day from a local timer and never requests anything on its own', () => {
    const { reminders, advanceTo, requestRenewal, timers } = setup();
    reminders.set(BROWSER_APPROVAL_SCOPE, 'This browser', lifetime());
    expect(reminders.reminder(BROWSER_APPROVAL_SCOPE)).toBeUndefined();
    expect(timers).toHaveLength(1);
    const seen: number[] = [];
    for (let day = 1; day <= 92; day += 1) {
      advanceTo(day);
      const due = reminders.reminder(BROWSER_APPROVAL_SCOPE);
      if (due && seen.at(-1) !== due.day) seen.push(due.day);
    }
    // Day 90 is the expiry itself: the expired state, with its Renew action, replaces the reminder.
    expect(seen).toEqual([45, 50, 55, 60, ...Array.from({ length: 25 }, (_, index) => 65 + index)]);
    expect(requestRenewal).not.toHaveBeenCalled();
    // Past the last day nothing is armed.
    expect(timers).toHaveLength(0);
  });

  it('dismissal lasts until the next reminder day and survives a reload', () => {
    const first = setup();
    first.reminders.set('binding', 'Studio', lifetime());
    first.advanceTo(46);
    expect(first.reminders.reminder('binding')?.day).toBe(45);
    first.reminders.dismiss('binding');
    expect(first.reminders.reminder('binding')).toBeUndefined();
    first.advanceTo(49);
    expect(first.reminders.reminder('binding')).toBeUndefined();
    first.advanceTo(50);
    expect(first.reminders.reminder('binding')?.day).toBe(50);
    first.reminders.dismiss('binding');
    // A reload reads the stored dismissal.
    const again = createRemoteApprovalReminders({
      requestRenewal: async () => undefined,
      now: () => RENEWED + 51 * DAY_SECONDS,
      storage: { getItem: (key) => first.store.get(key) ?? null, setItem: () => undefined },
      schedule: () => () => undefined,
    });
    again.set('binding', 'Studio', lifetime());
    expect(again.reminder('binding')).toBeUndefined();
    // A renewal starts a fresh series: nothing until its own day 45.
    first.reminders.set('binding', 'Studio', lifetime(RENEWED + 50 * DAY_SECONDS));
    expect(first.reminders.reminder('binding')).toBeUndefined();
  });

  it('Renew asks the host once, marks the reminder requested, and reports a failure', async () => {
    const { reminders, advanceTo, requestRenewal } = setup();
    reminders.set('binding', 'Studio', lifetime());
    advanceTo(70);
    expect(await reminders.renew('binding')).toBe(true);
    expect(requestRenewal).toHaveBeenCalledWith('binding');
    const due = reminders.reminder('binding');
    expect(due?.requested).toBe(true);
    expect(approvalReminderText(due!)).toContain('Renewal requested');
    requestRenewal.mockRejectedValueOnce(new Error('The host is not answering.'));
    expect(await reminders.renew('binding')).toBe(false);
    expect(reminders.message.value).toBe('The host is not answering.');
    reminders.remove('binding');
    expect(reminders.reminder('binding')).toBeUndefined();
  });

  it('reads only complete lifetimes from a server answer', () => {
    expect(sessionLifetimeOf({ renewed_at: 1, expires_at: 2, idle_expires_at: 3 })).toEqual({
      renewed_at: 1,
      expires_at: 2,
      idle_expires_at: 3,
      renewal_requested_at: null,
    });
    expect(sessionLifetimeOf({ renewed_at: 1 })).toBeUndefined();
    expect(sessionLifetimeOf(null)).toBeUndefined();
    expect(approvalReminderText({ scope: 's', name: 'n', day: 89, daysLeft: 1, requested: false })).toContain(
      'tomorrow',
    );
  });
});
