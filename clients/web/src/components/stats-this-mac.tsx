import './stats-this-mac.css';

import { Button } from '@kerfjs/ui/button';
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

import type { HostCpuFrame } from '../host-cpu-feed';
import { type HostHealthSignalId, type HostHealthStatus, pendingDisks } from '../host-health';
import type { HostHealthResourceState } from '../host-health-resource';
import { SHELL_AND_GLOBAL_ACTIONS } from '../interaction-attrs/shell-and-global';
import { formatBytes, hostPlatform, thisMacCards } from '../stats-this-mac';
import { CpuCoreMeter } from './cpu-core-meter';
import { GlanceSparkline } from './glance-sparkline';

export interface StatsThisMacProps {
  state: HostHealthResourceState;
  /** The live CPU frame while Stats shows (HS2-CPFBWP); absent when the feed is stopped. */
  liveCpu?: HostCpuFrame;
  /** Why the last disk choice failed (HS2-NHK8V8). */
  diskMessage?: string;
  /**
   * What the machine is to this viewer, leading the context line; Stats says "Server host", Systems
   * "This Computer" or "Paired Remote" (HS2-DR0ZSQ).
   */
  context?: string;
  /**
   * Whether a newly detected disk asks Monitor or Ignore here. The choice acts on this app's own
   * server, so Systems turns it off for a remote machine (HS2-DR0ZSQ).
   */
  diskChoices?: boolean;
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
export function StatsThisMac({
  state,
  liveCpu,
  diskMessage,
  context = 'Server host',
  diskChoices = true,
}: StatsThisMacProps) {
  const { snapshot } = state;
  // The empty states name the machine as the context line does, in title case.
  const subject = context.replace(/\b\w/gu, (letter) => letter.toUpperCase());
  if (!snapshot)
    return (
      <section class="stats-this-mac" data-component="stats-this-mac" aria-label={subject} data-state="empty">
        {state.error === undefined ? (
          <EmptyState busy={state.loading} title={`Loading ${subject} Health`} />
        ) : (
          <StateBanner
            title={`${context.charAt(0).toUpperCase()}${context.slice(1)} health is unavailable.`}
            detail={state.error}
            tone="warning"
            copyLayout="stacked"
          />
        )}
      </section>
    );
  const sampled = snapshot.sampled_at === null ? undefined : TIME.format(new Date(snapshot.sampled_at));
  return (
    <section
      class="stats-this-mac"
      data-component="stats-this-mac"
      aria-label={`${subject} ${snapshot.host.name}`}
      data-state="ready"
      data-level={snapshot.level}
    >
      <List gap="m">
        <div>
          <Text variant="h3" size="medium" flush>
            {snapshot.host.name}
          </Text>
          <Text variant="p" size="compact" tone="quiet" flush rootAttributes={{ 'data-host-context': '' }}>
            {context} · {hostPlatform(snapshot)}
            {sampled === undefined ? '' : ` · sampled ${sampled}`}
          </Text>
        </div>
        {state.error !== undefined && (
          <StateBanner
            title="Host health may be out of date."
            detail={state.error}
            tone="warning"
            copyLayout="stacked"
            rootAttributes={{ 'data-key': 'host-health-stale' }}
          />
        )}
        {/* A newly detected disk asks once whether it counts (HS2-NHK8V8); the choice is machine-wide.
            Stacked copy wraps the detail, so a phone still shows the whole question (HS2-4JRM43). */}
        {(diskChoices ? pendingDisks(snapshot) : []).map((disk) => (
          <StateBanner
            tone="info"
            copyLayout="stacked"
            title={`New Disk Detected: ${disk.label}`}
            detail={`${disk.total_bytes === null ? 'A disk' : formatBytes(disk.total_bytes)}${disk.mount === null ? '' : ` at ${disk.mount}`}. Should host health watch its free space?`}
            rootAttributes={{ 'data-key': `pending-disk:${disk.id}`, 'data-pending-disk': disk.id }}
            action={
              // The action zone lays the two choices out itself (KF-8TF7Y8, HS2-W77711).
              <>
                <Button
                  type="button"
                  size="compact"
                  shape="pill"
                  action={SHELL_AND_GLOBAL_ACTIONS.chooseHostDisk.value}
                  rootAttributes={{ 'data-disk-id': disk.id, 'data-disk-monitor': 'true' }}
                >
                  Monitor
                </Button>
                <Button
                  type="button"
                  size="compact"
                  shape="pill"
                  action={SHELL_AND_GLOBAL_ACTIONS.chooseHostDisk.value}
                  rootAttributes={{ 'data-disk-id': disk.id, 'data-disk-monitor': 'false' }}
                >
                  Ignore
                </Button>
              </>
            }
          />
        ))}
        {diskMessage ? (
          <StateBanner
            tone="warning"
            title="The disk choice was not saved."
            detail={diskMessage}
            copyLayout="stacked"
            rootAttributes={{ 'data-key': 'host-disk-message' }}
          />
        ) : null}
        <Grid minColumnWidth={px(224)} gap="m">
          {thisMacCards(snapshot, liveCpu).map((card) => (
            <ContentItem
              appearance={appearance(card.status)}
              frame="framed"
              rootAttributes={{
                'data-key': `this-mac:${card.key}`,
                'data-card': card.id,
                'data-card-key': card.key,
                'data-status': card.status,
              }}
            >
              <List gap="2xs">
                <Row gap="xs" vAlign="middle">
                  <LucideIcon icon={ICONS[card.id].icon} name={ICONS[card.id].name} size="s" />
                  <Text variant="span" size="compact" weight="strong" flush>
                    {card.title}
                  </Text>
                </Row>
                <Row gap="xs" vAlign="middle">
                  <Text
                    variant="span"
                    size="large"
                    weight="strong"
                    tone={card.status === 'unsupported' || card.status === 'unavailable' ? 'quiet' : 'default'}
                    flush
                  >
                    {card.headline}
                  </Text>
                  {card.cores && <CpuCoreMeter cores={card.cores} />}
                </Row>
                {card.trend && (
                  <GlanceSparkline
                    values={card.trend.values}
                    label={card.trend.label}
                    max={100}
                    tone={card.status === 'warning' || card.status === 'critical' ? 'warning' : 'neutral'}
                  />
                )}
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
