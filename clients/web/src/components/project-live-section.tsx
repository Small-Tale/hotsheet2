import { Badge } from '@kerfjs/ui/badge';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Spacer } from '@kerfjs/ui/spacer';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { Text } from '@kerfjs/ui/text';
import {
  BatteryWarning,
  ChevronsDownUp,
  ChevronsUpDown,
  Cpu,
  HardDrive,
  type IconNode,
  MemoryStick,
  Thermometer,
  Timer,
  WifiOff,
} from 'lucide';

import type { HostHealthSignalId } from '../host-health';
import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';
import { LIVE_STATE_ICONS } from '../live-state-icons';
import {
  formatGlanceDuration,
  type HostCallout,
  lastProgressDescription,
  type LiveChip,
  liveChips,
  liveStateDescription,
  type ProjectLiveSummary,
} from '../project-live-summary';
import { GlanceGauge } from './glance-gauge';

export interface ProjectLiveSectionProps {
  /** The project's live numbers; absent until one of its stores reports. */
  summary?: ProjectLiveSummary;
  /** Host conditions past a threshold; each is a full-width callout in both modes. */
  callouts?: readonly HostCallout[];
  /** The expanded detail rows (`m1`) instead of the compact line (`r3`, the default). */
  expanded?: boolean;
}

const CALLOUT_ICONS: Readonly<Record<HostHealthSignalId, { icon: IconNode; name: string }>> = {
  disk: { icon: HardDrive, name: 'hard-drive' },
  memory: { icon: MemoryStick, name: 'memory-stick' },
  cpu: { icon: Cpu, name: 'cpu' },
  power: { icon: BatteryWarning, name: 'battery-warning' },
  network: { icon: WifiOff, name: 'wifi-off' },
  thermal: { icon: Thermometer, name: 'thermometer' },
};

function chipIcon(chip: LiveChip, summary: ProjectLiveSummary): { icon: IconNode; name: string } {
  switch (chip.kind) {
    case 'state':
      return LIVE_STATE_ICONS[summary.activity];
    case 'waiting':
      return LIVE_STATE_ICONS.waiting_on_you;
    case 'overrun':
      return { icon: Timer, name: 'timer' };
    default:
      return chip.kind satisfies never;
  }
}

function gauges(summary: ProjectLiveSummary, expanded: boolean) {
  const variant = expanded ? 'full' : 'bare',
    size = expanded ? 'medium' : 'chip';
  return (
    <>
      <GlanceGauge
        value={summary.tokensPerMinute}
        peak={summary.tokensPeakPerMinute}
        label="Token rate"
        unit="tokens/min"
        variant={variant}
        size={size}
        tone="brand"
      />
      <GlanceGauge
        value={summary.ticketsUpdatedPerHour}
        peak={summary.ticketsPeakPerHour}
        label="Tickets updated"
        unit="tickets/h"
        variant={variant}
        size={size}
        tone="success"
      />
    </>
  );
}

/** One expanded detail row: glyph, label, and a quiet trailing value. */
function detailRow(kind: string, icon: { icon: IconNode; name: string }, label: string, trailing: string) {
  return (
    <Row gap="xs" vAlign="middle" rootAttributes={{ 'data-live-row': kind }}>
      <LucideIcon icon={icon.icon} name={icon.name} size="s" />
      <Text variant="span" size="compact" wrap="truncate">
        {label}
      </Text>
      <Spacer flex />
      <Text variant="span" size="compact" tone="quiet" wrap="nowrap">
        {trailing}
      </Text>
    </Row>
  );
}

/**
 * The project sidebar's Live section (HS2-F788GW, docs/34-glance-surfaces.md §34.6). Compact (the
 * default) is one line of two bare gauges and glyph chips that appear only when they have something
 * to say; expanded shows the `m1` detail rows. Explanations live only in titles and accessible names
 * (§34.4); host conditions past a threshold keep a full-width callout in both modes (§34.1).
 */
export function ProjectLiveSection({ summary, callouts = [], expanded = false }: ProjectLiveSectionProps) {
  const chips = summary ? liveChips(summary) : [];
  return (
    <section
      class="project-live-section"
      data-component="project-live-section"
      data-mode={expanded ? 'expanded' : 'compact'}
      aria-label="Live"
    >
      <List gap="xs">
        <ListHeader
          label="Live"
          action={COMMANDS_AND_AI_ACTIONS.toggleLiveSection.value}
          actionLabel={expanded ? 'Show compact live summary' : 'Show live details'}
          actionIcon={
            expanded ? (
              <LucideIcon icon={ChevronsDownUp} name="chevrons-down-up" />
            ) : (
              <LucideIcon icon={ChevronsUpDown} name="chevrons-up-down" />
            )
          }
        />
        {summary && !expanded && (
          <Row
            gap="xs"
            vAlign="middle"
            wrap
            textInsets="rl"
            ariaLabel={`${liveStateDescription(summary)}, ${lastProgressDescription(summary)}`}
          >
            {gauges(summary, false)}
            {chips.map((chip) => {
              const icon = chipIcon(chip, summary);
              return (
                <Badge
                  tone={chip.tone}
                  appearance="quiet"
                  shape="pill"
                  title={chip.title}
                  label={chip.title}
                  rootAttributes={{ 'data-live-chip': chip.kind }}
                  {...(chip.text ? { icon: <LucideIcon icon={icon.icon} name={icon.name} /> } : {})}
                >
                  {chip.text || <LucideIcon icon={icon.icon} name={icon.name} />}
                </Badge>
              );
            })}
          </Row>
        )}
        {summary && expanded && (
          <List gap="xs" textInsets="rl">
            <Row gap="m" vAlign="middle">
              {gauges(summary, true)}
            </Row>
            {detailRow(
              'state',
              LIVE_STATE_ICONS[summary.activity],
              liveStateDescription(summary),
              summary.progressAgeMs === undefined ? '' : `${formatGlanceDuration(summary.progressAgeMs)} ago`,
            )}
            {summary.waiting > 0 &&
              detailRow('waiting', LIVE_STATE_ICONS.waiting_on_you, 'Needs you', String(summary.waiting))}
            {summary.overrun &&
              detailRow(
                'overrun',
                { icon: Timer, name: 'timer' },
                `${summary.overrun.slug} past ETA`,
                `+${formatGlanceDuration(summary.overrun.ms)}`,
              )}
          </List>
        )}
        {callouts.map((callout) => (
          <StateBanner
            title={callout.text}
            tone={callout.level === 'critical' ? 'danger' : 'warning'}
            urgency={callout.level === 'critical' ? 'alert' : 'status'}
            icon={<LucideIcon icon={CALLOUT_ICONS[callout.signal].icon} name={CALLOUT_ICONS[callout.signal].name} />}
            rootAttributes={{ 'data-host-callout': callout.signal }}
          />
        ))}
      </List>
    </section>
  );
}
