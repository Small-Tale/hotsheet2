import type { APIResponse, Route } from '@playwright/test';

/** Retries for an idempotent proxied read whose reused keep-alive socket was just closed. */
const idempotentProxyRetries = 2;

/**
 * Forward a routed browser request upstream: the one place browser tests and vitest browser
 * integration tests call `route.fetch` (HS2-6E87E4, HS2-QY232J). Playwright reuses keep-alive
 * sockets for it, and the server (hyper) closes an idle connection after 30 s; a request that picks
 * up the socket as it closes fails with `socket hang up` / `ECONNRESET` and the page never gets its
 * answer (HS2-NWFZ7J). Idempotent methods (GET, HEAD) are retried; a POST, PUT, PATCH, or DELETE is
 * never replayed, because the server may already have applied it. Stop proxying
 * (`unrouteAll({ behavior: 'ignoreErrors' })`, or pass the page to `realTicketServer().stop`)
 * before stopping the server it targets.
 *
 * This module imports only Playwright types, so `src/` integration tests can share it.
 */
export function proxyFetch(route: Route, options: Parameters<Route['fetch']>[0] = {}): Promise<APIResponse> {
  const method = (options.method ?? route.request().method()).toUpperCase();
  const idempotent = method === 'GET' || method === 'HEAD';
  return route.fetch(
    idempotent && options.maxRetries === undefined ? { ...options, maxRetries: idempotentProxyRetries } : options,
  );
}
