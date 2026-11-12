import type { TicketRow as WireTicketRow } from './api';

/** The holder name shown for a claim from a separate Hot Sheet server (HS2-DQJPW0). */
export const REMOTE_CLAIM_HOLDER = 'Remote server';

/**
 * Who holds a ticket's live claim, for row and inspector labels. A claim made by a separate Hot Sheet
 * server and seen through the synced store names that server rather than its worker, whose name is
 * local to that machine (HS2-DQJPW0). A claim whose origin is unknown (written before hosts were
 * recorded) keeps its worker name.
 */
export function claimHolderName(ticket: Pick<WireTicketRow, 'worker_label' | 'claimed_by' | 'claim_origin'>): string {
  if (ticket.claim_origin === 'remote') return REMOTE_CLAIM_HOLDER;
  return ticket.worker_label || ticket.claimed_by || 'AI';
}
