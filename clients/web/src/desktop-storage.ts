import { browserRandomId } from './browser-id';
import { DESKTOP_STORAGE_PATH, desktopStorageKey, parseDesktopStorageSnapshot } from './desktop-storage-contract';
import { responseJsonBody } from './json-value';

/** Hydrate before application defaults run; a desktop bridge's ephemeral port is not its identity. */
export async function initializeDesktopStorage(
  host: Pick<Window, 'localStorage' | 'addEventListener' | 'removeEventListener'> = window,
  fetcher: typeof fetch = fetch,
) {
  const storage = host.localStorage,
    response = await fetcher(DESKTOP_STORAGE_PATH, { cache: 'no-store' });
  if (!response.ok) throw new Error('Could not load desktop preferences.');
  const snapshot = parseDesktopStorageSnapshot(await responseJsonBody(response)),
    known = new Set(snapshot.map((entry) => entry.key)),
    prototype = Object.getPrototypeOf(storage) as Storage,
    originalSet = Object.getOwnPropertyDescriptor(prototype, 'setItem')?.value as Storage['setItem'],
    originalRemove = Object.getOwnPropertyDescriptor(prototype, 'removeItem')?.value as Storage['removeItem'],
    originalClear = Object.getOwnPropertyDescriptor(prototype, 'clear')?.value as Storage['clear'],
    pending = new Map<string, { value: string | null; revision: number }>(),
    clientId = browserRandomId();
  let revision = 0;
  for (const { key, value } of snapshot) {
    if (value === null) originalRemove.call(storage, key);
    else originalSet.call(storage, key, value);
  }
  // Import only visible legacy origin data. Abandoned random-port origins cannot be read here.
  for (let index = 0; index < storage.length; index++) {
    const key = storage.key(index);
    if (key && desktopStorageKey(key) && !known.has(key))
      pending.set(key, { value: storage.getItem(key), revision: ++revision });
  }
  let running: Promise<void> | undefined,
    closeRequested = false;
  const closeSent = new Map<string, number>();
  async function persist(key: string, mutation: { value: string | null; revision: number }, closing = false) {
    const body = JSON.stringify({ key, ...mutation, clientId }),
      keepalive = new Blob([body]).size < 60_000;
    if (closing && !keepalive) return;
    const result = await fetcher(DESKTOP_STORAGE_PATH, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body,
      // The browser's keepalive budget is 64 KiB; larger drafts use an ordinary request.
      keepalive,
    });
    if (!result.ok) throw new Error('Could not save desktop preferences.');
    if (pending.get(key) === mutation) pending.delete(key);
  }
  function flush(): Promise<void> {
    if (running) return running;
    running = (async () => {
      const failed = new Set<string>();
      while (pending.size) {
        const first = [...pending].find(([key]) => !failed.has(key));
        if (!first) break;
        try {
          await persist(...first);
        } catch {
          // A rejected draft must not prevent unrelated project/settings writes.
          failed.add(first[0]);
        }
      }
      if (failed.size) throw new Error('Could not save desktop preferences.');
    })().finally(() => {
      running = undefined;
    });
    return running;
  }
  const reportFailure = () => {
    // Keep pending entries for the next mutation/pagehide; never log preference content.
    console.error('Could not save desktop preferences. Pending changes will be retried.');
  };
  const save = () => {
    if (closeRequested) {
      for (const [key, mutation] of pending) {
        if (closeSent.get(key) === mutation.revision) continue;
        closeSent.set(key, mutation.revision);
        void persist(key, mutation, true).catch(() => {
          if (closeSent.get(key) === mutation.revision) closeSent.delete(key);
          reportFailure();
        });
      }
      return;
    }
    void flush().catch(reportFailure);
  };
  prototype.setItem = function (key: string, value: string) {
    originalSet.call(this, key, value);
    if (this === storage && desktopStorageKey(key)) {
      pending.set(key, { value, revision: ++revision });
      save();
    }
  };
  prototype.removeItem = function (key: string) {
    originalRemove.call(this, key);
    if (this === storage && desktopStorageKey(key)) {
      pending.set(key, { value: null, revision: ++revision });
      save();
    }
  };
  prototype.clear = function () {
    const keys = Array.from({ length: this.length }, (_, index) => this.key(index));
    originalClear.call(this);
    if (this === storage) {
      for (const key of keys)
        if (key && desktopStorageKey(key)) pending.set(key, { value: null, revision: ++revision });
      save();
    }
  };
  const closing = () => {
    // Runtime pagehide handlers registered after this one also save immediately.
    closeRequested = true;
    save();
  };
  const reopening = () => {
    closeRequested = false;
    closeSent.clear();
    save();
  };
  host.addEventListener('pagehide', closing);
  host.addEventListener('pageshow', reopening);
  await flush().catch(reportFailure);
  return {
    flush,
    dispose() {
      prototype.setItem = originalSet;
      prototype.removeItem = originalRemove;
      prototype.clear = originalClear;
      host.removeEventListener('pagehide', closing);
      host.removeEventListener('pageshow', reopening);
    },
  };
}
