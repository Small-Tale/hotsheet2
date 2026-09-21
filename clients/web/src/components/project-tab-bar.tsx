import '@awesome.me/webawesome/dist/components/button/button.js';
import '@kerfjs/ui/tab-bar.css';
import './project-tab-bar.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Select } from '@kerfjs/ui/select';
import { TabBar } from '@kerfjs/ui/tab-bar';
import { ChartNoAxesCombined, Grid3X3, Plus } from 'lucide';

import { ProjectTab, type ProjectTabProps } from './project-tab';

export interface ProjectTabBarProps {
  tabs: ProjectTabProps[];
  label?: string;
  mode?: ProjectTabBarMode;
  /** Mobile: the horizontal tab strip does not fit a single narrow column, so the project tabs are
   * replaced with a project Select while the dashboard mode switcher and Add-project action remain
   * (HS2-4C5RM7). */
  mobile?: boolean;
}
export type ProjectTabBarMode = 'project' | 'terminals' | 'stats';

/** Stable id for the projects tab strip; `wireTabBars`'s reorder reports carry it as `barId` so the
 * host can route project reorders (main.tsx) separately from other tab bars. */
export const PROJECT_TAB_BAR_ID = 'projects';

export function ProjectTabBar({ tabs, label = 'Open projects', mode = 'project', mobile = false }: ProjectTabBarProps) {
  const modes = (
    <div class="project-tab-bar__modes" role="group" aria-label="Global dashboards">
      <button
        type="button"
        tabindex="0"
        data-action="set-shell-mode"
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
        data-action="set-shell-mode"
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
      <wa-button appearance="plain" data-action="choose-project" aria-label="Add project" title="Add project">
        <LucideIcon icon={Plus} name="plus" />
      </wa-button>
    </div>
  );
  if (mobile) {
    const active = tabs.find((tab) => tab.selected) ?? tabs[0];
    return (
      <div class="project-tab-bar project-tab-bar--mobile" data-component="project-tab-bar" data-mode={mode}>
        {modes}
        {tabs.length ? (
          <Select
            className="project-tab-bar__select"
            name="mobile-project"
            value={active.id}
            ariaLabel="Project"
            choices={tabs.map((tab) => ({ value: tab.id, label: tab.name }))}
            renderSelected={(choice) => <span>{choice.label}</span>}
          />
        ) : (
          <span class="project-tab-bar__select-empty" aria-hidden="true" />
        )}
        {actions}
      </div>
    );
  }
  return (
    <TabBar
      id={PROJECT_TAB_BAR_ID}
      label={label}
      className="project-tab-bar"
      // Selecting a project loads/refreshes it, so keep manual activation: arrow keys move roving focus
      // only and the user selects with Enter/Space/click (HS2-08ZG4J). `wireTabBars` reads this.
      activation="manual"
      leading={modes}
      trailing={actions}
    >
      {tabs.map((tab) => (
        <ProjectTab {...tab} selected={mode === 'project' && tab.selected} />
      ))}
    </TabBar>
  );
}
