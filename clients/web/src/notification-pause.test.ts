import { describe, expect, it } from 'vitest';

import {
  loadNotificationsPaused,
  NOTIFICATIONS_PAUSED_KEY,
  notificationsPausedDetail,
  notificationsPausedFromStorageEvent,
  saveNotificationsPaused,
} from './notification-pause';

function memoryStorage() {
  const values = new Map<string, string>();
  return {
    values,
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => {
      values.set(key, value);
    },
    removeItem: (key: string) => {
      values.delete(key);
    },
  };
}

describe('app-wide notification pause (HS2-QYA9SC)', () => {
  it('round-trips pause and resume, removing the key on resume', () => {
    const storage = memoryStorage();
    expect(loadNotificationsPaused(storage)).toBe(false);
    saveNotificationsPaused(true, storage);
    expect(storage.values.get(NOTIFICATIONS_PAUSED_KEY)).toBe('true');
    expect(loadNotificationsPaused(storage)).toBe(true);
    // Repeating a pause is idempotent; resuming clears it; pausing again after a resume works.
    saveNotificationsPaused(true, storage);
    saveNotificationsPaused(false, storage);
    expect(storage.values.has(NOTIFICATIONS_PAUSED_KEY)).toBe(false);
    expect(loadNotificationsPaused(storage)).toBe(false);
    saveNotificationsPaused(true, storage);
    expect(loadNotificationsPaused(storage)).toBe(true);
  });

  it('treats unreadable, unwritable, or unexpected storage as not paused without throwing', () => {
    const throwing = {
      getItem: () => {
        throw new Error('blocked');
      },
      setItem: () => {
        throw new Error('blocked');
      },
      removeItem: () => {
        throw new Error('blocked');
      },
    };
    expect(loadNotificationsPaused(throwing)).toBe(false);
    expect(() => {
      saveNotificationsPaused(true, throwing);
    }).not.toThrow();
    expect(loadNotificationsPaused(undefined)).toBe(false);
    const storage = memoryStorage();
    storage.values.set(NOTIFICATIONS_PAUSED_KEY, 'yes');
    expect(loadNotificationsPaused(storage)).toBe(false);
  });

  it('follows another window only for its own key, and a cleared store resumes', () => {
    expect(notificationsPausedFromStorageEvent({ key: NOTIFICATIONS_PAUSED_KEY, newValue: 'true' })).toBe(true);
    expect(notificationsPausedFromStorageEvent({ key: NOTIFICATIONS_PAUSED_KEY, newValue: null })).toBe(false);
    expect(notificationsPausedFromStorageEvent({ key: 'hotsheet.permission-history', newValue: '[]' })).toBeUndefined();
    expect(notificationsPausedFromStorageEvent({ key: null, newValue: null })).toBe(false);
  });

  it('describes the waiting requests in the banner', () => {
    expect(notificationsPausedDetail(0)).toBe('Permission requests wait in Notifications.');
    expect(notificationsPausedDetail(1)).toBe('1 permission request waiting.');
    expect(notificationsPausedDetail(3)).toBe('3 permission requests waiting.');
  });
});
