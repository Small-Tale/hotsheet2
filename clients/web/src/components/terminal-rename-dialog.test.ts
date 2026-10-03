import { describe, expect, it } from 'vitest';

import { TerminalRenameDialog } from './terminal-rename-dialog';

describe('TerminalRenameDialog', () => {
  it('renders the active name and colocated dialog actions', () => {
    const markup = String(
      TerminalRenameDialog({ target: { projectId: 'p', terminalId: 't', value: 'API', session: 3 } }),
    );
    expect(markup).toContain('value="API"');
    expect(markup).toContain('data-key="terminal-rename-3"');
    expect(markup).toContain('data-action="rename-terminal-form"');
    expect(markup).toContain('data-action="cancel-terminal-rename"');
    expect(markup).toContain('class="kui-list"');
    expect(markup).toContain('class="kui-row"');
    expect(markup).toContain('data-v-align="middle"');
  });
  it('keys the name field by open session so each open reseeds it with the current name (HS2-MEW525)', () => {
    const first = String(
        TerminalRenameDialog({ target: { projectId: 'p', terminalId: 'a', value: 'Alpha', session: 1 } }),
      ),
      second = String(TerminalRenameDialog({ target: { projectId: 'p', terminalId: 'b', value: 'Beta', session: 2 } }));
    expect(first).toContain('data-key="terminal-rename-1"');
    expect(second).toContain('data-key="terminal-rename-2"');
    expect(second).toContain('value="Beta"');
    expect(second).not.toContain('Alpha');
    expect(String(TerminalRenameDialog({}))).toContain('open="false"');
  });
  it('offers Reset to default, naming the default, only while a rename applies (HS2-2Q7KTX)', () => {
    const unnamed = String(
        TerminalRenameDialog({ target: { projectId: 'p', terminalId: 'a', value: 'Claude 1', session: 1 } }),
      ),
      renamed = String(
        TerminalRenameDialog({
          target: { projectId: 'p', terminalId: 'a', value: 'Review', session: 2, defaultName: 'Claude 1' },
        }),
      );
    expect(unnamed).not.toContain('data-action="reset-terminal-rename"');
    expect(unnamed).not.toContain('hint=');
    expect(renamed).toContain('data-action="reset-terminal-rename"');
    expect(renamed).toContain('Reset to default');
    expect(renamed).toContain('hint="Default name: Claude 1"');
    expect(renamed).toContain('value="Review"');
    // Reset leads; Cancel and Rename stay together on the trailing side.
    expect(renamed.indexOf('reset-terminal-rename')).toBeLessThan(renamed.indexOf('kui-spacer'));
    expect(renamed.indexOf('kui-spacer')).toBeLessThan(renamed.indexOf('cancel-terminal-rename'));
  });
});
