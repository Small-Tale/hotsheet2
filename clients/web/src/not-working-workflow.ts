export interface NotWorkingSubmission {
  note: string;
  files: readonly File[];
}

export interface NotWorkingWorkflow {
  report(note: string, files: readonly File[]): Promise<void>;
}

export class NotWorkingSubmissionError extends Error {}

/**
 * Validate client input, then delegate one provider-neutral atomic mutation.
 */
export async function submitNotWorkingReport(submission: NotWorkingSubmission, workflow: NotWorkingWorkflow): Promise<void> {
  const note = submission.note.trim();
  if (!note && submission.files.length === 0) throw new NotWorkingSubmissionError('Describe what is wrong or add an attachment.');
  try {
    await workflow.report(note, submission.files);
  } catch (cause) {
    const detail = cause instanceof Error ? cause.message : String(cause);
    throw new NotWorkingSubmissionError(detail);
  }
}
