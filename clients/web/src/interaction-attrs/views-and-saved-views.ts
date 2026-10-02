import { attr } from 'kerfjs';
import { action } from 'kerfjs/actions';

/**
 * `data-action` specs: ticket views and saved views.
 * Markup spreads `.attrs`; the delegated handlers in
 * `interactions/views-and-saved-views.ts` register `.selector`.
 */
export const VIEWS_AND_SAVED_VIEWS_ACTIONS = {
  selectView: action('select-view'),
  addView: action('add-view'),
  openSavedViewMenu: action('open-saved-view-menu'),
  editSavedView: action('edit-saved-view'),
  deleteSavedView: action('delete-saved-view'),
  saveSavedView: action('save-saved-view'),
  cancelSavedView: action('cancel-saved-view'),
  confirmDeleteSavedView: action('confirm-delete-saved-view'),
  cancelDeleteSavedView: action('cancel-delete-saved-view'),
} as const;

/**
 * Other delegated targets (components, named fields, flags) for
 * ticket views and saved views.
 * Markup spreads `.attrs` where it renders a literal; handlers use `.selector`.
 */
export const VIEWS_AND_SAVED_VIEWS_TARGETS = {
  ticketSelectionRoot: attr('data-ticket-selection-root', 'true'),
  savedViewNameField: attr('name', 'saved-view-name'),
  savedViewDialog: attr('data-component', 'saved-view-dialog'),
  savedViewDeleteDialog: attr('data-component', 'saved-view-delete-dialog'),
} as const;
