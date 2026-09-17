/**
 * Development-only capture of the reason the Vite dev server full-reloads ("the client randomly
 * restarts", HS2-8JV12R).
 *
 * A Vite dev client can be told to reload by the server for several reasons (a watched file with no
 * HMR boundary changing, a self-invalidating module, a dependency re-optimization, or the server
 * restarting). Vite logs the reason to the console, but a full reload wipes the console, so a reload
 * that happens while the developer is not watching leaves no trace. This module records each reload
 * trigger to `sessionStorage` (which survives the reload) and re-surfaces the most recent one after
 * the page comes back, turning an unreproducible "it just refreshed" report into a captured cause.
 *
 * Install it only in dev, only when an HMR context exists. It never runs in a production build.
 */

/** The subset of Vite's `import.meta.hot` this module needs: subscribing to HMR lifecycle events. */
export interface DevHotEmitter {
  on(event: string, callback: (payload: unknown) => void): void;
}

/** A single recorded reload trigger, persisted across the reload it describes. */
export interface DevReloadRecord {
  /** Human-readable trigger kind (for example `full reload`, `module invalidate`, `error`). */
  type: string;
  /** The module/file path Vite blamed for the reload, when the event payload carries one. */
  path?: string;
  /** ISO timestamp of when the trigger fired. */
  at: string;
}

/** `sessionStorage` key holding the bounded reload log. */
export const DEV_RELOAD_LOG_KEY = 'hotsheet-dev-reload-log';
/** `localStorage` key holding only the single most recent trigger. Unlike the `sessionStorage` log,
 * this survives a full browsing-context replacement (a reload that starts a new document rather than
 * reusing the tab), so a connection-loss reload that clears `sessionStorage` still leaves a trace. */
export const DEV_RELOAD_LAST_KEY = 'hotsheet-dev-reload-last';
/** Keep only the most recent entries so a long-lived tab cannot grow the log without bound. */
export const DEV_RELOAD_LOG_LIMIT = 20;

/** The Vite HMR/websocket events that precede or accompany a client reload/restart. `vite:ws:disconnect`
 * is the key addition (HS2-8JV12R): when the dev server restarts or crashes, Vite reloads the client on
 * websocket reconnect via a direct `location.reload()` that never dispatches `vite:beforeFullReload`, so
 * the disconnect is the only in-client signal of that "random restart". */
const CAPTURED_EVENTS: ReadonlyArray<readonly [event: string, kind: string, imminent: string]> = [
  ['vite:beforeFullReload', 'full reload', 'the client is about to reload'],
  ['vite:invalidate', 'module invalidate', 'a module invalidated; the client is about to reload'],
  ['vite:error', 'error', 'a build or HMR error occurred'],
  ['vite:ws:disconnect', 'connection lost', 'the dev-server websocket dropped; a restart/crash reloads the client on reconnect'],
];

function readLog(storage: Pick<Storage, 'getItem'>): DevReloadRecord[] {
  try {
    const raw = storage.getItem(DEV_RELOAD_LOG_KEY);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    return Array.isArray(parsed) ? (parsed as DevReloadRecord[]) : [];
  } catch {
    // Storage may be unavailable (private mode, blocked, or holding malformed data); start clean.
    return [];
  }
}

function writeLog(storage: Pick<Storage, 'setItem'>, records: DevReloadRecord[]): void {
  try {
    storage.setItem(DEV_RELOAD_LOG_KEY, JSON.stringify(records.slice(-DEV_RELOAD_LOG_LIMIT)));
  } catch {
    // Best-effort diagnostic; a storage write failure must never break the app.
  }
}

function payloadPath(payload: unknown): string | undefined {
  if (payload && typeof payload === 'object' && 'path' in payload) {
    const { path } = payload as { path?: unknown };
    if (typeof path === 'string') return path;
  }
  return undefined;
}

function readLast(storage: Pick<Storage, 'getItem'>): DevReloadRecord | undefined {
  try {
    const raw = storage.getItem(DEV_RELOAD_LAST_KEY);
    if (!raw) return undefined;
    const parsed: unknown = JSON.parse(raw);
    return parsed && typeof parsed === 'object' ? (parsed as DevReloadRecord) : undefined;
  } catch {
    return undefined;
  }
}

function writeLast(storage: Pick<Storage, 'setItem'>, record: DevReloadRecord): void {
  try {
    storage.setItem(DEV_RELOAD_LAST_KEY, JSON.stringify(record));
  } catch {
    // Best-effort; a persistent-storage write failure must never break the app.
  }
}

