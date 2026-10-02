import './corrupt-ticket-row.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Bot, FileWarning, FolderOpen, RefreshCw } from 'lucide';

import type { CorruptTicket } from '../api';
import { NAVIGATION_AND_TABS_ACTIONS } from '../interaction-attrs/navigation-and-tabs';
import { inspectorToggle, SidebarPane, type SidebarPanelParts } from './sidebar-panel';

const filename = (path: string) => path.split(/[\\/]/).filter(Boolean).at(-1);

export function corruptTicketIdentity(ticket: CorruptTicket) {
  return ticket.slug ?? ticket.id ?? filename(ticket.path) ?? 'Unreadable ticket';
}

export interface CorruptTicketRecoveryState {
  pending?: 'reveal' | 'repair';
  message?: string;
  failed?: boolean;
}
export const corruptTicketKey = (ticket: CorruptTicket) => `${ticket.store}:${ticket.path}`;
export function revealFileLabel(platform = typeof navigator === 'undefined' ? '' : navigator.userAgent) {
  if (/mac/i.test(platform)) return 'Reveal in Finder';
  if (/win/i.test(platform)) return 'Show in File Explorer';
  return 'Show file location';
}

function RecoveryActions({ ticket, recovery }: { ticket: CorruptTicket; recovery?: CorruptTicketRecoveryState }) {
  const key = corruptTicketKey(ticket);
  return (
    <>
      <button
        type="button"
        {...NAVIGATION_AND_TABS_ACTIONS.revealCorruptTicket.attrs}
        data-corrupt-key={key}
        disabled={Boolean(recovery?.pending)}
      >
        <LucideIcon icon={FolderOpen} name="folder-open" />
        {recovery?.pending === 'reveal' ? 'Showing…' : revealFileLabel()}
      </button>
      {ticket.error_code !== 'upgrade_required' && (
        <button
          type="button"
          {...NAVIGATION_AND_TABS_ACTIONS.repairCorruptTicket.attrs}
          data-corrupt-key={key}
          disabled={Boolean(recovery?.pending)}
        >
          <LucideIcon icon={Bot} name="bot" />
          {recovery?.pending === 'repair' ? 'Queuing…' : 'Attempt AI repair'}
        </button>
      )}
    </>
  );
}

/** A selectable diagnostic row whose full recovery flow opens in the inspector. */
export function CorruptTicketRow({
  ticket,
  recovery,
  selected = false,
}: {
  ticket: CorruptTicket;
  recovery?: CorruptTicketRecoveryState;
  selected?: boolean;
}) {
  const identity = corruptTicketIdentity(ticket);
  const upgradeRequired = ticket.error_code === 'upgrade_required';
  const key = corruptTicketKey(ticket);
  return (
    <article
      class={`ticket-list-row ticket-list-row--list corrupt-ticket-row${selected ? ' ticket-list-row--selected' : ''}`}
      data-component="corrupt-ticket-row"
      data-selected={String(selected)}
      role="group"
      aria-label={`Unreadable ticket ${identity}`}
    >
      <span class="corrupt-ticket-row__icon">
        <LucideIcon
          icon={upgradeRequired ? RefreshCw : FileWarning}
          name={upgradeRequired ? 'refresh-cw' : 'file-warning'}
        />
      </span>
      <div class="corrupt-ticket-row__content">
        <button
          type="button"
          class="corrupt-ticket-row__select"
          {...NAVIGATION_AND_TABS_ACTIONS.selectCorruptTicket.attrs}
          data-corrupt-key={key}
          aria-label={`Open recovery for ${identity}`}
        >
          <strong>{identity}</strong>
          <span>{upgradeRequired ? 'Hot Sheet 2 update required' : 'Ticket file could not be read'}</span>
        </button>
        {recovery?.message && !selected && (
          <p
            class={`corrupt-ticket-row__recovery${recovery.failed ? ' corrupt-ticket-row__recovery--failed' : ''}`}
            role={recovery.failed ? 'alert' : 'status'}
          >
            {recovery.message}
          </p>
        )}
      </div>
    </article>
  );
}

/**
 * The unreadable ticket's recovery panel parts for the Workbench's right rail (HS2-QQW6CT): the fixed
 * header names the file's problem, the content explains it and offers recovery.
 */
export function corruptTicketInspectorPanel({
  ticket,
  recovery,
}: {
  ticket: CorruptTicket;
  recovery?: CorruptTicketRecoveryState;
}): SidebarPanelParts {
  const identity = corruptTicketIdentity(ticket),
    upgradeRequired = ticket.error_code === 'upgrade_required';
  return {
    label: `Recovery for ${identity}`,
    toolbar: { label: 'Ticket recovery toolbar', dividerSides: '' },
    toggle: inspectorToggle(),
    header: (
      <header class="corrupt-ticket-inspector__header" data-component="corrupt-ticket-inspector-header">
        <LucideIcon
          icon={upgradeRequired ? RefreshCw : FileWarning}
          name={upgradeRequired ? 'refresh-cw' : 'file-warning'}
        />
        <div>
          <span>{upgradeRequired ? 'Hot Sheet 2 update required' : 'Unreadable ticket'}</span>
          <h1>{identity}</h1>
        </div>
      </header>
    ),
    content: (
      <div class="corrupt-ticket-inspector__body" data-component="corrupt-ticket-inspector">
        <section>
          <h2>{upgradeRequired ? 'Update required' : 'Ticket parsing error'}</h2>
          <p>{ticket.error}</p>
        </section>
        <section>
          <h2>Ticket file</h2>
          <code title={ticket.path}>{ticket.path}</code>
        </section>
        <div class="corrupt-ticket-inspector__actions">
          <RecoveryActions ticket={ticket} recovery={recovery} />
        </div>
        {recovery?.message && (
          <p
            class={`corrupt-ticket-inspector__recovery${recovery.failed ? ' corrupt-ticket-inspector__recovery--failed' : ''}`}
            role={recovery.failed ? 'alert' : 'status'}
          >
            {recovery.message}
          </p>
        )}
      </div>
    ),
    pane: {},
  };
}

export function CorruptTicketInspector({
  ticket,
  recovery,
  collapseControl = false,
}: {
  ticket: CorruptTicket;
  recovery?: CorruptTicketRecoveryState;
  collapseControl?: boolean;
}) {
  return (
    <SidebarPane
      parts={corruptTicketInspectorPanel({ ticket, recovery })}
      side="right"
      collapseControl={collapseControl}
    />
  );
}
