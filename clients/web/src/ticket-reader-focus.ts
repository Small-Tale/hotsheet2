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
