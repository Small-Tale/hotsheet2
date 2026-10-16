import type { Signal } from 'kerfjs';

import { type Capabilities, type CustomView, type TicketRow as WireTicketRow } from '../api';
import { TicketContextMenuSurface, type TicketContextMenuSurfaceProps } from '../components/reader-overlay-surfaces';
import { type TicketStatus } from '../components/status-badge';
import { type TicketCloseDialogState } from '../components/ticket-close-dialog';
import { type TicketPriority } from '../components/ticket-row';
import { type TicketViewActionSpec } from '../components/workspace-header';
import { canBulkUpdate } from '../ticket-bulk-operations';
import {
  ALL_TICKETS_SCOPE,
  isArchivedTicket,
  isTrashedTicket,
  type TicketScope,
  type TicketView,
} from '../ticket-views';

/** Live application bindings createTicketViewSurface reads (HS2-K0VC2C). */
export interface TicketViewSurfaceDependencies {
  canStageNewTicketAttachments: () => boolean;
  capabilitiesFor: (connectionId: string) => Capabilities;
  customViewFor: (view: TicketView, projectId?: string) => CustomView | undefined;
  priority: (value?: string) => TicketPriority;
  selectedTicketSlugs: Signal<string[]>;
  status: (value?: string) => TicketStatus;
  ticketCloseDialog: Signal<TicketCloseDialogState | undefined>;
  ticketContextMenu: Signal<{ x: number; y: number; ticketSlug: string; hideUpNext?: boolean } | undefined>;
  tickets: Signal<WireTicketRow[]>;
}

/**
 * Ticket view titles, the view's primary action spec, and the ticket context-menu surface.
 * Extracted from the application runtime without behavior changes (HS2-K0VC2C).
 */
export function createTicketViewSurface(dependencies: TicketViewSurfaceDependencies) {
  const {
    canStageNewTicketAttachments,
    capabilitiesFor,
    customViewFor,
    priority,
    selectedTicketSlugs,
    status,
    ticketCloseDialog,
    ticketContextMenu,
    tickets,
  } = dependencies;
  function ticketViewTitle(view: TicketScope) {
    if (view === ALL_TICKETS_SCOPE) return 'All Tickets';
    return view === 'archive'
      ? 'Archive'
      : view === 'trash'
        ? 'Trash'
        : view === 'backlog'
          ? 'Backlog'
          : view === 'errors'
            ? 'Ticket errors'
            : (customViewFor(view)?.name ?? 'Queue');
  }
  function ticketViewActionSpec(
    view: TicketView,
    canCreate: boolean,
    label?: string,
    size?: 'default' | 'compact',
  ): TicketViewActionSpec | undefined {
    return view === 'trash'
      ? { kind: 'empty-trash' }
      : canCreate
        ? { kind: 'new-ticket', attachmentsEnabled: canStageNewTicketAttachments(), label, size }
        : undefined;
  }
  function ticketContextMenuSurface() {
    const menu = ticketContextMenu.value;
    let props: TicketContextMenuSurfaceProps['menu'];
    if (menu) {
      const ticket = tickets.value.find((item) => item.slug === menu.ticketSlug),
        slugs = selectedTicketSlugs.value.length ? selectedTicketSlugs.value : [menu.ticketSlug],
        selected = tickets.value.filter((item) => slugs.includes(item.slug)),
        onlyCompleted = slugs.length === 1 && ticket?.status === 'completed',
        reopenable = slugs.length === 1 && (ticket?.status === 'verified' || ticket?.status === 'archive'),
        allCompleted =
          slugs.length > 0 &&
          slugs.every((slug) => tickets.value.find((item) => item.slug === slug)?.status === 'completed'),
        capabilities = ticket ? capabilitiesFor(ticket.connection_id) : undefined;
      props = {
        x: menu.x,
        y: menu.y,
        category: ticket?.category,
        priority: priority(ticket?.priority),
        status: status(ticket?.status),
        upNextEligible: slugs.every((slug) => {
          const value = tickets.value.find((item) => item.slug === slug)?.status;
          return value === 'not_started' || value === 'started';
        }),
        hideUpNext: menu.hideUpNext,
        verifyAction: allCompleted && (capabilities?.update ?? true),
        notWorkingAction: onlyCompleted && (capabilities?.not_working_report ?? false),
        reopenAction: reopenable && (capabilities?.not_working_report ?? false),
        closeAction: slugs.length === 1 && Boolean(capabilities?.close && capabilities.close_reasons),
        selectionCount: slugs.length,
        canBulkUpdate: canBulkUpdate(selected, capabilitiesFor),
        allInBacklog: selected.length > 0 && selected.every((item) => item.status === 'backlog'),
        allInArchive: selected.length > 0 && selected.every(isArchivedTicket),
        allInTrash: selected.length > 0 && selected.every(isTrashedTicket),
      };
    }
    return <TicketContextMenuSurface menu={props} closeDialog={ticketCloseDialog.value} />;
  }
  return { ticketViewTitle, ticketViewActionSpec, ticketContextMenuSurface };
}
