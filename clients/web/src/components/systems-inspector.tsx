import { Badge } from '@kerfjs/ui/badge';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Text } from '@kerfjs/ui/text';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';
import { Bot } from 'lucide';

import { machineKindLabel, type SystemsMachine, type SystemsSessionRow } from '../systems-model';
import { inspectorToggle, type SidebarPanelParts } from './sidebar-panel';

export interface SystemsInspectorProps {
  machine: SystemsMachine;
  /** The machine's running AI sessions; absent while unknown. */
  sessions?: readonly SystemsSessionRow[];
  /** The capacity read failed and there is no earlier reading. */
  error?: string;
}

const quiet = (text: string) => (
  <List textInsets="rl">
    <Text tone="quiet" size="compact" flush>
      {text}
    </Text>
  </List>
);

/**
 * The Systems screen's right rail (HS2-DR0ZSQ, approved Concept B): the selected machine's AI
 * sessions, each named by its terminal when this app shows it, with whether it is working.
 */
export function SystemsInspector({ machine, sessions, error }: SystemsInspectorProps) {
  return (
    <div data-component="systems-inspector" data-machine-id={machine.id}>
      <List gap="l">
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
                  label={
                    <List gap="2xs">
                      <Text variant="span" flush>
                        {session.title}
                      </Text>
                      <Text variant="span" size="compact" tone="quiet" flush>
                        {session.description}
                      </Text>
                    </List>
                  }
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
