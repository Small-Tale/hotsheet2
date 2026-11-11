import './grid-sidebar-gauges.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';

import { type GridSidebarLive, LIVE_WORD_LABELS, liveStateTitle, liveWordColor } from '../grid-sidebar-live';
import { NAVIGATION_AND_TABS_ACTIONS } from '../interaction-attrs/navigation-and-tabs';
import { LIVE_STATE_ICONS } from '../live-state-icons';
import { GlanceGauge } from './glance-gauge';
import { formatGlanceValue, GLANCE_GAUGE_OWN_SCALE } from './glance-gauge-geometry';

export interface GridSidebarGaugesProps {
  /** The project id the tile opens statistics for, or `all`. */
  projectId: string;
  /** The group's name, for the state glyph's title. */
  name: string;
  /** The group's live numbers; absent while no store reports live state (gauges rest at zero). */
  live?: GridSidebarLive;
  /** Show the state glyph at the row end (per-project groups; the aggregate leaves it out). */
  showState?: boolean;
  /**
   * The All projects peaks to scale to instead of the group's own, so every project needle reads
   * against one comparable full scale (HS2-VFQM4D). Absent for the aggregate and a lone project.
   */
  scale?: GridSidebarGaugeScale;
}

/** A shared full scale for a gauge pair. */
export interface GridSidebarGaugeScale {
  tokensPeakPerMinute: number;
  ticketsPeakPerHour: number;
}

const SHARED_SCALE_LABEL = `All projects ${GLANCE_GAUGE_OWN_SCALE}`;

/**
 * The gauge page's group body (HS2-E1AQE8, mock `r4-grid-sidebar-page2`): two bare gauges, blue
 * tokens/min and green tickets updated/h, each on its own 24-hour peak or the shared All projects scale
 * (HS2-VFQM4D), and the state glyph at the row
 * end. Values and durations are only in hover titles and the accessible name (§34.4). Like the
 * completion chart it replaces, the tile is a native button that opens the group's statistics.
 */
export function GridSidebarGauges({ projectId, name, live, showState = true, scale }: GridSidebarGaugesProps) {
  const tokens = live?.tokensPerMinute ?? 0,
    tokensPeak = scale ? scale.tokensPeakPerMinute : (live?.tokensPeakPerMinute ?? 0),
    tickets = live?.ticketsUpdatedPerHour ?? 0,
    ticketsPeak = scale ? scale.ticketsPeakPerHour : (live?.ticketsPeakPerHour ?? 0),
    shared = scale ? { scaleLabel: SHARED_SCALE_LABEL } : {};
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
      data-gauge-scale={scale ? 'all-projects' : 'own'}
      {...NAVIGATION_AND_TABS_ACTIONS.openProjectStats.attrs}
      data-project-id={projectId}
      aria-label={label}
    >
      {GlanceGauge({
        value: tokens,
        peak: tokensPeak,
        ...shared,
        label: 'Token rate',
        unit: 'tokens/min',
        variant: 'bare',
        tone: 'brand',
        size: 'medium',
      })}
      {GlanceGauge({
        value: tickets,
        peak: ticketsPeak,
        ...shared,
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