/** Read the current document's navigation type via the Performance Navigation Timing API, guarded for
 * environments where it is unavailable. `'reload'` means the page was reloaded rather than navigated to. */
function currentNavigationType(): string | undefined {
  try {
    const [entry] = performance.getEntriesByType('navigation') as (PerformanceNavigationTiming | undefined)[];
    return entry?.type;
  } catch {
    return undefined;
  }
}

/** Options for {@link installDevReloadDiagnostics}; injectable so the logic is unit-testable. */
export interface DevReloadDiagnosticsOptions {
  /** The HMR emitter, normally `import.meta.hot`. */
  hot: DevHotEmitter;
  /** Per-tab persistence that survives a same-context reload, normally `sessionStorage`. */
  storage: Pick<Storage, 'getItem' | 'setItem'>;
  /** Cross-context persistence, normally `localStorage`. When provided, a reload with no session-log
   * trace (Vite's connection-loss/server-restart reload) is still surfaced from the last recorded
   * signal — the case that leaves `sessionStorage` empty (HS2-8JV12R). */
  persistentStorage?: Pick<Storage, 'getItem' | 'setItem'>;
  /** How this document was reached; `'reload'` distinguishes a reload from a fresh navigation.
   * Defaults to the Performance Navigation Timing API. */
  navigationType?: () => string | undefined;
  /** Where to surface captured causes; defaults to the console. */
  logger?: Pick<Console, 'warn'>;
  /** Clock for record timestamps; injectable for deterministic tests. */
  now?: () => string;
}

/**
 * Surface the reload that just happened (if any) and start recording future reload triggers.
 * Returns the prior record it surfaced, or `undefined` when the page did not arrive via a captured
 * reload.
 */
export function installDevReloadDiagnostics({
  hot,
  storage,
  persistentStorage,
  navigationType = currentNavigationType,
  logger = console,
  now = () => new Date().toISOString(),
}: DevReloadDiagnosticsOptions): DevReloadRecord | undefined {
  const last = readLog(storage).at(-1);
  let surfaced: DevReloadRecord | undefined;
  if (last) {
    logger.warn(
      `[hotsheet-dev] the client reloaded at ${last.at} — Vite reported "${last.type}"` +
        `${last.path ? ` for ${last.path}` : ''}. Full reload log: sessionStorage['${DEV_RELOAD_LOG_KEY}'].`,
    );
    surfaced = last;
  } else {
    surfaced = surfaceConnectionLossReload(persistentStorage, navigationType, logger);
  }
  for (const [event, kind, imminent] of CAPTURED_EVENTS) {
    hot.on(event, payload => {
      const record: DevReloadRecord = { type: kind, path: payloadPath(payload), at: now() };
      if (record.path === undefined) delete record.path;
      writeLog(storage, [...readLog(storage), record]);
      if (persistentStorage) writeLast(persistentStorage, record);
      logger.warn(`[hotsheet-dev] Vite "${kind}"${record.path ? ` for ${record.path}` : ''} — ${imminent}.`);
    });
  }
  return surfaced;
}

/** When the session log is empty but the page arrived via a reload, explain it from the cross-context
 * record: this is the Vite connection-loss/server-restart reload that clears `sessionStorage` and never
 * fires an HMR event, so it previously left no trace at all (HS2-8JV12R). */
function surfaceConnectionLossReload(
  persistentStorage: Pick<Storage, 'getItem' | 'setItem'> | undefined,
  navigationType: () => string | undefined,
  logger: Pick<Console, 'warn'>,
): DevReloadRecord | undefined {
  if (!persistentStorage || navigationType() !== 'reload') return undefined;
  const last = readLast(persistentStorage);
  if (last) {
    logger.warn(
      `[hotsheet-dev] the client reloaded (navigation type "reload") with no session-log trace; the last ` +
        `recorded dev-server signal was "${last.type}"${last.path ? ` for ${last.path}` : ''} at ${last.at}. ` +
        `A reload from outside HMR is typically Vite's connection-loss / server-restart reload — check the ` +
        `\`npm run dev\` terminal, and whether the machine was low on memory (which can kill/restart the dev server).`,
    );
  } else {
    logger.warn(
      `[hotsheet-dev] the client reloaded (navigation type "reload") with no HMR or websocket trace captured ` +
        `— an external/browser reload, or the dev server restarted before a trace could be written. Watch the ` +
        `\`npm run dev\` terminal for a restart, and check for memory pressure.`,
    );
  }
  return last;
}
