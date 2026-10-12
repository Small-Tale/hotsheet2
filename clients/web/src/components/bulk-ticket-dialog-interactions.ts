import { delegate } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { TICKET_SELECTION_ACTIONS } from '../interaction-attrs/ticket-selection';

/**
 * Side effects for the BulkTicketDialog actions (HS2-RWY78R). Production passes the bulk ticket
 * operations; the UX demo passes callbacks that report the request, so both run the same handlers.
 */
export interface BulkTicketDialogBindings {
  readonly submitTag: (mode: 'add' | 'remove', tag: string) => void;
  readonly confirmDelete: () => void;
  readonly confirmEmptyTrash: () => void;
  /** Cancel, or the dialog hid by any other route; the selection is unchanged. */
  readonly dismiss: (reason: 'cancel' | 'hide') => void;
}

const dialogSelector =
  '[data-component="bulk-tag-dialog"], [data-component="bulk-delete-dialog"], [data-component="empty-trash-dialog"]';

/** Wire the bulk tag, delete, and empty-trash dialog actions under `root`. */
export function wireBulkTicketDialog(root: HTMLElement, bindings: BulkTicketDialogBindings): () => void {
  const lifetime = createScope();
  lifetime.add(
    delegate(root, 'submit', TICKET_SELECTION_ACTIONS.submitBulkTag.selector, (event, target) => {
      event.preventDefault();
      const value = target.querySelector<HTMLInputElement>('[name="bulk-ticket-tag"]')?.value ?? '',
        mode = (target as HTMLElement).dataset.tagMode;
      if (mode === 'add' || mode === 'remove') bindings.submitTag(mode, value);
    }),
  );
  lifetime.add(
    delegate(root, 'click', TICKET_SELECTION_ACTIONS.chooseBulkTag.selector, (_event, target) => {
      const input = root.querySelector<HTMLInputElement>('[name="bulk-ticket-tag"]');
      if (!input) return;
      input.value = (target as HTMLElement).dataset.tag ?? '';
      input.focus();
    }),
  );
  lifetime.add(
    delegate(root, 'click', TICKET_SELECTION_ACTIONS.cancelBulkTicketAction.selector, () => {
      bindings.dismiss('cancel');
    }),
  );
  lifetime.add(
    delegate(root, 'click', TICKET_SELECTION_ACTIONS.confirmEmptyTrash.selector, () => {
      bindings.confirmEmptyTrash();
    }),
  );
  lifetime.add(
    delegate(root, 'click', TICKET_SELECTION_ACTIONS.confirmBulkDelete.selector, () => {
      bindings.confirmDelete();
    }),
  );
  lifetime.add(
    delegate(root, 'wa-hide', dialogSelector, (event, target) => {
      // A nested Web Awesome popup (tooltip, dropdown) also emits wa-hide; only the dialog's own counts.
      if (event.target !== target) return;
      bindings.dismiss('hide');
    }),
  );
  return () => {
    lifetime.dispose();
  };
}
