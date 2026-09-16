import { beforeEach, describe, expect, it, vi } from 'vitest';

import {
  DEV_RELOAD_LOG_KEY,
  DEV_RELOAD_LOG_LIMIT,
  type DevReloadRecord,
  installDevReloadDiagnostics,
} from './dev-reload-diagnostics';

function createHot() {
  const handlers = new Map<string, (payload: unknown) => void>();
  return {
    handlers,
    on: (event: string, callback: (payload: unknown) => void) => { handlers.set(event, callback); },
    emit: (event: string, payload?: unknown) => { handlers.get(event)?.(payload); },
  };
}

function createStorage(initial?: DevReloadRecord[]) {
  const map = new Map<string, string>();
  if (initial) map.set(DEV_RELOAD_LOG_KEY, JSON.stringify(initial));
  return {
    map,
    getItem: (key: string) => map.get(key) ?? null,
    setItem: (key: string, value: string) => { map.set(key, value); },
  };
}

function logOf(storage: ReturnType<typeof createStorage>): DevReloadRecord[] {
  const raw = storage.map.get(DEV_RELOAD_LOG_KEY);
  return raw ? (JSON.parse(raw) as DevReloadRecord[]) : [];
}

describe('installDevReloadDiagnostics', () => {
  let warn: ReturnType<typeof vi.fn<(message?: unknown, ...optionalParams: unknown[]) => void>>;

  beforeEach(() => { warn = vi.fn<(message?: unknown, ...optionalParams: unknown[]) => void>(); });

  it('records a full-reload trigger with its blamed path and warns before the reload', () => {
    const hot = createHot();
    const storage = createStorage();
    installDevReloadDiagnostics({ hot, storage, logger: { warn }, now: () => '2026-09-16T00:00:00.000Z' });
    expect(warn).not.toHaveBeenCalled(); // Nothing persisted yet, so nothing to surface on install.

    hot.emit('vite:beforeFullReload', { path: '/src/main.tsx' });
    expect(logOf(storage)).toEqual([{ type: 'full reload', path: '/src/main.tsx', at: '2026-09-16T00:00:00.000Z' }]);
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('full reload'));
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('/src/main.tsx'));
  });

  it('surfaces the most recent reload on install (the log survived the reload)', () => {
    const hot = createHot();
    const storage = createStorage([{ type: 'module invalidate', path: '/src/x.ts', at: '2026-09-16T01:02:03.000Z' }]);
    const prior = installDevReloadDiagnostics({ hot, storage, logger: { warn } });
    expect(prior).toEqual({ type: 'module invalidate', path: '/src/x.ts', at: '2026-09-16T01:02:03.000Z' });
    expect(warn).toHaveBeenCalledTimes(1);
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('2026-09-16T01:02:03.000Z'));
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('module invalidate'));
  });

  it('captures invalidate and error triggers and records a missing path as undefined', () => {
    const hot = createHot();
    const storage = createStorage();
    installDevReloadDiagnostics({ hot, storage, logger: { warn }, now: () => 't' });
    hot.emit('vite:invalidate', {});
    hot.emit('vite:error', undefined);
    expect(logOf(storage)).toEqual([
      { type: 'module invalidate', at: 't' },
      { type: 'error', at: 't' },
    ]);
  });

  it('bounds the persisted log to the most recent entries', () => {
    const hot = createHot();
    const seed = Array.from({ length: DEV_RELOAD_LOG_LIMIT }, (_, index) => ({ type: 'full reload', at: `t${index}` }));
    const storage = createStorage(seed);
    installDevReloadDiagnostics({ hot, storage, logger: { warn }, now: () => 'newest' });
    hot.emit('vite:beforeFullReload', { path: '/src/late.ts' });
    const log = logOf(storage);
    expect(log).toHaveLength(DEV_RELOAD_LOG_LIMIT);
    expect(log.at(-1)).toEqual({ type: 'full reload', path: '/src/late.ts', at: 'newest' });
    expect(log.at(0)).toEqual({ type: 'full reload', at: 't1' }); // Oldest entry (t0) evicted.
  });

  it('never throws when storage is unavailable', () => {
    const hot = createHot();
    const throwingStorage = {
      getItem: (): string | null => { throw new Error('blocked'); },
      setItem: (): void => { throw new Error('blocked'); },
    };
    expect(() => installDevReloadDiagnostics({ hot, storage: throwingStorage, logger: { warn } })).not.toThrow();
    expect(() => { hot.emit('vite:beforeFullReload', { path: '/src/main.tsx' }); }).not.toThrow();
  });

  it('ignores malformed persisted data instead of crashing', () => {
    const hot = createHot();
    const storage = createStorage();
    storage.map.set(DEV_RELOAD_LOG_KEY, '{not json');
    const prior = installDevReloadDiagnostics({ hot, storage, logger: { warn } });
    expect(prior).toBeUndefined();
    expect(warn).not.toHaveBeenCalled();
    hot.emit('vite:beforeFullReload', { path: '/src/a.ts' });
    expect(logOf(storage)).toEqual([expect.objectContaining({ type: 'full reload', path: '/src/a.ts' })]);
  });
});
