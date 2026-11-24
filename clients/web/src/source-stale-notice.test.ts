import { afterEach, describe, expect, it, vi } from 'vitest';

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
  afterEach(() => {
    staleSourceNoticeDismissed.value = [];
    vi.unstubAllGlobals();
  });

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

  it('remembers distinct build/source pairs across project tabs (HS2-P54NS2)', () => {
    const first = assessCompatibility(server('a', 'b')),
      sameInAnotherProject: CompatibilityAssessment = { ...assessCompatibility(server('a', 'b')) },
      second = assessCompatibility(server('x', 'y')),
      newerSource = assessCompatibility(server('a', 'c')),
      rebuilt = assessCompatibility(server('c', 'd'));
    expect(staleSourceNoticeKey(first)).toBe('a|b');
    const stored = new Map<string, string>();
    const storage = { setItem: (key: string, value: string) => stored.set(key, value) };
    staleSourceNoticeDismissed.value = [];
    expect(showsStaleSourceNotice(first)).toBe(true);
    dismissStaleSourceNotice(first, storage);
    expect([...stored.values()].map((value) => JSON.parse(value))).toEqual([['a|b']]);
    expect(showsStaleSourceNotice(first)).toBe(false);
    expect(showsStaleSourceNotice(sameInAnotherProject)).toBe(false);
    expect(showsStaleSourceNotice(second)).toBe(true);
    dismissStaleSourceNotice(second, storage);
    expect(staleSourceNoticeDismissed.value).toEqual(['a|b', 'x|y']);
    expect(showsStaleSourceNotice(first)).toBe(false);
    expect(showsStaleSourceNotice(second)).toBe(false);
    expect(showsStaleSourceNotice(newerSource)).toBe(true);
    expect(showsStaleSourceNotice(rebuilt)).toBe(true);
    // A storage failure still dismisses for this page.
    dismissStaleSourceNotice(rebuilt, {
      setItem: () => {
        throw new Error('quota');
      },
    });
    expect(showsStaleSourceNotice(rebuilt)).toBe(false);
  });

  it('keeps only the most recent 32 differences and ignores repeated dismissals (HS2-P54NS2)', () => {
    staleSourceNoticeDismissed.value = [];
    const storage = { setItem: vi.fn() };
    for (let index = 0; index < 33; index++)
      dismissStaleSourceNotice(assessCompatibility(server(`build-${index}`, `source-${index}`)), storage);
    expect(staleSourceNoticeDismissed.value).toHaveLength(32);
    expect(staleSourceNoticeDismissed.value[0]).toBe('build-1|source-1');
    const latest = assessCompatibility(server('build-32', 'source-32'));
    dismissStaleSourceNotice(latest, storage);
    expect(storage.setItem).toHaveBeenCalledTimes(33);
    expect(showsStaleSourceNotice(latest)).toBe(false);
    expect(showsStaleSourceNotice(assessCompatibility(server('build-0', 'source-0')))).toBe(true);
  });

  it('restores the saved set and the old single-pair format after a reload (HS2-P54NS2)', async () => {
    const saved = new Map([['hotsheet.source-stale-notice.dismissed', JSON.stringify(['a|b', 'x|y'])]]);
    vi.stubGlobal('localStorage', { getItem: (key: string) => saved.get(key) });
    vi.resetModules();
    const restored = await import('./source-stale-notice');
    expect(restored.staleSourceNoticeDismissed.value).toEqual(['a|b', 'x|y']);
    expect(restored.showsStaleSourceNotice(assessCompatibility(server('a', 'b')))).toBe(false);
    saved.set('hotsheet.source-stale-notice.dismissed', 'a|b');
    vi.resetModules();
    const legacy = await import('./source-stale-notice');
    expect(legacy.staleSourceNoticeDismissed.value).toEqual(['a|b']);
    expect(legacy.showsStaleSourceNotice(assessCompatibility(server('a', 'b')))).toBe(false);
  });

  it('keeps the in-page dismissal when browser storage is unavailable (HS2-P54NS2)', async () => {
    vi.stubGlobal('localStorage', {
      getItem: () => {
        throw new Error('blocked');
      },
    });
    vi.resetModules();
    const restored = await import('./source-stale-notice');
    expect(restored.staleSourceNoticeDismissed.value).toEqual([]);
    const assessment = assessCompatibility(server('a', 'b'));
    restored.dismissStaleSourceNotice(assessment, {
      setItem: () => {
        throw new Error('quota');
      },
    });
    expect(restored.showsStaleSourceNotice(assessment)).toBe(false);
  });
});
