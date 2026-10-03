import type { CorruptTicket } from '../api';
import { corruptTicketInspectorPanel, type CorruptTicketRecoveryState } from './corrupt-ticket-row';
import { notificationInspectorPanel } from './notification-inspector';
import type { SidebarPanelParts } from './sidebar-panel';
import { ticketInspectorPanel, type TicketInspectorProps, ticketInspectorSkeletonPanel } from './ticket-inspector';
import { ticketInspectorPlaceholderPanel } from './ticket-inspector-placeholder';

/**
 * The application shell's right-rail surfaces as Workbench panel parts (HS2-QQW6CT). Each routes to
 * one surface's parts, so every surface shares the standard toggle the Workbench relocates.
 */
export function inspectorPanel(props: TicketInspectorProps): SidebarPanelParts {
  return ticketInspectorPanel(props);
}

export function inspectorPlaceholderPanel({ selectionCount }: { selectionCount: number }): SidebarPanelParts {
  return ticketInspectorPlaceholderPanel({ selectionCount });
}

export function inspectorSkeletonPanel({ slug }: { slug?: string }): SidebarPanelParts {
  return ticketInspectorSkeletonPanel({ slug });
}

export function notificationInspectorSurfacePanel(): SidebarPanelParts {
  return notificationInspectorPanel();
}

export function corruptInspectorPanel({
  ticket,
  recovery,
  selectionCount,
}: {
  ticket?: CorruptTicket;
  recovery?: CorruptTicketRecoveryState;
  selectionCount: number;
}): SidebarPanelParts {
  return ticket ? corruptTicketInspectorPanel({ ticket, recovery }) : inspectorPlaceholderPanel({ selectionCount });
}
