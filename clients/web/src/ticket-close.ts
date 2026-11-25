import { parseRemoteProjectId, remoteProjectId } from './remote-projects/identity';

export type TicketCloseReason = 'completed' | 'not_planned' | 'duplicate' | 'obsolete' | 'works_as_designed';

export interface DuplicateTarget {
  id: string;
  slug: string;
  title: string;
  projectId: string;
  projectName: string;
  connectionId: string;
  nativeId: string;
  qualifiedId: string;
}

export interface DuplicateReference {
  project_id: string;
  connection_id: string;
  native_id: string;
}

export interface DuplicateLookupProject {
  id: string;
  name: string;
}

export interface DuplicateLookupTicket {
  id: string;
  slug: string;
  title: string;
  connection_id: string;
  native_id: string;
  qualified_id: string;
}

export const TICKET_CLOSE_REASON_CHOICES: ReadonlyArray<{ value: TicketCloseReason; label: string }> = [
  { value: 'completed', label: 'Completed' },
  { value: 'not_planned', label: 'Not planned' },
  { value: 'duplicate', label: 'Duplicate' },
  { value: 'obsolete', label: 'Obsolete' },
  { value: 'works_as_designed', label: 'Works as designed' },
];

export function duplicateTargetKey(target: Pick<DuplicateTarget, 'projectId' | 'qualifiedId'>): string {
  return `${encodeURIComponent(target.projectId)}::${encodeURIComponent(target.qualifiedId)}`;
}

export function prioritizeExactDuplicateTargets<T extends Pick<DuplicateTarget, 'slug'>>(
  candidates: readonly T[],
  query: string,
): T[] {
  const exact: T[] = [],
    other: T[] = [],
    slug = query.trim().toLocaleUpperCase();
  for (const candidate of candidates) (candidate.slug.toLocaleUpperCase() === slug ? exact : other).push(candidate);
  return [...exact, ...other];
}

export function duplicateReference(target: DuplicateTarget): DuplicateReference {
  return { project_id: target.projectId, connection_id: target.connectionId, native_id: target.nativeId };
}

export function parseDuplicateReference(value: string): DuplicateReference | undefined {
  const split = value.startsWith('@') ? value.slice(1).indexOf('/') : -1;
  if (split < 1) return undefined;
  const project_id = value.slice(1, split + 1);
  const qualified = value.slice(split + 2);
  const separator = qualified.indexOf(':');
  if (separator < 1 || separator === qualified.length - 1) return undefined;
  return { project_id, connection_id: qualified.slice(0, separator), native_id: qualified.slice(separator + 1) };
}

/**
 * The open project a relation's `project_id` names, read relative to the server of the ticket that
 * holds the relation (HS2-4SPTA4). A ticket in a remote tab stores its host's native checkout id,
 * while the tab's id is `remote:<binding>:<checkout>`, so the id maps to that same host's tab; it is
 * never matched to a tab of another server, where the same checkout id may mean another project.
 */
export function relationTargetProject<P extends { id: string }>(
  projects: readonly P[],
  referenceProjectId: string,
  sourceProjectId: string | undefined,
): P | undefined {
  const source = sourceProjectId ? parseRemoteProjectId(sourceProjectId) : undefined;
  if (!source) return projects.find((item) => item.id === referenceProjectId);
  // A reference that already names this tab (a composite id) still resolves to it.
  if (referenceProjectId === sourceProjectId) return projects.find((item) => item.id === sourceProjectId);
  let candidate: string;
  try {
    candidate = remoteProjectId(source.bindingId, referenceProjectId);
  } catch {
    return undefined;
  }
  return projects.find((item) => item.id === candidate);
}

function lookupTarget(project: DuplicateLookupProject, ticket: DuplicateLookupTicket): DuplicateTarget {
  return {
    id: ticket.id,
    slug: ticket.slug,
    title: ticket.title,
    projectId: project.id,
    projectName: project.name,
    connectionId: ticket.connection_id,
    nativeId: ticket.native_id,
    qualifiedId: ticket.qualified_id,
  };
}

export async function resolveDuplicateReferenceTarget(
  value: string,
  projects: readonly DuplicateLookupProject[],
  load: (project: DuplicateLookupProject, id: string) => Promise<DuplicateLookupTicket>,
): Promise<DuplicateTarget | undefined> {
  const reference = parseDuplicateReference(value);
  if (reference) {
    const project = projects.find((item) => item.id === reference.project_id);
    if (!project) return undefined;
    return lookupTarget(project, await load(project, `${reference.connection_id}:${reference.native_id}`));
  }
  const matches = (
    await Promise.allSettled(projects.map(async (project) => lookupTarget(project, await load(project, value))))
  ).flatMap((result) => (result.status === 'fulfilled' ? [result.value] : []));
  return matches.length === 1 ? matches[0] : undefined;
}

export function validateTicketClose(
  reason: TicketCloseReason,
  source: DuplicateTarget,
  target?: DuplicateTarget,
): string {
  if (reason !== 'duplicate') return '';
  if (!target) return 'Select the existing ticket that this duplicates.';
  if (duplicateTargetKey(target) === duplicateTargetKey(source)) return 'A ticket cannot be a duplicate of itself.';
  return '';
}

export function ticketCloseReasonLabel(reason?: TicketCloseReason): string | undefined {
  return TICKET_CLOSE_REASON_CHOICES.find((choice) => choice.value === reason)?.label;
}
