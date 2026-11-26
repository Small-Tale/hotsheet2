import { describe, expect, it } from 'vitest';

import type { LiveCiStatus } from './api';
import { CI_STALE_AFTER_MS, ciStaleness } from './ci-staleness';

const NOW = 1_800_000_000_000;
const report = (status: LiveCiStatus, reportedAgoMs: number | null) => ({
  status,
  reported_at_ms: reportedAgoMs === null ? null : NOW - reportedAgoMs,
});

describe('stale CI reports (HS2-CC7TCB)', () => {
  it('bounds a report at 2 hours', () => {
    expect(CI_STALE_AFTER_MS).toBe(2 * 60 * 60 * 1000);
  });

  it('keeps a fresh running or queued report and calls an older one stale', () => {
    expect(ciStaleness(report('in_progress', 0), NOW)).toBeUndefined();
    expect(ciStaleness(report('in_progress', CI_STALE_AFTER_MS), NOW)).toBeUndefined();
    expect(ciStaleness(report('in_progress', CI_STALE_AFTER_MS + 1), NOW)).toBe('running');
    expect(ciStaleness(report('queued', CI_STALE_AFTER_MS), NOW)).toBeUndefined();
    expect(ciStaleness(report('queued', CI_STALE_AFTER_MS + 1), NOW)).toBe('queued');
  });

  it('is renewed by a newer report of the run', () => {
    const stale = report('in_progress', 3 * CI_STALE_AFTER_MS);
    expect(ciStaleness(stale, NOW)).toBe('running');
    expect(ciStaleness({ ...stale, reported_at_ms: NOW - 1_000 }, NOW)).toBeUndefined();
  });

  it('never makes a finished run, an unreported store, or a report without a time stale', () => {
    for (const status of ['success', 'failure', 'cancelled', 'unavailable'] as const)
      expect(ciStaleness(report(status, 10 * CI_STALE_AFTER_MS), NOW)).toBeUndefined();
    expect(ciStaleness(report('in_progress', null), NOW)).toBeUndefined();
  });
});
