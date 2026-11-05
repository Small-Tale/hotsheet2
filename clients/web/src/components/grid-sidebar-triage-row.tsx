import { uiColor } from '@kerfjs/ui/css-values';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Text } from '@kerfjs/ui/text';

import {
  type GridSidebarLive,
  LIVE_WORD_LABELS,
  liveStateTitle,
  liveWordColor,
  liveWorkSummary,
} from '../grid-sidebar-live';
import { NAVIGATION_AND_TABS_ACTIONS } from '../interaction-attrs/navigation-and-tabs';
import { LIVE_STATE_ICONS } from '../live-state-icons';

export interface GridSidebarTriageRowProps {
  projectId: string;
  /** The project name, for the hover title. */
  name: string;
  /** The project's live numbers; absent while no store reports live state. */
  live?: GridSidebarLive;
}

/**
 * The triage page's group body (HS2-E1AQE8, mock `m2-grid-sidebar-projects`): the state glyph and
 * state word, then the current ticket and phase. Durations and causes live only in the hover title
 * (§34.4). A stopped project renders quiet (dark cockpit, §34.1). The row opens the project's
 * statistics, like the other pages' bodies.
 */
export function GridSidebarTriageRow({ projectId, name, live }: GridSidebarTriageRowProps) {
  const action = NAVIGATION_AND_TABS_ACTIONS.openProjectStats.value;
  if (!live) {
    const title = `${name}: no live state from this project's server yet`;
    return (
      <ListItem
        action={action}
        rootAttributes={{ 'data-project-id': projectId, 'data-live-word': 'none' }}
        label={
          <Text variant="span" tone="quiet">
            No live state
          </Text>
        }
        title={title}
        accessibleLabel={`Open project statistics. ${title}`}
      />
    );
  }
  const icon = LIVE_STATE_ICONS[live.state];
  const title = liveStateTitle(name, live);
  const stopped = live.state === 'stopped';
  const work = liveWorkSummary(live);
  return (
    <ListItem
      action={action}
      rootAttributes={{ 'data-project-id': projectId, 'data-live-word': live.state }}
      icon={<LucideIcon icon={icon.icon} name={icon.name} color={liveWordColor(live.state)} />}
      label={
        <Text variant="span" color={stopped ? uiColor('text-quiet') : liveWordColor(live.state)}>
          {LIVE_WORD_LABELS[live.state]}
        </Text>
      }
      {...(work ? { description: work } : {})}
      title={title}
      accessibleLabel={`Open project statistics. ${title}`}
    />
  );
}
