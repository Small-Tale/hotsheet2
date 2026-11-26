import type { TicketRow } from './api';
import { parseRemoteProjectId } from './remote-projects/identity';

// Current slugs always have at least two suffix characters. Imported HS1 tickets can
// retain the single-digit legacy numbers HS-1..HS-9, so admit exactly that narrow
// exception without turning every one-character PREFIX-X token into a reference.
const TICKET_SLUG_SOURCE = '(?:HS-[0-9]|[A-Z][A-Z0-9]{1,15}-[A-Z0-9]{2,24})';
const PROJECT_ID_SOURCE = '[A-Za-z0-9][A-Za-z0-9._~-]{0,127}';
const TICKET_REFERENCE = new RegExp(`^(?:@(${PROJECT_ID_SOURCE})/)?(${TICKET_SLUG_SOURCE})$`);

export interface TicketLinkReference {
  raw: string;
  slug: string;
  projectId?: string;
}

export interface TicketLinkProject {
  id: string;
  name: string;
  tickets: readonly TicketRow[];
}

export interface TicketLinkMatch {
  projectId: string;
  projectName: string;
  ticketId: string;
  qualifiedId: string;
  connectionId: string;
  slug: string;
  title: string;
  status?: string;
  /** The ticket's id in its provider; a Hot Sheet ticket's ULID, the same in every clone. */
  nativeId?: string;
  updatedAt?: string;
}

export type TicketLinkResolution =
  | { kind: 'open'; reference: TicketLinkReference; match: TicketLinkMatch }
  | { kind: 'not_found'; reference: TicketLinkReference; message: string }
  | { kind: 'choose'; reference: TicketLinkReference; matches: TicketLinkMatch[] };

/** Matches prose references. Explicit cross-project references use `@<project-id>/<ticket-slug>`. */
export function ticketReferencePattern(): RegExp {
  return new RegExp(`(?<![A-Za-z0-9_])(?:@${PROJECT_ID_SOURCE}/)?${TICKET_SLUG_SOURCE}(?![A-Za-z0-9_])`, 'g');
}

export function parseTicketLinkReference(value: string): TicketLinkReference | undefined {
  const raw = value.trim();
  const match = TICKET_REFERENCE.exec(raw);
  if (!match) return undefined;
  return { raw, slug: match[2], ...(match[1] ? { projectId: match[1] } : {}) };
}

function ticketLinkMatch(project: TicketLinkProject, ticket: TicketRow): TicketLinkMatch {
  return {
    projectId: project.id,
    projectName: project.name,
    ticketId: ticket.id,
    qualifiedId: ticket.qualified_id,
    connectionId: ticket.connection_id,
    slug: ticket.slug,
    title: ticket.title,
    status: ticket.status,
    nativeId: ticket.native_id,
    updatedAt: ticket.updated_at,
  };
}

// A Hot Sheet ticket id: a ULID minted once when the ticket is created and carried unchanged by sync.
const TICKET_ULID = /^[0-9A-HJKMNP-TV-Z]{26}$/;

/**
 * Copies of one ticket open in several tabs, such as a local and a remote clone of one project
 * (HS2-MBFGRV). Two matches are the same ticket when they share the slug and the same Hot Sheet ticket
 * ULID: a store's connection hash differs per clone, but the ULID is minted once and synced, so equal
 * ULIDs are one ticket. Provider ids that are not ULIDs (an issue number) never merge, since two
 * projects can number different issues alike.
 */
function sameTicketKey(match: TicketLinkMatch): string | undefined {
  return match.nativeId && TICKET_ULID.test(match.nativeId)
    ? `${match.slug.toLocaleLowerCase()}\u0000${match.nativeId}`
    : undefined;
}

const updatedMs = (match: TicketLinkMatch) => {
  const ms = match.updatedAt ? Date.parse(match.updatedAt) : Number.NaN;
  return Number.isNaN(ms) ? Number.NEGATIVE_INFINITY : ms;
};

/**
 * Keeps one copy of each ticket (HS2-MBFGRV): the most recently updated; among equally recent copies a
 * local tab's over a remote tab's; otherwise the first in order. Different tickets that share a slug
 * all stay, so the chooser still asks between them. Order follows each ticket's first match.
 */
export function bestTicketCopies(matches: readonly TicketLinkMatch[]): TicketLinkMatch[] {
  const best = new Map<string, TicketLinkMatch>(),
    order: (string | TicketLinkMatch)[] = [];
  const better = (candidate: TicketLinkMatch, current: TicketLinkMatch) => {
    const difference = updatedMs(candidate) - updatedMs(current);
    if (difference !== 0 && !Number.isNaN(difference)) return difference > 0;
    return !parseRemoteProjectId(candidate.projectId) && Boolean(parseRemoteProjectId(current.projectId));
  };
  for (const match of matches) {
    const key = sameTicketKey(match);
    if (!key) {
      order.push(match);
      continue;
    }
    const current = best.get(key);
    if (!current) order.push(key);
    if (!current || better(match, current)) best.set(key, match);
  }
  return order.map((item) => (typeof item === 'string' ? best.get(item)! : item));
}

/** Resolve exact slugs across open projects, or within an explicitly named project. */
export function resolveTicketLink(
  reference: TicketLinkReference,
  projects: readonly TicketLinkProject[],
  activeProjectId: string,
): TicketLinkResolution {
  const candidates = reference.projectId
    ? projects.filter((item) => item.id === reference.projectId)
    : [
        ...projects.filter((item) => item.id === activeProjectId),
        ...projects.filter((item) => item.id !== activeProjectId),
      ];
  const matches: TicketLinkMatch[] = [];
  const seen = new Set<string>();
  const needle = reference.slug.toLocaleLowerCase();
  for (const project of candidates) {
    for (const ticket of project.tickets) {
      // Match the current HS2 slug or the retained Hot Sheet 1 number, so a bare legacy
      // `HS-N` reference resolves to the imported ticket the same way a slug does (HS2-XB5R3Y).
      if (ticket.slug.toLocaleLowerCase() !== needle && (ticket.legacy_number ?? '').toLocaleLowerCase() !== needle)
        continue;
      const key = `${project.id}\u0000${ticket.qualified_id}`;
      if (seen.has(key)) continue;
      seen.add(key);
      matches.push(ticketLinkMatch(project, ticket));
    }
  }
  const copies = bestTicketCopies(matches);
  if (copies.length === 1) return { kind: 'open', reference, match: copies[0] };
  if (copies.length > 1) return { kind: 'choose', reference, matches: copies };
  const projectLabel = reference.projectId ? ` in project ${reference.projectId}` : '';
  return { kind: 'not_found', reference, message: `No exact match for ${reference.slug}${projectLabel}.` };
}

export function ticketLinkMatchKey(match: Pick<TicketLinkMatch, 'projectId' | 'qualifiedId'>): string {
  return `${encodeURIComponent(match.projectId)}::${encodeURIComponent(match.qualifiedId)}`;
}
