import type { TicketRow } from './api';

const TICKET_SLUG_SOURCE = '[A-Z][A-Z0-9]{1,15}-[A-Z0-9]{2,24}';
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

export function formatTicketLinkReference(slug: string, projectId?: string): string {
  const raw = projectId ? `@${projectId}/${slug}` : slug;
  if (!parseTicketLinkReference(raw)) throw new Error('Ticket references require a valid project ID and ticket slug.');
  return raw;
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
  };
}

/** Resolve exact slugs across open projects, or within an explicitly named project. */
export function resolveTicketLink(
  reference: TicketLinkReference,
  projects: readonly TicketLinkProject[],
  activeProjectId: string,
): TicketLinkResolution {
  const candidates = reference.projectId
    ? projects.filter(item => item.id === reference.projectId)
    : [...projects.filter(item => item.id === activeProjectId), ...projects.filter(item => item.id !== activeProjectId)];
  const matches: TicketLinkMatch[] = [];
  const seen = new Set<string>();
  for (const project of candidates) {
    for (const ticket of project.tickets) {
      if (ticket.slug.toLocaleLowerCase() !== reference.slug.toLocaleLowerCase()) continue;
      const key = `${project.id}\u0000${ticket.qualified_id}`;
      if (seen.has(key)) continue;
      seen.add(key);
      matches.push(ticketLinkMatch(project, ticket));
    }
  }
  if (matches.length === 1) return { kind: 'open', reference, match: matches[0] };
  if (matches.length > 1) return { kind: 'choose', reference, matches };
  const projectLabel = reference.projectId ? ` in project ${reference.projectId}` : '';
  return { kind: 'not_found', reference, message: `No exact match for ${reference.slug}${projectLabel}.` };
}

export function ticketLinkMatchKey(match: Pick<TicketLinkMatch, 'projectId' | 'qualifiedId'>): string {
  return `${encodeURIComponent(match.projectId)}::${encodeURIComponent(match.qualifiedId)}`;
}
