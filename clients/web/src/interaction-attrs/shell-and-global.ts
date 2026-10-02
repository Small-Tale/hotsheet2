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
} as const;

/**
 * Other delegated targets (components, named fields, flags) for
 * the app shell, rails, resizing, and global ticket drag.
 * Markup spreads `.attrs` where it renders a literal; handlers use `.selector`.
 */
export const SHELL_AND_GLOBAL_TARGETS = {
  duplicateTicketDropAction: attr('data-ticket-drop-action', 'duplicate'),
} as const;
