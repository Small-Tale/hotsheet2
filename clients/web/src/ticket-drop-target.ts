import { signal } from 'kerfjs';

/**
 * The sidebar view, board column, or new-ticket target a ticket drag is over (HS2-G8EK08). Each
 * target renders its drop highlight from this signal and names itself with `data-ticket-drop-key`.
 * The drag handler used to write the highlight onto the target's node, and any render of that
 * region mid-drag, such as the sidebar after a view switch, morphed it away until the next
 * dragover. Project tabs keep their own `projectTabDropTarget` (HS2-6KNWYT).
 */
export const ticketDropTarget = signal<string | undefined>(undefined);

/** The `data-ticket-drop-key` of a sidebar view row. */
export const viewTicketDropKey = (viewId: string) => `view:${viewId}`;
/** The `data-ticket-drop-key` of a board column. */
export const columnTicketDropKey = (columnId: string) => `column:${columnId}`;
/** The `data-ticket-drop-key` of the New ticket launcher and of the expanded composer form. */
export const NEW_TICKET_LAUNCHER_DROP_KEY = 'new-ticket:launcher';
export const NEW_TICKET_COMPOSER_DROP_KEY = 'new-ticket:composer';

/** Mark `key` as the drag's target, writing the signal only when the target changes. */
export function markTicketDropTarget(key: string): void {
  if (ticketDropTarget.value !== key) ticketDropTarget.value = key;
}

/** Clear the drop target, or only `key` when given, so a late leave cannot clear a newer target. */
export function clearTicketDropTarget(key?: string): void {
  if (ticketDropTarget.value === undefined) return;
  if (key !== undefined && ticketDropTarget.value !== key) return;
  ticketDropTarget.value = undefined;
}

/** Whether the target named `key` is the drag's current drop target. */
export const isTicketDropTarget = (key: string) => ticketDropTarget.value === key;
