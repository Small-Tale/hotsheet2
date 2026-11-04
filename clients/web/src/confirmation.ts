import { signal } from 'kerfjs';

import { CONFIRMATION_TARGETS } from './interaction-attrs/confirmation';

const CONFIRMATION_DIALOG = CONFIRMATION_TARGETS.dialog.selector;

/**
 * One question the person must answer before an action that undo cannot cover (HS2-6S1346). The
 * desktop webview shows no `window.confirm`, so every such question goes through this workflow and
 * the in-app `ConfirmationDialog`.
 */
export interface ConfirmationOptions {
  /** The dialog title: the question, such as "Stop the active Codex turn?". */
  title: string;
  /** Optional detail below the title: the consequence the person is accepting. */
  message?: string;
  /** The confirming action's label, a verb phrase such as "Stop Turn". */
  confirmLabel: string;
  /** The declining action's label; defaults to Cancel. */
  cancelLabel?: string;
  /** `danger` marks a destructive or safety-removing action; `brand` (the default) an ordinary one. */
  tone?: 'danger' | 'brand';
}

/** The request the dialog shows: its options plus a per-request id so each request remounts it. */
export interface ConfirmationRequest extends ConfirmationOptions {
  id: number;
}

/** Ask the person; resolves `true` on confirm and `false` on cancel or dismissal. */
export type Confirm = (options: ConfirmationOptions) => Promise<boolean>;

/** The element focus returns to: the one focused when the request was made. */
export interface FocusReturnTarget {
  readonly isConnected: boolean;
  focus(options?: FocusOptions): void;
}

export interface ConfirmationEnvironment {
  /** The currently focused element, captured as each request's opener. */
  focused?: () => FocusReturnTarget | undefined;
  /** Run once the closed dialog has left the page, so focus can leave the modal layer. */
  afterClose?: (run: () => void) => void;
}

function documentFocus(): FocusReturnTarget | undefined {
  if (typeof document === 'undefined') return undefined;
  const active = document.activeElement;
  return active instanceof HTMLElement && active !== document.body ? active : undefined;
}

function insideConfirmation(target: FocusReturnTarget) {
  return typeof Element === 'function' && target instanceof Element && Boolean(target.closest(CONFIRMATION_DIALOG));
}

function nextFrame(run: () => void) {
  if (typeof requestAnimationFrame === 'function') requestAnimationFrame(run);
  else queueMicrotask(run);
}

interface Pending {
  request: ConfirmationRequest;
  resolve: (confirmed: boolean) => void;
  opener: FocusReturnTarget | undefined;
}

/**
 * A promise-based confirmation queue. `request` resolves `true` when the person confirms and `false`
 * when they cancel or dismiss the dialog (Escape or its close button). Concurrent requests show one
 * at a time in request order, and each settles on its own answer. When the last queued request
 * settles, focus returns to the element that was focused when that request was made.
 */
export function createConfirmation(environment: ConfirmationEnvironment = {}) {
  const focused = environment.focused ?? documentFocus,
    afterClose = environment.afterClose ?? nextFrame;
  const queue: Pending[] = [];
  const current = signal<ConfirmationRequest | undefined>(undefined);
  let nextId = 1;
  function settle(confirmed: boolean, id?: number) {
    // An answer names the request it was given for, so a repeated click cannot answer the next one.
    if (id !== undefined && queue[0]?.request.id !== id) return;
    const head = queue.shift();
    if (!head) return;
    current.value = queue[0]?.request;
    head.resolve(confirmed);
    const opener = head.opener;
    // A request made in the meantime (or still queued) keeps focus in its dialog.
    if (opener && !queue.length)
      afterClose(() => {
        if (!queue.length && opener.isConnected) opener.focus({ preventScroll: true });
      });
  }
  const request: Confirm = (options) =>
    new Promise<boolean>((resolve) => {
      // A request made while another is on screen would capture a dialog button; it inherits the
      // opener of the request it queues behind instead.
      const pending: Pending = {
        request: { ...options, id: nextId++ },
        resolve,
        opener: queue.length ? queue[queue.length - 1].opener : focused(),
      };
      if (!queue.length)
        // The asking control may still be moving focus in the same task: a Select reports its change
        // from the chosen option, then closes and focuses its trigger. Read focus again once that task
        // ends, which is queued before the dialog opens and takes focus.
        queueMicrotask(() => {
          const settled = focused();
          if (settled?.isConnected && !insideConfirmation(settled)) pending.opener = settled;
        });
      queue.push(pending);
      if (queue.length === 1) current.value = pending.request;
    });
  return {
    /** The request on screen, if any. */
    current,
    request,
    /** Confirm the request on screen, or only request `id` when given. */
    confirm(id?: number) {
      settle(true, id);
    },
    /** Decline the request on screen, or only request `id` when given. */
    cancel(id?: number) {
      settle(false, id);
    },
    /** Decline every queued request, such as when the owning surface is torn down. */
    cancelAll() {
      while (queue.length) settle(false);
    },
  };
}

export type ConfirmationWorkflow = ReturnType<typeof createConfirmation>;
