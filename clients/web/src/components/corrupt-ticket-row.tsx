import './corrupt-ticket-row.css';

import { FileWarning } from 'lucide';

import type { CorruptTicket } from '../api';
import { LucideIcon } from './lucide-icon';

const filename = (path: string) => path.split(/[\\/]/).filter(Boolean).at(-1);

export function corruptTicketIdentity(ticket: CorruptTicket) {
  return ticket.slug ?? ticket.id ?? filename(ticket.path) ?? 'Unreadable ticket';
}

/** A deliberately non-interactive placeholder for a ticket that could not be parsed. */
export function CorruptTicketRow({ ticket }: { ticket: CorruptTicket }) {
  const identity = corruptTicketIdentity(ticket);
  return <article class="corrupt-ticket-row" data-component="corrupt-ticket-row" role="option" aria-disabled="true" aria-selected="false">
    <LucideIcon icon={FileWarning} name="file-warning" class="corrupt-ticket-row__icon" />
    <div class="corrupt-ticket-row__content">
      <strong>{identity}</strong>
      <span>Ticket file could not be read</span>
      <code title={ticket.path}>{ticket.path}</code>
      <p>{ticket.error}</p>
    </div>
  </article>;
}
