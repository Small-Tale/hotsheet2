import { describe, expect, it } from 'vitest';

import {
  animateReaderMinimize,
  animateReaderRestore,
  readerDialogRect,
  readerTargetRect,
  rectTransform,
} from './reader-minimize-motion';

describe('reader minimize motion (HS2-Q8WF64)', () => {
  it('maps a card laid out at one rectangle onto another', () => {
    expect(
      rectTransform({ left: 200, top: 20, width: 1000, height: 800 }, { left: 900, top: 820, width: 40, height: 40 }),
    ).toBe('translate(700px, 800px) scale(0.04, 0.05)');
    expect(rectTransform({ left: 0, top: 0, width: 0, height: 0 }, { left: 5, top: 6, width: 10, height: 10 })).toBe(
      'translate(5px, 6px) scale(1, 1)',
    );
  });

  it('centers a restored reader at most 1024px wide and nearly viewport high', () => {
    expect(readerTargetRect({ width: 1440, height: 900 })).toEqual({ left: 208, top: 24, width: 1024, height: 852 });
    expect(readerTargetRect({ width: 390, height: 844 })).toEqual({ left: 16, top: 24, width: 358, height: 796 });
    expect(readerTargetRect({ width: 0, height: 0 }).width).toBe(0);
  });

  it('reads a dialog rectangle only when it is laid out', () => {
    const rect = { left: 1, top: 2, width: 3, height: 4 };
    expect(readerDialogRect({ getBoundingClientRect: () => rect } as unknown as Element)).toEqual(rect);
    expect(readerDialogRect({ getBoundingClientRect: () => ({ ...rect, width: 0 }) } as unknown as Element)).toBe(
      undefined,
    );
    expect(readerDialogRect(null)).toBeUndefined();
  });

  it('does nothing without a document or a rectangle, and restore always returns a release', async () => {
    await expect(animateReaderMinimize({ left: 0, top: 0, width: 10, height: 10 }, undefined)).resolves.toBeUndefined();
    await expect(
      animateReaderMinimize(undefined, { document: {} as Document, reduceMotion: false }),
    ).resolves.toBeUndefined();
    const release = await animateReaderRestore(undefined, undefined);
    expect(() => {
      release();
    }).not.toThrow();
  });
});
