/**
 * Overlays that must paint above every app surface (the magnified terminal, HS2-Z9PQSC, and the shell's
 * permission popup, HS2-ZESCM2)
 * render as manual popovers so the browser lifts them into the top layer. A `position: fixed` overlay
 * inside the shell is trapped by Kerf's Workbench (its root isolates a stacking context and its regions
 * clip), so it ended up clipped to the terminal drawer or painted under the side rails.
 *
 * A manual popover only enters the top layer when `showPopover()` runs, and Kerf renders the overlay
 * declaratively, so this watcher opens every marked overlay as soon as it is in the DOM. Closing is the
 * app's own state change: the element is removed, which also removes it from the top layer.
 */
export const TOP_LAYER_OVERLAY_ATTRIBUTE = 'data-top-layer-overlay';

type PopoverElement = HTMLElement & { showPopover?: () => void };

/** Open every marked overlay under `root` that is not yet in the top layer. */
export function openTopLayerOverlays(root: ParentNode): void {
  for (const overlay of root.querySelectorAll<PopoverElement>(`[${TOP_LAYER_OVERLAY_ATTRIBUTE}][popover]`)) {
    if (!overlay.isConnected || overlay.matches(':popover-open') || typeof overlay.showPopover !== 'function') continue;
    overlay.showPopover();
  }
}

/** Keep marked overlays under `root` in the top layer as Kerf renders them; returns a disposer. */
export function wireTopLayerOverlays(root: HTMLElement): () => void {
  const observer = new MutationObserver(() => {
    openTopLayerOverlays(root);
  });
  observer.observe(root, { childList: true, subtree: true });
  openTopLayerOverlays(root);
  return () => {
    observer.disconnect();
  };
}
