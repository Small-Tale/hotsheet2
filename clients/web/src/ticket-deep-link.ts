import type { Checkout } from './api';

export interface TicketDeepLink {
  project: string;
  ticket?: string;
}

/** A project root or registered checkout id, optionally with an exact ticket slug/qualified id. */
export function parseTicketDeepLink(search: string): TicketDeepLink | undefined {
  const params = new URLSearchParams(search);
  const project = params.get('project')?.trim() || params.get('store')?.trim();
  const ticket = params.get('ticket')?.trim();
  if (!project || project.length > 4096 || (ticket?.length ?? 0) > 512) return undefined;
  return ticket ? { project, ticket } : { project };
}

/** Registered ids are resolved before treating the project reference as a filesystem path. */
export function ticketDeepLinkRoot(project: string, checkouts: readonly Checkout[]): string | undefined {
  const registered = checkouts.find(
    (checkout) =>
      checkout.id === project ||
      checkout.alias === project ||
      checkout.root === project ||
      checkout.stores.includes(project),
  );
  if (registered) return registered.root;
  return /^(?:\/|[A-Za-z]:[\\/]|\\\\)/.test(project) ? project : undefined;
}
