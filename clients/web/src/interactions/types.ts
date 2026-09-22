import { type CodeReview, type RepositoryFile } from '../api';
import { type CompatibilityAssessment } from '../compatibility';
import { type AttachmentContextMenuKind } from '../components/attachment-context-menu';
import { type RepositoryStatusView } from '../components/repository-status-popover';

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
  compatibility: CompatibilityAssessment;
  needsTicketSetup?: boolean;
  needsHs1Migration?: boolean;
  hs1ImportCompleted?: boolean;
  hs1CleanupEligible?: boolean;
  hs1SourcePath?: string;
  hs1DatabasePath?: string;
  hs1PostgresVersion?: string;
}

export type Control = HTMLElement & { value: string; open?: boolean; show?(): void; hide?(): void };

export interface RepositoryDetailState {
  view: RepositoryStatusView;
  files: RepositoryFile[];
  commits: CodeReview['commits'];
  nextCursor?: number;
  loading: boolean;
  loaded: boolean;
  error: string;
}

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
}

export interface DetailsFinishTask {
  reader: boolean;
  ticketId?: string;
  generation: number;
  saved: Promise<boolean>;
}
