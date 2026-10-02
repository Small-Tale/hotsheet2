import '@kerfjs/ui/layout.css';
import './project-sidebar.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { MessageSquare } from 'lucide';

import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';
import { CommandNavigation, type CommandNavigationItem } from './command-navigation';
import { DriveControl } from './drive-control';
import { type AiToolDescriptor, type AiToolSelection, DriveOptionsMenu } from './drive-options-menu';
import { ProjectSummary } from './project-summary';
import { RepositorySummary } from './repository-summary';
import { SidebarPane, type SidebarPanelParts } from './sidebar-panel';
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
  collapsedCommandGroups?: readonly string[];
  driveRunning: boolean;
  driveTool: string;
  driveToolLabel?: string;
  driveDisabled?: boolean;
  driveDisabledReason?: string;
  driveOptionsOpen?: boolean;
  driveOptionsAnchor?: { x: number; y: number };
  driveTools?: readonly AiToolDescriptor[];
  driveToolsLoading?: boolean;
  driveToolsError?: string;
  driveSelection?: AiToolSelection;
  driveDefaultSelection?: AiToolSelection;
  conversationOpen?: boolean;
  conversationDisabled?: boolean;
  openCount: number;
  upNextCount: number;
  activeCount: number;
  collapseControl?: boolean;
}

/**
 * The project sidebar's panel parts (HS2-RWGQWN): the shell hands them to the Workbench's left
 * rail, whose toolbar carries the standard collapse toggle; {@link ProjectSidebar} renders them
 * standalone.
 */
export function projectSidebarPanel(props: ProjectSidebarProps): SidebarPanelParts {
  const driveToolLabel =
    props.driveToolLabel ??
    props.driveTools?.find((tool) => tool.id === props.driveTool)?.display_name ??
    `${props.driveTool.slice(0, 1).toUpperCase()}${props.driveTool.slice(1)}`;
  // Kerf's Pane owns its footer's (safe-area) edges since beta.51, so the footer's own inset lives on
  // this inner content box rather than on the pane footer (HS2-KMDJRH).
  const footer = (
    <div class="project-sidebar__footer-content">
      <p class="project-sidebar__work-summary" data-component="project-work-summary">
        {props.openCount} open, {props.upNextCount} up next, {props.activeCount} active
      </p>
      <div class="project-sidebar__drive-row">
        <DriveControl
          running={props.driveRunning}
          tool={driveToolLabel}
          disabled={props.driveDisabled}
          disabledReason={props.driveDisabledReason}
          optionsOpen={props.driveOptionsOpen}
        />
        {props.driveOptionsOpen && (
          <DriveOptionsMenu
            tools={props.driveTools ?? []}
            selection={props.driveSelection ?? { tool: props.driveTool }}
            defaultSelection={props.driveDefaultSelection ?? { tool: props.driveTool }}
            loading={props.driveToolsLoading}
            error={props.driveToolsError}
            x={props.driveOptionsAnchor?.x}
            y={props.driveOptionsAnchor?.y}
          />
        )}
        <button
          type="button"
          class="project-sidebar__conversation"
          {...COMMANDS_AND_AI_ACTIONS.openConversation.attrs}
          aria-label={`Open ${driveToolLabel} conversation`}
          title="Open chat without starting the Hot Sheet workflow"
          aria-pressed={props.conversationOpen ? 'true' : 'false'}
          disabled={props.conversationDisabled}
        >
          <LucideIcon icon={MessageSquare} name="message-square" />
        </button>
      </div>
    </div>
  );
  const content = (
    <div class="project-sidebar__content">
      <div class="project-sidebar__summary">
        <ProjectSummary
          completedToday={props.completedToday}
          inProgress={props.inProgress}
          trend={props.completionTrend}
        />
      </div>
      <RepositorySummary
        branch={props.branch}
        unpushed={props.unpushed}
        behind={props.behind}
        uncommitted={props.uncommitted}
        conflicted={props.conflicted}
        error={props.repositoryError}
      />
      <ViewNavigation items={props.views} selectedId={props.selectedViewId} />
      {props.commands.length > 0 ? (
        <CommandNavigation
          label={props.commandGroupLabel}
          commands={props.commands}
          expanded={props.commandGroupExpanded}
          collapsedGroups={props.collapsedCommandGroups}
        />
      ) : (
        <></>
      )}
    </div>
  );
  return {
    label: 'Project sidebar',
    toolbar: { label: 'Project sidebar toolbar', dividerSides: '' },
    toggle: { action: 'toggle-project-sidebar', name: 'project sidebar' },
    content,
    footer,
    pane: {
      contentElement: 'section',
      contentLabel: 'Project sidebar',
      safeAreaEdges: ['block-start', 'block-end', 'inline-start'],
    },
  };
}

export function ProjectSidebar(props: ProjectSidebarProps) {
  return (
    <SidebarPane
      parts={projectSidebarPanel(props)}
      className="project-sidebar"
      collapseControl={props.collapseControl}
    />
  );
}
