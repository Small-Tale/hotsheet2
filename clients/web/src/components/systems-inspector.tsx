import { Badge } from '@kerfjs/ui/badge';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Text } from '@kerfjs/ui/text';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';
import { Bot } from 'lucide';

import {
  machineKindLabel,
  type SystemsAdmissionRow,
  type SystemsMachine,
  type SystemsSessionRow,
  type SystemsValueRow,
} from '../systems-model';
import { inspectorToggle, type SidebarPanelParts } from './sidebar-panel';

export interface SystemsInspectorProps {
  machine: SystemsMachine;
  /** The machine's running AI sessions; absent while unknown. */
  sessions?: readonly SystemsSessionRow[];
  /** The capacity read failed and there is no earlier reading. */
  error?: string;
  /** HS2-5H8R0N: each orchestrated project placed on this machine, with its admission answer. */
  admission?: readonly SystemsAdmissionRow[];
  /** HS2-5H8R0N: whether any open project orchestrates; without one, Admission by Project says so. */
  orchestrating?: boolean;
  /** HS2-5H8R0N: the machine owner's placement limits; absent while unread or unreported. */
  limits?: readonly SystemsValueRow[];
  /** Whether the machine's capacity snapshot has been read, so absent limits mean unreported. */
  capacityRead?: boolean;
}

const quiet = (text: string) => (
  <List textInsets="rl">
    <Text tone="quiet" size="compact" flush>
      {text}
    </Text>
  </List>
);

const twoLine = (title: string, description: string | undefined) => (
  <List gap="2xs">
    <Text variant="span" flush>
      {title}
    </Text>
    {description ? (
      <Text variant="span" size="compact" tone="quiet" flush>
        {description}
      </Text>
    ) : null}
  </List>
);

/**
 * The Systems screen's right rail (HS2-DR0ZSQ, approved Concept B): Admission by Project (each
 * orchestrated project placed here and whether its sessions may start now, HS2-5H8R0N), the
 * machine's AI sessions, each named by its terminal when this app shows it, and the machine
 * owner's Placement Limits.
 */
export function SystemsInspector({
  machine,
  sessions,
  error,
  admission = [],
  orchestrating = false,
  limits,
  capacityRead = false,
}: SystemsInspectorProps) {
  return (
    <div data-component="systems-inspector" data-machine-id={machine.id}>
      <List gap="l">
        <section aria-label="Admission by Project">
          <ListHeader label="Admission by Project" />
          {machine.unavailable ? (
            quiet('Admission shows while the machine is connected.')
          ) : admission.length === 0 ? (
            quiet(
              orchestrating
                ? 'No orchestrated project runs on this machine.'
                : 'Open an orchestration project to see what it can start here.',
            )
          ) : (
            <ValueTable label={`Admission on ${machine.name}`} density="compact" stackBelow="compact">
              {admission.map((row) => (
                <ValueTableRow
                  label={twoLine(row.label, row.description)}
                  value={
                    <Badge tone={row.status.tone} appearance="quiet" size="compact">
                      {row.status.label}
                    </Badge>
                  }
                  rootAttributes={{ 'data-systems-admission': row.key.replace('\u0000', ':') }}
                />
              ))}
            </ValueTable>
          )}
        </section>
        <section aria-label="AI Sessions Here">
          <ListHeader label="AI Sessions Here" />
          {machine.unavailable ? (
            quiet('Sessions show while the machine is connected.')
          ) : sessions === undefined ? (
            quiet(error ? `Sessions are not available. ${error}` : 'Loading…')
          ) : sessions.length === 0 ? (
            quiet('No AI session is running here.')
          ) : (
            <ValueTable label={`AI sessions on ${machine.name}`} density="compact" stackBelow="compact">
              {sessions.map((session) => (
                <ValueTableRow
                  icon={<LucideIcon icon={Bot} name="bot" />}
                  label={twoLine(session.title, session.description)}
                  value={
                    <Badge tone={session.busy ? 'success' : 'neutral'} appearance="quiet" size="compact">
                      {session.busy ? 'Working' : 'Idle'}
                    </Badge>
                  }
                  rootAttributes={{ 'data-systems-session': session.id }}
                />
              ))}
            </ValueTable>
          )}
        </section>
        <section aria-label="Placement Limits">
          <ListHeader label="Placement Limits" />
          {machine.unavailable ? (
            quiet('Limits show while the machine is connected.')
          ) : limits ? (
            <ValueTable label={`Placement limits on ${machine.name}`} density="compact" stackBelow="compact">
              {limits.map((row) => (
                <ValueTableRow
                  label={row.label}
                  value={row.value}
                  rootAttributes={{ 'data-systems-limit': row.label }}
                />
              ))}
            </ValueTable>
          ) : (
            quiet(
              error
                ? `Limits are not available. ${error}`
                : capacityRead
                  ? 'This machine does not report its limits.'
                  : 'Loading…',
            )
          )}
        </section>
      </List>
    </div>
  );
}

/** The right rail's panel parts: the machine's name and kind as its title. */
export function systemsInspectorPanel(props: SystemsInspectorProps): SidebarPanelParts {
  return {
    label: `${props.machine.name} Details`,
    toolbar: {
      label: 'Machine details toolbar',
      dividerSides: '',
      title: <ToolbarText text={props.machine.name} description={machineKindLabel(props.machine)} />,
    },
    toggle: inspectorToggle('Machine Details'),
    content: <SystemsInspector {...props} />,
    pane: {},
  };
}
