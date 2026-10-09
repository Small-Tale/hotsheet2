import '@kerfjs/ui/tab-bar.css';
import './project-tab-bar.css';

import { uiColor } from '@kerfjs/ui/css-values';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Select } from '@kerfjs/ui/select';
import { TabBar } from '@kerfjs/ui/tab-bar';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { ArchiveRestore, CircleAlert } from 'lucide';

import { AddProjectAction, ProjectDashboardModes, type ProjectStripMode } from './project-strip-actions';
import { ProjectTab, type ProjectTabProps } from './project-tab';
import { TicketViewAction, type TicketViewActionSpec } from './workspace-controls';

export interface ProjectTabBarProps {
  tabs: ProjectTabProps[];
  label?: string;
  mode?: ProjectTabBarMode;
  /** The current ticket view's primary action, pinned in TabBar's far-edge `end` zone. It is data, not
   * markup, so the zone renders literal JSX that Kerf's composition rule checks (HS2-PNCDAE). */
  workspaceAction?: TicketViewActionSpec;
  /** Mobile: the horizontal tab strip does not fit a single narrow column, so the project tabs are
   * replaced with a project Select while the dashboard mode switcher and Add-project action remain
   * (HS2-4C5RM7). */
  mobile?: boolean;
  /** The strip's surface: `lowered` (default) sets it apart as its own band; `default` shares the
   * surface of a column it heads, such as the app shell's main column. */
  surface?: 'lowered' | 'default';
  /** Draw the bottom rule separating the strip from the content below it (default true). */
  divider?: boolean;
}
export type ProjectTabBarMode = ProjectStripMode;

/** Stable id for the projects tab strip; `wireTabBars`'s reorder reports carry it as `barId` so the
 * host can route project reorders (main.tsx) separately from other tab bars. */
export const PROJECT_TAB_BAR_ID = 'projects';

export function ProjectTabBar({
  tabs,
  label = 'Open projects',
  mode = 'project',
  workspaceAction,
  mobile = false,
  surface = 'lowered',
  divider = true,
}: ProjectTabBarProps) {
  const rootAttributes = {
    'data-component': 'project-tab-bar',
    'data-mode': mode,
    'data-surface': surface,
    'data-divider': String(divider),
  };
  if (mobile) {
    // Still-opening projects join the list once they register rather than appearing as choices
    // that cannot yet be selected (HS2-2BEJXD).
    const choosable = tabs.filter((tab) => !tab.pending),
      active = choosable.find((tab) => tab.selected) ?? choosable[0];
    return (
      <div class="project-tab-bar project-tab-bar--mobile" {...rootAttributes}>
        {/* The phone strip is a Kerf Toolbar whose leading zone holds the mode switcher, the project
            Select, and Add project as control groups, so each has a cataloged parent (HS2-PNCDAE). */}
        <Toolbar
          label="Projects"
          dividerSides=""
          leading={
            <>
              <ProjectDashboardModes mode={mode} />
              {choosable.length > 0 && (
                <ToolbarControlGroup single appearance="borderless" size="compact">
                  <Select
                    presentation="toolbar-borderless"
                    size="compact"
                    name="mobile-project"
                    value={active.id}
                    ariaLabel="Project"
                    choices={choosable.map((tab) => ({
                      value: tab.id,
                      label: tab.attention ? `${tab.name} — Needs attention` : tab.name,
                      icon: tab.attention ? CircleAlert : undefined,
                      iconName: tab.attention ? 'circle-alert' : undefined,
                      color: tab.attention ? uiColor('danger-on-quiet') : undefined,
                    }))}
                    renderSelected={(choice) => (
                      <span class="project-tab-bar__selected-project">
                        {choosable.find((tab) => tab.id === choice.value)?.name ?? choice.label}
                        {active.attention && (
                          <LucideIcon
                            icon={CircleAlert}
                            name="circle-alert"
                            size="s"
                            color={uiColor('danger-on-quiet')}
                            label={active.attentionLabel ?? 'Needs attention'}
                          />
                        )}
                        {active.operation && (
                          <span
                            class="project-tab-bar__operation"
                            aria-label={active.operation.label}
                            title={active.operation.label}
                          >
                            <LucideIcon icon={ArchiveRestore} name="archive-restore" />
                            <small>
                              {active.operation.percent !== undefined
                                ? `${Math.floor(active.operation.percent)}%`
                                : active.operation.state === 'running'
                                  ? 'Working'
                                  : active.operation.state === 'succeeded'
                                    ? 'Backup'
                                    : 'Attention'}
                            </small>
                          </span>
                        )}
                      </span>
                    )}
                  />
                </ToolbarControlGroup>
              )}
              <AddProjectAction />
            </>
          }
        />
      </div>
    );
  }
  return (
    <div class="project-tab-bar" {...rootAttributes}>
      <TabBar
        id={PROJECT_TAB_BAR_ID}
        label={label}
        // Selecting a project loads/refreshes it, so keep manual activation: arrow keys move roving focus
        // only and the user selects with Enter/Space/click (HS2-08ZG4J). `wireTabBars` reads this.
        activation="manual"
        // Project views keep Add project beside the last tab while their workspace action uses the
        // far-edge `end` zone. Dashboards have no workspace action, so Add project takes that edge.
        trailingPlacement={mode === 'project' ? 'adjacent' : 'separate'}
        leading={<ProjectDashboardModes mode={mode} />}
        trailing={<AddProjectAction />}
        end={workspaceAction && <TicketViewAction action={workspaceAction} />}
      >
        {tabs.map((tab) => (
          <ProjectTab {...tab} selected={mode === 'project' && tab.selected} />
        ))}
      </TabBar>
    </div>
  );
}
