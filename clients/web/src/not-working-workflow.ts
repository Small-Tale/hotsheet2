export interface NotWorkingSubmission {
  note: string;
  files: readonly File[];
}

export interface NotWorkingWorkflow {
  upload(file: File): Promise<string>;
  removeAttachment(id: string): Promise<void>;
  reopen(note?: string): Promise<void>;
}

export class NotWorkingSubmissionError extends Error {
  constructor(message: string, readonly cleanupErrors: readonly unknown[] = []) { super(message); }
}

/**
 * Upload evidence before reopening the ticket, compensating every new upload when
 * a later step fails. This keeps a failed report completed instead of presenting
 * partially reopened work as ready for another attempt.
 */
export async function submitNotWorkingReport(submission: NotWorkingSubmission, workflow: NotWorkingWorkflow): Promise<void> {
  const note = submission.note.trim();
  if (!note && submission.files.length === 0) throw new NotWorkingSubmissionError('Describe what is wrong or add an attachment.');
  const uploaded: string[] = [];
  try {
    for (const file of submission.files) uploaded.push(await workflow.upload(file));
    await workflow.reopen(note || undefined);
  } catch (cause) {
    const cleanupErrors: unknown[] = [];
    for (const id of uploaded.reverse()) {
      try { await workflow.removeAttachment(id); } catch (error) { cleanupErrors.push(error); }
    }
    const detail = cause instanceof Error ? cause.message : String(cause);
    throw new NotWorkingSubmissionError(cleanupErrors.length ? `${detail} Some uploaded evidence could not be removed.` : detail, cleanupErrors);
  }
}
