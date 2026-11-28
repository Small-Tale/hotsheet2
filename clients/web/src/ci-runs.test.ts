import { describe, expect, it } from 'vitest';

import type { LiveCi, LiveStoreState } from './api';
import { CI_TRUST_MS, ciClearQuery, ciRunShown, shownCiRuns } from './ci-runs';

const NOW = 1_795_700_000_000;

const run = (overrides: Partial<LiveCi> = {}): LiveCi => ({
  status: 'in_progress',
  checkout: 'demo-ab12',
  branch: 'main',
  sha: 'abcdef1',
  run_url: null,
  workflow: 'CI',
  started_at_ms: NOW - 60_000,
  finished_at_ms: null,
  reported_at_ms: NOW - 1_000,
  ...overrides,
});
const store = (overrides: Partial<LiveStoreState>) => overrides as LiveStoreState;

describe('ciRunShown (HS2-D3JJ49)', () => {
  it('never shows an unverifiable or unavailable run', () => {
    expect(ciRunShown(run({ verification: 'unverifiable', verify_by_ms: null }), NOW)).toBe(false);
    expect(ciRunShown(run({ status: 'unavailable' }), NOW)).toBe(false);
  });

  it('shows a verified or transient run in flight until its verify_by_ms, or always without a bound', () => {
    expect(ciRunShown(run({ verification: 'verified', verify_by_ms: NOW }), NOW)).toBe(true);
    expect(ciRunShown(run({ verification: 'verified', verify_by_ms: NOW - 1 }), NOW)).toBe(false);
    expect(ciRunShown(run({ verification: 'verified', verify_by_ms: null }), NOW + 10 * CI_TRUST_MS)).toBe(true);
    expect(ciRunShown(run({ verification: 'transient', verify_by_ms: null }), NOW + 10 * CI_TRUST_MS)).toBe(true);
    expect(ciRunShown(run({ status: 'queued', verify_by_ms: NOW - 1 }), NOW)).toBe(false);
  });

  it('bounds an older server report 2 h after it was reported, and never a finished run', () => {
    const older = run({ reported_at_ms: NOW - CI_TRUST_MS });
    expect(ciRunShown(older, NOW)).toBe(true);
    expect(ciRunShown(older, NOW + 1)).toBe(false);
    expect(ciRunShown(run({ reported_at_ms: null }), NOW + 10 * CI_TRUST_MS)).toBe(true);
    for (const status of ['success', 'failure', 'cancelled'] as const)
      expect(ciRunShown(run({ status, verify_by_ms: NOW - 1, reported_at_ms: 0 }), NOW), status).toBe(true);
  });
});

describe('shownCiRuns (HS2-D3JJ49)', () => {
  it('lists each shown run of every store, the newest per branch and workflow, newest first', () => {
    const ci = run(),
      platforms = run({ workflow: 'Platforms', reported_at_ms: NOW - 5_000 }),
      hidden = run({ workflow: 'Release', verification: 'unverifiable' }),
      olderCi = run({ checkout: 'other', reported_at_ms: NOW - 9_000 }),
      docs = run({ branch: 'docs', reported_at_ms: NOW - 2_000 });
    const states = [
      store({ ci_runs: [ci, platforms, hidden] }),
      store({ ci_runs: [olderCi, docs] }),
      // An older server sends only `ci`.
      store({ ci: run({ workflow: 'Lint', reported_at_ms: NOW - 3_000 }) }),
      store({}),
    ];
    expect(shownCiRuns(states, NOW).map((each) => [each.branch, each.workflow, each.checkout])).toEqual([
      ['main', 'CI', 'demo-ab12'],
      ['docs', 'CI', 'demo-ab12'],
      ['main', 'Lint', 'demo-ab12'],
      ['main', 'Platforms', 'demo-ab12'],
    ]);
    expect(shownCiRuns([], NOW)).toEqual([]);
    // A newer server's empty list wins over its `ci`.
    expect(shownCiRuns([store({ ci: run(), ci_runs: [] })], NOW)).toEqual([]);
    expect(shownCiRuns([store({ ci: run({ status: 'unavailable' }) })], NOW)).toEqual([]);
  });

  it('orders by report, then finish, then start time when a time is missing', () => {
    const finished = run({ status: 'success', reported_at_ms: null, finished_at_ms: NOW - 10, workflow: 'A' }),
      started = run({ reported_at_ms: null, started_at_ms: NOW - 20, workflow: 'B' }),
      none = run({ reported_at_ms: null, started_at_ms: null, workflow: 'C' });
    expect(shownCiRuns([store({ ci_runs: [none, started, finished] })], NOW).map((each) => each.workflow)).toEqual([
      'A',
      'B',
      'C',
    ]);
  });
});

describe('ciClearQuery (HS2-7PSBHQ, HS2-D3JJ49)', () => {
  it('names the report time, then the branch and workflow when given', () => {
    expect(ciClearQuery(17.9)).toBe('reported_at_ms=17');
    expect(ciClearQuery(17, { branch: 'release/2', workflow: 'Build all' })).toBe(
      'reported_at_ms=17&branch=release%2F2&workflow=Build+all',
    );
    expect(ciClearQuery(5, { workflow: 'CI' })).toBe('reported_at_ms=5&workflow=CI');
  });
});
