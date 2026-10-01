import { describe, expect, it } from 'vitest';

import {
  applyKnownActiveTicketExpiries,
  claimEtaPresentation,
  claimExpiryWakeDelay,
  isTicketActivelyWorkedOn,
  nextActiveTicketExpiry,
  nextClaimEtaTick,
  projectTabTicketState,
} from './active-ticket-work';
import type { TicketRow } from './api';

describe('active ticket work', () => {
  const now = Date.parse('2026-09-02T12:00:00Z');

  it('distinguishes a live renewable claim from started or previously claimed state', () => {
    expect(isTicketActivelyWorkedOn({}, now)).toBe(false);
    expect(isTicketActivelyWorkedOn({ claimed_by: 'codex' }, now)).toBe(false);
    expect(isTicketActivelyWorkedOn({ claimed_by: 'codex', claim_lease_expires_at: 'invalid' }, now)).toBe(false);
    expect(isTicketActivelyWorkedOn({ claimed_by: 'codex', claim_lease_expires_at: '2026-09-02T12:00:00Z' }, now)).toBe(
      false,
    );
    expect(isTicketActivelyWorkedOn({ claimed_by: 'codex', claim_lease_expires_at: '2026-09-02T12:30:00Z' }, now)).toBe(
      true,
    );
    expect(
      isTicketActivelyWorkedOn(
        { status: 'completed', claimed_by: 'codex', claim_lease_expires_at: '2026-09-02T12:30:00Z' },
        now,
      ),
    ).toBe(false);
  });

  it('schedules the nearest live expiry while ignoring released and stale claims', () => {
    expect(
      nextActiveTicketExpiry(
        [
          {},
          { claimed_by: 'old', claim_lease_expires_at: '2026-09-02T11:00:00Z' },
          { claimed_by: 'later', claim_lease_expires_at: '2026-09-02T13:00:00Z' },
          { status: 'verified', claimed_by: 'legacy', claim_lease_expires_at: '2026-09-02T12:01:00Z' },
          { claimed_by: 'next', claim_lease_expires_at: '2026-09-02T12:05:00Z' },
        ],
        now,
      ),
    ).toBe(Date.parse('2026-09-02T12:05:00Z'));
  });

  it('clamps the expiry wake-up delay so a far-future lease cannot wrap setTimeout into a tight loop', () => {
    expect(claimExpiryWakeDelay(now + 5_000, now)).toBe(5_025);
    expect(claimExpiryWakeDelay(now - 5_000, now)).toBe(1);
    expect(claimExpiryWakeDelay(Date.parse('2099-09-11T12:00:00Z'), now)).toBe(2_147_483_647);
  });

  it('derives tab counts from one cached ticket pass using workflow-correct Up Next and live-claim semantics', () => {
    const ticket = (overrides: Partial<TicketRow>): TicketRow => ({
      connection_id: 'git',
      native_id: crypto.randomUUID(),
      qualified_id: `git:${crypto.randomUUID()}`,
      id: crypto.randomUUID(),
      slug: 'HS2-DEMO01',
      title: 'Ticket',
      up_next: false,
      feedback_needed: false,
      tags: [],
      blocked_by: [],
      claim_count: 0,
      ...overrides,
    });
    expect(
      projectTabTicketState(
        [
          ticket({ status: 'not_started', up_next: true }),
          ticket({
            status: 'started',
            up_next: true,
            claimed_by: 'codex',
            claim_lease_expires_at: '2026-09-02T12:30:00Z',
          }),
          ticket({
            status: 'completed',
            up_next: true,
            claimed_by: 'stale',
            claim_lease_expires_at: '2026-09-02T12:30:00Z',
          }),
          ticket({ status: 'started', claimed_by: 'expired', claim_lease_expires_at: '2026-09-02T11:59:00Z' }),
          ticket({ status: 'backlog', up_next: true }),
        ],
        now,
      ),
    ).toEqual({ upNextCount: 2, activeTicketCount: 1 });
  });

  it('decrements an exact aggregate when a cached claim expires without losing uncached active work', () => {
    const claimed = {
      connection_id: 'git',
      native_id: '01',
      qualified_id: 'git:01',
      id: '01',
      slug: 'HS2-ONE',
      title: 'Claimed',
      status: 'started',
      up_next: true,
      feedback_needed: false,
      tags: [],
      blocked_by: [],
      claimed_by: 'codex',
      claim_lease_expires_at: '2026-09-02T12:01:00Z',
      claim_count: 1,
    } satisfies TicketRow;
    const counts = {
      total: 300,
      queued: 300,
      backlog: 0,
      archive: 0,
      open: 300,
      up_next: 250,
      active: 4,
      started: 4,
      completed_today: 0,
    };

    expect(applyKnownActiveTicketExpiries(counts, [claimed], now, Date.parse('2026-09-02T12:01:00Z'))).toEqual({
      ...counts,
      active: 3,
    });
    expect(applyKnownActiveTicketExpiries(counts, [claimed], now, now)).toBe(counts);
  });
});

