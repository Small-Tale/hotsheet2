import type { CheckoutTicketCounts, TicketRow } from './api';
import { isUpNextTicket } from './ticket-views';

type ClaimState = Pick<TicketRow, 'claimed_by' | 'claim_lease_expires_at'> & Pick<Partial<TicketRow>, 'status'>;

export function isTicketActivelyWorkedOn(ticket: ClaimState, now = Date.now()): boolean {
  if (ticket.status && ticket.status !== 'not_started' && ticket.status !== 'started') return false;
  if (!ticket.claimed_by || !ticket.claim_lease_expires_at) return false;
  const expiry = Date.parse(ticket.claim_lease_expires_at);
  return Number.isFinite(expiry) && expiry > now;
}

export function nextActiveTicketExpiry(tickets: ClaimState[], now = Date.now()): number | undefined {
  const expiries = tickets
    .filter((ticket) => isTicketActivelyWorkedOn(ticket, now))
    .map((ticket) => Date.parse(ticket.claim_lease_expires_at!));
  return expiries.length ? Math.min(...expiries) : undefined;
}

/** Longest delay `setTimeout` honors; larger values wrap to 0 and fire immediately. */
const MAX_TIMER_DELAY_MS = 2_147_483_647;

/** Delay until a claim expiry check should wake, clamped so a far-future lease (beyond ~24.8 days) cannot wrap
 * `setTimeout` to an immediate wake-up and spin a tight refresh loop (HS2-43Z35K). A clamped wake-up must
 * re-check the expiry before treating the lease as expired. */
export function claimExpiryWakeDelay(nextExpiry: number, now = Date.now()): number {
  return Math.min(Math.max(1, nextExpiry - now + 25), MAX_TIMER_DELAY_MS);
}

export function projectTabTicketState(
  tickets: readonly TicketRow[],
  now = Date.now(),
): { upNextCount: number; activeTicketCount: number } {
  let upNextCount = 0;
  let activeTicketCount = 0;
  for (const ticket of tickets) {
    if (isUpNextTicket(ticket)) upNextCount += 1;
    if (isTicketActivelyWorkedOn(ticket, now)) activeTicketCount += 1;
  }
  return { upNextCount, activeTicketCount };
}

/** Keep an authoritative aggregate honest when known cached claims expire without a server event. */
export function applyKnownActiveTicketExpiries(
  counts: CheckoutTicketCounts,
  tickets: readonly TicketRow[],
  before: number,
  now: number,
): CheckoutTicketCounts {
  const previous = projectTabTicketState(tickets, before).activeTicketCount,
    current = projectTabTicketState(tickets, now).activeTicketCount;
  return current === previous ? counts : { ...counts, active: Math.max(0, counts.active + current - previous) };
}
