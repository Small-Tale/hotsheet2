/** The Details edit whose focus-loss save failed, identified by its ticket and edit generation (HS2-SJ5TCW). */
export interface FailedDetailsSave {
  ticketId?: string | undefined;
  generation: number;
}

export interface DetailsRetryContext {
  /** The failed save still waiting for a retry, if any. */
  failed: FailedDetailsSave | undefined;
  /** The ticket and edit generation the focus loss is finishing. */
  ticketId: string | undefined;
  generation: number;
  /** The editor is still in write mode. */
  writing: boolean;
  /** Typing after the failure already queued (or is sending) a newer save. */
  pending: boolean;
  /** The Details field-conflict card owns this edit, so a retry would raise it again. */
  conflictOpen: boolean;
  draft: string;
  /** The Details text the ticket holds now. */
  current: string | undefined;
}

/**
 * Whether a focus loss with no new typing should resend the editor's text because its previous
 * save failed (HS2-SJ5TCW). The retry only belongs to the same ticket and edit, never runs while
 * the conflict card is open, and has nothing to send once the text matches the ticket again (after
 * Use Theirs or a successful Apply).
 */
export function shouldRetryFailedDetailsSave(context: DetailsRetryContext): boolean {
  const { failed } = context;
  return Boolean(
    failed &&
    failed.ticketId !== undefined &&
    failed.ticketId === context.ticketId &&
    failed.generation === context.generation &&
    context.writing &&
    !context.pending &&
    !context.conflictOpen &&
    context.current !== undefined &&
    context.draft !== context.current,
  );
}

/**
 * The failed save to remember once a focus-loss save settles: a failure that did not open the
 * conflict card is kept for the next focus loss, a success forgets it, and a conflict hands the edit
 * to the card (HS2-SJ5TCW).
 */
export function settleFailedDetailsSave(
  previous: FailedDetailsSave | undefined,
  task: FailedDetailsSave,
  saved: boolean,
  conflictOpen: boolean,
): FailedDetailsSave | undefined {
  if (saved || conflictOpen) return undefined;
  if (task.ticketId === undefined) return previous;
  return { ticketId: task.ticketId, generation: task.generation };
}
