import { attr } from 'kerfjs';
import { action } from 'kerfjs/actions';

/**
 * `data-action` specs: notifications, permission requests, and ticket links.
 * Markup spreads `.attrs`; the delegated handlers in
 * `interactions/notifications-and-links.ts` register `.selector`.
 */
export const NOTIFICATIONS_AND_LINKS_ACTIONS = {
  selectNotificationView: action('select-notification-view'),
  ignorePermission: action('ignore-permission'),
  cancelPermissionAutomation: action('cancel-permission-automation'),
  pauseNotifications: action('pause-notifications'),
  resumeNotifications: action('resume-notifications'),
  resolvePermission: action('resolve-permission'),
  revokePermissionRule: action('revoke-permission-rule'),
  changePermissionRuleScope: action('change-permission-rule-scope'),
  openPermissionRules: action('open-permission-rules'),
  loadMorePermissionRules: action('load-more-permission-rules'),
  selectPermissionRule: action('select-permission-rule'),
  togglePermissionRuleRow: action('toggle-permission-rule-row'),
  selectVisiblePermissionRules: action('select-visible-permission-rules'),
  clearPermissionRuleSelection: action('clear-permission-rule-selection'),
  editSelectedPermissionRules: action('edit-selected-permission-rules'),
  requestDeleteSelectedPermissionRules: action('request-delete-selected-permission-rules'),
  cancelDeleteSelectedPermissionRules: action('cancel-delete-selected-permission-rules'),
  confirmDeleteSelectedPermissionRules: action('confirm-delete-selected-permission-rules'),
  dismissAppError: action('dismiss-app-error'),
  openHaltedSession: action('open-halted-session'),
  dismissHaltedSession: action('dismiss-halted-session'),
  answerTerminalQuestion: action('answer-terminal-question'),
  returnTerminalQuestion: action('return-terminal-question'),
  toggleVerifiedColumn: action('toggle-verified-column'),
  openLinkedTicket: action('open-linked-ticket'),
  selectTicketLinkMatch: action('select-ticket-link-match'),
  cancelTicketLinkChoice: action('cancel-ticket-link-choice'),
} as const;

/**
 * Other delegated targets (components, named fields, flags) for
 * notifications, permission requests, and ticket links.
 * Markup spreads `.attrs` where it renders a literal; handlers use `.selector`.
 */
export const NOTIFICATIONS_AND_LINKS_TARGETS = {
  ticketLinkChoiceDialog: attr('data-dialog-component', 'ticket-link-choice-dialog'),
  permissionRulesDialog: attr('data-dialog-component', 'permission-rules-dialog'),
  permissionRulesSearch: attr('name', 'permission-rule-search'),
} as const;
