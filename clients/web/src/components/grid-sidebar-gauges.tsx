import './grid-sidebar-gauges.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';

import { type GridSidebarLive, LIVE_WORD_LABELS, liveStateTitle, liveWordColor } from '../grid-sidebar-live';
import { NAVIGATION_AND_TABS_ACTIONS } from '../interaction-attrs/navigation-and-tabs';
import { LIVE_STATE_ICONS } from '../live-state-icons';
import { GlanceGauge } from './glance-gauge';
import { formatGlanceValue } from './glance-gauge-geometry';

export interface GridSidebarGaugesProps {
  /** The project id the tile opens statistics for, or `all`. */
  projectId: string;
  /** The group's name, for the state glyph's title. */
  name: string;
  /** The group's live numbers; absent while no store reports live state (gauges rest at zero). */
  live?: GridSidebarLive;
  /** Show the state glyph at the row end (per-project groups; the aggregate leaves it out). */
  showState?: boolean;
}

/**
 * The gauge page's group body (HS2-E1AQE8, mock `r4-grid-sidebar-page2`): two bare gauges, blue
 * tokens/min and green tickets updated/h, each on its own 30-day peak, and the state glyph at the row
 * end. Values and durations are only in hover titles and the accessible name (§34.4). Like the
 * completion chart it replaces, the tile is a native button that opens the group's statistics.
 */
export function GridSidebarGauges({ projectId, name, live, showState = true }: GridSidebarGaugesProps) {
  const tokens = live?.tokensPerMinute ?? 0,
    tokensPeak = live?.tokensPeakPerMinute ?? 0,
    tickets = live?.ticketsUpdatedPerHour ?? 0,
    ticketsPeak = live?.ticketsPeakPerHour ?? 0;
  const stateTitle = live && showState ? liveStateTitle(name, live) : undefined;
  const label = [
    'Open project statistics',
    `${formatGlanceValue(tokens)} tokens/min`,
    `${formatGlanceValue(tickets)} tickets updated in the last hour`,
    ...(live && showState ? [LIVE_WORD_LABELS[live.state]] : []),
  ].join(', ');
  return (
    <button
      type="button"
      class="grid-sidebar-gauges"
      data-component="grid-sidebar-gauges"
      data-live-state={live?.state ?? 'none'}
      {...NAVIGATION_AND_TABS_ACTIONS.openProjectStats.attrs}
      data-project-id={projectId}
      aria-label={label}
    >
      {GlanceGauge({
        value: tokens,
        peak: tokensPeak,
        label: 'Token rate',
        unit: 'tokens/min',
        variant: 'bare',
        tone: 'brand',
        size: 'medium',
      })}
      {GlanceGauge({
        value: tickets,
        peak: ticketsPeak,
        label: 'Tickets updated',
        unit: 'tickets/h',
        variant: 'bare',
        tone: 'success',
        size: 'medium',
      })}
      {live && showState ? (
        <span class="grid-sidebar-gauges__state">
          <LucideIcon
            icon={LIVE_STATE_ICONS[live.state].icon}
            name={LIVE_STATE_ICONS[live.state].name}
            size="m"
            color={liveWordColor(live.state)}
            title={stateTitle}
            rootAttributes={{ 'data-live-word': live.state }}
          />
        </span>
      ) : (
        <></>
      )}
    </button>
  );
}
