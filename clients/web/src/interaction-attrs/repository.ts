import { attr } from 'kerfjs';
import { action } from 'kerfjs/actions';

/**
 * `data-action` specs: repository status, comparison, review, and file actions.
 * Markup spreads `.attrs`; the delegated handlers in
 * `interactions/repository.ts` register `.selector`.
 */
export const REPOSITORY_ACTIONS = {
  openRepositoryStatus: action('open-repository-status'),
  refreshRepositoryStatus: action('refresh-repository-status'),
  initializeRepository: action('initialize-repository'),
  connectRepositoryRemote: action('connect-repository-remote'),
  skipRepositoryRemote: action('skip-repository-remote'),
  selectRepositoryView: action('select-repository-view'),
  toggleRepositoryComparison: action('toggle-repository-comparison'),
  setRepositoryComparisonSide: action('set-repository-comparison-side'),
  selectRepositoryComparisonCommit: action('select-repository-comparison-commit'),
  toggleCodeReviewCommit: action('toggle-code-review-commit'),
  openRepositoryFileMenuTrigger: action('open-repository-file-menu-trigger'),
  openRepositoryReview: action('open-repository-review'),
  openChangeEvidence: action('open-change-evidence'),
  selectChangeEvidenceView: action('select-change-evidence-view'),
  openProjectForm: action('open-project-form'),
  /** Code Review's Follow-Ups action and its link dialog (HS2-MCGCPQ). */
  openFollowUpLink: action('open-follow-up-link'),
  selectFollowUpLinkTarget: action('select-follow-up-link-target'),
  clearFollowUpLinkTarget: action('clear-follow-up-link-target'),
  submitFollowUpLink: action('submit-follow-up-link'),
  cancelFollowUpLink: action('cancel-follow-up-link'),
} as const;

/** Named fields and dialog roots of the Code Review follow-up link flow (HS2-MCGCPQ). */
export const REPOSITORY_TARGETS = {
  followUpLinkSearchField: attr('name', 'follow-up-link-target-search'),
  followUpLinkDialog: attr('data-dialog-component', 'ticket-follow-up-link-dialog'),
} as const;
