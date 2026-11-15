import type { CheckoutTicketRows, TicketRow } from './api';
import { type DuplicateTarget, duplicateTargetKey, prioritizeExactDuplicateTargets } from './ticket-close';

/** One open project the ticket-reference picker searches. */
export interface TicketReferenceSearchProject {
  id: string;
  name: string;
}

/** The candidates the picker shows and the per-project problems it reports beside them. */
export interface TicketReferenceSearchResult {
  candidates: DuplicateTarget[];
  errors: string[];
}

/** The most matches the picker lists; an exact ticket-number match always sorts first. */
export const TICKET_REFERENCE_RESULT_LIMIT = 20;

/** Project one ticket row into the picker's qualified, project-scoped reference. */
export function ticketReference(project: TicketReferenceSearchProject, ticket: TicketRow): DuplicateTarget {
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

/**
 * Search every open project for tickets matching `query` (number or title), as the close
 * dialog's duplicate target and the Inspector's follow-up link both do (HS2-MCGCPQ). A project
 * that fails or answers partially stays visible as an error rather than hiding the others.
 */
export async function searchTicketReferences<P extends TicketReferenceSearchProject>(
  projects: readonly P[],
  query: string,
  search: (project: P) => Promise<CheckoutTicketRows>,
): Promise<TicketReferenceSearchResult> {
  const results = await Promise.allSettled(
    projects.map(async (project) => ({ project, result: await search(project) })),
  );
  const candidates = new Map<string, DuplicateTarget>(),
    errors: string[] = [];
  for (const result of results) {
    if (result.status === 'rejected') {
      errors.push(result.reason instanceof Error ? result.reason.message : String(result.reason));
      continue;
    }
    if (result.value.result.partial || result.value.result.truncated)
      errors.push(`Search results may be incomplete for ${result.value.project.name}.`);
    for (const row of result.value.result.items) {
      const candidate = ticketReference(result.value.project, row);
      candidates.set(duplicateTargetKey(candidate), candidate);
    }
  }
  return {
    candidates: prioritizeExactDuplicateTargets([...candidates.values()], query).slice(
      0,
      TICKET_REFERENCE_RESULT_LIMIT,
    ),
    errors,
  };
}
