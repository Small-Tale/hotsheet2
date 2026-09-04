import type { FullTicket } from './api';
import type { TicketPatch } from './ticket-operations';

export interface TicketFieldConflict {
  key: string;
  field: string;
  label: string;
  base: string;
  mine: string;
  theirs: string;
}

export type DraftReconciliation =
  | { kind: 'unchanged'; base: string; draft: string }
  | { kind: 'adopt-remote'; base: string; draft: string }
  | { kind: 'converged'; base: string; draft: string }
  | { kind: 'conflict'; base: string; draft: string };

const labels: Record<string, string> = {
  blocked_reason: 'Blocked reason',
  category: 'Category',
  details: 'Details',
  priority: 'Priority',
  status: 'Status',
  tags: 'Tags',
  title: 'Title',
  up_next: 'Up Next',
};

const equal = (left: unknown, right: unknown) => JSON.stringify(left ?? null) === JSON.stringify(right ?? null);
function display(value: unknown): string {
  if (Array.isArray(value)) return value.join(', ');
  if (value == null) return '';
  if (typeof value === 'string') return value;
  if (typeof value === 'number' || typeof value === 'boolean' || typeof value === 'bigint') return `${value}`;
  if (typeof value === 'symbol') return value.description ?? '';
  return JSON.stringify(value, null, 2);
}

/** Reconcile an actively edited text field against a newer server value. */
export function reconcileActiveDraft(base: string, draft: string, remote: string): DraftReconciliation {
  if (remote === base) return { kind: 'unchanged', base, draft };
  if (draft === base) return { kind: 'adopt-remote', base: remote, draft: remote };
  if (draft === remote) return { kind: 'converged', base: remote, draft };
  return { kind: 'conflict', base: remote, draft };
}

function ticketField(ticket: FullTicket, field: string, noteId?: string): unknown {
  if (field === 'note') return ticket.notes.find(note => note.id === noteId)?.text ?? '';
  return ticket[field as keyof FullTicket];
}

/**
 * Compares only fields in a local patch. Whole-ticket token drift caused by unrelated
 * remote changes is safe to retry; a divergent change to the same field is surfaced.
 */
export function reconcileTicketPatch(base: FullTicket, remote: FullTicket, patch: TicketPatch): { retry: TicketPatch; conflicts: TicketFieldConflict[] } {
  const retry: TicketPatch = {};
  const conflicts: TicketFieldConflict[] = [];
  const noteId = typeof patch.note_id === 'string' ? patch.note_id : undefined;
  const fields = Object.keys(patch).filter(field => field !== 'expected_token' && field !== 'note_id' && field !== 'note_kind' && field !== 'note_summary');
  for (const field of fields) {
    const logicalField = field === 'note' && noteId ? 'note' : field;
    const baseValue = ticketField(base, logicalField, noteId);
    const remoteValue = ticketField(remote, logicalField, noteId);
    const localValue = patch[field];
    if (equal(remoteValue, localValue)) continue;
    if (equal(remoteValue, baseValue)) {
      retry[field] = localValue;
      continue;
    }
    conflicts.push({
      key: logicalField === 'note' ? `note:${noteId}` : logicalField,
      field: logicalField,
      label: logicalField === 'note' ? 'Note' : labels[logicalField] ?? logicalField,
      base: display(baseValue),
      mine: display(localValue),
      theirs: display(remoteValue),
    });
  }
  if (noteId && Object.hasOwn(retry, 'note')) retry.note_id = noteId;
  if (Object.hasOwn(retry, 'note') && patch.note_kind !== undefined) retry.note_kind = patch.note_kind;
  if (Object.hasOwn(retry, 'note') && patch.note_summary !== undefined) retry.note_summary = patch.note_summary;
  return { retry, conflicts };
}

export function isTicketConcurrencyConflict(reason: unknown): boolean {
  return reason instanceof Error && reason.message.toLocaleLowerCase().includes('ticket changed since it was read');
}
