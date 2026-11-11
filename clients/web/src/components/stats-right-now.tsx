import './stats-right-now.css';

import { ContentItem } from '@kerfjs/ui/content-item';
import { px } from '@kerfjs/ui/css-values';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { Grid } from '@kerfjs/ui/grid';
import { List } from '@kerfjs/ui/list';
import { Text } from '@kerfjs/ui/text';

import type { RightNowRow, RightNowTiles } from '../stats-right-now';
import { GlanceGauge } from './glance-gauge';
import { formatGlanceValue } from './glance-gauge-geometry';
import { GlanceSparkline } from './glance-sparkline';
import { StatsProjectsTable } from './stats-projects-table';

export interface StatsRightNowProps {
  /** HS2-PZPD83: a project's Stats shows that project; cross-project Stats shows all of them. */
  scope: { kind: 'project'; name: string } | { kind: 'all' };
  /** Stores in scope with a live snapshot; zero renders the empty state. */
  stores: number;
  tiles: RightNowTiles;
  rows: readonly RightNowRow[];
  /** Today's completions and when "today" began; absent while analytics load. */
  completed?: { count: number; sinceMs: number };
  /** When the client last applied a pushed snapshot. */
  asOfMs?: number;
}

const TIME = new Intl.DateTimeFormat(undefined, { hour: 'numeric', minute: '2-digit' });

/** "4 PM" on the hour, else "4:30 PM". */
function clockTime(ms: number): string {
  return TIME.format(new Date(ms)).replace(/:00(?=\D|$)/u, '');
}

/**
 * The Stats page's **Right now** section (HS2-ERTNJ7, docs/34-glance-surfaces.md §34.8, mock
 * `m3-stats-right-now`): the glance numbers at full size, then a per-project table sorted by need.
 * Everything is derived from the pushed live state; only problems tint (dark cockpit, §34.1).
 */
export function StatsRightNow({ scope, stores, tiles, rows, completed, asOfMs }: StatsRightNowProps) {
  const scopeText = scope.kind === 'all' ? 'All projects' : scope.name;
  const freshness = asOfMs === undefined ? 'waiting for live data' : `data as of ${clockTime(asOfMs)}`;
  return (
    <section class="stats-right-now" data-component="stats-right-now" aria-label="Right Now">
      <List gap="l">
        <div>
          <Text variant="h2" size="large" flush>
            Right now
          </Text>
          <Text variant="p" tone="quiet" flush rootAttributes={{ 'data-freshness': '' }}>
            {scopeText} · live over the change stream · {freshness}
          </Text>
        </div>
        {stores === 0 ? (
          <EmptyState
            title="No Live Projects"
            detail="Live numbers appear here once a project's Git ticket store reports its live state."
          />
        ) : (
          <>
            <Grid minColumnWidth={px(176)} gap="m" rootAttributes={{ 'data-right-now-tiles': '' }}>
              <ContentItem appearance="surface" frame="framed" rootAttributes={{ 'data-tile': 'tokens' }}>
                <List gap="2xs">
                  <GlanceGauge
                    value={tiles.tokensPerMinute}
                    peak={tiles.tokensPeakPerMinute}
                    label="Token rate"
                    unit="tokens/min"
                  />
                  <Text variant="span" size="compact" tone="quiet" flush>
                    Scale: 24-hour peak {formatGlanceValue(tiles.tokensPeakPerMinute)}/min
                  </Text>
                </List>
              </ContentItem>
              <ContentItem appearance="surface" frame="framed" rootAttributes={{ 'data-tile': 'updated' }}>
                <List gap="2xs">
                  <Text variant="span" size="xlarge" weight="strong" flush>
                    {tiles.ticketsUpdated}
                  </Text>
                  <Text variant="span" size="compact" tone="quiet" flush>
                    Tickets updated · last 60 min
                  </Text>
                  <GlanceSparkline
                    values={tiles.updatedSparkline}
                    label="Tickets updated per 5 minutes, last hour"
                    tone="success"
                  />
                </List>
              </ContentItem>
              {completed && (
                <ContentItem appearance="surface" frame="framed" rootAttributes={{ 'data-tile': 'completed' }}>
                  <List gap="2xs">
                    <Text variant="span" size="xlarge" weight="strong" flush>
                      {completed.count}
                    </Text>
                    <Text
                      variant="span"
                      size="compact"
                      tone="quiet"
                      flush
                      title="Completions are counted per UTC calendar day"
                    >
                      Completed since {clockTime(completed.sinceMs)}
                    </Text>
                  </List>
                </ContentItem>
              )}
              <ContentItem
                appearance={tiles.needsYou > 0 ? 'warning' : 'surface'}
                frame="framed"
                rootAttributes={{ 'data-tile': 'needs-you', 'data-attention': String(tiles.needsYou > 0) }}
              >
                <List gap="2xs">
                  <Text variant="span" size="xlarge" weight="strong" flush>
                    {tiles.needsYou}
                  </Text>
                  <Text variant="span" size="compact" tone={tiles.needsYou > 0 ? 'default' : 'quiet'} flush>
                    Needs you · feedback and reviews
                  </Text>
                </List>
              </ContentItem>
              <ContentItem appearance="surface" frame="framed" rootAttributes={{ 'data-tile': 'workers' }}>
                <List gap="2xs">
                  <Text variant="span" size="xlarge" weight="strong" flush>
                    {tiles.workersLive} / {tiles.workersExpected}
                  </Text>
                  <Text variant="span" size="compact" tone="quiet" flush>
                    Workers live
                  </Text>
                </List>
              </ContentItem>
            </Grid>
            <ContentItem appearance="surface" frame="framed" rootAttributes={{ 'data-right-now-projects': '' }}>
              <List gap="xs">
                <Text variant="h3" size="medium" flush>
                  Projects
                </Text>
                <StatsProjectsTable rows={rows} />
              </List>
            </ContentItem>
          </>
        )}
      </List>
    </section>
  );
}
