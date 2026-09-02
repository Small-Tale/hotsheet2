import './corrupt-ticket-row.css';

import { Bot, FileWarning, FolderOpen, PanelRightClose, RefreshCw } from 'lucide';

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

function RecoveryActions({ ticket, recovery }: { ticket: CorruptTicket;recovery?:CorruptTicketRecoveryState }) {
  const key = corruptTicketKey(ticket);
  return <>
    <button type="button" data-action="reveal-corrupt-ticket" data-corrupt-key={key} disabled={Boolean(recovery?.pending)}><LucideIcon icon={FolderOpen} name="folder-open" />{recovery?.pending==='reveal'?'Showing…':revealFileLabel()}</button>
    {ticket.error_code!=='upgrade_required'&&<button type="button" data-action="repair-corrupt-ticket" data-corrupt-key={key} disabled={Boolean(recovery?.pending)}><LucideIcon icon={Bot} name="bot" />{recovery?.pending==='repair'?'Queuing…':'Attempt AI repair'}</button>}
  </>;
}

/** A selectable diagnostic row whose full recovery flow opens in the inspector. */
export function CorruptTicketRow({ ticket, recovery, selected = false }: { ticket: CorruptTicket;recovery?:CorruptTicketRecoveryState;selected?:boolean }) {
  const identity = corruptTicketIdentity(ticket);
  const upgradeRequired = ticket.error_code === 'upgrade_required';
  const key = corruptTicketKey(ticket);
  return <article class="corrupt-ticket-row" data-component="corrupt-ticket-row" data-selected={String(selected)} role="group" aria-label={`Unreadable ticket ${identity}`}>
    <LucideIcon icon={upgradeRequired ? RefreshCw : FileWarning} name={upgradeRequired ? 'refresh-cw' : 'file-warning'} class="corrupt-ticket-row__icon" />
    <div class="corrupt-ticket-row__content">
      <button type="button" class="corrupt-ticket-row__select" data-action="select-corrupt-ticket" data-corrupt-key={key} aria-label={`Open recovery for ${identity}`}>
        <strong>{identity}</strong>
        <span>{upgradeRequired ? 'Hot Sheet 2 update required' : 'Ticket file could not be read'}</span>
      </button>
      {recovery?.message&&!selected&&<p class={`corrupt-ticket-row__recovery${recovery.failed?' corrupt-ticket-row__recovery--failed':''}`} role={recovery.failed?'alert':'status'}>{recovery.message}</p>}
    </div>
  </article>;
}

export function CorruptTicketInspector({ ticket, recovery }: { ticket: CorruptTicket;recovery?:CorruptTicketRecoveryState }) {
  const identity=corruptTicketIdentity(ticket),upgradeRequired=ticket.error_code==='upgrade_required';
  return <section class="corrupt-ticket-inspector" data-component="corrupt-ticket-inspector" aria-label={`Recovery for ${identity}`}>
    <header class="corrupt-ticket-inspector__header">
      <LucideIcon icon={upgradeRequired?RefreshCw:FileWarning} name={upgradeRequired?'refresh-cw':'file-warning'} />
      <div><span>{upgradeRequired?'Hot Sheet 2 update required':'Unreadable ticket'}</span><h1>{identity}</h1></div>
      <button type="button" data-action="close-ticket-inspector" aria-label="Hide inspector" title="Hide inspector"><LucideIcon icon={PanelRightClose} name="panel-right-close" /></button>
    </header>
    <div class="corrupt-ticket-inspector__body">
      <section><h2>{upgradeRequired?'Update required':'Ticket parsing error'}</h2><p>{ticket.error}</p></section>
      <section><h2>Ticket file</h2><code title={ticket.path}>{ticket.path}</code></section>
      <div class="corrupt-ticket-inspector__actions"><RecoveryActions ticket={ticket} recovery={recovery} /></div>
      {recovery?.message&&<p class={`corrupt-ticket-inspector__recovery${recovery.failed?' corrupt-ticket-inspector__recovery--failed':''}`} role={recovery.failed?'alert':'status'}>{recovery.message}</p>}
    </div>
  </section>;
}
