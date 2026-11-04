import { delegate, delegateCapture } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import type { ConfirmationWorkflow } from './confirmation';
import { CONFIRMATION_ACTIONS as actions, CONFIRMATION_TARGETS as targets } from './interaction-attrs/confirmation';

/** The request a button or dialog was rendered for. */
function requestId(element: Element) {
  const id = Number((element as HTMLElement).dataset.confirmationId);
  return Number.isInteger(id) && id > 0 ? id : undefined;
}

/**
 * Answer the shared confirmation from its dialog (HS2-6S1346): the confirming button confirms; Cancel,
 * Escape, and the close button decline. The same delegates serve production and the UX catalog.
 */
export function wireConfirmation(root: HTMLElement, confirmation: ConfirmationWorkflow) {
  const scope = createScope();
  scope.add(
    delegate(root, 'click', actions.confirm.selector, (_event, target) => {
      confirmation.confirm(requestId(target));
    }),
  );
  scope.add(
    delegate(root, 'click', actions.cancel.selector, (_event, target) => {
      confirmation.cancel(requestId(target));
    }),
  );
  // Escape and the close button ask to close; clearing the request in the same turn lets the next
  // render close the dialog instead of reopening it.
  scope.add(
    delegateCapture(root, 'kui-request-close', targets.dialog.selector, (event, target) => {
      if (event.target !== target) return;
      confirmation.cancel(requestId(target));
    }),
  );
  return () => {
    scope.dispose();
  };
}
