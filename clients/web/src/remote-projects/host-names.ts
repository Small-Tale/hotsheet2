/**
 * The last machine name each remote binding's peer reported (HS2-GN0WKS), cached per device so a
 * remembered remote tab is named before its host answers exactly as it will be once open:
 * `<host> · <project>` (HS2-HGG426). It is display data only, never identity or trust. The cache
 * lives in browser storage rather than the machine registry, whose strict schema older CLI and
 * server builds would refuse to read with a new field.
 */
import { parseRemoteProjectId } from './identity';

export const REMOTE_HOST_NAMES_KEY = 'hotsheet.remote-host-names.v1';
/** Bounds the cache: a host name is short, and a device pairs with few servers. */
const MAX_NAME = 128,
  MAX_ENTRIES = 64;

export function loadRemoteHostNames(storage: Pick<Storage, 'getItem'>): Record<string, string> {
  try {
    const value: unknown = JSON.parse(storage.getItem(REMOTE_HOST_NAMES_KEY) ?? '{}');
    if (!value || typeof value !== 'object' || Array.isArray(value)) return {};
    return Object.fromEntries(
      Object.entries(value).filter(
        (entry): entry is [string, string] => typeof entry[1] === 'string' && entry[1].trim() !== '',
      ),
    );
  } catch {
    return {};
  }
}

/**
 * Remember the host names of the open remote projects. Returns the updated cache, or the same
 * object when nothing changed, so callers write storage only on a change.
 */
export function rememberRemoteHostNames(
  cache: Readonly<Record<string, string>>,
  projects: ReadonlyArray<{ id: string; remoteHost?: string }>,
): Readonly<Record<string, string>> {
  let next: Map<string, string> | undefined;
  for (const project of projects) {
    const bindingId = parseRemoteProjectId(project.id)?.bindingId,
      host = project.remoteHost?.trim().slice(0, MAX_NAME);
    if (!bindingId || !host || cache[bindingId] === host) continue;
    next ??= new Map(Object.entries(cache));
    // Re-insert at the end, so the bound keeps the most recently seen hosts.
    next.delete(bindingId);
    next.set(bindingId, host);
  }
  if (!next) return cache;
  return Object.fromEntries([...next].slice(-MAX_ENTRIES));
}

export function saveRemoteHostNames(storage: Pick<Storage, 'setItem'>, cache: Readonly<Record<string, string>>) {
  try {
    storage.setItem(REMOTE_HOST_NAMES_KEY, JSON.stringify(cache));
  } catch {
    // Storage full or blocked: the placeholder falls back to the origin's host name.
  }
}
