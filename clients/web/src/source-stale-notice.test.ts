import { describe, expect, it } from 'vitest';

import { assessCompatibility, type CompatibilityAssessment } from './compatibility';
import {
  dismissStaleSourceNotice,
  isStaleSourceOnly,
  showsStaleSourceNotice,
  staleSourceNoticeDismissed,
  staleSourceNoticeKey,
} from './source-stale-notice';

const server = (build: string, source: string) => ({
  generation: 'hs2',
  build_revision: build,
  source_revision: source,
  protocol: { min: 1, max: 1 },
});

describe('stale-source notice (HS2-B84P1S)', () => {
  it('treats newer workspace source behind a consistent pair as informational only', () => {
    // Dev: no client revision, source moved past the running build.
    expect(isStaleSourceOnly(assessCompatibility(server('a', 'b')))).toBe(true);
    // The client was built from the running server's revision: still the same pair.
    expect(isStaleSourceOnly(assessCompatibility(server('a', 'b'), undefined, 'a'))).toBe(true);
    // A client built from another revision is a real mismatch and keeps the alert.
    expect(isStaleSourceOnly(assessCompatibility(server('a', 'b'), undefined, 'c'))).toBe(false);
    // Nothing stale, nothing to say.
    expect(isStaleSourceOnly(assessCompatibility(server('a', 'a')))).toBe(false);
    // Protocol problems stay alerts even when the source is also stale.
    expect(isStaleSourceOnly(assessCompatibility({ ...server('a', 'b'), protocol: { min: 0, max: 0 } }))).toBe(false);
    expect(isStaleSourceOnly(assessCompatibility(undefined))).toBe(false);
    expect(isStaleSourceOnly(undefined)).toBe(false);
  });

  it('keys a dismissal on the build pair, globally, so a later change re-announces', () => {
    const first = assessCompatibility(server('a', 'b')),
      sameInAnotherProject: CompatibilityAssessment = { ...assessCompatibility(server('a', 'b')) },
      newerSource = assessCompatibility(server('a', 'c')),
      rebuilt = assessCompatibility(server('c', 'd'));
    expect(staleSourceNoticeKey(first)).toBe('a|b');
    const stored = new Map<string, string>();
    staleSourceNoticeDismissed.value = undefined;
    expect(showsStaleSourceNotice(first)).toBe(true);
    dismissStaleSourceNotice(first, { setItem: (key, value) => stored.set(key, value) });
    expect([...stored.values()]).toEqual(['a|b']);
    expect(showsStaleSourceNotice(first)).toBe(false);
    expect(showsStaleSourceNotice(sameInAnotherProject)).toBe(false);
    expect(showsStaleSourceNotice(newerSource)).toBe(true);
    expect(showsStaleSourceNotice(rebuilt)).toBe(true);
    // A storage failure still dismisses for this page.
    dismissStaleSourceNotice(rebuilt, {
      setItem: () => {
        throw new Error('quota');
      },
    });
    expect(showsStaleSourceNotice(rebuilt)).toBe(false);
    staleSourceNoticeDismissed.value = undefined;
  });
});
