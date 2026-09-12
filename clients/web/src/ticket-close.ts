export type TicketCloseReason = 'completed' | 'not_planned' | 'duplicate' | 'obsolete';

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
];

export function duplicateTargetKey(target: Pick<DuplicateTarget, 'projectId' | 'qualifiedId'>): string {
  return `${encodeURIComponent(target.projectId)}::${encodeURIComponent(target.qualifiedId)}`;
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

function lookupTarget(project: DuplicateLookupProject, ticket: DuplicateLookupTicket): DuplicateTarget {
  return { id: ticket.id, slug: ticket.slug, title: ticket.title, projectId: project.id, projectName: project.name, connectionId: ticket.connection_id, nativeId: ticket.native_id, qualifiedId: ticket.qualified_id };
}

export async function resolveDuplicateReferenceTarget(
  value: string,
  projects: readonly DuplicateLookupProject[],
  load: (project: DuplicateLookupProject, id: string) => Promise<DuplicateLookupTicket>,
): Promise<DuplicateTarget | undefined> {
  const reference = parseDuplicateReference(value);
  if (reference) {
    const project = projects.find(item => item.id === reference.project_id);
    if (!project) return undefined;
    return lookupTarget(project, await load(project, `${reference.connection_id}:${reference.native_id}`));
  }
  const matches = (await Promise.allSettled(projects.map(async project => lookupTarget(project, await load(project, value)))))
    .flatMap(result => result.status === 'fulfilled' ? [result.value] : []);
  return matches.length === 1 ? matches[0] : undefined;
}

export function duplicateOutcomeLabel(target: DuplicateTarget, sourceProjectId: string): string {
  return `${target.slug}${target.projectId === sourceProjectId ? '' : ` · ${target.projectName}`}`;
}

export function validateTicketClose(reason: TicketCloseReason, source: DuplicateTarget, target?: DuplicateTarget): string {
  if (reason !== 'duplicate') return '';
  if (!target) return 'Select the existing ticket that this duplicates.';
  if (duplicateTargetKey(target) === duplicateTargetKey(source)) return 'A ticket cannot be a duplicate of itself.';
  return '';
}

export function ticketCloseReasonLabel(reason?: TicketCloseReason): string | undefined {
  return TICKET_CLOSE_REASON_CHOICES.find(choice => choice.value === reason)?.label;
}
