/**
 * Controlled `open` for custom elements (HS2-KMDJRH).
 *
 * Since kerfjs 5.0.0-beta.51 (KF-900A8V) the morph treats `open` on a custom element as
 * user-agent-owned: a template can set it, but omitting it no longer removes it, so a controlled
 * `<wa-dialog open={state}>` could open but never close. A controlled element also renders
 * `data-controlled-open={String(state)}`, and one post-render pass drives the live `open` to that
 * state. Uncontrolled popups (a user-opened `wa-select`) carry no marker and keep Kerf's protection.
 */
export const CONTROLLED_OPEN_ATTRIBUTE = 'data-controlled-open';

type OpenableElement = Element & { open?: boolean };

/** Drive every marked element under `root` to its declared open state. */
export function syncControlledOpen(root: ParentNode): void {
  for (const element of root.querySelectorAll<OpenableElement>(`[${CONTROLLED_OPEN_ATTRIBUTE}]`)) {
    const desired = element.getAttribute(CONTROLLED_OPEN_ATTRIBUTE) === 'true';
    if (typeof element.open === 'boolean') {
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
