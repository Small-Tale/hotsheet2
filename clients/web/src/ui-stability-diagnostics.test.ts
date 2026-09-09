import { describe, expect, it } from 'vitest';

import { hasDismissalThrash, isUnexpectedQuickDismiss } from './ui-stability-diagnostics';

describe('UI stability diagnostics', () => {
  it('distinguishes an unexpected fast dismissal from a direct user action', () => {
    expect(isUnexpectedQuickDismiss(1_000, 1_700, 1_100)).toBe(true);
    expect(isUnexpectedQuickDismiss(1_000, 1_100, 1_050)).toBe(false);
    expect(isUnexpectedQuickDismiss(1_000, 2_500, 1_100)).toBe(false);
  });

  it('requires three quick dismissals inside one bounded window', () => {
    expect(hasDismissalThrash([1_000, 5_000], 9_000)).toBe(false);
    expect(hasDismissalThrash([1_000, 5_000, 9_000], 9_000)).toBe(true);
    expect(hasDismissalThrash([1_000, 5_000, 16_000], 16_000)).toBe(false);
  });
});
