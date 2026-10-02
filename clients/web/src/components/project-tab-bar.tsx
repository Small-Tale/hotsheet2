import '@kerfjs/ui/tab-bar.css';
import './project-tab-bar.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Select } from '@kerfjs/ui/select';
import { TabBar } from '@kerfjs/ui/tab-bar';
import type { SafeHtml } from 'kerfjs/jsx-runtime';
import { ArchiveRestore, ChartNoAxesCombined, Grid3X3, Plus } from 'lucide';

import { NAVIGATION_AND_TABS_ACTIONS } from '../interaction-attrs/navigation-and-tabs';
import { PROJECT_LIFECYCLE_ACTIONS } from '../interaction-attrs/project-lifecycle';
import { ProjectTab, type ProjectTabProps } from './project-tab';

export interface ProjectTabBarProps {
  tabs: ProjectTabProps[];
  label?: string;
  mode?: ProjectTabBarMode;
  workspaceAction?: SafeHtml;
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
export type ProjectTabBarMode = 'project' | 'terminals' | 'stats';

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
  const modes = (
    <div class="project-tab-bar__modes" role="group" aria-label="Global dashboards">
      <button
        type="button"
        tabindex="0"
        {...NAVIGATION_AND_TABS_ACTIONS.setShellMode.attrs}
        data-shell-mode="terminals"
        aria-label="Workspace grid"
        title="Workspace grid"
        aria-pressed={String(mode === 'terminals')}
      >
        <LucideIcon icon={Grid3X3} name="grid-3x3" />
      </button>
      <button
        type="button"
        tabindex="0"
        {...NAVIGATION_AND_TABS_ACTIONS.setShellMode.attrs}
        data-shell-mode="stats"
        aria-label="Cross-project stats"
        title="Cross-project stats"
        aria-pressed={String(mode === 'stats')}
      >
        <LucideIcon icon={ChartNoAxesCombined} name="chart-no-axes-combined" />
      </button>
    </div>
  );
  const actions = (
    <div class="project-tab-bar__actions">
      {/* A native button styled by the app: the strip sits beside Kerf's TabBar (whose trailing zone
          takes only dormant decoration), so no Toolbar owns a control group here (HS2-402AXQ). */}
      <button
        type="button"
        class="project-tab-bar__action"
        {...PROJECT_LIFECYCLE_ACTIONS.chooseProject.attrs}
        aria-label="Add project"
        title="Add project"
      >
        <LucideIcon icon={Plus} name="plus" />
      </button>
      {!mobile && workspaceAction && <div class="project-tab-bar__workspace-action">{workspaceAction}</div>}
    </div>
  );
  const rootAttributes = {
    'data-component': 'project-tab-bar',
    'data-mode': mode,
    'data-surface': surface,
    'data-divider': String(divider),
  };
  if (mobile) {
    // Kerf's Select has no disabled choices, so still-opening projects join the list once they
    // register rather than appearing as choices that cannot be selected (HS2-2BEJXD).
    const choosable = tabs.filter((tab) => !tab.pending),
      active = choosable.find((tab) => tab.selected) ?? choosable[0];
    return (
      <div class="project-tab-bar project-tab-bar--mobile" {...rootAttributes}>
        {modes}
        {choosable.length ? (
          <div class="project-tab-bar__select">
            <Select
              presentation="toolbar-borderless"
              size="compact"
              name="mobile-project"
              value={active.id}
              ariaLabel="Project"
              choices={choosable.map((tab) => ({ value: tab.id, label: tab.name }))}
              renderSelected={(choice) => (
                <span class="project-tab-bar__selected-project">
                  {choice.label}
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
          </div>
        ) : (
          <span class="project-tab-bar__select-empty" aria-hidden="true" />
        )}
        {actions}
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
        // Add-project stays beside the last tab; the workspace action is pushed to the far edge
        // inside the growing trailing group (HS2-NE8JBS).
        trailingPlacement="adjacent"
        leading={modes}
        trailing={actions}
      >
        {tabs.map((tab) => (
          <ProjectTab {...tab} selected={mode === 'project' && tab.selected} />
        ))}
      </TabBar>
    </div>
  );
}
