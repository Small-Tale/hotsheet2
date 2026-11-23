/**
 * The remote session lifetime as clients see it (HS2-4B0KSD, docs/08 §8.2.2). A paired device's
 * approval expires after 30 days without use or 90 days after its approval or last renewal. Renewal
 * happens in place on the host, so nothing on the device is set up again. Reminders fall on days 45,
 * 50, 55 and 60, then every day from 65 to 90, counted from `renewed_at`; the server's
 * `REMINDER_DAYS` in `crates/hotsheet-server/src/remote_auth.rs` is the same list, pinned by a test.
 * Everything here is pure: callers compute from stored timestamps and arm local timers that issue no
 * request.
 */

export const DAY_SECONDS = 24 * 60 * 60;

export const REMINDER_DAYS: readonly number[] = [45, 50, 55, 60, ...Array.from({ length: 26 }, (_, i) => 65 + i)];

/** The lifetime fields a server reports for one paired session. */
export interface SessionLifetime {
  renewed_at: number;
  expires_at: number;
  idle_expires_at: number;
  renewal_requested_at?: number | null;
}

/** The latest reminder day reached at `now` (seconds), or undefined before day 45. */
export function reminderDay(renewedAt: number, now: number): number | undefined {
  const age = now - renewedAt;
  return [...REMINDER_DAYS].reverse().find((day) => age >= day * DAY_SECONDS);
}

/** When the next reminder day starts (seconds), or undefined after the last one. A local display
 * timer uses it; it never drives a request. */
export function nextReminderAt(renewedAt: number, now: number): number | undefined {
  const day = REMINDER_DAYS.find((candidate) => renewedAt + candidate * DAY_SECONDS > now);
  return day === undefined ? undefined : renewedAt + day * DAY_SECONDS;
}

/** The moment the session ends, by whichever clock runs out first. */
export const sessionEndsAt = (lifetime: SessionLifetime) => Math.min(lifetime.expires_at, lifetime.idle_expires_at);

/** Whole days left before the absolute limit, never negative. */
export function daysLeft(lifetime: SessionLifetime, now: number): number {
  return Math.max(0, Math.ceil((lifetime.expires_at - now) / DAY_SECONDS));
}

/** "in 3 days", "tomorrow", "today". */
export function daysLeftPhrase(days: number): string {
  if (days <= 0) return 'today';
  if (days === 1) return 'tomorrow';
  return `in ${String(days)} days`;
}

/** The reminder to show now, if any: the day reached and not dismissed for that day. A dismissal lasts
 * until the next reminder day, because the next day has another key. */
export function dueReminder(
  lifetime: SessionLifetime,
  now: number,
  dismissedDay: number | undefined,
): { day: number; daysLeft: number } | undefined {
  if (lifetime.expires_at <= now || lifetime.idle_expires_at <= now) return undefined;
  const day = reminderDay(lifetime.renewed_at, now);
  if (day === undefined || day === dismissedDay) return undefined;
  return { day, daysLeft: daysLeft(lifetime, now) };
}

/** The storage key of one reminder dismissal: scoped to the session and its renewal, so a renewal
 * starts a fresh series. */
export const reminderDismissKey = (scope: string, renewedAt: number) =>
  `hotsheet.remote-reminder.${scope}.${String(renewedAt)}`;

/** Read the day a reminder was dismissed on; storage may be unavailable. */
export function readDismissedDay(storage: Pick<Storage, 'getItem'> | undefined, key: string): number | undefined {
  try {
    const value = Number(storage?.getItem(key));
    return Number.isInteger(value) && value > 0 ? value : undefined;
  } catch {
    return undefined;
  }
}

export function writeDismissedDay(storage: Pick<Storage, 'setItem'> | undefined, key: string, day: number) {
  try {
    storage?.setItem(key, String(day));
  } catch {
    // Private mode: the reminder returns on the next load, which is acceptable.
  }
}

/** Why a remote connection does not work, by kind (HS2-4B0KSD): each kind has its own state. An
 * expired approval is renewed, never retried; a revoked one needs a new pairing; an unreachable host is
 * retried with backoff; anything else shows its error. */
export type RemoteFailureKind = 'expired' | 'revoked' | 'unreachable' | 'error';

/** Classify a stored or live remote failure code. Network-level codes are unreachable. */
export function remoteFailureKind(code: string | null | undefined): RemoteFailureKind {
  if (code === 'session_expired') return 'expired';
  if (code === 'session_revoked') return 'revoked';
  if (!code || code === 'unreachable') return 'unreachable';
  return 'error';
}

/** A short, human sentence for a non-network remote failure. */
export function remoteFailureText(code: string | null | undefined): string {
  switch (code) {
    case 'session_expired':
      return 'This computer’s approval on the remote server expired.';
    case 'session_revoked':
      return 'The remote server revoked this computer’s access.';
    case 'unauthorized':
      return 'The remote server no longer accepts this computer’s approval.';
    case 'trust_failed':
      return 'The remote server’s certificate is not trusted.';
    case 'identity_changed':
      return 'The remote server’s identity changed. Review the connection.';
    case 'missing_credentials':
      return 'This computer’s credentials for the remote server are missing.';
    case 'unsupported':
      return 'The remote server does not support this request.';
    case 'unreachable':
    case null:
    case undefined:
      return 'The server did not answer.';
    default:
      return 'The remote server sent an unexpected answer.';
  }
}
