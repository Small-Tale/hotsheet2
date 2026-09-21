import './terminal-rename-dialog.css';

export interface TerminalRenameTarget {
  projectId: string;
  terminalId: string;
  value: string;
}

export function TerminalRenameDialog({ target }: { target?: TerminalRenameTarget }) {
  return (
    <wa-dialog data-terminal-rename-dialog label="Rename terminal" open={Boolean(target)}>
      <form class="terminal-rename" data-action="rename-terminal-form">
        <wa-input name="terminal-name" label="Terminal name" value={target?.value ?? ''} required autofocus></wa-input>
        <footer>
          <wa-button appearance="plain" type="button" data-action="cancel-terminal-rename">
            Cancel
          </wa-button>
          <wa-button appearance="accent" type="submit">
            Rename
          </wa-button>
        </footer>
      </form>
    </wa-dialog>
  );
}
