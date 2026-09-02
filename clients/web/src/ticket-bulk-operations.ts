import type { Capabilities, TicketRow } from './api';
import type { TicketPatch } from './ticket-operations';

export type BulkTicketAction =
  | { kind: 'field'; field: 'category' | 'priority' | 'status'; value: string }
  | { kind: 'add-tag'; tag: string }
  | { kind: 'remove-tag'; tag: string }
  | { kind: 'delete' };

/** Bulk editing is only offered when every selected ticket's provider can update it. */
export function canBulkUpdate(
  tickets: readonly TicketRow[],
  capabilitiesFor: (connectionId: string) => Capabilities | undefined,
): boolean {
  return tickets.length > 0 && tickets.every(ticket => capabilitiesFor(ticket.connection_id)?.update === true);
}

export function bulkTagChoices(tickets: readonly TicketRow[]): string[] {
  return [...new Set(tickets.flatMap(ticket => ticket.tags))].sort((left, right) => left.localeCompare(right));
}

export function bulkTicketPatch(ticket: Pick<TicketRow, 'tags'>, action: BulkTicketAction): TicketPatch | undefined {
  if (action.kind === 'field') return { [action.field]: action.value };
  if (action.kind === 'delete') return { status: 'deleted' };
  const tag = action.tag.trim();
  if (!tag) return undefined;
  if (action.kind === 'add-tag') {
    if (ticket.tags.includes(tag)) return undefined;
    return { tags: [...ticket.tags, tag] };
  }
  if (!ticket.tags.includes(tag)) return undefined;
  return { tags: ticket.tags.filter(value => value !== tag) };
}
