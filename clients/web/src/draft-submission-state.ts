import {
  loadProjectWorkspaceSession,
  type ProjectWorkspaceSession,
  saveProjectWorkspaceSession,
} from './workspace-session';

/** Clear only the submitted origin draft; a newer draft keeps its text and new refs (HS2-791W19). */
export function finishDraftSubmission(
  storage: Pick<Storage, 'getItem' | 'setItem'>,
  projectId: string,
  submission:
    | { kind: 'composer'; draft: ProjectWorkspaceSession['composer'] }
    | { kind: 'not-working'; draft: NonNullable<ProjectWorkspaceSession['notWorking']> },
) {
  const stored = loadProjectWorkspaceSession(storage, projectId);
  if (!stored) return;
  const ids = new Set(submission.draft.attachments.map((item) => item.id));
  const sameRefs = (refs: readonly { id: string }[]) =>
    refs.length === ids.size && refs.every((item) => ids.has(item.id));
  if (submission.kind === 'composer') {
    const current = stored.composer,
      submitted = submission.draft,
      unchanged =
        sameRefs(current.attachments) &&
        current.title === submitted.title &&
        current.details === submitted.details &&
        current.category === submitted.category &&
        current.upNext === submitted.upNext;
    stored.composer = {
      ...current,
      ...(unchanged ? { open: false, title: '', details: '', upNext: false } : {}),
      attachments: current.attachments.filter((item) => !ids.has(item.id)),
    };
  } else {
    const current = stored.notWorking,
      submitted = submission.draft;
    if (
      !current ||
      current.ticketId !== submitted.ticketId ||
      current.connectionId !== submitted.connectionId ||
      (current.mode ?? 'not-working') !== (submitted.mode ?? 'not-working')
    )
      return;
    stored.notWorking =
      sameRefs(current.attachments) && current.note === submitted.note
        ? undefined
        : { ...current, attachments: current.attachments.filter((item) => !ids.has(item.id)) };
  }
  saveProjectWorkspaceSession(storage, projectId, stored);
}
