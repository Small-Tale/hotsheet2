import type { FullTicket } from './api';
import { mergeSet, mergeText } from './text-merge';
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
  /** Both sides changed different parts; `draft` is the merged text, still to be saved (HS2-A4XCXE). */
  | { kind: 'merged'; base: string; draft: string; mine: string }
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
  const merged = mergeText(base, draft, remote);
  if (merged.clean) return { kind: 'merged', base: remote, draft: merged.merged, mine: draft };
  return { kind: 'conflict', base: remote, draft };
}

/** Text fields a concurrent edit can merge line by line; `blocked_reason` stores `null` for empty. */
const TEXT_FIELDS = new Set(['details', 'title', 'blocked_reason', 'note']);

/** A clean three-way merge of one patched field, or `undefined` when both sides truly conflict. */
function mergeField(field: string, base: unknown, mine: unknown, theirs: unknown): { value: unknown } | undefined {
  if (field === 'tags' && Array.isArray(mine))
    return {
      value: mergeSet(
        Array.isArray(base) ? (base as string[]) : [],
        mine as string[],
        Array.isArray(theirs) ? (theirs as string[]) : [],
      ),
    };
  if (!TEXT_FIELDS.has(field)) return undefined;
  const text = (value: unknown) => (typeof value === 'string' ? value : ''),
    merged = mergeText(text(base), text(mine), text(theirs));
  if (!merged.clean) return undefined;
  return { value: field === 'blocked_reason' && !merged.merged.trim() ? null : merged.merged };
}

function ticketField(ticket: FullTicket, field: string, noteId?: string): unknown {
  if (field === 'note') return ticket.notes.find((note) => note.id === noteId)?.text ?? '';
  return ticket[field as keyof FullTicket];
}

/**
 * Compares only fields in a local patch. Whole-ticket token drift caused by unrelated
 * remote changes is safe to retry; concurrent edits to the same text or tag field are merged,
 * and only an edit that cannot be merged is surfaced as a conflict.
 */
export function reconcileTicketPatch(
  base: FullTicket,
  remote: FullTicket,
  patch: TicketPatch,
): { retry: TicketPatch; conflicts: TicketFieldConflict[] } {
  const retry: TicketPatch = {};
  const conflicts: TicketFieldConflict[] = [];
  const noteId = typeof patch.note_id === 'string' ? patch.note_id : undefined;
  const fields = Object.keys(patch).filter(
    (field) => field !== 'expected_token' && field !== 'note_id' && field !== 'note_kind' && field !== 'note_summary',
  );
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
    // Both sides changed this field: merge disjoint text edits and tag additions/removals (HS2-A4XCXE).
    const merged = mergeField(logicalField, baseValue, localValue, remoteValue);
    if (merged) {
      if (!equal(merged.value, remoteValue)) retry[field] = merged.value;
      continue;
    }
    conflicts.push({
      key: logicalField === 'note' ? `note:${noteId}` : logicalField,
      field: logicalField,
      label: logicalField === 'note' ? 'Note' : (labels[logicalField] ?? logicalField),
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

/** The draft-backed text value of `field` (a note's text for `note`), as the editors hold it. */
export function ticketFieldText(ticket: FullTicket, field: string, noteId?: string): string {
  const value = ticketField(ticket, field, noteId);
  return typeof value === 'string' ? value : '';
}

/**
 * Rebase a draft value that was typed on top of `draftBase` onto the ticket's newer `current` value
 * (HS2-A4XCXE): unchanged when nothing moved, merged when the edits are disjoint, else a conflict.
 */
export function rebaseDraftValue(
  draftBase: string,
  mine: string,
  current: string,
): { kind: 'unchanged' } | { kind: 'merged'; value: string } | { kind: 'conflict' } {
  if (draftBase === current || mine === current) return { kind: 'unchanged' };
  const merged = mergeText(draftBase, mine, current);
  return merged.clean ? { kind: 'merged', value: merged.merged } : { kind: 'conflict' };
}

/** Human label for a reconciled field. */
export function ticketFieldLabel(field: string): string {
  return field === 'note' ? 'Note' : (labels[field] ?? field);
}

export function isTicketConcurrencyConflict(reason: unknown): boolean {
  return reason instanceof Error && reason.message.toLocaleLowerCase().includes('ticket changed since it was read');
}
