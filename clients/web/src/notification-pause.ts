/**
 * The app-wide notification pause (HS2-QYA9SC). While paused, interruptive notifications (today the
 * permission-request popup) stop appearing in every open project; requests keep collecting in
 * Notifications and on project-tab badges until the user resumes. It is a per-device client
 * preference, like the other app-global `hotsheet.*` keys, and other windows follow it through the
 * `storage` event.
 */
export const NOTIFICATIONS_PAUSED_KEY = 'hotsheet.notifications-paused';

type PauseStorage = Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>;

function defaultStorage(): PauseStorage | undefined {
  try {
    return globalThis.localStorage;
  } catch {
    return undefined;
  }
}

/** Whether notifications were left paused on this device; unreadable storage means not paused. */
export function loadNotificationsPaused(storage: PauseStorage | undefined = defaultStorage()): boolean {
  try {
    return storage?.getItem(NOTIFICATIONS_PAUSED_KEY) === 'true';
  } catch {
    return false;
  }
}

/** Persists the pause; resuming removes the key rather than storing `false`. */
export function saveNotificationsPaused(paused: boolean, storage: PauseStorage | undefined = defaultStorage()): void {
  try {
    if (paused) storage?.setItem(NOTIFICATIONS_PAUSED_KEY, 'true');
    else storage?.removeItem(NOTIFICATIONS_PAUSED_KEY);
  } catch {
    /* Storage unavailable: the pause still applies to this window until it reloads. */
  }
}

/**
 * The pause another window just wrote, or `undefined` when a `storage` event concerns another key.
 * A cleared storage area (`key === null`) means not paused.
 */
export function notificationsPausedFromStorageEvent(
  event: Pick<StorageEvent, 'key' | 'newValue'>,
): boolean | undefined {
  if (event.key === null) return false;
  if (event.key !== NOTIFICATIONS_PAUSED_KEY) return undefined;
  return event.newValue === 'true';
}

/** The banner detail for a pause: how many permission requests are waiting, if any. */
export function notificationsPausedDetail(waiting: number): string {
  if (waiting <= 0) return 'Permission requests wait in Notifications.';
  return `${waiting} permission ${waiting === 1 ? 'request' : 'requests'} waiting.`;
}
