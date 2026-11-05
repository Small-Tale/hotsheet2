import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import type { SafeHtml } from 'kerfjs/jsx-runtime';

export interface GridSidebarGroupProps {
  /** The project id, or `all` for the All projects aggregate. */
  projectId: string;
  /** The group's list header: `All projects` or the project name. */
  name: string;
  /** Draw the divider under the group (only the All projects aggregate above per-project groups). */
  divider?: boolean;
  /** The page's body for this group: a completion chart, a gauge pair, or a triage row. */
  children: SafeHtml;
}

/**
 * One project group of the workspace grid sidebar (HS2-E1AQE8, docs/34-glance-surfaces.md §34.7):
 * the list header, the divider under All projects, and a per-page body. Every page renders its groups
 * through this component, so the pages keep the same header, divider, alignment, and project order.
 */
export function GridSidebarGroup({ projectId, name, divider = false, children }: GridSidebarGroupProps) {
  return (
    <List
      className="grid-sidebar-group"
      rootAttributes={{ 'data-project-id': projectId, 'data-grid-sidebar-group': '' }}
      dividerSides={divider ? 'b' : ''}
    >
      <ListHeader label={name} inline />
      {children}
    </List>
  );
}
