import { describe, expect, it } from 'vitest';

import type { TicketRow } from '../api';
import { claimNotice } from './claim-clock';

const NOW = Date.parse('2026-11-24T12:00:00Z');
const row = (rest: Partial<TicketRow>): TicketRow => ({
  connection_id: 'local',
  native_id: '01JSYNTHETIC0000000000000A',
  qualified_id: 'local:01JSYNTHETIC0000000000000A',
  id: '01JSYNTHETIC0000000000000A',
  slug: 'HS2-CLAIMS',
  title: 'Claimed work',
  status: 'started',
  up_next: false,
  feedback_needed: false,
  tags: [],
  blocked_by: [],
  claim_count: 1,
  ...rest,
});
const live = { claimed_by: 'worker-a', claim_lease_expires_at: '2026-11-24T12:30:00Z' };

describe('claimNotice (HS2-ZR8B4N)', () => {
  it('walks exclusive, provisional, lost and released claims', () => {
    expect(claimNotice(row({}), NOW)).toBeUndefined();
    expect(claimNotice(row(live), NOW)).toMatchObject({ agentName: 'worker-a' });
    expect(claimNotice(row(live), NOW)?.standing).toBeUndefined();
    expect(claimNotice(row({ ...live, claim_standing: { kind: 'provisional' } }), NOW)?.standing).toEqual({
      kind: 'provisional',
    });
    // A provisional record without a live lease is not active work.
    expect(
      claimNotice(
        row({ ...live, claim_lease_expires_at: '2026-11-24T11:00:00Z', claim_standing: { kind: 'provisional' } }),
        NOW,
      ),
    ).toBeUndefined();
    // A loss names the session that lost it and the holder, live lease or not.
    const lost = {
      kind: 'lost',
      worker: 'worker-a',
      holder: 'worker-b',
      detected_at: '2026-11-24T11:59:00Z',
    } as const;
    for (const claim of [{ ...live, claimed_by: 'worker-b' }, {}])
      expect(claimNotice(row({ ...claim, claim_standing: lost }), NOW)).toEqual({
        agentName: 'worker-a',
        standing: { kind: 'lost', holder: 'worker-b' },
      });
    // Once the loser was told, the holder's claim reads as theirs.
    expect(claimNotice(row({ ...live, claimed_by: 'worker-b' }), NOW)).toMatchObject({ agentName: 'worker-b' });
  });
});
