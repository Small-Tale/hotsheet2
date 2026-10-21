import type { CodeReview, RepositoryFile } from './api';

export type RepositorySetupStep = 'initialize' | 'remote';
export type RepositoryStatusView = 'staged' | 'unstaged' | 'untracked' | 'conflicted' | 'commits';

export interface RepositoryDetailState {
  view: RepositoryStatusView;
  files: RepositoryFile[];
  commits: CodeReview['commits'];
  nextCursor?: number;
  loading: boolean;
  loaded: boolean;
  error: string;
}
