import { afterEach, describe, expect, it, vi } from 'vitest';

import { copyWhenReady } from './clipboard-when-ready';

describe('copyWhenReady (HS2-1JT25R)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('starts a promise-backed clipboard write before the text is known', async () => {
    const writes: ClipboardItem[] = [];
    class FakeItem {
      constructor(readonly items: Record<string, Promise<Blob>>) {}
    }
    vi.stubGlobal('ClipboardItem', FakeItem);
    vi.stubGlobal('navigator', {
      clipboard: {
        write: vi.fn((items: ClipboardItem[]) => {
          writes.push(...items);
          return Promise.resolve();
        }),
      },
    });
    let resolve!: (value: string) => void;
    const copied = copyWhenReady(new Promise<string>((done) => (resolve = done)));
    // The write began synchronously, inside the (simulated) user gesture.
    expect(writes).toHaveLength(1);
    resolve('ABCD-EFGH');
    expect(await copied).toBe(true);
    const blob = await (writes[0] as unknown as FakeItem).items['text/plain'];
    expect(await blob.text()).toBe('ABCD-EFGH');
  });

  it('falls back to writeText once the text resolves', async () => {
    const writeText = vi.fn(() => Promise.resolve());
    vi.stubGlobal('ClipboardItem', undefined);
    vi.stubGlobal('navigator', { clipboard: { writeText } });
    expect(await copyWhenReady(Promise.resolve('CODE'))).toBe(true);
    expect(writeText).toHaveBeenCalledWith('CODE');
  });

  it('reports false instead of throwing when copying is refused or unavailable', async () => {
    vi.stubGlobal('ClipboardItem', undefined);
    vi.stubGlobal('navigator', { clipboard: { writeText: () => Promise.reject(new Error('denied')) } });
    expect(await copyWhenReady(Promise.resolve('CODE'))).toBe(false);
    vi.stubGlobal('navigator', {});
    expect(await copyWhenReady(Promise.resolve('CODE'))).toBe(false);
    vi.stubGlobal('navigator', { clipboard: { writeText: vi.fn() } });
    expect(await copyWhenReady(Promise.reject(new Error('sign-in failed')))).toBe(false);
  });
});
