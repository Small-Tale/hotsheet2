import { describe, expect, it } from 'vitest';

import { timelineStageEstimate, typicalPhaseDurations } from './timeline-stage-estimate';

const now = Date.parse('2026-10-28T12:00:00Z');
const live = {
  status: 'started',
  started_phase: 'working' as const,
  started_phase_since: '2026-10-28T11:50:00Z',
  claimed_by: 'claude',
  claim_lease_expires_at: '2026-10-28T13:00:00Z',
};

describe('timelineStageEstimate (HS2-ST6RN1)', () => {
  it('estimates the remaining time from the phase median minus the time already in the phase', () => {
    expect(timelineStageEstimate(live, { working: 25 * 60_000 }, now)).toEqual({
      label: '~15m left',
      title: 'This phase typically takes about 25m',
      remainingMs: 15 * 60_000,
    });
  });

  it('says Soon once the phase runs past its typical duration', () => {
    expect(timelineStageEstimate(live, { working: 5 * 60_000 }, now)).toEqual({
      label: 'Soon',
      title: "Past this phase's typical duration (about 5m)",
      remainingMs: 0,
    });
  });

  it('falls back to the live claim ETA without phase history, then to no estimate', () => {
    const withEta = { ...live, claim_eta_at: '2026-10-28T14:00:00Z', claim_started_at: '2026-10-28T11:00:00Z' };
    expect(timelineStageEstimate(withEta, { analyzing: 60_000 }, now)).toMatchObject({
      label: '~2h left',
      remainingMs: 2 * 3_600_000,
    });
    expect(timelineStageEstimate(withEta, undefined, now)?.label).toBe('~2h left');
    // A phase with no start time cannot use the median, so the ETA answers.
    expect(timelineStageEstimate({ ...withEta, started_phase_since: undefined }, { working: 1 }, now)?.label).toBe(
      '~2h left',
    );
    expect(timelineStageEstimate(live, {}, now)).toEqual({ title: 'In progress; no estimate yet' });
  });

  it('has no in-progress stage without a live claim or outside Started', () => {
    expect(timelineStageEstimate({ ...live, claim_lease_expires_at: '2026-10-28T11:00:00Z' }, {}, now)).toBeUndefined();
    expect(timelineStageEstimate({ ...live, claimed_by: undefined }, {}, now)).toBeUndefined();
    expect(timelineStageEstimate({ ...live, status: 'completed' }, { working: 1 }, now)).toBeUndefined();
    expect(timelineStageEstimate({ ...live, status: 'not_started' }, { working: 1 }, now)).toBeUndefined();
  });

  it('reads each phase median from a phase report', () => {
    expect(
      typicalPhaseDurations({
        phases: [
          { phase: 'working', wall: { median_ms: 1_500_000 } },
          { phase: 'final_testing', wall: { median_ms: 600_000 } },
        ],
      }),
    ).toEqual({ working: 1_500_000, final_testing: 600_000 });
  });
});
