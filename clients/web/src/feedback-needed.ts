import type { Note } from './api';

type RelevantNote = Pick<Note, 'id' | 'kind' | 'created_at'>;

const after = (candidate: RelevantNote, note: RelevantNote) => candidate.created_at > note.created_at || (candidate.created_at === note.created_at && candidate.id > note.id);

/** A regular note answers the preceding feedback request; other note kinds are neutral. */
export function isFeedbackNeeded(notes: readonly RelevantNote[]) {
  let latest: RelevantNote | undefined;
  for (const note of notes) {
    if (note.kind !== 'regular' && note.kind !== 'feedback_needed') continue;
    if (!latest || note.created_at > latest.created_at || (note.created_at === latest.created_at && note.id > latest.id)) latest = note;
  }
  return latest?.kind === 'feedback_needed';
}

/** A feedback ask that has already received a later regular response reads as an ordinary note. */
export function presentedNoteKind(note: RelevantNote, notes: readonly RelevantNote[]): RelevantNote['kind'] {
  return note.kind==='feedback_needed'&&notes.some(candidate=>candidate.kind==='regular'&&after(candidate,note))?'regular':note.kind;
}
