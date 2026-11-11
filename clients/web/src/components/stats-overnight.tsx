import './stats-overnight.css';

import { EmptyState } from '@kerfjs/ui/empty-state';
import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import { Select } from '@kerfjs/ui/select';
import { Spacer } from '@kerfjs/ui/spacer';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { Text } from '@kerfjs/ui/text';

import type { HistoryState } from '../api';
import {
  formatSegmentDuration,
  HISTORY_STATE_LABELS,
  HISTORY_STATES,
  type OvernightWindow,
  type TimelineLane,
} from '../overnight-timeline';

export interface StatsOvernightProps {
  window: OvernightWindow;
  /** "Since standby" is offered once standby was entered in this session. */
  standbyAvailable: boolean;
  fromMs: number;
  toMs: number;
  lanes: readonly TimelineLane[];
  /** Hour marks inside the window, as Unix milliseconds. */
  axis: readonly number[];
  status: 'loading' | 'ready' | 'error';
  message?: string;
  /** Formats times; the catalog passes a fixed locale. */
  locale?: string;
}

/** The SVG's horizontal resolution: a lane is 1000 units wide and stretches to the track. */
const SPAN = 1000;

/**
 * Overnight (HS2-9AN4DN, docs/34-glance-surfaces.md §34.8, mock `m3-stats-right-now`): one swimlane
 * per project over the selected window, segments coloured by state, and a tick per completed
 * ticket. Each segment and tick names its state or ticket and times in its title.
 */
export function StatsOvernight({
  window,
  standbyAvailable,
  fromMs,
  toMs,
  lanes,
  axis,
  status,
  message,
  locale,
}: StatsOvernightProps) {
  const time = new Intl.DateTimeFormat(locale, {
      ...(toMs - fromMs > 18 * 3_600_000 ? { weekday: 'short' } : {}),
      hour: 'numeric',
      minute: '2-digit',
    }),
    hour = new Intl.DateTimeFormat(locale, { hour: 'numeric' }),
    clock = (ms: number) => time.format(new Date(ms)),
    span = Math.max(1, toMs - fromMs);
  const used = new Set<HistoryState>(lanes.flatMap((lane) => lane.segments.map((segment) => segment.state)));
  return (
    <section class="stats-overnight" data-component="stats-overnight" aria-label="Overnight" data-status={status}>
      <List gap="m">
        <Row vAlign="middle" gap="xs" wrap>
          <Text variant="h3" size="medium" flush>
            Overnight · {clock(fromMs)} – {clock(toMs)}
          </Text>
          <Spacer flex />
          <Select
            name="stats-overnight-window"
            ariaLabel="Overnight Window"
            presentation="inline"
            size="compact"
            value={window}
            choices={[
              ...(standbyAvailable ? [{ value: 'standby', label: 'Since Standby' }] : []),
              { value: '12h', label: 'Last 12 Hours' },
              { value: '24h', label: 'Last 24 Hours' },
            ]}
          />
        </Row>
        {status === 'error' ? (
          <StateBanner tone="warning" title="Overnight history could not load." detail={message ?? ''} />
        ) : lanes.length === 0 ? (
          status === 'loading' ? (
            <Text variant="p" tone="quiet" flush>
              Loading history…
            </Text>
          ) : (
            <EmptyState
              title="No History Yet"
              detail="State changes are recorded while the server hosts a project's Git ticket store."
            />
          )
        ) : (
          <div class="stats-overnight__lanes" role="list">
            {lanes.map((lane) => (
              <div class="stats-overnight__lane" role="listitem" data-store={lane.store}>
                <Text variant="span" size="compact" flush>
                  {lane.name}
                </Text>
                <svg
                  class="stats-overnight__track"
                  viewBox={`0 0 ${String(SPAN)} 10`}
                  preserveAspectRatio="none"
                  role="img"
                  aria-label={`${lane.name}: ${lane.segments
                    .map((segment) => `${HISTORY_STATE_LABELS[segment.state]} from ${clock(segment.start_ms)}`)
                    .join(', ')}${lane.ticks.length ? `; ${String(lane.ticks.length)} completed` : ''}`}
                >
                  {lane.segments.map((segment) => (
                    <rect
                      class="stats-overnight__segment"
                      data-state={segment.state}
                      x={(segment.left * SPAN).toFixed(2)}
                      y="0"
                      width={(segment.width * SPAN).toFixed(2)}
                      height="10"
                    >
                      <title>
                        {`${HISTORY_STATE_LABELS[segment.state]} · ${clock(segment.start_ms)} – ${clock(segment.end_ms)} · ${formatSegmentDuration(segment.end_ms - segment.start_ms)}`}
                      </title>
                    </rect>
                  ))}
                  {lane.ticks.map((tick) => (
                    <line
                      class="stats-overnight__tick"
                      x1={(tick.left * SPAN).toFixed(2)}
                      x2={(tick.left * SPAN).toFixed(2)}
                      y1="0"
                      y2="10"
                    >
                      <title>{`${tick.slug} completed at ${clock(tick.at_ms)}`}</title>
                    </line>
                  ))}
                </svg>
              </div>
            ))}
            <div class="stats-overnight__lane" aria-hidden="true">
              <span />
              <div class="stats-overnight__axis" data-overnight-axis="">
                {axis.map((at, index) => (
                  <span
                    class="stats-overnight__hour"
                    data-minor={index % 2 === 1 ? 'true' : undefined}
                    style={`inset-inline-start:${(((at - fromMs) / span) * 100).toFixed(3)}%`}
                  >
                    <Text variant="span" size="compact" tone="quiet" flush>
                      {hour.format(new Date(at))}
                    </Text>
                  </span>
                ))}
              </div>
            </div>
          </div>
        )}
        <Row gap="m" wrap rootAttributes={{ 'data-overnight-legend': '' }}>
          {HISTORY_STATES.filter((state) => used.has(state) || !['stalled', 'halted'].includes(state)).map((state) => (
            <Row gap="xs" vAlign="middle">
              <svg class="stats-overnight__swatch" viewBox="0 0 10 10" aria-hidden="true">
                <rect class="stats-overnight__segment" data-state={state} width="10" height="10" />
              </svg>
              <Text variant="span" size="compact" tone="quiet" flush>
                {HISTORY_STATE_LABELS[state]}
              </Text>
            </Row>
          ))}
          <Row gap="xs" vAlign="middle">
            <svg class="stats-overnight__swatch" viewBox="0 0 10 10" aria-hidden="true">
              <line class="stats-overnight__tick" x1="5" x2="5" y1="0" y2="10" />
            </svg>
            <Text variant="span" size="compact" tone="quiet" flush>
              Completed ticket
            </Text>
          </Row>
        </Row>
      </List>
    </section>
  );
}
