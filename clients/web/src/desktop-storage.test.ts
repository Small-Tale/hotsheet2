import { describe, expect, it, vi } from 'vitest';

import { initializeDesktopStorage } from './desktop-storage';
import { DESKTOP_STORAGE_PATH } from './desktop-storage-contract';

function browser(entries: Record<string, string> = {}) {
  class MemoryStorage implements Storage {
    private values = new Map(Object.entries(entries));
    get length() {
      return this.values.size;
    }
    key(index: number) {
      return [...this.values.keys()][index] ?? null;
    }
    getItem(key: string) {
      return this.values.get(key) ?? null;
    }
    setItem(key: string, value: string) {
      this.values.set(key, value);
    }
    removeItem(key: string) {
      this.values.delete(key);
    }
    clear() {
      this.values.clear();
    }
  }
  const events = new EventTarget(),
    localStorage = new MemoryStorage();
  return {
    localStorage,
    addEventListener: events.addEventListener.bind(events),
    removeEventListener: events.removeEventListener.bind(events),
    events,
    MemoryStorage,
  };
}
function transport(initial: Array<{ key: string; value: string | null }> = []) {
  const saved = new Map(initial.map(({ key, value }) => [key, value]));
  const fetcher = vi.fn<typeof fetch>(async (url, options) => {
    expect(url).toBe(DESKTOP_STORAGE_PATH);
    if (!options?.method) return Response.json([...saved].map(([key, value]) => ({ key, value })));
    if (typeof options.body !== 'string') throw new Error('Expected JSON string body.');
    const entry = JSON.parse(options.body) as { key: string; value: string | null };
    saved.set(entry.key, entry.value);
    return new Response(null, { status: 204 });
  });
  return { saved, fetcher };
}
describe('desktop browser storage (HS2-RKZKX7)', () => {
  it('hydrates before defaults, imports visible old data, and honors tombstones', async () => {
    const host = browser({
      'hotsheet.open-projects': '["/synthetic/project"]',
      'hotsheet.removed': 'old',
      other: 'private',
    });
    const { saved, fetcher } = transport([
      { key: 'hotsheet.layout', value: 'board' },
      { key: 'hotsheet.removed', value: null },
    ]);
    const persistence = await initializeDesktopStorage(host, fetcher);
    expect(host.localStorage.getItem('hotsheet.layout')).toBe('board');
    expect(host.localStorage.getItem('hotsheet.removed')).toBeNull();
    expect(saved.get('hotsheet.open-projects')).toBe('["/synthetic/project"]');
    expect(saved.has('other')).toBe(false);
    persistence.dispose();
  });
  it('restores projects/settings in fresh origins, then removes, clears and refills', async () => {
    const { saved, fetcher } = transport();
    const first = browser(),
      one = await initializeDesktopStorage(first, fetcher);
    first.localStorage.setItem('hotsheet.open-projects', '["/synthetic/project"]');
    first.localStorage.setItem('hotsheet.layout', 'list');
    first.localStorage.setItem('hotsheet.layout', 'board');
    await one.flush();
    one.dispose();
    const second = browser(),
      two = await initializeDesktopStorage(second, fetcher);
    expect(second.localStorage.getItem('hotsheet.open-projects')).toBe('["/synthetic/project"]');
    expect(second.localStorage.getItem('hotsheet.layout')).toBe('board');
    const session = new second.MemoryStorage();
    session.setItem('hotsheet.session-only', 'ignored');
    second.localStorage.removeItem('hotsheet.layout');
    await two.flush();
    expect(saved.get('hotsheet.layout')).toBeNull();
    second.localStorage.clear();
    await two.flush();
    expect(saved.get('hotsheet.open-projects')).toBeNull();
    second.localStorage.setItem('hotsheet.layout', 'settings');
    await two.flush();
    two.dispose();
    const three = await initializeDesktopStorage(first, fetcher);
    expect(first.localStorage.getItem('hotsheet.layout')).toBe('settings');
    expect(first.localStorage.getItem('hotsheet.open-projects')).toBeNull();
    expect(saved.has('hotsheet.session-only')).toBe(false);
    three.dispose();
  });
  it('retains failed writes and retries on page hide without logging values', async () => {
    const host = browser(),
      { saved, fetcher } = transport(),
      persistence = await initializeDesktopStorage(host, fetcher);
    fetcher.mockResolvedValueOnce(new Response(null, { status: 503 }));
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    host.localStorage.setItem('hotsheet.layout', 'board');
    await expect(persistence.flush()).rejects.toThrow('Could not save');
    host.events.dispatchEvent(new Event('pagehide'));
    await persistence.flush();
    expect(saved.get('hotsheet.layout')).toBe('board');
    expect(error).toHaveBeenCalledWith('Could not save desktop preferences. Pending changes will be retried.');
    error.mockRestore();
    persistence.dispose();
  });
  it('starts queued small keepalive writes during pagehide while an older write is pending', async () => {
    const host = browser(),
      { fetcher } = transport(),
      persistence = await initializeDesktopStorage(host, fetcher);
    let release!: (response: Response) => void;
    fetcher.mockImplementationOnce(
      () =>
        new Promise<Response>((resolve) => {
          release = resolve;
        }),
    );
    host.localStorage.setItem('hotsheet.layout', 'list');
    host.localStorage.setItem('hotsheet.layout', 'board');
    host.localStorage.setItem('hotsheet.other', 'changed');
    host.events.addEventListener('pagehide', () => {
      host.localStorage.setItem('hotsheet.layout', 'settings');
    });
    host.events.dispatchEvent(new Event('pagehide'));
    const bodies = fetcher.mock.calls.slice(1).map(([, options]) => options);
    expect(bodies).toHaveLength(4);
    expect(bodies.every((options) => options?.keepalive === true)).toBe(true);
    expect(bodies.at(-1)?.body).toContain('settings');
    release(new Response(null, { status: 204 }));
    await persistence.flush();
    persistence.dispose();
  });

  it('keeps a rejected legacy draft pending while importing and saving unrelated settings', async () => {
    const host = browser({ 'hotsheet.draft': 'synthetic draft', 'hotsheet.layout': 'board' }),
      { saved, fetcher } = transport(),
      delegate = fetcher.getMockImplementation()!;
    fetcher.mockImplementation((url, options) => {
      if (typeof options?.body === 'string' && options.body.includes('hotsheet.draft'))
        return Promise.resolve(new Response(null, { status: 413 }));
      return delegate(url, options);
    });
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const persistence = await initializeDesktopStorage(host, fetcher);
    expect(saved.get('hotsheet.layout')).toBe('board');
    host.localStorage.setItem('hotsheet.layout', 'settings');
    await expect(persistence.flush()).rejects.toThrow('Could not save');
    expect(saved.get('hotsheet.layout')).toBe('settings');
    expect(host.localStorage.getItem('hotsheet.draft')).toBe('synthetic draft');
    error.mockRestore();
    persistence.dispose();
  });

  it('preserves visible data on malformed/unavailable startup snapshots', async () => {
    for (const response of [new Response(null, { status: 503 }), Response.json([{ key: '../escape', value: 'bad' }])]) {
      const host = browser({ 'hotsheet.layout': 'board' });
      await expect(
        initializeDesktopStorage(
          host,
          vi.fn(async () => response),
        ),
      ).rejects.toThrow();
      expect(host.localStorage.getItem('hotsheet.layout')).toBe('board');
    }
  });
});
