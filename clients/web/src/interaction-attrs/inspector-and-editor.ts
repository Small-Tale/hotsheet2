import { attr } from 'kerfjs';
import { action } from 'kerfjs/actions';

/**
 * `data-action` specs: the ticket inspector, reader, notes, and inline editors.
 * Markup spreads `.attrs`; the delegated handlers in
 * `interactions/inspector-and-editor.ts` register `.selector`.
 */
export const INSPECTOR_AND_EDITOR_ACTIONS = {
  toggleInspectorUpNext: action('toggle-inspector-up-next'),
  copyTicketSlug: action('copy-ticket-slug'),
  acceptRemoteTicketField: action('accept-remote-ticket-field'),
  applyTicketFieldMerge: action('apply-ticket-field-merge'),
  editTicketTitle: action('edit-ticket-title'),
  editMarkdown: action('edit-markdown'),
  addTicketNote: action('add-ticket-note'),
  cancelNewNote: action('cancel-new-note'),
  createNoteForm: action('create-note-form'),
  addInlineFeedbackReply: action('add-inline-feedback-reply'),
  removeInlineFeedbackReply: action('remove-inline-feedback-reply'),
  toggleFeedbackChoice: action('toggle-feedback-choice'),
  saveNoteEdit: action('save-note-edit'),
  dismissFeedback: action('dismiss-feedback'),
  deleteNote: action('delete-note'),
  loadNextTicketPage: action('load-next-ticket-page'),
  editBlockedReason: action('edit-blocked-reason'),
  openTicketReader: action('open-ticket-reader'),
  respondToFeedback: action('respond-to-feedback'),
  toggleReaderTextSize: action('toggle-reader-text-size'),
  setInspectorTab: action('set-inspector-tab'),
  openCodeReview: action('open-code-review'),
} as const;

/**
 * Other delegated targets (components, named fields, flags) for
 * the ticket inspector, reader, notes, and inline editors.
 * Markup spreads `.attrs` where it renders a literal; handlers use `.selector`.
 */
export const INSPECTOR_AND_EDITOR_TARGETS = {
  inspectorCategoryField: attr('name', 'inspector-category'),
  inspectorPriorityField: attr('name', 'inspector-priority'),
  inspectorStatusField: attr('name', 'inspector-status'),
  inspectorStartedPhaseField: attr('name', 'inspector-started-phase'),
  ticketConflictResolutionField: attr('name', 'ticket-conflict-resolution'),
  ticketTitleField: attr('name', 'ticket-title'),
  ticketTagInputField: attr('name', 'ticket-tag-input'),
  markdownSourceField: attr('name', 'markdown-source'),
  newNoteBodyField: attr('name', 'new-note-body'),
  editOnClick: attr('data-edit-on-click', 'true'),
  noteBodyField: attr('name', 'note-body'),
  inlineFeedbackResponseField: attr('name', 'inline-feedback-response'),
  editBlockedReason: attr('data-edit-blocked-reason', 'true'),
  blockedReasonField: attr('name', 'blocked-reason'),
  ticketReader: attr('data-component', 'ticket-reader'),
  ticketInspectorHeader: attr('data-component', 'ticket-inspector-header'),
  ticketInspectorBody: attr('data-component', 'ticket-inspector-body'),
} as const;
