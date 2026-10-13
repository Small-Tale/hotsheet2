import { delegate, delegateCapture } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { routeFilePicker } from '../file-picker-routes';
import { SEARCH_AND_COMPOSER_ACTIONS, SEARCH_AND_COMPOSER_TARGETS } from '../interaction-attrs/search-and-composer';
import { NEW_TICKET_ATTACHMENT_PICKER } from './quick-ticket-composer';

const COMPOSER = SEARCH_AND_COMPOSER_TARGETS.quickTicketComposer.selector;
/** Kerf's FileDropZone stages its own drops through `wireFilePickers` (see `routeFilePicker`). */
const FILE_DROP_ZONE = '[data-component="file-drop-zone"]';
const DROP_TARGET = SEARCH_AND_COMPOSER_TARGETS.newTicketDropTarget.selector;
type Control = HTMLElement & { value: string };

/**
 * Side effects for the QuickTicketComposer (HS2-NRW5ZR). Production passes the composer draft signals,
 * session persistence, draft-file staging, and ticket creation; the UX demo passes fixture callbacks,
 * so both resolve the same expand, dismissal, field, attachment, drop, and submit gestures.
 */
export interface QuickTicketComposerBindings {
  /** Open the composer from its collapsed trigger (or a drop on a collapsed composer). */
  readonly expand: (trigger: HTMLElement) => void;
  readonly expanded: () => boolean;
  /** False while a submission is in flight, so the popup refuses to hide. */
  readonly canHide: () => boolean;
  /** The composer popup finished hiding while expanded: reset the draft. */
  readonly dismissed: () => void;
  readonly setTitle: (title: string) => void;
  readonly setDetails: (details: string) => void;
  readonly setCategory: (category: string) => void;
  /** The destination source pick; an empty pick is undefined. */
  readonly setSource: (source: string | undefined) => void;
  readonly toggleUpNext: () => void;
  /** Stage picked or dropped files on the new ticket. */
  readonly addFiles: (files: FileList | File[]) => void;
  readonly removeAttachment: (id: string | undefined) => void;
  readonly clearAttachments: () => void;
  /** True while tickets (not files) are being dragged, so the drop target ignores them. */
  readonly draggingTickets?: () => boolean;
  readonly submit: () => void;
}

/** Wire every QuickTicketComposer under `root`. */
export function wireQuickTicketComposer(root: HTMLElement, bindings: QuickTicketComposerBindings): () => void {
  const lifetime = createScope(),
    draggingTickets = () => bindings.draggingTickets?.() ?? false;
  lifetime.add(
    delegate(root, 'click', SEARCH_AND_COMPOSER_ACTIONS.expandTicketComposer.selector, (_event, target) => {
      bindings.expand(target as HTMLElement);
    }),
  );
  lifetime.add(
    delegateCapture(root, 'wa-hide', COMPOSER, (event, target) => {
      if (event.target === target && !bindings.canHide()) event.preventDefault();
    }),
  );
  lifetime.add(
    delegateCapture(root, 'wa-after-hide', COMPOSER, (event, target) => {
      if (event.target === target && bindings.expanded()) bindings.dismissed();
    }),
  );
  lifetime.add(
    delegate(root, 'input', SEARCH_AND_COMPOSER_TARGETS.newTicketTitleField.selector, (_event, target) => {
      bindings.setTitle((target as Control).value);
    }),
  );
  lifetime.add(
    delegate(root, 'input', SEARCH_AND_COMPOSER_TARGETS.newTicketDetailsField.selector, (_event, target) => {
      bindings.setDetails((target as Control).value);
    }),
  );
  lifetime.add(
    delegate(root, 'change', SEARCH_AND_COMPOSER_TARGETS.newTicketCategoryField.selector, (_event, target) => {
      bindings.setCategory((target as Control).value);
    }),
  );
  lifetime.add(
    delegate(root, 'change', SEARCH_AND_COMPOSER_TARGETS.newTicketSourceField.selector, (_event, target) => {
      bindings.setSource((target as Control).value || undefined);
    }),
  );
  lifetime.add(
    delegate(root, 'click', SEARCH_AND_COMPOSER_ACTIONS.toggleNewTicketUpNext.selector, () => {
      bindings.toggleUpNext();
    }),
  );
  // The header's Kerf FilePickerTrigger and the FileDropZone report through Kerf's wiring contract
  // (HS2-ZWRCAZ, HS2-RB7KXZ); the zone's own drops are left to it so files are staged once.
  lifetime.add(
    routeFilePicker(root, NEW_TICKET_ATTACHMENT_PICKER, (files) => {
      bindings.addFiles([...files]);
    }),
  );
  lifetime.add(
    delegate(root, 'click', SEARCH_AND_COMPOSER_ACTIONS.removeNewTicketAttachment.selector, (_event, target) => {
      bindings.removeAttachment((target as HTMLElement).dataset.pendingAttachmentId);
    }),
  );
  lifetime.add(
    delegate(root, 'click', SEARCH_AND_COMPOSER_ACTIONS.clearNewTicketAttachments.selector, () => {
      bindings.clearAttachments();
    }),
  );
  lifetime.add(
    delegate(root, 'dragover', DROP_TARGET, (event, target) => {
      if (draggingTickets()) return;
      event.preventDefault();
      (target as HTMLElement).dataset.dragging = 'true';
    }),
  );
  lifetime.add(
    delegate(root, 'dragleave', DROP_TARGET, (_event, target) => {
      delete (target as HTMLElement).dataset.dragging;
    }),
  );
  lifetime.add(
    delegate(root, 'drop', DROP_TARGET, (event, target) => {
      if (draggingTickets() || (event.target as Element).closest(FILE_DROP_ZONE)) return;
      event.preventDefault();
      event.stopPropagation();
      delete (target as HTMLElement).dataset.dragging;
      const files = (event as DragEvent).dataTransfer?.files;
      if (!files?.length) return;
      if (!bindings.expanded()) bindings.expand(target as HTMLElement);
      bindings.addFiles(files);
    }),
  );
  lifetime.add(
    delegate(root, 'submit', SEARCH_AND_COMPOSER_ACTIONS.createTicketForm.selector, (event) => {
      event.preventDefault();
      bindings.submit();
    }),
  );
  return () => {
    lifetime.dispose();
  };
}
