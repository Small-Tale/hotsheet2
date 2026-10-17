/**
 * One accessible dialog per raw Web Awesome `wa-dialog` whose host carries the dialog semantics
 * (`role="dialog"`, `aria-modal`, `aria-label`) for the ticket reader, the quick ticket composer,
 * and the Not Working dialog.
 *
 * The host is the dialog the app and its tests address, since its light-DOM content is not inside the
 * shadow `<dialog part="dialog">`. That inner element therefore renders as `role="presentation"`.
 * Web Awesome 3.14 also labels the inner element (`aria-label`, or `aria-labelledby` its header title),
 * and a global ARIA attribute overrides a presentational role, which would expose a second, duplicate
 * named dialog (HS2-BH7NZ4). Strip those labels, and keep stripping them when Web Awesome re-renders
 * a changed label.
 */
const quieted = new WeakSet<Element>();

function quiet(dialog: Element) {
  if (dialog.getAttribute('role') !== 'presentation') dialog.setAttribute('role', 'presentation');
  if (dialog.hasAttribute('aria-label')) dialog.removeAttribute('aria-label');
  if (dialog.hasAttribute('aria-labelledby')) dialog.removeAttribute('aria-labelledby');
}

/**
 * Make the host the only exposed dialog: quiet the shadow `<dialog>` now and on later re-renders.
 * Before Web Awesome's first render there is no shadow dialog yet; quiet it once that render settles.
 */
export function quietWebAwesomeNativeDialog(host: Element): HTMLDialogElement | null {
  const dialog = host.shadowRoot?.querySelector('dialog') ?? null;
  if (!dialog) {
    const pending = (host as Element & { updateComplete?: Promise<unknown> }).updateComplete;
    if (pending && !quieted.has(host)) {
      quieted.add(host);
      void pending.then(() => {
        quieted.delete(host);
        if (host.shadowRoot?.querySelector('dialog')) quietWebAwesomeNativeDialog(host);
      });
    }
    return null;
  }
  quiet(dialog);
  if (!quieted.has(dialog) && typeof MutationObserver !== 'undefined') {
    quieted.add(dialog);
    new MutationObserver(() => {
      quiet(dialog);
    }).observe(dialog, { attributes: true, attributeFilter: ['role', 'aria-label', 'aria-labelledby'] });
  }
  return dialog;
}
