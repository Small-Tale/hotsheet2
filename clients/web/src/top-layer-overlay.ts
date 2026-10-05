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
 *
 * A modal dialog (such as the Create ticket `wa-dialog`) makes everything outside it inert, a top-layer
 * popover included: the popover still paints above the dialog, but clicks fall through to the dialog
 * beneath (HS2-MAE27T). A marked overlay that is a `<dialog>` is therefore lifted with `showModal()`
 * above every open modal dialog, re-lifted when a newer one opens, and returned to a non-blocking
 * popover once none remains. Escape cannot cancel it, so the app's state stays the only way to close it.
 *
 * While an overlay is lifted it is the topmost surface, so Escape belongs to it, not to the modal beneath:
 * the watcher stops the key before Web Awesome's document-level handler closes that modal, and clicks the
 * overlay's `[data-top-layer-dismiss]` control (the permission popup's client-only Ignore) if it has one
 * (HS2-S8K9BG).
 */
export const TOP_LAYER_OVERLAY_ATTRIBUTE = 'data-top-layer-overlay';
/** Marks the control inside an overlay that Escape activates while the overlay is lifted above a modal. */
export const TOP_LAYER_DISMISS_ATTRIBUTE = 'data-top-layer-dismiss';

const OVERLAY_SELECTOR = `[${TOP_LAYER_OVERLAY_ATTRIBUTE}][popover]`;
/** Native dialogs plus the Web Awesome hosts whose modal dialog lives in their shadow root. */
const MODAL_HOST_SELECTOR = 'dialog, wa-dialog[open], wa-drawer[open]';
/** Web Awesome opens and closes its modal dialog after these events, outside any DOM mutation. */
const MODAL_LIFECYCLE_EVENTS = ['wa-show', 'wa-after-show', 'wa-after-hide'] as const;
const EDITING_CONTROL_SELECTOR = 'input, textarea, select, [role="textbox"], [role="combobox"]';

type OverlayElement = HTMLElement & {
  showModal?: () => void;
  close?: () => void;
};

/** The foreign modal dialogs each overlay was last lifted above. */
const liftedAbove = new WeakMap<Element, ReadonlySet<Element>>();

function isOverlay(target: EventTarget | null): target is OverlayElement {
  return (target as Partial<Element> | null)?.hasAttribute?.(TOP_LAYER_OVERLAY_ATTRIBUTE) === true;
}

/** Follow focus into custom-element shadow roots, where Web Awesome keeps its form controls. */
function isEditingFieldFocused(root: ParentNode): boolean {
  let active = (root as Node).ownerDocument?.activeElement;
  while (active) {
    if (active.matches(EDITING_CONTROL_SELECTOR) || (active as HTMLElement).isContentEditable) return true;
    active = active.shadowRoot?.activeElement ?? null;
  }
  return false;
}

/** Every open modal dialog under `root` other than a marked overlay, as its host element. */
export function openModalDialogs(root: ParentNode): Element[] {
  const modals: Element[] = [];
  for (const host of root.querySelectorAll<HTMLElement>(MODAL_HOST_SELECTOR)) {
    if (isOverlay(host)) continue;
    const dialog = host.localName === 'dialog' ? host : host.shadowRoot?.querySelector('dialog');
    if (dialog?.matches(':modal')) modals.push(host);
  }
  return modals;
}

/** Lift `overlay` above `modals`, unless it already sits above exactly those (or a subset). */
function liftAboveModals(overlay: OverlayElement, modals: readonly Element[]): void {
  const lifted = liftedAbove.get(overlay),
    modal = overlay.matches(':modal');
  if (modal && lifted && modals.every((host) => lifted.has(host))) return;
  if (overlay.matches(':popover-open')) overlay.hidePopover();
  if (modal) overlay.close?.();
  overlay.showModal?.();
  // Browsers ignore `autofocus` on the dialog itself and focus its first focusable descendant, which
  // could be a decision button armed for Enter; the overlay takes focus itself instead.
  overlay.focus({ preventScroll: true });
  liftedAbove.set(overlay, new Set(modals));
}

/** Open every marked overlay under `root` in the top layer, above any open modal dialog it can cover. */
export function openTopLayerOverlays(root: ParentNode): void {
  let modals: Element[] | undefined;
  for (const overlay of root.querySelectorAll<OverlayElement>(OVERLAY_SELECTOR)) {
    if (!overlay.isConnected) continue;
    if (typeof overlay.showModal === 'function') {
      modals ??= openModalDialogs(root);
      if (modals.length > 0) {
        // A modal lift moves focus into this dialog. Keep a pending request out of the way while
        // someone is editing another modal's form, then lift it when editing focus leaves.
        if (isEditingFieldFocused(root)) continue;
        liftAboveModals(overlay, modals);
        continue;
      }
      if (overlay.matches(':modal')) overlay.close?.();
      liftedAbove.delete(overlay);
    }
    if (overlay.matches(':popover-open') || typeof overlay.showPopover !== 'function') continue;
    overlay.showPopover();
  }
}

/**
 * Give Escape to the topmost lifted overlay: stop it before any modal beneath sees it (and before the
 * overlay's own `cancel`), then activate the overlay's dismiss control. Returns whether it handled the key.
 */
export function dismissLiftedOverlay(root: ParentNode, event: KeyboardEvent): boolean {
  if (event.key !== 'Escape' || event.isComposing) return false;
  const lifted = [...root.querySelectorAll<OverlayElement>(OVERLAY_SELECTOR)].filter((overlay) =>
    overlay.matches(':modal'),
  );
  const overlay = lifted.at(-1);
  if (!overlay) return false;
  event.preventDefault();
  event.stopPropagation();
  overlay.querySelector<HTMLElement>(`[${TOP_LAYER_DISMISS_ATTRIBUTE}]`)?.click();
  return true;
}

/** Keep marked overlays under `root` in the top layer as Kerf renders them; returns a disposer. */
export function wireTopLayerOverlays(root: HTMLElement): () => void {
  const reopen = () => {
      openTopLayerOverlays(root);
    },
    // Web Awesome calls `showModal()` right after dispatching `wa-show`, so a microtask sees it open.
    reopenSoon = () => {
      queueMicrotask(reopen);
    },
    preventCancel = (event: Event) => {
      if (isOverlay(event.target)) event.preventDefault();
    },
    // Capture on `root` runs before Web Awesome's bubbling `document` keydown handler.
    escape = (event: KeyboardEvent) => {
      dismissLiftedOverlay(root, event);
    },
    editingFocusLeft = () => {
      queueMicrotask(reopen);
    },
    observer = new MutationObserver(reopen);
  observer.observe(root, { childList: true, subtree: true, attributes: true, attributeFilter: ['open'] });
  for (const type of MODAL_LIFECYCLE_EVENTS) root.addEventListener(type, reopenSoon);
  // A modal overlay that still closes (a forced Escape) reopens; `cancel` and `close` do not bubble.
  root.addEventListener('cancel', preventCancel, true);
  root.addEventListener('close', reopenSoon, true);
  root.addEventListener('keydown', escape, true);
  root.addEventListener('focusout', editingFocusLeft, true);
  openTopLayerOverlays(root);
  return () => {
    observer.disconnect();
    for (const type of MODAL_LIFECYCLE_EVENTS) root.removeEventListener(type, reopenSoon);
    root.removeEventListener('cancel', preventCancel, true);
    root.removeEventListener('close', reopenSoon, true);
    root.removeEventListener('keydown', escape, true);
    root.removeEventListener('focusout', editingFocusLeft, true);
  };
}
