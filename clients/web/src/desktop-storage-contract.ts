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

/** Staged draft attachment bytes, keyed by `<draft kind>:<project>:<draft file id>` (HS2-791W19). */
export const DESKTOP_DRAFT_FILES_PATH = '/__hotsheet/desktop-draft-files';
// Matches the server's attachment body limit; a larger file could never be uploaded anyway.
export const DESKTOP_DRAFT_FILE_LIMIT = 100 * 1024 * 1024;
export const DESKTOP_DRAFT_FILE_NAME_LIMIT = 1024;
export const DESKTOP_DRAFT_FILE_TYPE_LIMIT = 256;
const DRAFT_FILE_KINDS = ['composer:', 'not-working:'];

export function desktopDraftFileKey(key: string): boolean {
  return key.length <= DESKTOP_STORAGE_KEY_LIMIT && DRAFT_FILE_KINDS.some((kind) => key.startsWith(kind));
}

/** Name, type, and modification time travel in headers so the body stays the file's raw bytes. */
export const DESKTOP_DRAFT_FILE_HEADERS = {
  name: 'X-Hotsheet-Draft-Name',
  type: 'X-Hotsheet-Draft-Type',
  lastModified: 'X-Hotsheet-Draft-Modified',
} as const;

export function parseDesktopDraftFileMetadata(value: unknown) {
  if (
    !isRecord(value) ||
    typeof value.key !== 'string' ||
    !desktopDraftFileKey(value.key) ||
    typeof value.name !== 'string' ||
    !value.name ||
    value.name.length > DESKTOP_DRAFT_FILE_NAME_LIMIT ||
    typeof value.type !== 'string' ||
    value.type.length > DESKTOP_DRAFT_FILE_TYPE_LIMIT ||
    typeof value.lastModified !== 'number' ||
    !Number.isSafeInteger(value.lastModified) ||
    value.lastModified < 0
  )
    throw new Error('Invalid desktop draft file.');
  return { key: value.key, name: value.name, type: value.type, lastModified: value.lastModified };
}
