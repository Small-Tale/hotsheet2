import { wireFilePickers } from '@kerfjs/ui/wire-file-pickers';
import { delegate, delegateCapture } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { NEW_TICKET_ATTACHMENT_PICKER } from './quick-ticket-composer';

const COMPOSER = '[data-component="quick-ticket-composer"]';
const DROP_TARGET = '[data-new-ticket-drop-target="true"]';
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
    on = (event: string, selector: string, handle: (event: Event, target: Element) => void) => {
      lifetime.add(delegate(root, event, selector, handle));
    },
    draggingTickets = () => bindings.draggingTickets?.() ?? false;
  on('click', '[data-action="expand-ticket-composer"]', (_event, target) => {
    bindings.expand(target as HTMLElement);
  });
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
  on('input', '[name="new-ticket-title"]', (_event, target) => {
    bindings.setTitle((target as Control).value);
  });
  on('input', '[name="new-ticket-details"]', (_event, target) => {
    bindings.setDetails((target as Control).value);
  });
  on('change', '[name="new-ticket-category"]', (_event, target) => {
    bindings.setCategory((target as Control).value);
  });
  on('change', '[name="new-ticket-source"]', (_event, target) => {
    bindings.setSource((target as Control).value || undefined);
  });
  on('click', '[data-action="toggle-new-ticket-up-next"]', () => {
    bindings.toggleUpNext();
  });
  on('change', '.quick-ticket-composer__drop input[name="new-ticket-attachments"]', (_event, target) => {
    const input = target as HTMLInputElement;
    if (input.files?.length) bindings.addFiles(input.files);
    input.value = '';
  });
  // The header's Kerf FilePickerTrigger reports through its documented wiring contract (HS2-ZWRCAZ).
  lifetime.add(
    wireFilePickers(root, {
      onFiles: ({ id, files }) => {
        if (id === NEW_TICKET_ATTACHMENT_PICKER && files.length) bindings.addFiles([...files]);
      },
    }),
  );
  on('click', '[data-action="remove-new-ticket-attachment"]', (_event, target) => {
    bindings.removeAttachment((target as HTMLElement).dataset.pendingAttachmentId);
  });
  on('click', '[data-action="clear-new-ticket-attachments"]', () => {
    bindings.clearAttachments();
  });
  on('dragover', DROP_TARGET, (event, target) => {
    if (draggingTickets()) return;
    event.preventDefault();
    (target as HTMLElement).dataset.dragging = 'true';
  });
  on('dragleave', DROP_TARGET, (_event, target) => {
    delete (target as HTMLElement).dataset.dragging;
  });
  on('drop', DROP_TARGET, (event, target) => {
    if (draggingTickets()) return;
    event.preventDefault();
    event.stopPropagation();
    delete (target as HTMLElement).dataset.dragging;
    const files = (event as DragEvent).dataTransfer?.files;
    if (!files?.length) return;
    if (!bindings.expanded()) bindings.expand(target as HTMLElement);
    bindings.addFiles(files);
  });
  on('submit', '[data-action="create-ticket-form"]', (event) => {
    event.preventDefault();
    bindings.submit();
  });
  return () => {
    lifetime.dispose();
  };
}
