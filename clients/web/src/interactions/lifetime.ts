import { createScope } from 'kerfjs/scope';

/** Removes every listener an interaction group registered. Calling it more than once is a no-op. */
export type InteractionTeardown = () => void;

/**
 * Compose several group teardowns into one that runs them in reverse registration order. Each
 * interaction group registers through its own Kerf `createScope()` from `kerfjs/scope` (HS2-PNFYCV):
 * Kerf `delegate()` disposers and Kerf UI wiring handles go through `add`, native listeners pass the
 * per-generation `signal`, and the scope is refillable after `dispose()`. Kerf runs the disposers
 * newest first and best-effort, so one throwing disposer cannot strand the rest.
 *
 * Groups return `() => { lifetime.dispose(); }` rather than the detached method: Kerf implements
 * `dispose` as a closure, but types it as an unbound method (KF-9AH3G8).
 */
export function combineInteractionTeardowns(teardowns: readonly InteractionTeardown[]): InteractionTeardown {
  const scope = createScope();
  for (const teardown of teardowns) scope.add(teardown);
  return () => {
    scope.dispose();
  };
}
