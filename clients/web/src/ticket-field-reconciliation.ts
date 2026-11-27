import { ApiHttpError, type FullTicket } from './api';
import { isFeedbackPromptEdit } from './feedback-prompt-edits';
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
  // An attachment batch label is ticket text with the same restore and conflict rules (HS2-9DQXGY).
  attachment_label: 'Attachment Label',
  blocked_reason: 'Blocked Reason',
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

/** A feedback response is a new note, not an edit of the request note it is displayed beneath. */
export function reconcileReaderNoteDraft(
  previous: FullTicket,
  refreshed: FullTicket,
  noteId: string,
  base: string,
  draft: string,
): DraftReconciliation | undefined {
  const source = previous.notes.find((note) => note.id === noteId);
  if (source?.kind === 'feedback_needed') return undefined;
  return reconcileActiveDraft(base, draft, refreshed.notes.find((note) => note.id === noteId)?.text ?? '');
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

/** Patch fields that set one existing note's metadata, keyed to the note property they write (HS2-WZSFHK). */
const NOTE_METADATA_FIELDS = {
  note_kind: 'kind',
  note_summary: 'summary',
  note_confidence: 'confidence',
  response_to: 'response_to',
} as const satisfies Record<string, keyof FullTicket['notes'][number]>;
const isNoteMetadataField = (field: string): field is keyof typeof NOTE_METADATA_FIELDS =>
  Object.hasOwn(NOTE_METADATA_FIELDS, field);

/**
 * Compares only fields in a local patch. Whole-ticket token drift caused by unrelated
 * remote changes is safe to retry; concurrent edits to the same text or tag field are merged,
 * and only an edit that cannot be merged is surfaced as a conflict.
 */
export function reconcileTicketPatch(
  base: FullTicket,
  remote: FullTicket,
  patch: TicketPatch,
): { retry: TicketPatch; conflicts: TicketFieldConflict[]; merged: string[] } {
  const retry: TicketPatch = {};
  const conflicts: TicketFieldConflict[] = [];
  /** Fields whose retry value combines this edit with the newer remote change (HS2-JJ3R1G). */
  const merged: string[] = [];
  const noteId = typeof patch.note_id === 'string' ? patch.note_id : undefined;
  const fields = Object.keys(patch).filter(
    (field) =>
      field !== 'expected_token' &&
      field !== 'note_id' &&
      field !== 'edit_feedback_prompt' &&
      field !== 'note_kind' &&
      field !== 'note_summary' &&
      field !== 'note_confidence' &&
      !(noteId && field === 'response_to'),
  );
  // An edit of one existing note's metadata (Edit Timeline Summary, Correct Confidence, Change Type, Link
  // as Feedback Response) is resent unless the server already holds the chosen value; an empty retry would
  // report the edit as saved without writing it (HS2-WZSFHK). The user confirmed the value in a dialog,
  // so it is last-write-wins for that note field.
  const remoteNote = noteId ? remote.notes.find((note) => note.id === noteId) : undefined;
  if (noteId && !Object.hasOwn(patch, 'note'))
    for (const field of Object.keys(patch).filter(isNoteMetadataField))
      if (!equal(remoteNote?.[NOTE_METADATA_FIELDS[field]], patch[field])) retry[field] = patch[field];
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
    const mergedValue = mergeField(logicalField, baseValue, localValue, remoteValue);
    if (mergedValue) {
      if (!equal(mergedValue.value, remoteValue)) retry[field] = mergedValue.value;
      // The saved value combines both edits, so the user is told (HS2-JJ3R1G).
      if (!equal(mergedValue.value, localValue) && !equal(mergedValue.value, remoteValue)) merged.push(logicalField);
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
  if (noteId && Object.keys(retry).some((field) => field === 'note' || isNoteMetadataField(field)))
    retry.note_id = noteId;
  // An explicit FEEDBACK NEEDED prompt edit stays explicit on retry (HS2-ZXGEFE).
  if (noteId && Object.hasOwn(retry, 'note') && patch.edit_feedback_prompt === true) retry.edit_feedback_prompt = true;
  if (Object.hasOwn(retry, 'note') && patch.note_kind !== undefined) retry.note_kind = patch.note_kind;
  if (Object.hasOwn(retry, 'note') && patch.note_summary !== undefined) retry.note_summary = patch.note_summary;
  if (Object.hasOwn(retry, 'note') && patch.note_confidence !== undefined)
    retry.note_confidence = patch.note_confidence;
  return { retry, conflicts, merged };
}

/**
 * The transient confirmation after an edit was three-way merged with a newer change instead of asking
 * (HS2-JJ3R1G). `restored` words it for a recovery copy merged as its editor reopens.
 */
export function mergedEditNotice(fields: readonly string[], { restored = false } = {}): string {
  const names = [...new Set(fields.map(ticketFieldLabel))],
    target = names.length === 1 ? `a newer change to ${names[0]}` : 'newer changes';
  return restored ? `Restored an unsaved edit and merged it with ${target}.` : `Merged your edit with ${target}.`;
}

/**
 * The patch for an in-place note edit. A FEEDBACK NEEDED prompt the user explicitly opened with Edit
 * Note says so; the server refuses a human rewrite of an AI prompt without it (HS2-MB2Q10,
 * HS2-ZXGEFE), so any other path that reaches a prompt still fails safe.
 */
export function noteEditPatch(ticket: FullTicket | null | undefined, noteId: string, note: string): TicketPatch {
  const prompt =
    isFeedbackPromptEdit(noteId) && ticket?.notes.some((item) => item.id === noteId && item.kind === 'feedback_needed');
  return prompt ? { note_id: noteId, note, edit_feedback_prompt: true } : { note_id: noteId, note };
}

/**
 * The patch that saves a conflict card's chosen value to `ticket`, the ticket the card belongs to: the
 * selected ticket's for the inspector, a linked reader frame's for its own card (HS2-JT2AFG).
 */
export function resolvedConflictPatch(
  conflict: TicketFieldConflict,
  value: string,
  ticket: FullTicket | null | undefined,
): TicketPatch {
  if (conflict.field === 'note') return noteEditPatch(ticket, conflict.key.slice('note:'.length), value);
  if (conflict.field === 'tags')
    return {
      tags: value
        .split(',')
        .map((item) => item.trim())
        .filter(Boolean),
    };
  if (conflict.field === 'up_next') return { up_next: value.toLocaleLowerCase() === 'true' };
  if (conflict.field === 'blocked_reason') return { blocked_reason: value.trim() || null };
  return { [conflict.field]: value };
}

/** The focus-loss save queue a text editor keeps for a conflicted field. */
export type ConflictedEditorQueue = 'details' | 'blocked_reason' | 'note' | 'title';

/**
 * The editor queue a field conflict card supersedes when the user resolves it, or undefined for a field
 * no text editor queues (tags, Up Next). Focus moving into the card keeps the editor open without
 * flushing (HS2-3TSEAX), so text typed before the choice is still queued and must be dropped, never
 * sent over the chosen value by a later focus loss (HS2-JT2AFG, HS2-A51MNY).
 */
export function conflictedEditorQueue(field: string): ConflictedEditorQueue | undefined {
  return field === 'details' || field === 'blocked_reason' || field === 'note' || field === 'title' ? field : undefined;
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

/** The server's `code` on every stale `expected_token` 409, whatever ticket its message names (HS2-9S8CV1). */
export const TICKET_CHANGED_CODE = 'ticket_changed';

/**
 * A stale `expected_token`, the one failure the editors rebase and retry (HS2-A4XCXE). Recognized by the
 * typed `code`, never the message: a Markdown checkout names the ticket inside it and provider adapters
 * word it as an issue (HS2-9S8CV1). Every other 409 is a refusal.
 */
export function isTicketConcurrencyConflict(reason: unknown): boolean {
  return reason instanceof ApiHttpError && reason.status === 409 && reason.code === TICKET_CHANGED_CODE;
}

/** The server's `code` on the 409 refusing a queued provider batch without durable write-behind (HS2-DEVEYE). */
export const WRITE_BEHIND_UNAVAILABLE_CODE = 'write_behind_unavailable';

/**
 * The connection has no durable queued edits, so the edit takes its synchronous path instead. Recognized by the
 * typed `code`, never the message: any other 409 stays a refusal and a wording change cannot stop the fallback.
 */
export function isWriteBehindUnavailable(reason: unknown): boolean {
  return reason instanceof ApiHttpError && reason.status === 409 && reason.code === WRITE_BEHIND_UNAVAILABLE_CODE;
}
