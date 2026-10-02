/** Removes every listener an interaction group registered. Calling it more than once is a no-op. */
export type InteractionTeardown = () => void;

/**
 * The registration lifetime of one interaction group. Kerf `delegate()`/`delegateCapture()`
 * disposers and Kerf UI wiring handles are retained through {@link InteractionLifetime.add};
 * native `addEventListener` registrations pass {@link InteractionLifetime.signal}. One
 * {@link InteractionLifetime.dispose} call then removes all of them, newest first.
 */
export interface InteractionLifetime {
  /** Aborted by `dispose`; pass it as the `signal` option of native listeners. */
  readonly signal: AbortSignal;
  /** Retain a disposer so `dispose` calls it; returns the same disposer for further use. */
  add<Dispose extends () => void>(dispose: Dispose): Dispose;
  /** Remove every retained registration (newest first) and abort `signal`. */
  readonly dispose: InteractionTeardown;
}

/** Start a registration lifetime for one interaction group. */
export function createInteractionLifetime(): InteractionLifetime {
  const disposers: (() => void)[] = [],
    controller = new AbortController();
  return {
    signal: controller.signal,
    add(dispose) {
      disposers.push(dispose);
      return dispose;
    },
    dispose() {
      for (const dispose of disposers.splice(0).reverse()) dispose();
      controller.abort();
    },
  };
}

/** Compose several group teardowns into one that runs them in reverse registration order. */
export function combineInteractionTeardowns(teardowns: readonly InteractionTeardown[]): InteractionTeardown {
  const pending = [...teardowns];
  return () => {
    for (const teardown of pending.splice(0).reverse()) teardown();
  };
}
