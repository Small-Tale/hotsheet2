import { describe, expect, it } from 'vitest';

import { isTicketActivelyWorkedOn, nextActiveTicketExpiry } from './active-ticket-work';

describe('active ticket work', () => {
  const now = Date.parse('2026-09-02T12:00:00Z');

  it('distinguishes a live renewable claim from started or previously claimed state', () => {
    expect(isTicketActivelyWorkedOn({}, now)).toBe(false);
    expect(isTicketActivelyWorkedOn({ claimed_by: 'codex' }, now)).toBe(false);
    expect(isTicketActivelyWorkedOn({ claimed_by: 'codex', claim_lease_expires_at: 'invalid' }, now)).toBe(false);
    expect(isTicketActivelyWorkedOn({ claimed_by: 'codex', claim_lease_expires_at: '2026-09-02T12:00:00Z' }, now)).toBe(false);
    expect(isTicketActivelyWorkedOn({ claimed_by: 'codex', claim_lease_expires_at: '2026-09-02T12:30:00Z' }, now)).toBe(true);
    expect(isTicketActivelyWorkedOn({ status: 'completed', claimed_by: 'codex', claim_lease_expires_at: '2026-09-02T12:30:00Z' }, now)).toBe(false);
  });

  it('schedules the nearest live expiry while ignoring released and stale claims', () => {
    expect(nextActiveTicketExpiry([
      {},
      { claimed_by: 'old', claim_lease_expires_at: '2026-09-02T11:00:00Z' },
      { claimed_by: 'later', claim_lease_expires_at: '2026-09-02T13:00:00Z' },
      { status: 'verified', claimed_by: 'legacy', claim_lease_expires_at: '2026-09-02T12:01:00Z' },
      { claimed_by: 'next', claim_lease_expires_at: '2026-09-02T12:05:00Z' },
    ], now)).toBe(Date.parse('2026-09-02T12:05:00Z'));
  });
});
