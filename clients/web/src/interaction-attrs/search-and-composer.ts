import { attr } from 'kerfjs';
import { action } from 'kerfjs/actions';

/**
 * `data-action` specs: workspace search, sort, and the quick ticket composer.
 * Markup spreads `.attrs`; the delegated handlers in
 * `interactions/search-and-composer.ts` register `.selector`.
 */
export const SEARCH_AND_COMPOSER_ACTIONS = {
  expandTicketComposer: action('expand-ticket-composer'),
  toggleNewTicketUpNext: action('toggle-new-ticket-up-next'),
  removeNewTicketAttachment: action('remove-new-ticket-attachment'),
  clearNewTicketAttachments: action('clear-new-ticket-attachments'),
  createTicketForm: action('create-ticket-form'),
  toggleRowUpNext: action('toggle-row-up-next'),
} as const;

/**
 * Other delegated targets (components, named fields, flags) for
 * workspace search, sort, and the quick ticket composer.
 * Markup spreads `.attrs` where it renders a literal; handlers use `.selector`.
 */
export const SEARCH_AND_COMPOSER_TARGETS = {
  quickTicketComposer: attr('data-component', 'quick-ticket-composer'),
  newTicketTitleField: attr('name', 'new-ticket-title'),
  newTicketDetailsField: attr('name', 'new-ticket-details'),
  newTicketCategoryField: attr('name', 'new-ticket-category'),
  newTicketSourceField: attr('name', 'new-ticket-source'),
  newTicketDropTarget: attr('data-new-ticket-drop-target', 'true'),
} as const;
