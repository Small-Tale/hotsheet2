import type { Capabilities, CodeReview, FullTicket } from '../api';
import { INSPECTOR_AND_EDITOR_TARGETS } from '../interaction-attrs/inspector-and-editor';
import type { Project } from '../interactions/types';
import { linkedReaderCanUpdate, reconcileTicketReaderFrame, type TicketReaderFrame } from '../ticket-reader-stack';

/** The frame id of the workspace reader, which shows the selected ticket. */
export const WORKSPACE_READER_SCOPE = 'workspace-reader';

/**
 * The reader frame that contains `target`, or undefined outside the modal reader. A dialog opened
 * from a reader renders inside that reader, so the reader's modality does not leave it inert
 * beneath (HS2-EZ10RS, HS2-NXSMC7).
 */
export function readerScopeOf(target: Element): string | undefined {
  return target.closest<HTMLElement>(INSPECTOR_AND_EDITOR_TARGETS.ticketReader.selector)?.dataset.readerFrameId;
}

/**
 * A linked reader frame's own ticket, project, and Code Review. An edit made in a linked reader
 * goes to that frame's checkout and ticket, the same routing as its field edits (HS2-KRVWKF).
 * The follow-up link (HS2-NXSMC7) and blocker (HS2-15PE59) controllers read it.
 */
export interface LinkedReaderSubject {
  frameId: string;
  project: Project;
  ticket: FullTicket;
  capabilities: Capabilities;
  /** The frame is the top open reader and its provider updates tickets. */
  canUpdate: boolean;
  codeReview?: CodeReview;
  /** Reload this frame's Code Review from its own project (HS2-9Q4AFB). */
  refreshCodeReview: () => Promise<void>;
  /** Show the server's answer in this frame while keeping its unsaved drafts. */
  adopt: (ticket: FullTicket) => void;
}

export interface LinkedReaderSubjectDependencies {
  linkedReaderStack: () => readonly TicketReaderFrame[];
  projects: () => readonly Project[];
  refreshCodeReview: (frameId: string) => Promise<void>;
  replaceFrame: (id: string, update: (frame: TicketReaderFrame) => TicketReaderFrame) => void;
}

/**
 * Resolve a reader scope to its linked frame. The workspace reader and the Inspector return
 * undefined, because they act on the selected ticket.
 */
export function createLinkedReaderSubjects(dependencies: LinkedReaderSubjectDependencies) {
  return (frameId: string | undefined): LinkedReaderSubject | undefined => {
    if (!frameId || frameId === WORKSPACE_READER_SCOPE) return undefined;
    const stack = dependencies.linkedReaderStack(),
      frame = stack.find((item) => item.id === frameId),
      project = frame && dependencies.projects().find((item) => item.id === frame.projectId);
    if (!frame || !project) return undefined;
    const top = stack.filter((item) => item.open).at(-1)?.id === frame.id;
    return {
      frameId,
      project,
      ticket: frame.ticket,
      capabilities: frame.capabilities,
      canUpdate: linkedReaderCanUpdate(frame, top),
      codeReview: frame.codeReview?.review,
      refreshCodeReview: () => dependencies.refreshCodeReview(frameId),
      adopt: (ticket) => {
        dependencies.replaceFrame(frameId, (current) =>
          current.ticket.qualified_id === ticket.qualified_id ? reconcileTicketReaderFrame(current, ticket) : current,
        );
      },
    };
  };
}
