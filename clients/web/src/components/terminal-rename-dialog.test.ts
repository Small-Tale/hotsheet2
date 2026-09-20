import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { TerminalRenameDialog } from './terminal-rename-dialog';

describe('TerminalRenameDialog', () => {
  it('renders the active name and colocated dialog actions', () => {
    const markup = String(TerminalRenameDialog({ target: { projectId: 'p', terminalId: 't', value: 'API' } }));
    const css = readFileSync(resolve(import.meta.dirname, 'terminal-rename-dialog.css'), 'utf8');
    expect(markup).toContain('value="API"');
    expect(markup).toContain('data-action="rename-terminal-form"');
    expect(markup).toContain('data-action="cancel-terminal-rename"');
    expect(css).toContain('.terminal-rename footer');
  });
});
