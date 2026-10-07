import '@kerfjs/ui/layout.css';

import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';

import { aggregateAlignedChartValues, chartDomainMaximum, ProjectSummary } from './project-summary';
import { SidebarPane, type SidebarPanelParts } from './sidebar-panel';

export interface TerminalProjectSummary {
  id: string;
  name: string;
  completedToday: number;
  inProgress: number;
  trend: number[];
  partial?: boolean;
}

export function aggregateTerminalProjectSummaries(projects: readonly TerminalProjectSummary[]): TerminalProjectSummary {
  const trend = aggregateAlignedChartValues(projects.map((project) => project.trend));
  return {
    id: 'all',
    name: 'All projects',
    completedToday: projects.reduce((sum, project) => sum + project.completedToday, 0),
    inProgress: projects.reduce((sum, project) => sum + project.inProgress, 0),
    trend,
    ...(projects.some((project) => project.partial) ? { partial: true } : {}),
  };
}

/** The operations sidebar's panel parts for the Workbench's left rail (HS2-RWGQWN). */
export function terminalOperationsPanel({
  projects,
}: {
  projects: readonly TerminalProjectSummary[];
}): SidebarPanelParts {
  const aggregate = projects.length > 1 ? aggregateTerminalProjectSummaries(projects) : undefined;
  const groups = aggregate ? [aggregate, ...projects] : projects;
  const chartMaximum = aggregate ? chartDomainMaximum(aggregate.trend) : undefined;
  const content = (
    <List gap="m" controlInsets="trbl">
      {groups.map((group) => (
        <List
          className="terminal-operations-sidebar__group"
          rootAttributes={{ 'data-project-id': group.id }}
          dividerSides={group.id === 'all' ? 'b' : ''}
        >
          <ListHeader label={group.name} inline />
          <ProjectSummary
            completedToday={group.completedToday}
            inProgress={group.inProgress}
            trend={group.trend}
            partial={group.partial}
            projectId={group.id}
            size="compact"
            chartTone={group.id === 'all' ? 'success' : 'brand'}
            chartMaximum={chartMaximum}
            backgroundTrend={group.id === 'all' ? undefined : aggregate?.trend}
          />
        </List>
      ))}
    </List>
  );
  return {
    label: 'Terminal operations sidebar',
    toolbar: { label: 'Operations sidebar toolbar', dividerSides: '' },
    toggle: { action: 'toggle-project-sidebar', name: 'operations sidebar' },
    content,
    pane: { contentElement: 'section', contentLabel: 'Terminal operations sidebar' },
  };
}

export function TerminalOperationsSidebar({ projects }: { projects: readonly TerminalProjectSummary[] }) {
  return (
    <SidebarPane
      parts={terminalOperationsPanel({ projects })}
      className="terminal-operations-sidebar"
      collapseControl
    />
  );
}
