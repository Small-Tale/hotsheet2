import { attr } from 'kerfjs';
import { action } from 'kerfjs/actions';

/**
 * `data-action` specs: the app shell, rails, resizing, and global ticket drag.
 * Markup spreads `.attrs`; the delegated handlers in
 * `interactions/shell-and-global.ts` register `.selector`.
 */
export const SHELL_AND_GLOBAL_ACTIONS = {
  toggleTicketInspector: action('toggle-ticket-inspector'),
  toggleProjectSidebar: action('toggle-project-sidebar'),
  /** Standby (HS2-EQVZTZ): the moon button, the night display itself, and its settings. */
  enterStandby: action('enter-standby'),
  exitStandby: action('exit-standby'),
  saveStandbySettings: action('save-standby-settings'),
  /** The while-you-were-away digest shown on leaving standby (HS2-JJ9RYN). */
  dismissAwayDigest: action('dismiss-away-digest'),
  showWhatNeedsMe: action('show-what-needs-me'),
  openAwayEvent: action('open-away-event'),
  /** Include or ignore a newly detected disk in host health (HS2-NHK8V8). */
  chooseHostDisk: action('choose-host-disk'),
} as const;

/**
 * Other delegated targets (components, named fields, flags) for
 * the app shell, rails, resizing, and global ticket drag.
 * Markup spreads `.attrs` where it renders a literal; handlers use `.selector`.
 */
export const SHELL_AND_GLOBAL_TARGETS = {
  duplicateTicketDropAction: attr('data-ticket-drop-action', 'duplicate'),
} as const;
