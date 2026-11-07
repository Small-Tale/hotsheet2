import type { NoteActor } from './api';
import { aiToolName } from './note-authorship';

/**
 * The accessible label a ticket summary shows for an AI-created ticket (HS2-9FPHWY), or
 * `undefined` when the ticket keeps the prominent presentation: a person, Hot Sheet itself,
 * or an unknown creator (tickets created before the field existed, unspecified callers, and
 * external providers) are never marked as AI-created.
 */
export function aiCreatorLabel(createdBy: NoteActor | undefined): string | undefined {
  if (createdBy?.role !== 'ai') return undefined;
  const tool = aiToolName(createdBy.id);
  return tool === 'AI' ? 'Created by AI' : `Created by AI (${tool})`;
}
