import type { Checkout } from './api';

export interface TicketDeepLink {
  store: string;
  ticket: string;
}

/** A project root or registered checkout id plus an exact ticket slug/qualified id. */
export function parseTicketDeepLink(search: string): TicketDeepLink | undefined {
  const params = new URLSearchParams(search);
  const store = params.get('store')?.trim();
  const ticket = params.get('ticket')?.trim();
  if (!store || !ticket || store.length > 4096 || ticket.length > 512) return undefined;
  return { store, ticket };
}

/** Registered ids are resolved before treating `store` as a filesystem path. */
export function ticketDeepLinkRoot(store: string, checkouts: readonly Checkout[]): string | undefined {
  const registered = checkouts.find(
    (checkout) => checkout.id === store || checkout.alias === store || checkout.stores.includes(store),
  );
  if (registered) return registered.root;
  return /^(?:\/|[A-Za-z]:[\\/]|\\\\)/.test(store) ? store : undefined;
}
