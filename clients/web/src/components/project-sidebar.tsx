import { CommandNavigation, type CommandNavigationItem } from './command-navigation';
import { DriveControl } from './drive-control';
import { ProjectSummary } from './project-summary';
import { RepositorySummary } from './repository-summary';
import { ViewNavigation, type ViewNavigationItem } from './view-navigation';
import './project-sidebar.css';

export interface ProjectSidebarProps {
  completedToday: number;
  inProgress: number;
  completionTrend: number[];
  branch: string;
  unpushed: number;
  uncommitted: number;
  views: ViewNavigationItem[];
  selectedViewId: string;
  commandGroupLabel: string;
  commands: CommandNavigationItem[];
  commandGroupExpanded: boolean;
  driveRunning: boolean;
  driveTool: string;
}

export function ProjectSidebar(props: ProjectSidebarProps) {
  return <aside class="project-sidebar" data-component="project-sidebar" aria-label="Project sidebar">
    <div class="project-sidebar__content">
      <ProjectSummary completedToday={props.completedToday} inProgress={props.inProgress} trend={props.completionTrend} />
      <RepositorySummary branch={props.branch} unpushed={props.unpushed} uncommitted={props.uncommitted} />
      <ViewNavigation items={props.views} selectedId={props.selectedViewId} />
      <CommandNavigation label={props.commandGroupLabel} commands={props.commands} expanded={props.commandGroupExpanded} />
    </div>
    <DriveControl running={props.driveRunning} tool={props.driveTool} />
  </aside>;
}
