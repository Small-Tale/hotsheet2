/**
 * One registration lifetime shared by the app's interaction groups and the UX demo (HS2-J26QXQ).
 *
 * Kerf `delegate()`/`delegateCapture()` disposers and Kerf UI wiring handles are retained through
 * {@link DisposerScope.add}; native `addEventListener` registrations pass {@link DisposerScope.signal}.
 * One {@link DisposerScope.dispose} call then removes all of them, newest first.
 *
 * Kerf's `kerfjs/scope` `disposeScope(el)` is keyed by a DOM element and has no `AbortSignal`, so it
 * does not fit listener groups that span `window`, `document`, and several roots.
 */
export interface DisposerScope {
  /**
   * Aborted by the next {@link DisposerScope.dispose}; pass it as the `signal` option of native
   * listeners. Read it when registering: each generation of the scope has its own signal, so a
   * scope refilled after teardown hands out a fresh one.
   */
  readonly signal: AbortSignal;
  /** Retain `dispose` until the scope is torn down; returns it so callers may also release it early. */
  add<Dispose extends () => void>(dispose: Dispose): Dispose;
  /**
   * Run every retained disposer (newest first), then abort {@link DisposerScope.signal}. The scope
   * is emptied and can be refilled; a disposer registered while teardown runs belongs to the next
   * generation. Repeating it without new registrations is a no-op. It is bound, so it can be
   * returned directly as a group's teardown.
   */
  readonly dispose: () => void;
  /** The number of disposers currently retained. */
  readonly size: number;
}

/** Create an empty {@link DisposerScope}. */
export function createDisposerScope(): DisposerScope {
  const disposers: (() => void)[] = [];
  let controller = new AbortController();
  return {
    get signal() {
      return controller.signal;
    },
    add(dispose) {
      disposers.push(dispose);
      return dispose;
    },
    dispose() {
      // Detach the list and the signal before running so a re-registration lands in the next generation.
      const finished = controller;
      controller = new AbortController();
      for (const dispose of disposers.splice(0).reverse()) dispose();
      finished.abort();
    },
    get size() {
      return disposers.length;
    },
  };
}
