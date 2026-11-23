/**
 * Controlled `open` for custom elements and Kerf dialogs (HS2-KMDJRH, HS2-BH7NZ4).
 *
 * Since kerfjs 5.0.0-beta.51 (KF-900A8V) the morph treats `open` on a custom element as
 * user-agent-owned: a template can set it, but omitting it no longer removes it, so a controlled
 * `<wa-dialog open={state}>` could open but never close. A controlled element also renders
 * `data-controlled-open={String(state)}`, and one post-render pass drives the live `open` to that
 * state. Uncontrolled popups carry no marker and keep Kerf's protection.
 *
 * A Kerf `DialogSurface` (a native `<dialog class="kui-dialog-surface">` since Kerf UI
 * 5.0.0-beta.106) must not have its `open` toggled, which would show it without modality: the pass
 * opens it with `openDialog` (modal, top layer, enter motion) and closes it with `closeDialog` (exit
 * motion). Both are idempotent, and opening a closing dialog reverses its exit, so the pass is safe
 * after every render. A user's close request (`kui-request-close`) must therefore clear the app
 * state in the same turn, or the next render would reopen the dialog.
 */
import { closeDialog, openDialog } from '@kerfjs/ui/surface-scaffold';

import { quietWebAwesomeNativeDialog } from './wa-dialog-semantics';

export const CONTROLLED_OPEN_ATTRIBUTE = 'data-controlled-open';

type OpenableElement = Element & { open?: boolean };

const isDialogSurface = (element: Element): element is HTMLDialogElement =>
  element.localName === 'dialog' && element.classList.contains('kui-dialog-surface');

const reopenPending = new WeakSet<Element>();

/**
 * Web Awesome's `requestClose` keeps the host's `open` property `true` through its hide animation
 * (at least one frame), then sets it `false` and closes the shadow dialog. A controlled reopen
 * declared during that window matches the live property, so writing `open` is a no-op and the
 * dialog would end closed while the app says it is open: a failed Not Working submission reopening
 * the dialog it closed while submitting lost its report and error that way (HS2-6TJRYX). Re-assert
 * the declared state once that hide finishes; a close declared meanwhile leaves the dialog closed.
 */
function reopenAfterInterruptedHide(element: OpenableElement): void {
  if (reopenPending.has(element)) return;
  const native = element.shadowRoot?.querySelector('dialog');
  if (!native?.open || !native.classList.contains('hide')) return;
  reopenPending.add(element);
  const afterHide = (event: Event) => {
    // Nested Web Awesome popups bubble their own wa-after-hide through the dialog.
    if (event.target !== element) return;
    element.removeEventListener('wa-after-hide', afterHide);
    reopenPending.delete(element);
    if (element.isConnected && element.getAttribute(CONTROLLED_OPEN_ATTRIBUTE) === 'true' && !element.open)
      element.open = true;
  };
  element.addEventListener('wa-after-hide', afterHide);
}

/** Drive every marked element under `root` to its declared open state. */
export function syncControlledOpen(root: ParentNode): void {
  for (const element of root.querySelectorAll<OpenableElement>(`[${CONTROLLED_OPEN_ATTRIBUTE}]`)) {
    const desired = element.getAttribute(CONTROLLED_OPEN_ATTRIBUTE) === 'true';
    if (isDialogSurface(element)) {
      // A detached dialog cannot open modally; the next render after it connects retries.
      if (!desired) closeDialog(element);
      else if (element.isConnected) {
        openDialog(element);
        // App morphs can remove Kerf's live aria-value* attributes from an unchanged resize
        // handle. Replacing that handle makes Kerf's own child observer rewire and restore them.
        if (element.getAttribute('data-presentation') === 'bottom-sheet' && element.hasAttribute('data-resizable')) {
          const handle = element.querySelector<HTMLElement>(':scope > .kui-dialog-surface__sheet-handle');
          if (handle && !handle.hasAttribute('aria-valuenow')) {
            const focused = element.ownerDocument.activeElement === handle;
            const replacement = handle.cloneNode(true) as HTMLElement;
            handle.replaceWith(replacement);
            if (focused) replacement.focus({ preventScroll: true });
          }
        }
      }
    } else if (typeof element.open === 'boolean') {
      if (element.localName === 'wa-dialog') {
        // A raw Web Awesome dialog whose host is the exposed dialog keeps its shadow dialog unnamed.
        if (element.getAttribute('role') === 'dialog') quietWebAwesomeNativeDialog(element);
        if (desired) reopenAfterInterruptedHide(element);
      }
      if (element.open !== desired) element.open = desired;
    } else if (element.hasAttribute('open') !== desired) element.toggleAttribute('open', desired);
  }
}

/** Wrap a `mount` render function so each render is followed by a controlled-open sync of `root`. */
export function withControlledOpen<Result>(root: ParentNode, render: () => Result): () => Result {
  return () => {
    const result = render();
    // `mount` morphs synchronously after render returns; sync once the DOM matches the template.
    queueMicrotask(() => {
      syncControlledOpen(root);
    });
    return result;
  };
}
