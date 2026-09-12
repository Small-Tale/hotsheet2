import './project-sidebar.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { MessageSquare,PanelLeftClose } from 'lucide';

import { CommandNavigation, type CommandNavigationItem } from './command-navigation';
import { DriveControl } from './drive-control';
import {type AiToolDescriptor,type AiToolSelection,DriveOptionsMenu} from './drive-options-menu';
import { ProjectSummary } from './project-summary';
import { RepositorySummary } from './repository-summary';
import { ViewNavigation, type ViewNavigationItem } from './view-navigation';

export interface ProjectSidebarProps {
  completedToday: number;
  inProgress: number;
  completionTrend: number[];
  branch: string;
  unpushed: number;
  behind?: number;
  uncommitted: number;
  conflicted?: number;
  repositoryError?: boolean;
  views: ViewNavigationItem[];
  selectedViewId: string;
  commandGroupLabel: string;
  commands: CommandNavigationItem[];
  commandGroupExpanded: boolean;
  collapsedCommandGroups?:readonly string[];
  driveRunning: boolean;
  driveTool: string;
  driveToolLabel?: string;
  driveDisabled?: boolean;
  driveDisabledReason?: string;
  driveOptionsOpen?: boolean;
  driveTools?: readonly AiToolDescriptor[];
  driveToolsLoading?:boolean;
  driveToolsError?:string;
  driveSelection?: AiToolSelection;
  driveDefaultSelection?: AiToolSelection;
  conversationOpen?: boolean;
  conversationDisabled?: boolean;
  openCount: number;
  upNextCount: number;
  activeCount: number;
  collapseControl?: boolean;
}

export function ProjectSidebar(props: ProjectSidebarProps) {
  const driveToolLabel=props.driveToolLabel??props.driveTools?.find(tool=>tool.id===props.driveTool)?.display_name??`${props.driveTool.slice(0,1).toUpperCase()}${props.driveTool.slice(1)}`;
  return <aside class="project-sidebar" data-component="project-sidebar" aria-label="Project sidebar">
    {props.collapseControl && <Toolbar divider={false} trailing={<ToolbarControlGroup appearance="borderless" single><button type="button" data-action="toggle-project-sidebar" aria-label="Hide project sidebar" title="Hide project sidebar"><LucideIcon icon={PanelLeftClose} name="panel-left-close" /></button></ToolbarControlGroup>} />}
    <div class="project-sidebar__content">
      <ProjectSummary completedToday={props.completedToday} inProgress={props.inProgress} trend={props.completionTrend} />
      <RepositorySummary branch={props.branch} unpushed={props.unpushed} behind={props.behind} uncommitted={props.uncommitted} conflicted={props.conflicted} error={props.repositoryError} />
      <ViewNavigation items={props.views} selectedId={props.selectedViewId} />
      {props.commands.length > 0 && <CommandNavigation label={props.commandGroupLabel} commands={props.commands} expanded={props.commandGroupExpanded} collapsedGroups={props.collapsedCommandGroups} />}
    </div>
    <footer class="project-sidebar__footer">
      <p class="project-sidebar__work-summary" data-component="project-work-summary">{props.openCount} open, {props.upNextCount} up next, {props.activeCount} claimed</p>
      <div class="project-sidebar__drive-row">
        <DriveControl running={props.driveRunning} tool={driveToolLabel} disabled={props.driveDisabled} disabledReason={props.driveDisabledReason} optionsOpen={props.driveOptionsOpen} />
        {props.driveOptionsOpen&&<DriveOptionsMenu tools={props.driveTools??[]} selection={props.driveSelection??{tool:props.driveTool}} defaultSelection={props.driveDefaultSelection??{tool:props.driveTool}} loading={props.driveToolsLoading} error={props.driveToolsError}/>}
        <button type="button" class="project-sidebar__conversation" data-action="open-conversation" aria-label={`Open ${driveToolLabel} conversation`} title="Open chat without starting the Hot Sheet workflow" aria-pressed={props.conversationOpen?'true':'false'} disabled={props.conversationDisabled}><LucideIcon icon={MessageSquare} name="message-square"/></button>
      </div>
    </footer>
  </aside>;
}
