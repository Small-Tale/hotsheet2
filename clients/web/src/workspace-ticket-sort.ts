import { applyWorkspaceSortDirection, type WorkspaceSort, type WorkspaceSortDirection } from './components/workspace-header';

export interface WorkspaceSortableTicket { slug: string; title: string; priority?: string; status?: string; updated_at?: string }

const PRIORITY_ORDER = ['low', 'default', 'high', 'urgent'] as const;
const STATUS_ORDER = ['backlog', 'not_started', 'started', 'completed', 'verified', 'archive', 'deleted', 'moved'] as const;

function rank(order: readonly string[], value: string | undefined): number {
  const index = order.indexOf(value ?? '');
  return index < 0 ? order.length : index;
}

export function compareWorkspaceTickets(left: WorkspaceSortableTicket, right: WorkspaceSortableTicket, sort: WorkspaceSort, direction: WorkspaceSortDirection): number {
  if (sort === 'updated') {
    const comparison = (left.updated_at ?? '').localeCompare(right.updated_at ?? '');
    return applyWorkspaceSortDirection(comparison || left.slug.localeCompare(right.slug), direction);
  }

  const primaryComparison = sort === 'priority'
    ? rank(PRIORITY_ORDER, left.priority ?? 'default') - rank(PRIORITY_ORDER, right.priority ?? 'default')
    : sort === 'status'
      ? rank(STATUS_ORDER, left.status ?? 'not_started') - rank(STATUS_ORDER, right.status ?? 'not_started')
      : left.title.localeCompare(right.title);
  const directedPrimary = applyWorkspaceSortDirection(primaryComparison, direction);
  if (directedPrimary !== 0) return directedPrimary;

  // Within equal status, priority, or title groups, the most recently touched work is
  // always the most relevant, independent of the selected primary direction.
  const recentFirst = (right.updated_at ?? '').localeCompare(left.updated_at ?? '');
  return recentFirst || left.slug.localeCompare(right.slug);
}
