/**
 * A collection of listener disposers (for example the `() => void` returned by Kerf's `delegate()`)
 * released together by one teardown.
 */
export interface DisposerScope {
  /** Track `dispose` until the scope is torn down; returns it so callers may also release it early. */
  add(dispose: () => void): () => void;
  /** Release every tracked disposer, newest first, and empty the scope so it can be refilled. */
  dispose(): void;
  /** The number of disposers currently tracked. */
  readonly size: number;
}

/** Create an empty {@link DisposerScope}. */
export function createDisposerScope(): DisposerScope {
  const disposers: Array<() => void> = [];
  return {
    add(dispose) {
      disposers.push(dispose);
      return dispose;
    },
    dispose() {
      // Detach the list before running so a disposer that re-registers lands in the next generation.
      for (const dispose of disposers.splice(0).reverse()) dispose();
    },
    get size() {
      return disposers.length;
    },
  };
}
