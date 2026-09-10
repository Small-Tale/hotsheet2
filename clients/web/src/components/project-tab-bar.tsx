import '@awesome.me/webawesome/dist/components/button/button.js';
import './project-tab-bar.css';

import { ChartNoAxesCombined, Grid3X3, Plus } from 'lucide';

import { LucideIcon } from './lucide-icon';
import { ProjectTab, type ProjectTabProps } from './project-tab';

export interface ProjectTabBarProps {
  tabs: ProjectTabProps[];
  label?: string;
  mode?: ProjectTabBarMode;
}
export type ProjectTabBarMode = 'project' | 'terminals' | 'stats';

export function ProjectTabBar({ tabs, label = 'Open projects', mode = 'project' }: ProjectTabBarProps) {
  return <nav class="project-tab-bar" data-component="project-tab-bar" aria-label={label}>
    <div class="project-tab-bar__modes" role="group" aria-label="Global dashboards">
      <button type="button" tabindex="0" data-action="set-shell-mode" data-shell-mode="terminals" aria-label="Workspace grid" title="Workspace grid" aria-pressed={String(mode === 'terminals')}><LucideIcon icon={Grid3X3} name="grid-3x3" /></button>
      <button type="button" tabindex="0" data-action="set-shell-mode" data-shell-mode="stats" aria-label="Cross-project stats" title="Cross-project stats" aria-pressed={String(mode === 'stats')}><LucideIcon icon={ChartNoAxesCombined} name="chart-no-axes-combined" /></button>
    </div>
    <div class="project-tab-bar__tabs" role="tablist" aria-label={label}>{tabs.map(tab => <ProjectTab {...tab} selected={mode === 'project' && tab.selected} />)}</div>
    <div class="project-tab-bar__actions">
      <wa-button appearance="plain" data-action="add-project" aria-label="Add project" title="Add project"><LucideIcon icon={Plus} name="plus" /></wa-button>
    </div>
  </nav>;
}
