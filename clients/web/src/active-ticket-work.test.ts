import { describe, expect, it } from 'vitest';

import {
  applyKnownActiveTicketExpiries,
  isTicketActivelyWorkedOn,
  nextActiveTicketExpiry,
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
