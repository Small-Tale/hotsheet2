/** The attribute marking the app-inserted element that hosts the toast stack inside a modal dialog. */
export const TOAST_MODAL_HOST_ATTRIBUTE = 'data-toast-modal-host';

/** The parts of the toast stack (a `wa-toast` popover) this module moves and re-shows. */
export interface ToastStack {
  readonly isConnected: boolean;
  readonly parentElement: unknown;
  /** Whether the stack holds at least one toast item. */
  hasItems(): boolean;
  isOpen(): boolean;
  showPopover(): void;
}

/** The DOM the host reads and writes, injectable so the state machine is testable without a browser. */
export interface ToastModalHostEnvironment<Parent> {
  /** The topmost open modal dialog, if any. */
  topmostModal(): Parent | null;
  /** Where the stack lives while no modal dialog is open. */
  body: Parent;
  /** The dialog's host element for the stack, created on first use. */
  hostIn(modal: Parent): Parent;
  /** Move the stack into `parent` (appending it). */
  move(stack: ToastStack, parent: Parent): void;
  /** Watch for modal dialogs opening, closing, or leaving the document. */
  observe(callback: () => void): { disconnect(): void };
}

/**
 * Keep the root toast stack usable over modal surfaces such as the media gallery (HS2-M2G31B).
 *
 * A modal `<dialog>` makes everything outside it inert, so a toast in `document.body` cannot be
 * clicked or focused while the dialog is open, even when it draws above it. While a toast is on
 * screen the stack therefore lives inside the topmost open modal dialog, and goes back to the body
 * when that dialog closes or unmounts. Moving a popover takes it out of the top layer, so a stack
 * that has toasts is re-shown after each move and re-enters the top layer above the modal.
 *
 * `start` runs when a toast shows and watches for modal changes, so a toast that is already up when a
 * modal opens moves into it too; `onChange` lets the caller re-measure anything that depends on the
 * surface under the toast (the gallery-footer clearance). After `stop` the stack stays in its modal
 * so a hiding toast finishes its exit there, and returns to the body once that modal closes or
 * unmounts. Nothing is observed while no toast shows and the stack is on the body.
 */
export function createToastModalHost<Parent>(
  stack: ToastStack,
  environment: ToastModalHostEnvironment<Parent>,
  onChange: () => void = () => undefined,
) {
  let active = false,
    hostModal: Parent | null = null,
    watcher: { disconnect(): void } | undefined;
  /** Put the stack where it belongs now; returns whether it moved. */
  const place = () => {
    const modal = environment.topmostModal();
    // An inactive stack only stays in the modal it already occupies, and only while that is open.
    const target = active ? modal : hostModal !== null && modal === hostModal && stack.isConnected ? hostModal : null,
      parent = target === null ? environment.body : environment.hostIn(target);
    hostModal = target;
    if (stack.isConnected && stack.parentElement === parent) return false;
    environment.move(stack, parent);
    if (active && stack.hasItems() && !stack.isOpen()) stack.showPopover();
    return true;
  };
  const settle = () => {
    if (active || hostModal !== null) return;
    watcher?.disconnect();
    watcher = undefined;
  };
  return {
    /** A toast is (still) showing: host it in the current modal and follow later modal changes. */
    start() {
      active = true;
      place();
      watcher ??= environment.observe(() => {
        // Re-measure only for a showing toast; a stack merely returning home needs nothing.
        if (place() && active) onChange();
        settle();
      });
    },
    /** No toast shows: stop following modals once the stack is back on the body. */
    stop() {
      active = false;
      settle();
    },
    /** Tear down for good (the application shell is disposed); the caller removes the stack. */
    dispose() {
      active = false;
      hostModal = null;
      watcher?.disconnect();
      watcher = undefined;
    },
    /** Whether a modal-change watcher is live (diagnostics and tests). */
    watching: () => watcher !== undefined,
  };
}

/** The browser form, for the application's `wa-toast` stack. */
export function createBrowserToastModalHost(stack: HTMLElement, onChange?: () => void) {
  const toastStack: ToastStack = {
    get isConnected() {
      return stack.isConnected;
    },
    get parentElement() {
      return stack.parentElement;
    },
    hasItems: () => stack.querySelector(':scope > wa-toast-item') !== null,
    isOpen: () => stack.matches(':popover-open'),
    showPopover: () => {
      stack.showPopover();
    },
  };
  return createToastModalHost<HTMLElement>(
    toastStack,
    {
      topmostModal: () => {
        // Top-layer order is not visible in the DOM; the last open modal in document order is the
        // one stacked dialogs open last in practice.
        const modals = [...document.querySelectorAll<HTMLDialogElement>('dialog[open]')].filter((dialog) =>
          dialog.matches(':modal'),
        );
        return modals.at(-1) ?? null;
      },
      body: document.body,
      hostIn: (modal) => {
        const existing = modal.querySelector<HTMLElement>(`:scope > [${TOAST_MODAL_HOST_ATTRIBUTE}]`);
        if (existing) return existing;
        // Kerf's documented foreign-node convention: `data-morph-preserve` keeps this app-inserted
        // element (and the stack inside it) when Kerf re-renders the dialog, as Kerf's own overlay
        // host does; `display: contents` keeps it out of the dialog's layout.
        const host = document.createElement('div');
        host.setAttribute(TOAST_MODAL_HOST_ATTRIBUTE, '');
        host.setAttribute('data-morph-preserve', '');
        host.style.display = 'contents';
        modal.append(host);
        return host;
      },
      move: (_stack, parent) => {
        parent.append(stack);
      },
      observe: (callback) => {
        const observer = new MutationObserver(callback);
        observer.observe(document.body, {
          subtree: true,
          childList: true,
          attributes: true,
          attributeFilter: ['open'],
        });
        return observer;
      },
    },
    onChange,
  );
}
