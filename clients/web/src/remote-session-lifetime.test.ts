import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import {
  DAY_SECONDS,
  daysLeft,
  daysLeftPhrase,
  dueReminder,
  nextReminderAt,
  readDismissedDay,
  REMINDER_DAYS,
  reminderDay,
  reminderDismissKey,
  remoteFailureKind,
  remoteFailureText,
  type SessionLifetime,
  writeDismissedDay,
} from './remote-session-lifetime';

const RENEWED = 1_000_000;
const lifetime = (overrides: Partial<SessionLifetime> = {}): SessionLifetime => ({
  renewed_at: RENEWED,
  expires_at: RENEWED + 90 * DAY_SECONDS,
  idle_expires_at: RENEWED + 1_000 * DAY_SECONDS,
  ...overrides,
});

describe('HS2-4B0KSD remote session lifetime', () => {
  it('reminds on days 45, 50, 55 and 60, then daily from 65 through 90', () => {
    const expected = [45, 50, 55, 60, ...Array.from({ length: 26 }, (_, index) => 65 + index)];
    expect(REMINDER_DAYS).toEqual(expected);
    const shown: number[] = [];
    for (let hour = 0; hour <= 92 * 24; hour += 1) {
      const now = RENEWED + hour * 3600,
        day = reminderDay(RENEWED, now);
      if (day !== undefined && shown.at(-1) !== day) {
        // Each reminder starts exactly on its day.
        expect(now).toBe(RENEWED + day * DAY_SECONDS);
        shown.push(day);
      }
    }
    expect(shown).toEqual(expected);
    expect(reminderDay(RENEWED, RENEWED + 45 * DAY_SECONDS - 1)).toBeUndefined();
    expect(reminderDay(RENEWED, RENEWED + 64 * DAY_SECONDS)).toBe(60);
  });

  it('matches the server’s reminder days', () => {
    const source = readFileSync(resolve(__dirname, '../../../crates/hotsheet-server/src/remote_auth.rs'), 'utf8');
    const block = /pub const REMINDER_DAYS: \[i64; \d+\] = \[([^\]]+)\]/.exec(source)?.[1] ?? '';
    const days = block
      .split(',')
      .map((value) => value.trim())
      .filter(Boolean)
      .map(Number);
    expect(days).toEqual([...REMINDER_DAYS]);
  });

  it('arms the next reminder from stored timestamps and stops after day 90', () => {
    expect(nextReminderAt(RENEWED, RENEWED)).toBe(RENEWED + 45 * DAY_SECONDS);
    expect(nextReminderAt(RENEWED, RENEWED + 45 * DAY_SECONDS)).toBe(RENEWED + 50 * DAY_SECONDS);
    expect(nextReminderAt(RENEWED, RENEWED + 70 * DAY_SECONDS + 5)).toBe(RENEWED + 71 * DAY_SECONDS);
    expect(nextReminderAt(RENEWED, RENEWED + 90 * DAY_SECONDS)).toBeUndefined();
  });

  it('dismissal hides one reminder day until the next one, and a renewal starts a fresh series', () => {
    const at = (day: number) => RENEWED + day * DAY_SECONDS + 60;
    expect(dueReminder(lifetime(), at(44), undefined)).toBeUndefined();
    expect(dueReminder(lifetime(), at(45), undefined)).toEqual({ day: 45, daysLeft: 45 });
    expect(dueReminder(lifetime(), at(47), 45)).toBeUndefined();
    expect(dueReminder(lifetime(), at(50), 45)).toEqual({ day: 50, daysLeft: 40 });
    expect(dueReminder(lifetime(), at(80), 79)).toEqual({ day: 80, daysLeft: 10 });
    // Expired by either clock: the expired state replaces the reminder.
    expect(dueReminder(lifetime(), RENEWED + 90 * DAY_SECONDS, undefined)).toBeUndefined();
    expect(dueReminder(lifetime({ idle_expires_at: at(46) }), at(47), undefined)).toBeUndefined();
    expect(reminderDismissKey('device', RENEWED)).not.toBe(reminderDismissKey('device', RENEWED + 1));
    const store = new Map<string, string>();
    const storage = {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => store.set(key, value),
    };
    writeDismissedDay(storage, 'k', 55);
    expect(readDismissedDay(storage, 'k')).toBe(55);
    expect(readDismissedDay(storage, 'missing')).toBeUndefined();
    const broken = {
      getItem: () => {
        throw new Error('blocked');
      },
      setItem: () => {
        throw new Error('blocked');
      },
    };
    expect(readDismissedDay(broken, 'k')).toBeUndefined();
    expect(() => {
      writeDismissedDay(broken, 'k', 1);
    }).not.toThrow();
  });

  it('counts days left and phrases them', () => {
    expect(daysLeft(lifetime(), RENEWED + 89 * DAY_SECONDS + 1)).toBe(1);
    expect(daysLeft(lifetime(), RENEWED + 100 * DAY_SECONDS)).toBe(0);
    expect([0, 1, 4].map(daysLeftPhrase)).toEqual(['today', 'tomorrow', 'in 4 days']);
  });

  it('tells expired, revoked, unreachable and other failures apart', () => {
    expect(remoteFailureKind('session_expired')).toBe('expired');
    expect(remoteFailureKind('session_revoked')).toBe('revoked');
    expect(remoteFailureKind('unreachable')).toBe('unreachable');
    expect(remoteFailureKind(undefined)).toBe('unreachable');
    expect(remoteFailureKind('trust_failed')).toBe('error');
    expect(remoteFailureKind('invalid_response')).toBe('error');
    expect(
      new Set(['session_expired', 'session_revoked', 'unreachable', 'trust_failed', 'other'].map(remoteFailureText))
        .size,
    ).toBe(5);
  });
});
