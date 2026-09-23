import '@kerfjs/ui/layout.css';
import './terminal-operations-sidebar.css';

import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Pane } from '@kerfjs/ui/pane';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { PanelLeftClose } from 'lucide';

import { aggregateAlignedChartValues, chartDomainMaximum, ProjectSummary } from './project-summary';

export interface TerminalProjectSummary {
  id: string;
  name: string;
  completedToday: number;
  inProgress: number;
  trend: number[];
}

export function aggregateTerminalProjectSummaries(projects: readonly TerminalProjectSummary[]): TerminalProjectSummary {
  const trend = aggregateAlignedChartValues(projects.map((project) => project.trend));
  return {
    id: 'all',
    name: 'All projects',
    completedToday: projects.reduce((sum, project) => sum + project.completedToday, 0),
    inProgress: projects.reduce((sum, project) => sum + project.inProgress, 0),
    trend,
  };
}

export function TerminalOperationsSidebar({ projects }: { projects: readonly TerminalProjectSummary[] }) {
  const aggregate = projects.length > 1 ? aggregateTerminalProjectSummaries(projects) : undefined;
  const groups = aggregate ? [aggregate, ...projects] : projects;
  const chartMaximum = aggregate ? chartDomainMaximum(aggregate.trend) : undefined;
  const header = (
    <Toolbar
      dividerSides=""
      trailing={
        <ToolbarControlGroup appearance="borderless" single>
          <button
            type="button"
            data-action="toggle-project-sidebar"
            aria-label="Hide operations sidebar"
            title="Hide operations sidebar"
          >
            <LucideIcon icon={PanelLeftClose} name="panel-left-close" />
          </button>
        </ToolbarControlGroup>
      }
    />
  );
  return (
    <Pane
      element="aside"
      label="Terminal operations sidebar"
      className="terminal-operations-sidebar"
      header={header}
      contentClassName="terminal-operations-sidebar__groups"
    >
      <List gap="m">
        {groups.map((group) => (
          <section class="terminal-operations-sidebar__group" data-project-id={group.id}>
            <ListHeader label={group.name} />
            <ProjectSummary
              completedToday={group.completedToday}
              inProgress={group.inProgress}
              trend={group.trend}
              projectId={group.id}
              chartTone={group.id === 'all' ? 'success' : 'brand'}
              chartMaximum={chartMaximum}
              backgroundTrend={group.id === 'all' ? undefined : aggregate?.trend}
            />
          </section>
        ))}
      </List>
    </Pane>
  );
}
