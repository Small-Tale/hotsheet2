import '@kerfjs/ui/layout.css';

import { Badge } from '@kerfjs/ui/badge';
import { px } from '@kerfjs/ui/css-values';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { Grid } from '@kerfjs/ui/grid';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Pane } from '@kerfjs/ui/pane';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { Text } from '@kerfjs/ui/text';
import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';
import { Bot } from 'lucide';

import type { HostCpuFrame } from '../host-cpu-feed';
import type { HostHealthResourceState } from '../host-health-resource';
import type { SystemsCapacityState } from '../systems-capacity-feed';
import {
  aiToolRows,
  capabilityRows,
  machineKindLabel,
  machineUnavailableMessage,
  type SystemsMachine,
  toolchainRows,
} from '../systems-model';
import { StatsThisMac } from './stats-this-mac';

export interface SystemsViewProps {
  machine: SystemsMachine;
  /** The machine's pushed host health (the local resource, or the remote tab's). */
  health: HostHealthResourceState;
  /** The live CPU frame, while the feed runs. */
  liveCpu?: HostCpuFrame;
  /** The machine's hosted capacity stream. */
  capacity: SystemsCapacityState;
  /** Why the last disk choice failed (This Computer only, HS2-NHK8V8). */
  diskMessage?: string;
  toolLabel?: (tool: string) => string;
}

const quiet = (text: string) => (
  <Text tone="quiet" size="compact" flush>
    {text}
  </Text>
);

/** A section's table, its loading placeholder, or why it has no rows. */
function section(label: string, body: ReturnType<typeof quiet>) {
  return (
    <section aria-label={label} data-systems-section={label}>
      <ListHeader label={label} />
      {body}
    </section>
  );
}

/**
 * The Systems screen's workspace (HS2-DR0ZSQ, approved Concept B: machine detail): the selected
 * machine's This Mac cards (disk, memory, CPU with its core meter and sparkline, power), then its AI
 * tools with their running sessions, its capabilities, and its toolchains. Every reading is pushed or
 * long-polled; a machine without a connected project says how to connect it.
 */
export function SystemsView({ machine, health, liveCpu, capacity, diskMessage, toolLabel }: SystemsViewProps) {
  const unavailable = machineUnavailableMessage(machine);
  const reading = capacity.capacity;
  const unread = (what: string) =>
    quiet(
      capacity.error
        ? `${what} is not available. ${capacity.error}`
        : reading
          ? `${what} is not reported.`
          : 'Loading…',
    );
  const tools = aiToolRows(reading, toolLabel);
  const capabilities = capabilityRows({ capacity: reading, health: health.snapshot });
  const toolchains = toolchainRows(reading);
  return (
    <Pane element="section" label={`Systems: ${machine.name}`} deepInset>
      <div class="kui-content" data-component="systems-view" data-machine-id={machine.id}>
        {unavailable ? (
          <List gap="m">
            <div>
              <Text variant="h3" size="medium" flush>
                {machine.name}
              </Text>
              {quiet(machineKindLabel(machine))}
            </div>
            <EmptyState title={unavailable.title} detail={unavailable.detail} />
          </List>
        ) : (
          <List gap="l">
            <StatsThisMac
              state={health}
              liveCpu={liveCpu}
              diskMessage={machine.kind === 'local' ? diskMessage : undefined}
              context={machineKindLabel(machine)}
              diskChoices={machine.kind === 'local'}
            />
            {capacity.error && reading ? (
              <StateBanner
                title="These readings may be out of date."
                detail={capacity.error}
                tone="warning"
                copyLayout="stacked"
                rootAttributes={{ 'data-key': 'systems-capacity-stale' }}
              />
            ) : null}
            {section(
              'AI Tools',
              tools && tools.length ? (
                <ValueTable label={`AI tools on ${machine.name}`} density="compact" stackBelow="compact">
                  {tools.map((tool) => (
                    <ValueTableRow
                      icon={<LucideIcon icon={Bot} name="bot" />}
                      label={
                        <List gap="2xs">
                          <Text variant="span" flush>
                            {tool.label}
                          </Text>
                          <Text variant="span" size="compact" tone="quiet" flush>
                            {tool.description}
                          </Text>
                        </List>
                      }
                      value={
                        // HS2-S1FEYY: Signed In, Signed Out, Not Installed, or Installed while unknown.
                        <Badge tone={tool.status.tone} appearance="quiet" size="compact">
                          {tool.status.label}
                        </Badge>
                      }
                      rootAttributes={{
                        'data-systems-ai-tool': tool.tool,
                        'data-installed': String(tool.installed),
                        'data-signed-in': tool.signedIn === undefined ? 'unknown' : String(tool.signedIn),
                      }}
                    />
                  ))}
                </ValueTable>
              ) : (
                unread('AI tool information')
              ),
            )}
            <Grid minColumnWidth={px(512)} gap="l">
              {section(
                'Capabilities',
                capabilities.length ? (
                  <ValueTable label={`${machine.name} capabilities`} stackBelow="compact">
                    {capabilities.map((row) => (
                      <ValueTableRow label={row.label} value={row.value} />
                    ))}
                  </ValueTable>
                ) : (
                  unread('What this machine is')
                ),
              )}
              {section(
                'Toolchains',
                toolchains && toolchains.length ? (
                  <ValueTable label={`${machine.name} toolchains`} stackBelow="compact">
                    {toolchains.map((row) => (
                      <ValueTableRow label={row.label} value={row.value} />
                    ))}
                  </ValueTable>
                ) : toolchains ? (
                  quiet('None of the declared toolchains is installed.')
                ) : (
                  unread('Toolchain information')
                ),
              )}
            </Grid>
          </List>
        )}
      </div>
    </Pane>
  );
}
