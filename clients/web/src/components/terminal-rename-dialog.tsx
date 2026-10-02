import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';

import { TERMINALS_ACTIONS } from '../interaction-attrs/terminals';

export interface TerminalRenameTarget {
  projectId: string;
  terminalId: string;
  /** The terminal's current tab name, which seeds the field. */
  value: string;
  /**
   * Increments on every open (HS2-MEW525). A Web Awesome input keeps its typed live `value` when
   * only its `value` attribute changes, so a new session replaces the field and it always shows
   * the current name of the terminal being renamed, never a previous rename's text.
   */
  session: number;
}

export function TerminalRenameDialog({ target }: { target?: TerminalRenameTarget }) {
  return (
    <wa-dialog
      data-terminal-rename-dialog
      label="Rename terminal"
      open={Boolean(target)}
      data-controlled-open={String(Boolean(target))}
    >
      <form class="terminal-rename" {...TERMINALS_ACTIONS.renameTerminalForm.attrs}>
        <List gap="l">
          <wa-input
            data-key={`terminal-rename-${target?.session ?? 0}`}
            name="terminal-name"
            label="Terminal name"
            value={target?.value ?? ''}
            required
            autofocus
          ></wa-input>
          <Row hAlign="right" vAlign="middle" gap="xs">
            <wa-button appearance="plain" type="button" {...TERMINALS_ACTIONS.cancelTerminalRename.attrs}>
              Cancel
            </wa-button>
            <wa-button appearance="accent" type="submit">
              Rename
            </wa-button>
          </Row>
        </List>
      </form>
    </wa-dialog>
  );
}
