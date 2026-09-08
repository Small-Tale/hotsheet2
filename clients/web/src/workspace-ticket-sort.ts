import { applyWorkspaceSortDirection, type WorkspaceSort, type WorkspaceSortDirection } from './components/workspace-header';

export interface WorkspaceSortableTicket { slug: string; title: string; priority?: string; status?: string; updated_at?: string }

const PRIORITY_ORDER = ['low', 'default', 'high', 'urgent'] as const;
const STATUS_ORDER = ['backlog', 'not_started', 'started', 'completed', 'verified', 'archive', 'deleted', 'moved'] as const;

function rank(order: readonly string[], value: string | undefined): number {
  const index = order.indexOf(value ?? '');
  return index < 0 ? order.length : index;
}

export function compareWorkspaceTickets(left: WorkspaceSortableTicket, right: WorkspaceSortableTicket, sort: WorkspaceSort, direction: WorkspaceSortDirection): number {
  const comparison = sort === 'priority'
    ? rank(PRIORITY_ORDER, left.priority ?? 'default') - rank(PRIORITY_ORDER, right.priority ?? 'default')
    : sort === 'status'
      ? rank(STATUS_ORDER, left.status ?? 'not_started') - rank(STATUS_ORDER, right.status ?? 'not_started')
      : sort === 'title'
        ? left.title.localeCompare(right.title)
        : (left.updated_at ?? '').localeCompare(right.updated_at ?? '');
  return applyWorkspaceSortDirection(comparison || left.slug.localeCompare(right.slug), direction);
}
