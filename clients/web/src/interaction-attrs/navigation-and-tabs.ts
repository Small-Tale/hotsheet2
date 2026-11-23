import { attr } from 'kerfjs';
import { action } from 'kerfjs/actions';

/**
 * `data-action` specs: project tabs, workspace navigation, and corrupt-ticket rows.
 * Markup spreads `.attrs`; the delegated handlers in
 * `interactions/navigation-and-tabs.ts` register `.selector`.
 */
export const NAVIGATION_AND_TABS_ACTIONS = {
  revealCorruptTicket: action('reveal-corrupt-ticket'),
  repairCorruptTicket: action('repair-corrupt-ticket'),
  selectCorruptTicket: action('select-corrupt-ticket'),
  setShellMode: action('set-shell-mode'),
  openProjectStats: action('open-project-stats'),
  selectProjectTab: action('select-project-tab'),
  retryProjectRestore: action('retry-project-restore'),
  /** Retry a remembered remote project's placeholder tab now (HS2-WNK73S). */
  retryRemoteProject: action('retry-remote-project'),
  /** Ask the approving host to renew a remote approval in place (HS2-4B0KSD). Carries `data-remote-scope`. */
  renewRemoteApproval: action('renew-remote-approval'),
  /** Hide a remote approval reminder until its next reminder day (HS2-4B0KSD). */
  dismissRemoteApprovalReminder: action('dismiss-remote-approval-reminder'),
  /** Review a remote server whose host revoked this computer: pair it again (HS2-4B0KSD). */
  reviewRemoteServer: action('review-remote-server'),
} as const;

/**
 * Other delegated targets (components, named fields, flags) for
 * project tabs, workspace navigation, and corrupt-ticket rows.
 * Markup spreads `.attrs` where it renders a literal; handlers use `.selector`.
 */
export const NAVIGATION_AND_TABS_TARGETS = {
  tabRole: attr('role', 'tab'),
} as const;
