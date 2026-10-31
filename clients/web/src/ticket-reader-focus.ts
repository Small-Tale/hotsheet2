/** Set initial reader focus after its native dialog has opened (HS2-XBWGP6). */
export function focusTopTicketReader(root: Pick<Document, 'querySelector' | 'activeElement'> = document) {
  requestAnimationFrame(() =>
    requestAnimationFrame(() => {
      const reader = root.querySelector<HTMLElement>('[data-component="ticket-reader"][data-reader-active="true"]');
      // The user can already be typing by this frame; initial focus must not replace that choice.
      if (!reader || reader.contains(root.activeElement)) return;
      reader.querySelector<HTMLElement>('[data-action="close-ticket-reader"]')?.focus({ preventScroll: true });
    }),
  );
}

/** HS2-FRFPA5: return focus only while no newer visible control owns it. */
export function restoreTicketReaderFocus(
  trigger: HTMLElement | undefined,
  slug: string,
  root: Pick<Document, 'querySelector' | 'querySelectorAll' | 'activeElement' | 'body' | 'documentElement'> = document,
) {
  setTimeout(() => {
    const active = root.activeElement;
    if (
      active &&
      active !== root.body &&
      active !== root.documentElement &&
      active.isConnected &&
      active.getClientRects().length > 0
    )
      return;
    if (trigger?.isConnected) {
      trigger.focus({ preventScroll: true });
      return;
    }
    const row = [
      ...root.querySelectorAll<HTMLElement>(
        `[data-action="select-ticket-row"][data-ticket-slug="${CSS.escape(slug)}"]`,
      ),
    ].find((item) => item.offsetParent !== null);
    if (row) {
      row.focus({ preventScroll: true });
      return;
    }
    const inspector = root.querySelector<HTMLElement>('#app-right-rail [data-action="open-ticket-reader"]');
    if (inspector) {
      inspector.focus({ preventScroll: true });
      return;
    }
    root.querySelector<HTMLElement>('[data-work-area-focus-owner]')?.focus({ preventScroll: true });
  }, 0);
}
