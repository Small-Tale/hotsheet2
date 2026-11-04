import { signal } from 'kerfjs';

import { loadedClientBuildId, validClientBuildId } from './client-build';

/**
 * The "newer client available" notice (HS2-KPJWBK). The host announces the client build it serves
 * on each change-bridge subscription, which every reconnect repeats; a long-lived page whose loaded
 * bundle differs learns that a reload would pick up a newer client. It never reloads by itself,
 * because text editors may hold unsaved drafts (HS2-RE1PS6).
 */
export const hostClientBuild = signal<string | undefined>(undefined);

/** Record the host's announced build. An absent or malformed id (an older host) keeps the last one. */
export function noteHostClientBuild(value: unknown) {
  const id = validClientBuildId(value);
  if (id && hostClientBuild.value !== id) hostClientBuild.value = id;
}

/** A newer client is available only when both ids are known and they differ. */
export function clientUpdateAvailable(loaded: string | undefined, served: string | undefined): boolean {
  return Boolean(loaded && served && loaded !== served);
}

/** The dismissal key: the loaded and served pair. A later host build makes a new key. */
export function clientUpdateNoticeKey(loaded: string, served: string): string {
  return `${loaded}|${served}`;
}

const STORAGE_KEY = 'hotsheet.client-update-notice.dismissed';

function browserStorage(): Storage | undefined {
  try {
    return typeof localStorage === 'undefined' ? undefined : localStorage;
  } catch {
    return undefined;
  }
}

function readDismissed(storage: Pick<Storage, 'getItem'> | undefined): string | undefined {
  try {
    return storage?.getItem(STORAGE_KEY) ?? undefined;
  } catch {
    return undefined;
  }
}

/** The dismissed pair, shared by every open project in this browser. */
export const clientUpdateNoticeDismissed = signal<string | undefined>(readDismissed(browserStorage()));

export function dismissClientUpdateNotice(
  loaded: string | undefined = loadedClientBuildId(),
  served: string | undefined = hostClientBuild.value,
  storage: Pick<Storage, 'setItem'> | undefined = browserStorage(),
) {
  if (!loaded || !served) return;
  const key = clientUpdateNoticeKey(loaded, served);
  clientUpdateNoticeDismissed.value = key;
  try {
    storage?.setItem(STORAGE_KEY, key);
  } catch {
    // Storage is a convenience: the dismissal still holds for this page.
  }
}

/** Whether the notice shows for this page right now. */
export function showsClientUpdateNotice(
  loaded: string | undefined = loadedClientBuildId(),
  served: string | undefined = hostClientBuild.value,
  dismissed: string | undefined = clientUpdateNoticeDismissed.value,
): boolean {
  return clientUpdateAvailable(loaded, served) && clientUpdateNoticeKey(loaded!, served!) !== dismissed;
}
