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
/** Keep only the most recent entries so a long-lived tab cannot grow the log without bound. */
export const DEV_RELOAD_LOG_LIMIT = 20;

/** The Vite HMR events that precede or accompany a client reload/restart, mapped to a readable kind. */
const CAPTURED_EVENTS: ReadonlyArray<readonly [event: string, kind: string]> = [
  ['vite:beforeFullReload', 'full reload'],
  ['vite:invalidate', 'module invalidate'],
  ['vite:error', 'error'],
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

/** Options for {@link installDevReloadDiagnostics}; injectable so the logic is unit-testable. */
export interface DevReloadDiagnosticsOptions {
  /** The HMR emitter, normally `import.meta.hot`. */
  hot: DevHotEmitter;
  /** Persistence that survives a reload, normally `sessionStorage`. */
  storage: Pick<Storage, 'getItem' | 'setItem'>;
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
  logger = console,
  now = () => new Date().toISOString(),
}: DevReloadDiagnosticsOptions): DevReloadRecord | undefined {
  const priorLog = readLog(storage);
  const last = priorLog.at(-1);
  if (last) {
    logger.warn(
      `[hotsheet-dev] the client reloaded at ${last.at} — Vite reported "${last.type}"` +
        `${last.path ? ` for ${last.path}` : ''}. Full reload log: sessionStorage['${DEV_RELOAD_LOG_KEY}'].`,
    );
  }
  for (const [event, kind] of CAPTURED_EVENTS) {
    hot.on(event, payload => {
      const path = payloadPath(payload);
      writeLog(storage, [...readLog(storage), { type: kind, path, at: now() }]);
      logger.warn(
        `[hotsheet-dev] Vite "${kind}"${path ? ` for ${path}` : ''} — the client is about to reload/restart.`,
      );
    });
  }
  return last;
}
