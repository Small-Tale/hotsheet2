import './terminal-operations-sidebar.css';

import { PanelLeftClose } from 'lucide';

import { LucideIcon } from './lucide-icon';
import { MenuHeader } from './menu-header';
import { aggregateAlignedChartValues, chartDomainMaximum, ProjectSummary } from './project-summary';
import { Toolbar } from './toolbar';
import { ToolbarControlGroup } from './toolbar-control-group';

export interface TerminalProjectSummary {
  id: string;
  name: string;
  completedToday: number;
  inProgress: number;
  trend: number[];
}

export function aggregateTerminalProjectSummaries(projects: readonly TerminalProjectSummary[]): TerminalProjectSummary {
  const trend=aggregateAlignedChartValues(projects.map(project=>project.trend));
  return{id:'all',name:'All projects',completedToday:projects.reduce((sum,project)=>sum+project.completedToday,0),inProgress:projects.reduce((sum,project)=>sum+project.inProgress,0),trend};
}

export function TerminalOperationsSidebar({projects}:{projects:readonly TerminalProjectSummary[]}) {
  const aggregate=projects.length>1?aggregateTerminalProjectSummaries(projects):undefined;
  const groups=aggregate?[aggregate,...projects]:projects;
  const chartMaximum=aggregate?chartDomainMaximum(aggregate.trend):undefined;
  return <aside class="terminal-operations-sidebar" data-component="terminal-operations-sidebar" aria-label="Terminal operations sidebar">
    <Toolbar divider={false} trailing={<ToolbarControlGroup appearance="borderless" single><button type="button" data-action="toggle-project-sidebar" aria-label="Hide operations sidebar" title="Hide operations sidebar"><LucideIcon icon={PanelLeftClose} name="panel-left-close"/></button></ToolbarControlGroup>}/>
    <div class="terminal-operations-sidebar__groups">{groups.map(group=><section class="terminal-operations-sidebar__group" data-project-id={group.id}><MenuHeader label={group.name}/><ProjectSummary completedToday={group.completedToday} inProgress={group.inProgress} trend={group.trend} projectId={group.id} chartTone={group.id==='all'?'success':'brand'} chartMaximum={chartMaximum}/></section>)}</div>
  </aside>;
}
