/**
 * FEEDBACK NEEDED prompts the user explicitly opened with Edit Note (HS2-ZXGEFE). Only these may be
 * rewritten in place: their saves carry `edit_feedback_prompt`, which the server requires before a
 * human may change an AI prompt. Every other path to a prompt is a reply that creates a new note
 * (HS2-MB2Q10). Note ids are ULIDs, so one set covers every open ticket and reader frame.
 */
const explicitPromptEdits = new Set<string>();

export function beginFeedbackPromptEdit(noteId: string): void {
  explicitPromptEdits.add(noteId);
}

export function endFeedbackPromptEdit(noteId: string | undefined): void {
  if (noteId) explicitPromptEdits.delete(noteId);
}

export function isFeedbackPromptEdit(noteId: string | undefined): boolean {
  return noteId !== undefined && explicitPromptEdits.has(noteId);
}
