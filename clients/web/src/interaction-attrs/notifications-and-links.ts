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
  cancelRememberedPermission: action('cancel-remembered-permission'),
  copyPermissionRuleAction: action('copy-permission-rule-action'),
  revokePermissionRule: action('revoke-permission-rule'),
  changePermissionRuleDecision: action('change-permission-rule-decision'),
  changePermissionRuleScope: action('change-permission-rule-scope'),
  changeSelectedPermissionRuleScope: action('change-selected-permission-rule-scope'),
  openPermissionRules: action('open-permission-rules'),
  loadMorePermissionRules: action('load-more-permission-rules'),
  filterPermissionRuleLifetime: action('filter-permission-rule-lifetime'),
  filterPermissionRuleDecision: action('filter-permission-rule-decision'),
  selectPermissionRuleRow: action('select-permission-rule-row'),
  togglePermissionRuleAction: action('toggle-permission-rule-action'),
  openPermissionRuleEditor: action('open-permission-rule-editor'),
  closePermissionRuleEditor: action('close-permission-rule-editor'),
  changePermissionRuleEditorKind: action('change-permission-rule-editor-kind'),
  savePermissionRuleEditor: action('save-permission-rule-editor'),
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
  permissionRuleEditor: attr('data-dialog-component', 'permission-rule-editor'),
  permissionRuleEditorPattern: attr('name', 'permission-rule-editor-pattern'),
  permissionRulesSearch: attr('name', 'permission-rule-search'),
} as const;
