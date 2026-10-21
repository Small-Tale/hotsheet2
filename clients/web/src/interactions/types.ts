import { type FullTicket } from '../api';
import { type CompatibilityAssessment } from '../compatibility';
import { type AttachmentContextMenuKind } from '../components/attachment-context-menu';

export type { RepositoryDetailState } from '../repository-state';

export interface UnhealthyServerRecovery {
  store: string;
  expected: { pid: number; url: string; started_at: string };
}

export interface Project {
  id: string;
  root: string;
  name: string;
  stores: string[];
  apiPath: string;
  /** The bridge's multiplexed change long-poll path, when it serves one (HS2-6W1NNF). */
  changeMuxPath?: string;
  compatibility: CompatibilityAssessment;
  needsTicketSetup?: boolean;
  needsHs1Migration?: boolean;
  hs1ImportCompleted?: boolean;
  hs1CleanupEligible?: boolean;
  hs1RegistrationRepairAvailable?: boolean;
  hs1SourcePath?: string;
  hs1DatabasePath?: string;
  hs1PostgresVersion?: string;
  /** Why opening skipped the project's setup refresh; shown as a dismissible warning (HS2-0TXM8S). */
  setupWarning?: string;
  /** Changed Codex hooks require a new trust review (HS2-4AR09Z). */
  codexHooksChanged?: string;
  /** The shared machine server differs from this host's selected build; dismissible info (HS2-46BKH7). */
  serverBuildNotice?: string;
  /** The user dismissed that notice for this open; the next open re-evaluates it (HS2-46BKH7). */
  serverBuildNoticeDismissed?: boolean;
  /** The repository size level whose banner the user dismissed for this open (HS2-P1GYWH). */
  repositorySizeWarningDismissed?: 'warning' | 'critical';
}

export type Control = HTMLElement & { value: string; open?: boolean; show?(): void; hide?(): void };

export interface NotWorkingTarget {
  projectId: string;
  apiPath: string;
  ticketId: string;
  slug: string;
  connectionId: string;
  mode: 'not-working' | 'reopen';
}

export interface PendingEvidence {
  id: string;
  name: string;
  file: File;
}

export interface AttachmentMenu {
  x: number;
  y: number;
  ticket: string;
  name: string;
  url: string;
  id?: string;
  kind: AttachmentContextMenuKind;
  reader?: string;
  /** The linked reader frame whose ticket the menu acts on, when it opened from that frame's gallery (HS2-W8VEMP). */
  owner?: string;
}

export interface DetailsFinishTask {
  reader: boolean;
  ticketId?: string;
  generation: number;
  saved: Promise<boolean>;
}

/**
 * The ticket a gallery was opened for (HS2-97E0QR). Media opened from a stacked reader belongs to
 * that reader's ticket and project, not the workspace selection; `update` receives the ticket after
 * an annotation save so the owning reader stays current.
 */
export interface GallerySource {
  ticket: FullTicket;
  project: Project;
  /** View-only source (its provider cannot edit attachments), so markup is unavailable. */
  readOnly?: boolean;
  /** The linked reader frame the media belongs to (HS2-W8VEMP). */
  frameId?: string;
  update?: (ticket: FullTicket) => void;
}
