import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu } from '@kerfjs/ui/popup-menu';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { ChartNoAxesCombined, Folder, Grid3X3, ListChevronsUpDown, Plus } from 'lucide';

import { featureOn } from '../feature-areas';
import { NAVIGATION_AND_TABS_ACTIONS } from '../interaction-attrs/navigation-and-tabs';
import { PROJECT_LIFECYCLE_ACTIONS } from '../interaction-attrs/project-lifecycle';
import { TERMINALS_ACTIONS } from '../interaction-attrs/terminals';

/** The shell mode the project strip's dashboard switcher reflects. */
export type ProjectStripMode = 'project' | 'terminals' | 'stats';

/**
 * The project strip's global dashboard switcher: a borderless compact `ToolbarControlGroup` of native
 * pressed buttons. It is zone content of `ProjectTabBar` (TabBar `leading`, or the phone strip's
 * Toolbar `leading`) and is declared `rendersAs @kerfjs/ui:toolbar-control-group` so Kerf's
 * composition rule checks each placement (HS2-PNCDAE).
 */
export function ProjectDashboardModes({
  mode,
  projectStats = false,
}: {
  mode: ProjectStripMode;
  projectStats?: boolean;
}) {
  // Each dashboard belongs to a feature area and leaves the strip while it is off (HS2-3GJZGG).
  const grid = featureOn('workspace-grid'),
    stats = featureOn('cross-project-stats');
  if (!grid && !stats) return <></>;
  return (
    <ToolbarControlGroup
      label="Global Dashboards"
      appearance="borderless"
      size="compact"
      content="icon"
      selectedChrome="raised"
      selectedTone="brand"
    >
      {grid && (
        <button
          type="button"
          {...NAVIGATION_AND_TABS_ACTIONS.setShellMode.attrs}
          data-shell-mode="terminals"
          aria-label="Workspace Grid"
          aria-pressed={String(mode === 'terminals')}
          data-feature="workspace-grid"
        >
          <LucideIcon size="s" icon={Grid3X3} name="grid-3x3" />
        </button>
      )}
      {stats && (
        <button
          type="button"
          {...NAVIGATION_AND_TABS_ACTIONS.setShellMode.attrs}
          data-shell-mode="stats"
          aria-label="Cross-Project Stats"
          aria-pressed={String(mode === 'stats' && !projectStats)}
          data-feature="cross-project-stats"
        >
          <LucideIcon size="s" icon={ChartNoAxesCombined} name="chart-no-axes-combined" />
        </button>
      )}
    </ToolbarControlGroup>
  );
}

/**
 * The strip's Add-project action: one native button in a single borderless compact
 * `ToolbarControlGroup` (declared `rendersAs @kerfjs/ui:toolbar-control-group`, HS2-PNCDAE).
 */
export interface MinimizedProjectChoice {
  id: string;
  name: string;
  description?: string;
}

export function AddProjectAction({
  minimizedProjects = [],
}: { minimizedProjects?: readonly MinimizedProjectChoice[] } = {}) {
  return (
    <ToolbarControlGroup
      single={minimizedProjects.length === 0}
      nestedDropdown={minimizedProjects.length > 0}
      menuInset="none"
      appearance="borderless"
      size="compact"
      content="icon"
    >
      {minimizedProjects.length > 0 && (
        <PopupMenu
          label="Minimized Projects"
          icon={<LucideIcon size="s" icon={ListChevronsUpDown} name="list-chevrons-up-down" />}
          caret={false}
          rootAttributes={{ 'data-minimized-projects': 'true' }}
          items={[
            { kind: 'heading', label: 'Restore Project' },
            ...minimizedProjects.map((item) => ({
              label: item.name,
              description: item.description,
              icon: <LucideIcon icon={Folder} name="folder" />,
              action: TERMINALS_ACTIONS.restoreMinimizedProject.value,
              attributes: { 'data-project-id': item.id },
            })),
          ]}
        />
      )}
      <button type="button" {...PROJECT_LIFECYCLE_ACTIONS.chooseProject.attrs} aria-label="Add Project">
        <LucideIcon size="s" icon={Plus} name="plus" />
      </button>
    </ToolbarControlGroup>
  );
}
