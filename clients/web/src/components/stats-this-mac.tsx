import './stats-this-mac.css';

import { ContentItem } from '@kerfjs/ui/content-item';
import { px } from '@kerfjs/ui/css-values';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { Grid } from '@kerfjs/ui/grid';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { Text } from '@kerfjs/ui/text';
import { Cpu, HardDrive, type IconNode, MemoryStick, Plug, Thermometer, WifiOff } from 'lucide';

import type { HostHealthSignalId, HostHealthStatus } from '../host-health';
import type { HostHealthResourceState } from '../host-health-resource';
import { hostPlatform, thisMacCards } from '../stats-this-mac';

export interface StatsThisMacProps {
  state: HostHealthResourceState;
}

const ICONS: Record<HostHealthSignalId, { icon: IconNode; name: string }> = {
  disk: { icon: HardDrive, name: 'hard-drive' },
  memory: { icon: MemoryStick, name: 'memory-stick' },
  cpu: { icon: Cpu, name: 'cpu' },
  power: { icon: Plug, name: 'plug' },
  network: { icon: WifiOff, name: 'wifi-off' },
  thermal: { icon: Thermometer, name: 'thermometer' },
};

/** Dark cockpit (§34.1): a card tints only after its signal crossed a threshold. */
function appearance(status: HostHealthStatus) {
  if (status === 'critical') return 'danger' as const;
  if (status === 'warning') return 'warning' as const;
  return 'surface' as const;
}

const TIME = new Intl.DateTimeFormat(undefined, { hour: 'numeric', minute: '2-digit' });

/**
 * The Stats page's **This Mac** cards (HS2-NS0AYR, docs/34-glance-surfaces.md §34.8, mock
 * `m3-stats-right-now`): the server host's disk, memory, CPU, and power, plus network and thermal
 * only while degraded. The group is labelled with the server's host name, because the viewer may be
 * on another device. Values come from the host-health push channel (HS2-DTAVQA); unsupported or
 * unreadable signals say so and never show a value.
 */
export function StatsThisMac({ state }: StatsThisMacProps) {
  const { snapshot } = state;
  if (!snapshot)
    return (
      <section class="stats-this-mac" data-component="stats-this-mac" aria-label="Server Host" data-state="empty">
        {state.error === undefined ? (
          <EmptyState busy={state.loading} title="Loading Server Host Health" />
        ) : (
          <StateBanner title="Server host health is unavailable." detail={state.error} tone="warning" />
        )}
      </section>
    );
  const sampled = snapshot.sampled_at === null ? undefined : TIME.format(new Date(snapshot.sampled_at));
  return (
    <section
      class="stats-this-mac"
      data-component="stats-this-mac"
      aria-label={`Server Host ${snapshot.host.name}`}
      data-state="ready"
      data-level={snapshot.level}
    >
      <List gap="m">
        <div>
          <Text variant="h3" size="medium" flush>
            {snapshot.host.name}
          </Text>
          <Text variant="p" size="compact" tone="quiet" flush rootAttributes={{ 'data-host-context': '' }}>
            Server host · {hostPlatform(snapshot)}
            {sampled === undefined ? '' : ` · sampled ${sampled}`}
          </Text>
        </div>
        {state.error !== undefined && (
          <StateBanner
            title="Host health may be out of date."
            detail={state.error}
            tone="warning"
            rootAttributes={{ 'data-key': 'host-health-stale' }}
          />
        )}
        <Grid minColumnWidth={px(224)} gap="m">
          {thisMacCards(snapshot).map((card) => (
            <ContentItem
              appearance={appearance(card.status)}
              frame="framed"
              rootAttributes={{ 'data-key': `this-mac:${card.id}`, 'data-card': card.id, 'data-status': card.status }}
            >
              <List gap="2xs">
                <Row gap="xs" vAlign="middle">
                  <LucideIcon icon={ICONS[card.id].icon} name={ICONS[card.id].name} size="s" />
                  <Text variant="span" size="compact" weight="strong" flush>
                    {card.title}
                  </Text>
                </Row>
                <Text
                  variant="span"
                  size="large"
                  weight="strong"
                  tone={card.status === 'unsupported' || card.status === 'unavailable' ? 'quiet' : 'default'}
                  flush
                >
                  {card.headline}
                </Text>
                {card.details.map((line) => (
                  <Text variant="span" size="compact" tone="quiet" flush>
                    {line}
                  </Text>
                ))}
              </List>
            </ContentItem>
          ))}
        </Grid>
      </List>
    </section>
  );
}
