/**
 * A ticket-editor save that failed, identified by its recovery destination: the project, ticket,
 * field, and note the text belongs to (HS2-SJ5TCW, HS2-Y0VRXR). The focus-loss autosave is shared
 * across tickets, so a retained failure is only ever retried into this exact destination.
 */
export interface FailedSave {
  destination: string;
}

export interface FailedSaveRetryContext {
  /** The failed save still waiting for a retry, if any. */
  failed: FailedSave | undefined;
  /** Where the editor's current text would be saved now. */
  destination: string | undefined;
  /** Typing after the failure already queued (or is sending) a newer save. */
  pending: boolean;
  /** A field conflict (the inspector's card, or a linked reader's overlap error) owns this edit. */
  conflictOwns: boolean;
  /** The editor's text, normalized the way the save writes it (a trimmed title or blocked reason). */
  text: string;
  /** The text the ticket holds now for that field, or undefined when the ticket is not loaded. */
  committed: string | undefined;
}

/**
 * Whether a focus loss with no new typing should resend the editor's text because its previous
 * save failed (HS2-SJ5TCW, HS2-Y0VRXR). The retry only belongs to the same destination, never runs
 * while a conflict owns the edit, and has nothing to send once the text matches the ticket again
 * (after Use Theirs or a successful Apply).
 */
export function shouldRetryFailedSave(context: FailedSaveRetryContext): boolean {
  const { failed } = context;
  return Boolean(
    failed &&
    context.destination !== undefined &&
    failed.destination === context.destination &&
    !context.pending &&
    !context.conflictOwns &&
    context.committed !== undefined &&
    context.text !== context.committed,
  );
}

/**
 * The failed save to remember once a save settles, from a focus loss, page hide, or reload flush:
 * a failure that no conflict took over is kept for the next focus loss, a success forgets it, and a
 * conflict hands the edit to its owner (HS2-SJ5TCW, HS2-Y0VRXR). A save without a destination keeps
 * whatever failure was already waiting.
 */
export function settleFailedSave(
  previous: FailedSave | undefined,
  destination: string | undefined,
  saved: boolean,
  conflictOwns: boolean,
): FailedSave | undefined {
  if (saved || conflictOwns) return undefined;
  if (destination === undefined) return previous;
  return { destination };
}
