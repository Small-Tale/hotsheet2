import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dialog/dialog.js';
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
        <DialogSurface size="small" bodyInset="comfortable" footerInset="comfortable">
          <wa-dialog
            data-key="command-cancellation-dialog"
            open
            label={`Stop ${command.title}?`}
            data-component="command-cancellation-dialog"
          >
            <Text flush>The command is still running. Stop it now?</Text>
            <Row slot="footer" hAlign="right" vAlign="middle" gap="xs" wrap>
              <wa-button appearance="outlined" {...COMMANDS_AND_AI_ACTIONS.dismissCommandDialog.attrs}>
                Keep running
              </wa-button>
              <wa-button variant="danger" {...COMMANDS_AND_AI_ACTIONS.confirmStopCommand.attrs} data-run-id={run?.id}>
                <LucideIcon slot="start" icon={Square} name="square" size={13.6} />
                Stop command
              </wa-button>
            </Row>
          </wa-dialog>
        </DialogSurface>
      </div>
    );
  return (
    <div class="command-run-dialog" data-key="command-run-dialog">
      <DialogSurface size="small" bodyInset="comfortable" footerInset="comfortable">
        <wa-dialog
          data-key="command-run-dialog"
          open
          label={command.title}
          data-component="command-run-dialog"
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
        </wa-dialog>
      </DialogSurface>
    </div>
  );
}
