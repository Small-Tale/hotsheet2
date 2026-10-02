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
} as const;
