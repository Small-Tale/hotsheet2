import type { Note } from './api';

type RelevantNote = Pick<Note, 'id' | 'kind' | 'created_at'>;

/** A regular note answers the preceding feedback request; other note kinds are neutral. */
export function isFeedbackNeeded(notes: readonly RelevantNote[]) {
  let latest: RelevantNote | undefined;
  for (const note of notes) {
    if (note.kind !== 'regular' && note.kind !== 'feedback_needed') continue;
    if (!latest || note.created_at > latest.created_at || (note.created_at === latest.created_at && note.id > latest.id)) latest = note;
  }
  return latest?.kind === 'feedback_needed';
}
