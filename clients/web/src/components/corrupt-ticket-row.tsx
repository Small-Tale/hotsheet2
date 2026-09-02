import './corrupt-ticket-row.css';

import { Bot, FileWarning, FolderOpen, RefreshCw } from 'lucide';

import type { CorruptTicket } from '../api';
import { LucideIcon } from './lucide-icon';

const filename = (path: string) => path.split(/[\\/]/).filter(Boolean).at(-1);

export function corruptTicketIdentity(ticket: CorruptTicket) {
  return ticket.slug ?? ticket.id ?? filename(ticket.path) ?? 'Unreadable ticket';
}

export interface CorruptTicketRecoveryState { pending?:'reveal'|'repair';message?:string;failed?:boolean }
export const corruptTicketKey = (ticket: CorruptTicket) => `${ticket.store}:${ticket.path}`;
export function revealFileLabel(platform = typeof navigator === 'undefined' ? '' : navigator.userAgent) {
  if (/mac/i.test(platform)) return 'Reveal in Finder';
  if (/win/i.test(platform)) return 'Show in File Explorer';
  return 'Show file location';
}

/** A diagnostic row with safe recovery actions; it is not a selectable ticket. */
export function CorruptTicketRow({ ticket, recovery }: { ticket: CorruptTicket;recovery?:CorruptTicketRecoveryState }) {
  const identity = corruptTicketIdentity(ticket);
  const upgradeRequired = ticket.error_code === 'upgrade_required';
  const key = corruptTicketKey(ticket);
  return <article class="corrupt-ticket-row" data-component="corrupt-ticket-row" role="group" aria-label={`Unreadable ticket ${identity}`}>
    <LucideIcon icon={upgradeRequired ? RefreshCw : FileWarning} name={upgradeRequired ? 'refresh-cw' : 'file-warning'} class="corrupt-ticket-row__icon" />
    <div class="corrupt-ticket-row__content">
      <strong>{identity}</strong>
      <span>{upgradeRequired ? 'Hot Sheet 2 update required' : 'Ticket file could not be read'}</span>
      <code title={ticket.path}>{ticket.path}</code>
      <p>{ticket.error}</p>
      <div class="corrupt-ticket-row__actions">
        <button type="button" data-action="reveal-corrupt-ticket" data-corrupt-key={key} disabled={Boolean(recovery?.pending)}><LucideIcon icon={FolderOpen} name="folder-open" />{recovery?.pending==='reveal'?'Showing…':revealFileLabel()}</button>
        {!upgradeRequired&&<button type="button" data-action="repair-corrupt-ticket" data-corrupt-key={key} disabled={Boolean(recovery?.pending)}><LucideIcon icon={Bot} name="bot" />{recovery?.pending==='repair'?'Queuing…':'Queue AI repair'}</button>}
      </div>
      {recovery?.message&&<p class={`corrupt-ticket-row__recovery${recovery.failed?' corrupt-ticket-row__recovery--failed':''}`} role={recovery.failed?'alert':'status'}>{recovery.message}</p>}
    </div>
  </article>;
}
