import type { Capabilities, Note } from './api';

/** The only note-level fields an AI may suggest for human review (HS2-5E8Y9R). */
export interface NoteRepairProposal {
  note_id: string;
  kind?: 'regular' | 'activity' | 'feedback_needed' | 'status';
  text?: string;
  confidence?: number | null;
  summary?: string | null;
  response_to?: string | null;
  explanation?: string;
}

const noteKinds = new Set(['regular', 'activity', 'feedback_needed', 'status']);
const allowedKeys = new Set(['note_id', 'kind', 'text', 'confidence', 'summary', 'response_to', 'explanation']);

/** Accept one JSON object, optionally in a single json fence; refuse extra prose or ticket-wide edits. */
export function parseNoteRepairProposal(source: string, noteId: string): NoteRepairProposal {
  const trimmed = source.trim();
  const fenced = /^```json\s*\n([\s\S]*?)\n```$/u.exec(trimmed);
  let value: unknown;
  try {
    value = JSON.parse(fenced ? fenced[1] : trimmed);
  } catch {
    throw new Error('The AI did not return a JSON note repair proposal.');
  }
  if (!value || typeof value !== 'object' || Array.isArray(value))
    throw new Error('The repair proposal must be one JSON object.');
  const record = value as Record<string, unknown>;
  if (Object.keys(record).some((key) => !allowedKeys.has(key)))
    throw new Error('The repair proposal includes a field outside the note repair scope.');
  if (record.note_id !== noteId) throw new Error('The repair proposal targets a different note.');
  if (record.kind !== undefined && (typeof record.kind !== 'string' || !noteKinds.has(record.kind)))
    throw new Error('The repair proposal has an unsupported note type.');
  if (record.text !== undefined && (typeof record.text !== 'string' || !record.text.trim()))
    throw new Error('The repair proposal needs non-empty note text.');
  if (
    record.confidence !== undefined &&
    record.confidence !== null &&
    (!Number.isInteger(record.confidence) || (record.confidence as number) < 0 || (record.confidence as number) > 100)
  )
    throw new Error('The repair confidence must be 0–100 or null.');
  if (record.summary !== undefined && record.summary !== null && typeof record.summary !== 'string')
    throw new Error('The repair summary must be text or null.');
  if (record.response_to !== undefined && record.response_to !== null && typeof record.response_to !== 'string')
    throw new Error('The repair response link must be a note id or null.');
  if (record.explanation !== undefined && typeof record.explanation !== 'string')
    throw new Error('The repair explanation must be text.');
  if (!['kind', 'text', 'confidence', 'summary', 'response_to'].some((key) => Object.hasOwn(record, key)))
    throw new Error('The repair proposal contains no note change.');
  return record as unknown as NoteRepairProposal;
}

export interface NoteRepairPreview {
  proposal: NoteRepairProposal;
  /** A reason each change cannot currently be applied, shown before the user can accept. */
  unavailable: string[];
  patch: Record<string, unknown>;
  changed: boolean;
}

/** Build the exact API patch only after capability and note-scope checks. */
export function previewNoteRepair(
  proposal: NoteRepairProposal,
  note: Note,
  capabilities: Capabilities,
  expectedToken?: string,
  relatedNotes: readonly Note[] = [],
): NoteRepairPreview {
  const unavailable: string[] = [];
  const patch: Record<string, unknown> = { note_id: note.id, actor: { role: 'human' } };
  if (expectedToken) patch.expected_token = expectedToken;
  if (!capabilities.note_edit) unavailable.push('This source cannot edit notes.');
  if (proposal.kind !== undefined && proposal.kind !== note.kind) {
    if (!capabilities.note_kind_edit) unavailable.push('This source cannot change note type.');
    if (relatedNotes.some((item) => item.response_to === note.id))
      unavailable.push('This note already has a linked response; changing its type needs separate review.');
    patch.note_kind = proposal.kind;
  }
  if (proposal.text !== undefined && proposal.text !== note.text) {
    patch.note = proposal.text;
    if (note.kind === 'feedback_needed' && note.actor?.role === 'ai') patch.edit_feedback_prompt = true;
  }
  if (proposal.confidence !== undefined && proposal.confidence !== (note.confidence ?? null)) {
    if (proposal.confidence === null) patch.clear_note_confidence = true;
    else patch.note_confidence = proposal.confidence;
  }
  if (proposal.summary !== undefined && proposal.summary !== (note.summary ?? null))
    unavailable.push('Editing an existing note summary is not supported yet.');
  // HS2-J4G9GB: an existing response link is append-only today. Never create a reply for the user.
  if (proposal.response_to !== undefined && proposal.response_to !== (note.response_to ?? null))
    unavailable.push('Linking an existing note as a response is not supported yet.');
  const changed = Object.keys(patch).some((key) =>
    ['note_kind', 'note', 'note_confidence', 'clear_note_confidence'].includes(key),
  );
  return { proposal, unavailable, patch, changed };
}
