import { attr } from 'kerfjs';
import { action } from 'kerfjs/actions';

/**
 * `data-action` specs: ticket rows, bulk actions, close, and Not Working flows.
 * Markup spreads `.attrs`; the delegated handlers in
 * `interactions/ticket-selection.ts` register `.selector`.
 */
export const TICKET_SELECTION_ACTIONS = {
  selectTicketRow: action('select-ticket-row'),
  selectTicketColumn: action('select-ticket-column'),
  toggleSelectedUpNext: action('toggle-selected-up-next'),
  openSelectedTicketActions: action('open-selected-ticket-actions'),
  submitBulkTag: action('submit-bulk-tag'),
  chooseBulkTag: action('choose-bulk-tag'),
  cancelBulkTicketAction: action('cancel-bulk-ticket-action'),
  openEmptyTrash: action('open-empty-trash'),
  confirmEmptyTrash: action('confirm-empty-trash'),
  selectTicketCloseTarget: action('select-ticket-close-target'),
  clearTicketCloseTarget: action('clear-ticket-close-target'),
  submitTicketClose: action('submit-ticket-close'),
  cancelTicketClose: action('cancel-ticket-close'),
  openDuplicateTarget: action('open-duplicate-target'),
  confirmBulkDelete: action('confirm-bulk-delete'),
  removeNotWorkingAttachment: action('remove-not-working-attachment'),
  submitNotWorking: action('submit-not-working'),
  cancelNotWorking: action('cancel-not-working'),
} as const;

/**
 * Other delegated targets (components, named fields, flags) for
 * ticket rows, bulk actions, close, and Not Working flows.
 * Markup spreads `.attrs` where it renders a literal; handlers use `.selector`.
 */
export const TICKET_SELECTION_TARGETS = {
  reportNotWorkingContextAction: attr('data-context-action', 'Report not working'),
  reopenTicketContextAction: attr('data-context-action', 'Reopen ticket'),
  closeTicketContextAction: attr('data-context-action', 'Close ticket'),
  ticketCloseReasonField: attr('name', 'ticket-close-reason'),
  ticketCloseTargetSearchField: attr('name', 'ticket-close-target-search'),
  ticketCloseDialog: attr('data-component', 'ticket-close-dialog'),
  notWorkingNoteField: attr('name', 'not-working-note'),
  notWorkingDropzone: attr('data-not-working-dropzone', 'true'),
  notWorkingDialog: attr('data-component', 'not-working-dialog'),
} as const;
