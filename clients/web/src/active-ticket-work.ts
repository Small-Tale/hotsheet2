import type { TicketRow } from './api';

export function isTicketActivelyWorkedOn(ticket: Pick<TicketRow, 'claimed_by' | 'claim_lease_expires_at'>, now = Date.now()): boolean {
  if (!ticket.claimed_by || !ticket.claim_lease_expires_at) return false;
  const expiry = Date.parse(ticket.claim_lease_expires_at);
  return Number.isFinite(expiry) && expiry > now;
}

export function nextActiveTicketExpiry(tickets: Array<Pick<TicketRow, 'claimed_by' | 'claim_lease_expires_at'>>, now = Date.now()): number | undefined {
  const expiries = tickets
    .filter(ticket => isTicketActivelyWorkedOn(ticket, now))
    .map(ticket => Date.parse(ticket.claim_lease_expires_at!));
  return expiries.length ? Math.min(...expiries) : undefined;
}
