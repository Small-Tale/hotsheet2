import { describe, expect, it } from 'vitest';

import type { CalibrationReport } from '../api';
import { ConfidenceCalibration, reopenRateLabel } from './confidence-calibration';

const calibrationFixture: CalibrationReport = {
  completions: 3,
  scored: 2,
  bands: [
    {
      band: 'verified',
      range: '90-100',
      completions: 1,
      verified: 0,
      reopened: 1,
      pending: 0,
      reopen_rate: 1,
      mean_confidence: 95,
    },
    { band: 'assumed', range: '70-89', completions: 0, verified: 0, reopened: 0, pending: 0 },
    {
      band: 'partial',
      range: '40-69',
      completions: 1,
      verified: 1,
      reopened: 0,
      pending: 0,
      reopen_rate: 0,
      mean_confidence: 65,
    },
    { band: 'unverified', range: '0-39', completions: 0, verified: 0, reopened: 0, pending: 0 },
    { band: 'unscored', range: '-', completions: 1, verified: 0, reopened: 0, pending: 1 },
  ],
  events: [
    { slug: 'HS2-A', completed_at: '2026-09-01T00:00:00Z', confidence: 95, outcome: 'reopened' },
    { slug: 'HS2-A', completed_at: '2026-09-02T00:00:00Z', confidence: 65, outcome: 'verified' },
    { slug: 'HS2-B', completed_at: '2026-09-03T00:00:00Z', outcome: 'pending' },
  ],
};

describe('ConfidenceCalibration (HS2-Q1WCCY)', () => {
  it('renders every band with counts, reopen rate bars, and mean scores', () => {
    const markup = String(ConfidenceCalibration({ state: { status: 'ready', report: calibrationFixture } }));
    expect(markup).toContain('data-state="ready"');
    expect(markup).toContain('3 completions, 2 scored');
    for (const band of ['verified', 'assumed', 'partial', 'unverified', 'unscored'])
      expect(markup).toContain(`data-band="${band}"`);
    expect(markup).toMatch(/Fully verified[\s\S]*90-100/u);
    expect(markup).toContain('no score');
    expect(markup).toContain('100%');
    expect(markup).toContain('95.0');
    // Only bands with a resolved completion draw a rate bar.
    expect(markup.match(/<wa-progress-bar/gu)).toHaveLength(2);
    expect(markup).toContain('label="Fully verified reopen rate"');
    expect(markup).toContain('scope="col"');
  });

  it('lists recent completions newest first with their score and outcome', () => {
    const markup = String(ConfidenceCalibration({ state: { status: 'ready', report: calibrationFixture } }));
    const recent = markup.slice(markup.indexOf('Recent completions'));
    expect(recent.indexOf('HS2-B')).toBeLessThan(recent.indexOf('HS2-A'));
    expect(recent).toContain('Unscored');
    expect(recent).toContain('Awaiting outcome');
    expect(recent).toContain('data-outcome="reopened"');
    expect(recent).toContain('data-component="confidence-badge"');
  });

  it('shows loading, error, and empty states', () => {
    expect(String(ConfidenceCalibration({ state: { status: 'loading' } }))).toContain('data-state="loading"');
    const error = String(ConfidenceCalibration({ state: { status: 'error', message: 'server offline' } }));
    expect(error).toContain('Confidence calibration is unavailable');
    expect(error).toContain('server offline');
    const empty = String(
      ConfidenceCalibration({
        state: { status: 'ready', report: { ...calibrationFixture, completions: 0, events: [] } },
      }),
    );
    expect(empty).toContain('No completions yet');
    expect(empty).not.toContain('<table');
  });

  it('labels an unresolved reopen rate with a dash', () => {
    expect(reopenRateLabel({ reopen_rate: undefined })).toBe('—');
    expect(reopenRateLabel({ reopen_rate: 0.333 })).toBe('33%');
  });
});
