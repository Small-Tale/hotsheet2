import { signal } from 'kerfjs';

import {
  daysLeftPhrase,
  dueReminder,
  nextReminderAt,
  readDismissedDay,
  reminderDismissKey,
  type SessionLifetime,
  writeDismissedDay,
} from '../remote-session-lifetime';

/** The scope of this browser's own paired session on a remote-mode host. */
export const BROWSER_APPROVAL_SCOPE = 'browser';

/** One approval this page reminds about: this browser's session, or one remote server binding. */
export interface ApprovalEntry {
  scope: string;
  /** What the reminder names: "this browser" or the remote server's label. */
  name: string;
  lifetime: SessionLifetime;
}

/** A reminder to show now. */
export interface ApprovalReminder {
  scope: string;
  name: string;
  day: number;
  daysLeft: number;
  requested: boolean;
}

export interface RemoteApprovalRemindersDependencies {
  /** Ask the approving host to renew this scope in place; resolves to the new lifetime, if reported. */
  requestRenewal: (scope: string) => Promise<SessionLifetime | undefined>;
  /** Wall-clock seconds; injected for tests. */
  now?: () => number;
  storage?: Pick<Storage, 'getItem' | 'setItem'>;
  /** Local display timer; it never issues a request. */
  schedule?: (callback: () => void, ms: number) => () => void;
}

/** The longest delay a browser timer holds; a later reminder re-arms on the way. */
const MAX_TIMER_MS = 2 ** 31 - 1;

function scheduleTimeout(callback: () => void, ms: number): () => void {
  const timer = setTimeout(callback, ms);
  return () => {
    clearTimeout(timer);
  };
}

function browserStorage(): Pick<Storage, 'getItem' | 'setItem'> | undefined {
  try {
    return typeof localStorage === 'undefined' ? undefined : localStorage;
  } catch {
    return undefined;
  }
}

/**
 * Renewal reminders for remote approvals (HS2-4B0KSD, docs/08 §8.2.2). Each entry's lifetime comes from
 * a server answer this page already received (`/remote-auth/me`, or a remote binding's session read
 * when its tab opens). Reminders appear on days 45, 50, 55 and 60, then daily from 65 to 90, computed
 * from `renewed_at`; one local timer, which issues no request, re-renders when the next day starts.
 * Dismissing hides a reminder until the next reminder day. Renew asks the approving host to renew in
 * place, so nothing is paired or set up again.
 */
export function createRemoteApprovalReminders(dependencies: RemoteApprovalRemindersDependencies) {
  const now = dependencies.now ?? (() => Date.now() / 1000),
    storage = dependencies.storage ?? browserStorage(),
    schedule = dependencies.schedule ?? scheduleTimeout;
  const entries = signal<ReadonlyMap<string, ApprovalEntry>>(new Map()),
    /** Advanced only by the local timer, so a render reads a current time without polling. */
    clock = signal(now()),
    dismissed = signal<Readonly<Record<string, number>>>({}),
    /** Scopes with a renewal request this page sent, or the server reported. */
    requested = signal<ReadonlySet<string>>(new Set()),
    message = signal('');
  let cancel: (() => void) | undefined,
    disposed = false;

  function arm() {
    cancel?.();
    cancel = undefined;
    if (disposed) return;
    const at = now(),
      next = [...entries.peek().values()]
        .map((entry) => nextReminderAt(entry.lifetime.renewed_at, at))
        .filter((value): value is number => value !== undefined);
    if (!next.length) return;
    const delay = Math.min(MAX_TIMER_MS, Math.max(0, (Math.min(...next) - at) * 1000));
    cancel = schedule(() => {
      clock.value = now();
      arm();
    }, delay);
  }

  /** Record or update one approval from a server answer. */
  function set(scope: string, name: string, lifetime: SessionLifetime) {
    const next = new Map(entries.peek());
    next.set(scope, { scope, name, lifetime });
    entries.value = next;
    const key = reminderDismissKey(scope, lifetime.renewed_at),
      day = readDismissedDay(storage, key);
    dismissed.value = { ...dismissed.peek(), [scope]: day ?? 0 };
    const asked = new Set(requested.peek());
    if (lifetime.renewal_requested_at) asked.add(scope);
    else asked.delete(scope);
    requested.value = asked;
    clock.value = now();
    arm();
  }

  function remove(scope: string) {
    if (!entries.peek().has(scope)) return;
    const next = new Map(entries.peek());
    next.delete(scope);
    entries.value = next;
    arm();
  }

  /** The reminder to show for `scope` now; reads signals, so a render follows it. */
  function reminder(scope: string): ApprovalReminder | undefined {
    const entry = entries.value.get(scope);
    if (!entry) return undefined;
    const due = dueReminder(entry.lifetime, clock.value, dismissed.value[scope] || undefined);
    return due && { scope, name: entry.name, ...due, requested: requested.value.has(scope) };
  }

  /** Hide the current reminder until the next reminder day. */
  function dismiss(scope: string) {
    const entry = entries.peek().get(scope);
    if (!entry) return;
    const due = dueReminder(entry.lifetime, now(), undefined);
    if (!due) return;
    writeDismissedDay(storage, reminderDismissKey(scope, entry.lifetime.renewed_at), due.day);
    dismissed.value = { ...dismissed.peek(), [scope]: due.day };
  }

  /** Ask the host to renew `scope` in place. */
  async function renew(scope: string): Promise<boolean> {
    message.value = '';
    try {
      const lifetime = await dependencies.requestRenewal(scope);
      requested.value = new Set([...requested.peek(), scope]);
      const entry = entries.peek().get(scope);
      if (lifetime && entry)
        set(scope, entry.name, { ...lifetime, renewal_requested_at: lifetime.renewal_requested_at ?? now() });
      return true;
    } catch (reason) {
      message.value = reason instanceof Error ? reason.message : String(reason);
      return false;
    }
  }

  function dispose() {
    disposed = true;
    cancel?.();
    cancel = undefined;
  }

  return { entries, message, set, remove, reminder, dismiss, renew, dispose };
}

export type RemoteApprovalReminders = ReturnType<typeof createRemoteApprovalReminders>;

/** Read the lifetime fields of a server's device answer; anything incomplete is not a lifetime. */
export function sessionLifetimeOf(device: unknown): SessionLifetime | undefined {
  if (typeof device !== 'object' || device === null) return undefined;
  const value = device as Record<string, unknown>,
    renewed = value.renewed_at,
    expires = value.expires_at,
    idle = value.idle_expires_at,
    asked = value.renewal_requested_at;
  if (typeof renewed !== 'number' || typeof expires !== 'number' || typeof idle !== 'number') return undefined;
  return {
    renewed_at: renewed,
    expires_at: expires,
    idle_expires_at: idle,
    renewal_requested_at: typeof asked === 'number' ? asked : null,
  };
}

/** The reminder sentence. */
export function approvalReminderText(reminder: ApprovalReminder): string {
  const when = daysLeftPhrase(reminder.daysLeft);
  return reminder.requested
    ? `Renewal requested. The host approves it in Settings → Devices before the approval expires ${when}.`
    : `The approval expires ${when}. Renew asks the host to approve it again; nothing is set up again.`;
}
