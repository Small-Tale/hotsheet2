import '@awesome.me/webawesome/dist/components/button/button.js';
import './command-run-dialog.css';

import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';
import { Square } from 'lucide';

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
  // Both presentations are Web Awesome dialogs inside Kerf DialogSurface; each keeps its own key so a
  // morph between them never hands one dialog's host to the other (HS2-Z7C27E, HS2-EKJRMJ). The
  // output dialog is dismissed through Web Awesome's own header close (or Escape), which fires the
  // wa-hide that clears the app's dialog state, so it carries no duplicate footer Close.
  if (confirmStop)
    return (
      <div class="command-run-dialog" data-key="command-cancellation-dialog">
        <DialogSurface
          label={`Stop ${command.title}?`}
          size="small"
          rootAttributes={{
            'data-key': 'command-cancellation-dialog',
            'data-dialog-component': 'command-cancellation-dialog',
            'data-controlled-open': 'true',
          }}
          footer={
            <Row hAlign="right" vAlign="middle" gap="xs" wrap>
              <wa-button appearance="outlined" {...COMMANDS_AND_AI_ACTIONS.dismissCommandDialog.attrs}>
                Keep Running
              </wa-button>
              <wa-button variant="danger" {...COMMANDS_AND_AI_ACTIONS.confirmStopCommand.attrs} data-run-id={run?.id}>
                <LucideIcon slot="start" icon={Square} name="square" size={13.6} />
                Stop Command
              </wa-button>
            </Row>
          }
        >
          <Text flush>The command is still running. Stop it now?</Text>
        </DialogSurface>
      </div>
    );
  return (
    <div class="command-run-dialog" data-key="command-run-dialog">
      <DialogSurface
        label={command.title}
        size="small"
        rootAttributes={{
          'data-key': 'command-run-dialog',
          'data-dialog-component': 'command-run-dialog',
          'data-controlled-open': 'true',
        }}
      >
        <List gap="m">
          <Row hAlign="full" vAlign="middle" gap="xs">
            <strong>{run ? run.state : 'Never run'}</strong>
            {run?.exit_code !== undefined && <span>Exit {run.exit_code}</span>}
          </Row>
          <pre class="command-run-dialog__output" aria-label="Command output">
            {run?.output.length
              ? run.output.map((line) => `${line.stream === 'stderr' ? 'error: ' : ''}${line.text}`).join('\n')
              : 'No output recorded.'}
          </pre>
        </List>
      </DialogSurface>
    </div>
  );
}
