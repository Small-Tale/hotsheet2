import type { TicketRow } from './api';

type ClaimState = Pick<TicketRow, 'claimed_by' | 'claim_lease_expires_at'> & Pick<Partial<TicketRow>, 'status'>;

export function isTicketActivelyWorkedOn(ticket: ClaimState, now = Date.now()): boolean {
  if (ticket.status && ticket.status !== 'not_started' && ticket.status !== 'started') return false;
  if (!ticket.claimed_by || !ticket.claim_lease_expires_at) return false;
  const expiry = Date.parse(ticket.claim_lease_expires_at);
  return Number.isFinite(expiry) && expiry > now;
}

export function nextActiveTicketExpiry(tickets: ClaimState[], now = Date.now()): number | undefined {
  const expiries = tickets
    .filter(ticket => isTicketActivelyWorkedOn(ticket, now))
    .map(ticket => Date.parse(ticket.claim_lease_expires_at!));
  return expiries.length ? Math.min(...expiries) : undefined;
}