describe('claim ETA presentation (HS2-XQMDQB)', () => {
  const at = (iso: string) => Date.parse(iso),
    live = {
      status: 'started',
      claimed_by: 'agent-1',
      claim_lease_expires_at: '2026-10-01T12:00:00Z',
      claim_started_at: '2026-10-01T10:00:00Z',
      claim_eta_at: '2026-10-01T11:00:00Z',
    };

  it('walks from no estimate through progress to overrun and back after a re-estimate', () => {
    expect(claimEtaPresentation({ ...live, claim_eta_at: undefined }, at('2026-10-01T10:30:00Z'))).toBeUndefined();
    expect(claimEtaPresentation(live, at('2026-10-01T10:00:00Z'))).toMatchObject({
      kind: 'estimate',
      percent: 0,
      label: '~1h left',
    });
    expect(claimEtaPresentation(live, at('2026-10-01T10:15:00Z'))).toMatchObject({
      kind: 'estimate',
      percent: 25,
      label: '~45m left',
    });
    const almost = claimEtaPresentation(live, at('2026-10-01T10:59:45Z'));
    expect(almost).toMatchObject({ kind: 'estimate', percent: 99, label: '<1m left' });
    const overrun = claimEtaPresentation(live, at('2026-10-01T11:10:00Z'));
    expect(overrun).toMatchObject({ kind: 'overrun', label: 'Soon' });
    expect(overrun?.title).toContain('by about 10m');
    // A renewal with a new estimate leaves the overrun state.
    expect(
      claimEtaPresentation({ ...live, claim_eta_at: '2026-10-01T13:30:00Z' }, at('2026-10-01T11:10:00Z')),
    ).toMatchObject({ kind: 'estimate', percent: 33, label: '~2h 20m left' });
  });

  it('shows nothing once the claim is released, expired, or the ticket is done', () => {
    const now = at('2026-10-01T10:30:00Z');
    expect(claimEtaPresentation({ ...live, claimed_by: undefined }, now)).toBeUndefined();
    expect(claimEtaPresentation({ ...live, claim_lease_expires_at: '2026-10-01T10:29:00Z' }, now)).toBeUndefined();
    expect(claimEtaPresentation({ ...live, status: 'completed' }, now)).toBeUndefined();
    expect(claimEtaPresentation({ ...live, claim_eta_at: 'not a time' }, now)).toBeUndefined();
  });

  it('falls back to zero progress without a known claim start and formats long estimates in days', () => {
    expect(claimEtaPresentation({ ...live, claim_started_at: undefined }, at('2026-10-01T10:30:00Z'))).toMatchObject({
      kind: 'estimate',
      percent: 0,
      label: '~30m left',
    });
    expect(
      claimEtaPresentation(
        { ...live, claim_lease_expires_at: '2026-10-09T00:00:00Z', claim_eta_at: '2026-10-04T10:00:00Z' },
        at('2026-10-01T10:00:00Z'),
      ),
    ).toMatchObject({ label: '~3d left' });
  });

  it('ticks the local countdown at most every 30s and just after the nearest ETA, never for overruns', () => {
    const now = at('2026-10-01T10:59:50Z');
    expect(nextClaimEtaTick([], now)).toBeUndefined();
    expect(nextClaimEtaTick([{ ...live, claim_eta_at: undefined }], now)).toBeUndefined();
    expect(nextClaimEtaTick([live], at('2026-10-01T10:00:00Z'))).toBe(30_000);
    expect(nextClaimEtaTick([live], now)).toBe(10_025);
    expect(nextClaimEtaTick([live], at('2026-10-01T11:00:01Z'))).toBeUndefined();
    expect(nextClaimEtaTick([live, { ...live, claim_eta_at: '2026-10-01T10:59:55Z' }], now)).toBe(5_025);
  });
});
