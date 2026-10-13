import { existsSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { initialTerminalVisibilityState } from '../terminal-visibility';
import { TerminalVisibilityDialog, TerminalVisibilityNameDialog } from './terminal-visibility-dialog';

const groups = [
  {
    projectId: 'project',
    projectName: 'Project',
    sessions: [
      {
        id: 'one',
        projectId: 'project',
        projectName: 'Project',
        title: 'AI',
        alive: true,
        busy: false,
        scrollback: '',
      },
    ],
  },
];
describe('TerminalVisibilityDialog', () => {
  it('renders a Kerf multiple Select with bulk actions and no matching empty project headers', () => {
    const markup = String(
      TerminalVisibilityDialog({
        open: true,
        state: initialTerminalVisibilityState(),
        scope: 'dashboard',
        groups,
        types: [],
      }),
    );
    expect(markup).toContain('data-component="select"');
    expect(markup).toContain('name="terminal-visibility-types"');
    expect(markup).toContain('placeholder="No types selected"');
    expect(markup).not.toContain('terminal-visibility-dialog__type-icon');
    expect(markup).toContain('multiple');
    expect(markup).toMatch(/<wa-option value="shell"(?! selected)/);
    expect(markup).toContain('data-select-action="all"');
    expect(markup).toContain('data-select-action="clear"');
    expect(markup).toContain('value="browser" disabled title="Web browser tiles are not available yet."');
    expect(markup).toContain('class="kui-select__icon"');
    expect(markup).toContain('No workspace items match the selected types.');
    expect(markup).not.toContain('data-component="list-header"');
    expect(markup).not.toContain('data-action="toggle-terminal-visibility"');
  });
  it('composes Kerf TabBar, List and DialogSurface with no stylesheet (HS2-W68SWJ, HS2-N940N4)', () => {
    // Kerf's context PopupMenu renders the group menu's anchor (HS2-H4JZDQ), so the dialog owns no CSS at all.
    expect(existsSync(resolve(import.meta.dirname, 'terminal-visibility-dialog.css'))).toBe(false);
    const markup = String(
      TerminalVisibilityDialog({ open: true, state: initialTerminalVisibilityState(), scope: 'dashboard', groups }),
    );
    expect(markup).toContain('terminal-visibility-dialog__toolbar');
    expect(markup).toContain('data-component="tab-bar"');
    // The Add icon is sized by its LucideIcon prop inside a Kerf ToolbarControlGroup (HS2-YNW0B3).
    expect(markup).toMatch(/aria-label="Add visibility group"[^]*?<svg[^>]*data-lucide="plus"[^>]*data-size="16"/);
  });
  it('uses selected app-tab styling, shared menu headers, and shared menu rows', () => {
    const markup = String(
      TerminalVisibilityDialog({ open: true, state: initialTerminalVisibilityState(), scope: 'dashboard', groups }),
    );
    expect(markup).toContain('role="tablist"');
    expect(markup).toContain('class="kui-app-tab terminal-visibility-dialog__tab"');
    expect(markup).toContain('data-component="app-tab"');
    expect(markup).toContain('data-selected="true"');
    expect(markup).toContain('data-component="list-header"');
    expect(markup).toContain('data-action="toggle-terminal-visibility"');
    expect(markup).toContain('aria-label="Hide AI"');
    expect(markup).toContain('>Visible</span>');
    expect(markup.indexOf('hide-all-terminals-in-group')).toBeLessThan(markup.indexOf('show-all-terminals-in-group'));
  });
  it('limits a project scope to that project and gives only named groups a context menu', () => {
    const state = {
        groups: [...initialTerminalVisibilityState().groups, { id: 'focus', name: 'Focus', hiddenKeys: [] }],
        activeByScope: { 'project:other': 'focus' },
      },
      markup = String(
        TerminalVisibilityDialog({
          open: true,
          state,
          scope: 'project:other',
          groups: [
            ...groups,
            {
              ...groups[0],
              projectId: 'other',
              projectName: 'Other',
              sessions: [{ ...groups[0].sessions[0], projectId: 'other', projectName: 'Other' }],
            },
          ],
          contextMenu: { id: 'focus', x: 20, y: 30 },
        }),
      );
    expect(markup).not.toContain('>Project</h2>');
    expect(markup).toContain('>Other</h2>');
    expect(markup).toContain('aria-label="Visibility group actions"');
    expect(markup).toMatch(/popup-menu-context-x:20px;[^"]*popup-menu-context-y:30px/);
    expect(markup).toContain('data-context-menu="terminal-visibility-group"');
    expect(markup).toContain('data-component="popup-menu"');
    expect(markup).toContain('rename-terminal-visibility-group');
    expect(markup).toContain('remove-terminal-visibility-group');
    expect(
      String(
        TerminalVisibilityDialog({
          open: true,
          state,
          scope: 'project:other',
          groups,
          contextMenu: { id: 'default', x: 0, y: 0 },
        }),
      ),
    ).not.toContain('Visibility group actions');
  });
  it('prompts for a name before add and rename mutations', () => {
    const add = String(TerminalVisibilityNameDialog({ prompt: { mode: 'add', value: '' } })),
      rename = String(TerminalVisibilityNameDialog({ prompt: { mode: 'rename', groupId: 'focus', value: 'Focus' } }));
    expect(add).toContain('label="Add Visibility Group"');
    expect(add).toContain('data-action="submit-terminal-visibility-name"');
    expect(add).toContain('>Add</wa-button>');
    expect(rename).toContain('label="Rename Visibility Group"');
    expect(rename).toContain('value="Focus"');
    expect(rename).toContain('>Rename</wa-button>');
  });
});
