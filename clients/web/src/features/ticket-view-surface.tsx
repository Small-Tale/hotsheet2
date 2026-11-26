import type { Signal } from 'kerfjs';

import { type Capabilities, type CustomView, type TicketRow as WireTicketRow } from '../api';
import { TicketContextMenuSurface, type TicketContextMenuSurfaceProps } from '../components/reader-overlay-surfaces';
import { type TicketStatus } from '../components/status-badge';
import { type TicketCloseDialogState } from '../components/ticket-close-dialog';
import { type TicketPriority } from '../components/ticket-row';
import { type TicketViewActionSpec } from '../components/workspace-header';
import { featureOn } from '../feature-areas';
import {
  canBulkUpdate,
  ticketGroupDisabledReason,
  ticketGroupMenuModel,
  ticketUpNextDisabledReason,
} from '../ticket-bulk-operations';
import { isTicketDropTarget, NEW_TICKET_LAUNCHER_DROP_KEY } from '../ticket-drop-target';
import { ticketGroupKey, type TicketGroupOption, ticketGroupValue } from '../ticket-groups';
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
  isGitSource: (connectionId: string) => boolean;
  /** A connection's ticket source display name, for the Up Next disabled reason (HS2-Y32T8A). */
  sourceNameFor?: (connectionId: string) => string | undefined;
  customViewFor: (view: TicketView, projectId?: string) => CustomView | undefined;
  priority: (value?: string) => TicketPriority;
  selectedTicketSlugs: Signal<string[]>;
  status: (value?: string) => TicketStatus;
  ticketCloseDialog: Signal<TicketCloseDialogState | undefined>;
  ticketContextMenu: Signal<
    { x: number; y: number; ticketSlug: string; hideUpNext?: boolean; group?: string; groupScope?: string } | undefined
  >;
  tickets: Signal<WireTicketRow[]>;
  /** Every known group in the project, for the Add to Group submenu (HS2-TX17CM, HS2-9KZSD4). */
  ticketGroups: () => readonly TicketGroupOption[];
  /** A stored group value's display name, and whether same-named duplicates can merge into it. */
  ticketGroupName: (group: string) => string;
  ticketGroupMergeable: (group: string) => boolean;
  /** An Add to Group › Automatic request is running (HS2-4C32A0). */
  automaticGroupBusy: () => boolean;
}

/**
 * Ticket view titles, the view's primary action spec, and the ticket context-menu surface.
 * Extracted from the application runtime without behavior changes (HS2-K0VC2C).
 */
export function createTicketViewSurface(dependencies: TicketViewSurfaceDependencies) {
  const {
    canStageNewTicketAttachments,
    capabilitiesFor,
    isGitSource,
    sourceNameFor,
    customViewFor,
    priority,
    selectedTicketSlugs,
    status,
    ticketCloseDialog,
    ticketContextMenu,
    tickets,
    ticketGroups,
    ticketGroupName,
    ticketGroupMergeable,
    automaticGroupBusy,
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
        ? {
            kind: 'new-ticket',
            attachmentsEnabled: canStageNewTicketAttachments(),
            label,
            size,
            dropTarget: isTicketDropTarget(NEW_TICKET_LAUNCHER_DROP_KEY),
          }
        : undefined;
  }
  // The Automatic choice belongs to the Automatic ticket grouping feature (HS2-3GJZGG).
  const withAutomaticBusy = (model: ReturnType<typeof ticketGroupMenuModel>) =>
    model && {
      ...model,
      automatic: featureOn('auto-grouping'),
      ...(automaticGroupBusy() ? { automaticBusy: true } : {}),
    };
  function ticketContextMenuSurface() {
    const menu = ticketContextMenu.value;
    let props: TicketContextMenuSurfaceProps['menu'];
    if (menu?.group) {
      const members = tickets.value.filter((item) => {
        const value = ticketGroupValue(item);
        return value !== undefined && ticketGroupKey(value) === ticketGroupKey(menu.group!);
      });
      return (
        <TicketContextMenuSurface
          groupMenu={{
            x: menu.x,
            y: menu.y,
            group: menu.group,
            scope: menu.groupScope,
            name: ticketGroupName(menu.group),
            mergeable: ticketGroupMergeable(menu.group),
            disabledReason: ticketGroupDisabledReason(members, capabilitiesFor),
          }}
          closeDialog={ticketCloseDialog.value}
        />
      );
    }
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
        upNextDisabledReason: ticketUpNextDisabledReason(selected, capabilitiesFor, sourceNameFor),
        hideUpNext: menu.hideUpNext,
        verifyAction: allCompleted && (capabilities?.update ?? true),
        notWorkingAction: onlyCompleted && (capabilities?.not_working_report ?? false) && featureOn('not-working'),
        reopenAction: reopenable && (capabilities?.not_working_report ?? false),
        closeAction: slugs.length === 1 && Boolean(capabilities?.close && capabilities.close_reasons),
        selectionCount: slugs.length,
        canBulkUpdate: canBulkUpdate(selected, capabilitiesFor),
        canCreateFollowUp: Boolean(ticket && isGitSource(ticket.connection_id)),
        allInBacklog: selected.length > 0 && selected.every((item) => item.status === 'backlog'),
        allInArchive: selected.length > 0 && selected.every(isArchivedTicket),
        allInTrash: selected.length > 0 && selected.every(isTrashedTicket),
        // Ticket groups and saved views is an optional feature area (HS2-3GJZGG).
        group: featureOn('ticket-groups')
          ? withAutomaticBusy(ticketGroupMenuModel(selected, ticketGroups(), capabilitiesFor))
          : undefined,
      };
    }
    return <TicketContextMenuSurface menu={props} closeDialog={ticketCloseDialog.value} />;
  }
  return { ticketViewTitle, ticketViewActionSpec, ticketContextMenuSurface };
}
