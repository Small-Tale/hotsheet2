import { Badge } from '@kerfjs/ui/badge';
import { px } from '@kerfjs/ui/css-values';
import { Gauge } from '@kerfjs/ui/gauge';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Text } from '@kerfjs/ui/text';
import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';
import { Cloud, Monitor } from 'lucide';

import { NAVIGATION_AND_TABS_ACTIONS } from '../interaction-attrs/navigation-and-tabs';
import type { SystemsMachine, SystemsToolCapacityRow } from '../systems-model';
import type { SidebarPanelParts } from './sidebar-panel';

/** One Machines row: the machine, its quiet line, and its CPU reading for the gauge. */
export interface SystemsSidebarMachine {
  machine: SystemsMachine;
  description: string;
  /** CPU busy percent; absent while unknown, when the row shows no gauge. */
  cpu?: number;
}

/** AI Tool Capacity across the open orchestration projects (HS2-AHG7HW). */
export type SystemsToolCapacityState =
  | { status: 'loading' }
  | { status: 'none' }
  | { status: 'ready'; rows: readonly SystemsToolCapacityRow[] }
  | { status: 'error'; message: string };

export interface SystemsSidebarProps {
  machines: readonly SystemsSidebarMachine[];
  selectedId: string;
  toolCapacity: SystemsToolCapacityState;
}

/** CPU above these reads as warning and danger on the bare gauge. */
const CPU_WARNING = 80,
  CPU_DANGER = 95;

const cpuTone = (value: number) => (value >= CPU_DANGER ? 'danger' : value >= CPU_WARNING ? 'warning' : 'brand');

function machineStatus({ machine, cpu }: SystemsSidebarMachine) {
  if (machine.unavailable === 'revoked')
    return (
      <Badge tone="danger" appearance="quiet" size="compact">
        Revoked
      </Badge>
    );
  if (machine.unavailable === 'not-connected')
    return (
      <Badge tone="neutral" appearance="quiet" size="compact">
        Offline
      </Badge>
    );
  if (cpu === undefined) return undefined;
  return (
    <Gauge
      value={cpu}
      max={100}
      label={`${machine.name} CPU`}
      valueText={`${String(Math.round(cpu))}%`}
      unit="%"
      variant="bare"
      tone={cpuTone(cpu)}
      size={px(40)}
      rootAttributes={{ 'data-systems-cpu': machine.id }}
    />
  );
}

/** A read-only row's label: the name over a quiet detail line. */
const stacked = (label: string, detail: string) => (
  <List gap="2xs">
    <Text variant="span" flush>
      {label}
    </Text>
    {detail ? (
      <Text variant="span" size="compact" tone="quiet" flush>
        {detail}
      </Text>
    ) : null}
  </List>
);

function toolCapacitySection(state: SystemsToolCapacityState) {
  const quiet = (text: string) => (
    <List textInsets="rl">
      <Text tone="quiet" size="compact" flush>
        {text}
      </Text>
    </List>
  );
  return (
    <section aria-label="AI Tool Capacity" data-systems-tool-capacity={state.status}>
      <ListHeader label="AI Tool Capacity" />
      {state.status === 'loading' ? (
        quiet('Loading…')
      ) : state.status === 'none' ? (
        quiet('No open orchestration project sets AI tool capacity.')
      ) : state.status === 'error' ? (
        quiet(`AI tool capacity is not available. ${state.message}`)
      ) : state.rows.length === 0 ? (
        quiet('The orchestration policy configures no AI tools.')
      ) : (
        <ValueTable label="AI tool capacity" density="compact" stackBelow="compact">
          {state.rows.map((row) => (
            <ValueTableRow
              label={stacked(row.label, row.description)}
              value={
                <Badge tone={row.status.tone} appearance="quiet" size="compact">
                  {row.status.label}
                </Badge>
              }
              rootAttributes={{ 'data-systems-tool': row.key }}
            />
          ))}
        </ValueTable>
      )}
    </section>
  );
}

/**
 * The Systems screen's left rail (HS2-DR0ZSQ, approved Concept B): the Machines list, This Computer
 * then each paired remote with a bare CPU gauge (or why it has none), and AI Tool Capacity below.
 */
export function SystemsSidebar({ machines, selectedId, toolCapacity }: SystemsSidebarProps) {
  return (
    <div data-component="systems-sidebar">
      <List gap="l">
        <section aria-label="Machines">
          <ListHeader label="Machines" />
          <List gap="2xs">
            {machines.map((row) => (
              <ListItem
                label={row.machine.name}
                description={row.description}
                icon={
                  row.machine.kind === 'local' ? (
                    <LucideIcon icon={Monitor} name="monitor" />
                  ) : (
                    <LucideIcon icon={Cloud} name="cloud" />
                  )
                }
                status={machineStatus(row)}
                selected={row.machine.id === selectedId}
                action={NAVIGATION_AND_TABS_ACTIONS.selectSystemsMachine.value}
                itemId={row.machine.id}
                rootAttributes={{ 'data-machine-id': row.machine.id, 'data-machine-kind': row.machine.kind }}
              />
            ))}
          </List>
        </section>
        {toolCapacitySection(toolCapacity)}
      </List>
    </div>
  );
}

/** The left rail's panel parts for the Workbench. */
export function systemsSidebarPanel(props: SystemsSidebarProps): SidebarPanelParts {
  return {
    label: 'Systems sidebar',
    toolbar: { label: 'Systems sidebar toolbar', dividerSides: '' },
    toggle: { action: 'toggle-project-sidebar', name: 'Systems Sidebar' },
    content: <SystemsSidebar {...props} />,
    pane: {},
  };
}
