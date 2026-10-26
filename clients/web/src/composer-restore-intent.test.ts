import { describe, expect, it } from 'vitest';

import { createComposerRestoreIntent } from './composer-restore-intent';

describe('parked composer intent (HS2-40JX5Q)', () => {
  it('keeps an open parked draft through repeated closed-placeholder saves, then consumes it', () => {
    const intent = createComposerRestoreIntent();
    intent.capture('one', 1, 4, true);
    for (let write = 0; write < 3; write++) expect(intent.persistedOpen('one', 1, 4, false)).toBe(true);
    expect(intent.canRestore('one', 1, 4)).toBe(true);
    intent.finish('one', 1);
    expect(intent.canRestore('one', 1, 4)).toBe(true);
    expect(intent.persistedOpen('one', 1, 4, false)).toBe(false);
    expect(intent.persistedOpen('one', 1, 4, true)).toBe(true);
  });

  it('preserves closed preferences and empty-then-refill activations', () => {
    const intent = createComposerRestoreIntent();
    intent.capture('one', 1, 4, false);
    expect(intent.persistedOpen('one', 1, 4, false)).toBe(false);
    intent.capture('two', 2, 5, undefined);
    expect(intent.persistedOpen('two', 2, 5, false)).toBe(false);
    intent.capture('one', 3, 6, true);
    expect(intent.persistedOpen('one', 3, 6, false)).toBe(true);
  });

  it('cannot restore or retain the old intent after open/cancel/reset/re-edit changes its surface epoch', () => {
    const intent = createComposerRestoreIntent();
    intent.capture('one', 1, 4, true);
    for (const epoch of [5, 6, 7]) {
      expect(intent.canRestore('one', 1, epoch)).toBe(false);
      expect(intent.persistedOpen('one', 1, epoch, false)).toBe(false);
      expect(intent.persistedOpen('one', 1, epoch, true)).toBe(true);
    }
  });

  it('ignores interrupted project/generation completions and retains intent across failed refresh', () => {
    const intent = createComposerRestoreIntent();
    intent.capture('one', 1, 4, true);
    intent.capture('two', 2, 5, true);
    intent.finish('one', 1);
    expect(intent.canRestore('one', 1, 4)).toBe(false);
    expect(intent.persistedOpen('one', 1, 4, false)).toBe(false);
    expect(intent.canRestore('two', 1, 5)).toBe(false);
    expect(intent.persistedOpen('two', 1, 5, false)).toBe(false);
    expect(intent.persistedOpen('two', 2, 5, false)).toBe(true);
    // A failed refresh has no successful restore completion; later timer writes still keep it.
    expect(intent.canRestore('two', 2, 5)).toBe(true);
    intent.finish('two', 1);
    expect(intent.persistedOpen('two', 2, 5, false)).toBe(true);
    intent.finish('two', 2);
    expect(intent.persistedOpen('two', 2, 5, false)).toBe(false);
  });
});
