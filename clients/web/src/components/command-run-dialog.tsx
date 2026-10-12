import '@awesome.me/webawesome/dist/components/button/button.js';
import './command-run-dialog.css';

import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Text } from '@kerfjs/ui/text';
import { Square, X } from 'lucide';

import type { CommandDefinition, CommandRun } from '../api';
import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';

export function CommandRunDialog({
  command,
  run,
  confirmStop = false,
}: {
  command?: CommandDefinition;
  run?: CommandRun;
  confirmStop?: boolean;
}) {
  if (!command) return null;
  if (confirmStop)
    return (
      <dialog
        class="command-run-dialog"
        aria-label={`Stop ${command.title}?`}
        data-component="command-cancellation-dialog"
      >
        <List gap="m">
          <Text variant="h2" size="large" flush>
            Stop {command.title}?
          </Text>
          <Text flush>The command is still running. Stop it now?</Text>
          <Row hAlign="right" vAlign="middle" gap="xs" wrap>
            <wa-button appearance="outlined" {...COMMANDS_AND_AI_ACTIONS.dismissCommandDialog.attrs}>
              Keep running
            </wa-button>
            <wa-button variant="danger" {...COMMANDS_AND_AI_ACTIONS.confirmStopCommand.attrs} data-run-id={run?.id}>
              <LucideIcon slot="start" icon={Square} name="square" size={13.6} />
              Stop command
            </wa-button>
          </Row>
        </List>
      </dialog>
    );
  return (
    <dialog class="command-run-dialog" aria-label={command.title} data-component="command-run-dialog">
      <List gap="m">
        <Text variant="h2" size="large" flush>
          {command.title}
        </Text>
        <Row hAlign="full" vAlign="middle" gap="xs">
          <strong>{run ? run.state : 'Never run'}</strong>
          {run?.exit_code !== undefined && <span>Exit {run.exit_code}</span>}
        </Row>
        <pre class="command-run-dialog__output" aria-label="Command output">
          {run?.output.length
            ? run.output.map((line) => `${line.stream === 'stderr' ? 'error: ' : ''}${line.text}`).join('\n')
            : 'No output recorded.'}
        </pre>
        <Row hAlign="right" vAlign="middle" gap="xs">
          <wa-button appearance="outlined" {...COMMANDS_AND_AI_ACTIONS.dismissCommandDialog.attrs}>
            <LucideIcon slot="start" icon={X} name="x" size={13.6} />
            Close
          </wa-button>
        </Row>
      </List>
    </dialog>
  );
}
