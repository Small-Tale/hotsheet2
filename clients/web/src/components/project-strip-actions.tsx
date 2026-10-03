import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { ChartNoAxesCombined, Grid3X3, Plus } from 'lucide';

import { NAVIGATION_AND_TABS_ACTIONS } from '../interaction-attrs/navigation-and-tabs';
import { PROJECT_LIFECYCLE_ACTIONS } from '../interaction-attrs/project-lifecycle';

/** The shell mode the project strip's dashboard switcher reflects. */
export type ProjectStripMode = 'project' | 'terminals' | 'stats';

/**
 * The project strip's global dashboard switcher: a borderless compact `ToolbarControlGroup` of native
 * pressed buttons. It is zone content of `ProjectTabBar` (TabBar `leading`, or the phone strip's
 * Toolbar `leading`) and is declared `rendersAs @kerfjs/ui:toolbar-control-group` so Kerf's
 * composition rule checks each placement (HS2-PNCDAE).
 */
export function ProjectDashboardModes({ mode }: { mode: ProjectStripMode }) {
  return (
    <ToolbarControlGroup
      label="Global dashboards"
      appearance="borderless"
      size="compact"
      content="icon"
      selectedChrome="raised"
      selectedTone="brand"
    >
      <button
        type="button"
        {...NAVIGATION_AND_TABS_ACTIONS.setShellMode.attrs}
        data-shell-mode="terminals"
        aria-label="Workspace grid"
        aria-pressed={String(mode === 'terminals')}
      >
        <LucideIcon size="s" icon={Grid3X3} name="grid-3x3" />
      </button>
      <button
        type="button"
        {...NAVIGATION_AND_TABS_ACTIONS.setShellMode.attrs}
        data-shell-mode="stats"
        aria-label="Cross-project stats"
        aria-pressed={String(mode === 'stats')}
      >
        <LucideIcon size="s" icon={ChartNoAxesCombined} name="chart-no-axes-combined" />
      </button>
    </ToolbarControlGroup>
  );
}

/**
 * The strip's Add-project action: one native button in a single borderless compact
 * `ToolbarControlGroup` (declared `rendersAs @kerfjs/ui:toolbar-control-group`, HS2-PNCDAE).
 */
export function AddProjectAction() {
  return (
    <ToolbarControlGroup single appearance="borderless" size="compact" content="icon">
      <button type="button" {...PROJECT_LIFECYCLE_ACTIONS.chooseProject.attrs} aria-label="Add project">
        <LucideIcon size="s" icon={Plus} name="plus" />
      </button>
    </ToolbarControlGroup>
  );
}
