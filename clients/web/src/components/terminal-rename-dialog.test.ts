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
    expect(markup).toContain('data-h-align="right"');
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
});
