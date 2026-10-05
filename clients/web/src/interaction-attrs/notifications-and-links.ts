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
  dismissAppError: action('dismiss-app-error'),
  openHaltedSession: action('open-halted-session'),
  dismissHaltedSession: action('dismiss-halted-session'),
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
  ticketLinkChoiceDialog: attr('data-component', 'ticket-link-choice-dialog'),
} as const;
