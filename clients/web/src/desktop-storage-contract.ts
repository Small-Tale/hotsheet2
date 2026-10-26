import { isRecord } from './json-value';

export const DESKTOP_STORAGE_PATH = '/__hotsheet/desktop-storage';
export const DESKTOP_STORAGE_KEY_LIMIT = 512;
export const DESKTOP_STORAGE_VALUE_LIMIT = 5 * 1024 * 1024;
// JSON can escape one UTF-16 code unit as six ASCII bytes; include key/metadata overhead.
export const DESKTOP_STORAGE_BODY_LIMIT = DESKTOP_STORAGE_VALUE_LIMIT * 6 + DESKTOP_STORAGE_KEY_LIMIT * 6 + 1024;

export function desktopStorageKey(key: string): boolean {
  return key.startsWith('hotsheet.') && key.length <= DESKTOP_STORAGE_KEY_LIMIT;
}

/** The desktop transport owns only Hot Sheet's device-local browser preferences. */
export function parseDesktopStorageEntry(value: unknown) {
  if (
    !isRecord(value) ||
    typeof value.key !== 'string' ||
    !desktopStorageKey(value.key) ||
    (value.value !== null && (typeof value.value !== 'string' || value.value.length > DESKTOP_STORAGE_VALUE_LIMIT))
  )
    throw new Error('Invalid desktop storage entry.');
  return { key: value.key, value: value.value };
}

export function parseDesktopStorageSnapshot(value: unknown) {
  if (!Array.isArray(value)) throw new Error('Invalid desktop storage snapshot.');
  return value.map((entry: unknown) => parseDesktopStorageEntry(entry));
}

export function parseDesktopStorageMutation(value: unknown) {
  const entry = parseDesktopStorageEntry(value);
  if (
    !isRecord(value) ||
    typeof value.clientId !== 'string' ||
    !/^[a-zA-Z0-9-]{1,128}$/.test(value.clientId) ||
    !Number.isSafeInteger(value.revision) ||
    typeof value.revision !== 'number' ||
    value.revision < 1
  )
    throw new Error('Invalid desktop storage mutation.');
  return { ...entry, clientId: value.clientId, revision: value.revision };
}
