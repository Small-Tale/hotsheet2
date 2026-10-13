import '@awesome.me/webawesome/dist/components/button/button.js';
import './corrupt-ticket-row.css';

import { uiColor } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { Text } from '@kerfjs/ui/text';
import { Bot, FileWarning, FolderOpen, RefreshCw } from 'lucide';

import type { CorruptTicket } from '../api';
import { hostActionsAvailableHere, hostOnlyDisabledReason } from '../host-only-actions';
import { NAVIGATION_AND_TABS_ACTIONS } from '../interaction-attrs/navigation-and-tabs';
import { inspectorToggle, SidebarPane, type SidebarPanelParts } from './sidebar-panel';
import { type TicketRowListEdge, ticketRowShellAttributes } from './ticket-row';

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

function RecoveryActions({
  ticket,
  recovery,
  hostActionsAvailable,
}: {
  ticket: CorruptTicket;
  recovery?: CorruptTicketRecoveryState;
  hostActionsAvailable: boolean;
}) {
  const key = corruptTicketKey(ticket);
  return (
    <>
      <wa-button
        variant="danger"
        appearance="outlined"
        size="small"
        type="button"
        {...NAVIGATION_AND_TABS_ACTIONS.revealCorruptTicket.attrs}
        data-corrupt-key={key}
        disabled={Boolean(recovery?.pending) || !hostActionsAvailable}
        title={hostOnlyDisabledReason('reveal-file', hostActionsAvailable)}
      >
        <LucideIcon slot="start" icon={FolderOpen} name="folder-open" size="s" />
        {recovery?.pending === 'reveal' ? 'Showing…' : revealFileLabel()}
      </wa-button>
      {ticket.error_code !== 'upgrade_required' && (
        <wa-button
          variant="danger"
          appearance="outlined"
          size="small"
          type="button"
          {...NAVIGATION_AND_TABS_ACTIONS.repairCorruptTicket.attrs}
          data-corrupt-key={key}
          disabled={Boolean(recovery?.pending)}
        >
          <LucideIcon slot="start" icon={Bot} name="bot" size="s" />
          {recovery?.pending === 'repair' ? 'Queuing…' : 'Attempt AI repair'}
        </wa-button>
      )}
    </>
  );
}

/** A selectable diagnostic row whose full recovery flow opens in the inspector. */
export function CorruptTicketRow({
  ticket,
  recovery,
  selected = false,
  listEdge,
}: {
  ticket: CorruptTicket;
  recovery?: CorruptTicketRecoveryState;
  selected?: boolean;
  /** Rounds the row's corners at the edge of its clipping list section, as TicketRow does. */
  listEdge?: TicketRowListEdge;
}) {
  const identity = corruptTicketIdentity(ticket);
  const upgradeRequired = ticket.error_code === 'upgrade_required';
  const key = corruptTicketKey(ticket);
  return (
    <article
      class="ticket-row-shell corrupt-ticket-row"
      data-component="corrupt-ticket-row"
      {...ticketRowShellAttributes({ selected, listEdge, tone: 'danger' })}
      role="group"
      aria-label={`Unreadable ticket ${identity}`}
    >
      <span class="corrupt-ticket-row__icon">
        <LucideIcon
          icon={upgradeRequired ? RefreshCw : FileWarning}
          name={upgradeRequired ? 'refresh-cw' : 'file-warning'}
          size={26.4}
          color={uiColor('danger-on-quiet')}
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
            class="corrupt-ticket-row__recovery"
            data-failed={String(Boolean(recovery.failed))}
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
  hostActionsAvailable = hostActionsAvailableHere(),
}: {
  ticket: CorruptTicket;
  recovery?: CorruptTicketRecoveryState;
  /** False on a remote client: host-only controls render disabled with an explanation (HS2-TYS5SS). */
  hostActionsAvailable?: boolean;
}): SidebarPanelParts {
  const identity = corruptTicketIdentity(ticket),
    upgradeRequired = ticket.error_code === 'upgrade_required';
  return {
    label: `Recovery for ${identity}`,
    toolbar: { label: 'Ticket recovery toolbar', dividerSides: '' },
    toggle: inspectorToggle(),
    header: (
      <header class="corrupt-ticket-inspector__header" data-component="corrupt-ticket-inspector-header">
        <Row gap="xs" vAlign="top">
          <LucideIcon
            icon={upgradeRequired ? RefreshCw : FileWarning}
            name={upgradeRequired ? 'refresh-cw' : 'file-warning'}
            size="s"
            color={uiColor('danger-on-quiet')}
          />
          <List gap="2xs" flex>
            <Text variant="span" size="compact" tone="danger">
              <strong>{upgradeRequired ? 'Hot Sheet 2 update required' : 'Unreadable ticket'}</strong>
            </Text>
            <Text role="heading" aria-level={1} tone="danger" flush lineHeight="tight" wrap="anywhere">
              <strong>{identity}</strong>
            </Text>
          </List>
        </Row>
      </header>
    ),
    content: (
      <div class="corrupt-ticket-inspector__body" data-component="corrupt-ticket-inspector">
        <section>
          <ListHeader label={upgradeRequired ? 'Update required' : 'Ticket parsing error'} />
          <Text tone="danger" controlMargins="rl" wrap="anywhere">
            {ticket.error}
          </Text>
        </section>
        <section>
          <ListHeader label="Ticket file" />
          <code title={ticket.path}>{ticket.path}</code>
        </section>
        <div class="corrupt-ticket-inspector__actions">
          <List gap="xs">
            <RecoveryActions ticket={ticket} recovery={recovery} hostActionsAvailable={hostActionsAvailable} />
          </List>
        </div>
        {recovery?.message && (
          <StateBanner
            tone={recovery.failed ? 'danger' : 'success'}
            urgency={recovery.failed ? 'alert' : 'status'}
            title={recovery.message}
          />
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
