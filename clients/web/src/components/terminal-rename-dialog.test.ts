import { describe, expect, it } from 'vitest';

import { TerminalRenameDialog } from './terminal-rename-dialog';

describe('TerminalRenameDialog', () => {
  it('renders the active name and colocated dialog actions', () => {
    const markup = String(TerminalRenameDialog({ target: { projectId: 'p', terminalId: 't', value: 'API' } }));
    expect(markup).toContain('value="API"');
    expect(markup).toContain('data-action="rename-terminal-form"');
    expect(markup).toContain('data-action="cancel-terminal-rename"');
    expect(markup).toContain('class="kui-list"');
    expect(markup).toContain('class="kui-row"');
    expect(markup).toContain('data-h-align="right"');
    expect(markup).toContain('data-v-align="middle"');
  });
});
