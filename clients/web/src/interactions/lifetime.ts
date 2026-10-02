import { createDisposerScope } from '../disposer-scope';

/** Removes every listener an interaction group registered. Calling it more than once is a no-op. */
export type InteractionTeardown = () => void;

/**
 * Compose several group teardowns into one that runs them in reverse registration order. Each
 * interaction group registers through its own `createDisposerScope()` (see `../disposer-scope`).
 */
export function combineInteractionTeardowns(teardowns: readonly InteractionTeardown[]): InteractionTeardown {
  const scope = createDisposerScope();
  for (const teardown of teardowns) scope.add(teardown);
  return scope.dispose;
}
