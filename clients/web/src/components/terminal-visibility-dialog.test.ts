import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import {
  initialTerminalVisibilityState,
  setShowMinimizedInGroup,
  setTerminalTypeVisibleInGroup,
  setTerminalVisibleInGroup,
} from '../terminal-visibility';
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
  it('renders three pressed type buttons and no matching empty project headers', () => {
    const state = (['shell', 'ai', 'chat'] as const).reduce(
      (next, type) => setTerminalTypeVisibleInGroup(next, 'default', type, false),
      initialTerminalVisibilityState(),
    );
    const markup = String(
      TerminalVisibilityDialog({
        open: true,
        state,
        scope: 'dashboard',
        groups,
        chatFeatureOn: true,
      }),
    );
    expect(markup).toContain('role="group" aria-label="Item Types"');
    expect(markup).toContain('data-visibility-type="shell"');
    expect(markup).toContain('data-visibility-type="ai"');
    expect(markup).toContain('data-visibility-type="chat"');
    expect(markup).toContain('aria-pressed="false"');
    expect(markup).not.toContain('Web Browsers');
    expect(markup).toContain('No workspace items match the selected types.');
    expect(markup).not.toContain('data-component="list-header"');
    expect(markup).not.toContain('data-action="toggle-terminal-visibility"');
  });
  it('keeps type controls above the divider and rows in the filling scroller (HS2-AEM9P5, HS2-220DT0)', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'terminal-visibility-dialog.css'), 'utf8');
    expect(css).toContain('grid-template-columns: repeat(3, minmax(0, 1fr))');
    expect(css).not.toMatch(/\.kui-/);
    const markup = String(
      TerminalVisibilityDialog({
        open: true,
        state: initialTerminalVisibilityState(),
        scope: 'dashboard',
        groups,
        chatFeatureOn: true,
      }),
    );
    expect(markup).toContain('terminal-visibility-dialog__type-grid');
    expect(markup).toMatch(/data-body-layout="fill-last"/);
    expect(markup).toMatch(/data-visibility-type="chat"[^]*?data-scrollable="true"/);
    expect(markup).toContain('terminal-visibility-dialog__toolbar');
    expect(markup).toContain('data-component="tab-bar"');
    // The Add icon is sized by its LucideIcon prop inside a Kerf ToolbarControlGroup (HS2-YNW0B3).
    expect(markup).toMatch(/aria-label="Add Visibility Group"[^]*?<svg[^>]*data-lucide="plus"[^>]*data-size="16"/);
  });
  it('uses selected tab-bar-item styling, shared menu headers, and shared menu rows', () => {
    const markup = String(
      TerminalVisibilityDialog({
        open: true,
        state: initialTerminalVisibilityState(),
        scope: 'dashboard',
        groups,
        chatFeatureOn: true,
      }),
    );
    expect(markup).toContain('role="tablist"');
    expect(markup).toContain('class="kui-tab-bar-item terminal-visibility-dialog__tab"');
    expect(markup).toContain('data-component="tab-bar-item"');
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
          chatFeatureOn: true,
          contextMenu: { id: 'focus', x: 20, y: 30 },
        }),
      );
    expect(markup).not.toContain('>Project</h2>');
    expect(markup).toContain('>Other</h2>');
    expect(markup).toContain('aria-label="Visibility Group Actions"');
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
          chatFeatureOn: true,
          contextMenu: { id: 'default', x: 0, y: 0 },
        }),
      ),
    ).not.toContain('Visibility Group Actions');
  });
  it('prompts for a name before add and rename mutations', () => {
    const add = String(TerminalVisibilityNameDialog({ prompt: { mode: 'add', value: '' } })),
      rename = String(TerminalVisibilityNameDialog({ prompt: { mode: 'rename', groupId: 'focus', value: 'Focus' } }));
    expect(add).toContain('label="Add Visibility Group"');
    expect(add).toContain('data-action="submit-terminal-visibility-name"');
    expect(add).toContain('>Add</span></button>');
    expect(rename).toContain('label="Rename Visibility Group"');
    expect(rename).toContain('value="Focus"');
    expect(rename).toContain('>Rename</span></button>');
  });
  it('shows AI Chats only while the feature is on or a chat is listed, never pressed while hidden (HS2-4TKASN)', () => {
    const off = String(
      TerminalVisibilityDialog({
        open: true,
        state: initialTerminalVisibilityState(),
        scope: 'dashboard',
        groups,
        chatFeatureOn: false,
      }),
    );
    expect(off).not.toContain('data-visibility-type="chat"');
    expect(off).not.toContain('AI Chats');
    expect(off).toContain('data-type-count="2"');
    // A chat-only choice falls back to Terminals while AI Chats is hidden.
    const chatOnly = (['shell', 'ai'] as const).reduce(
      (next, type) => setTerminalTypeVisibleInGroup(next, 'default', type, false),
      initialTerminalVisibilityState(),
    );
    const fallback = String(
      TerminalVisibilityDialog({ open: true, state: chatOnly, scope: 'dashboard', groups, chatFeatureOn: false }),
    );
    expect(fallback).toMatch(
      /aria-pressed="true"[^>]*data-visibility-type="shell"|data-visibility-type="shell"[^>]*aria-pressed="true"/,
    );
    const withChat = String(
      TerminalVisibilityDialog({
        open: true,
        state: initialTerminalVisibilityState(),
        scope: 'dashboard',
        groups: [
          {
            ...groups[0],
            chats: [{ id: 'ai-chat:c', projectId: 'project', projectName: 'Project', name: 'Chat', tool: 'Codex' }],
          },
        ],
        chatFeatureOn: false,
      }),
    );
    expect(withChat).toContain('data-visibility-type="chat"');
    expect(withChat).toContain('data-type-count="3"');
    expect(withChat).toContain('aria-label="Hide Chat"');
  });
  it('makes each project heading a named show-all or hide-all action (HS2-HC67SP)', () => {
    const twoAgents = [
      {
        ...groups[0],
        sessions: [
          { ...groups[0].sessions[0], kind: 'ai' as const },
          { ...groups[0].sessions[0], id: 'two', kind: 'ai' as const, title: 'Second' },
        ],
      },
    ];
    const render = (state = initialTerminalVisibilityState()) =>
      String(
        TerminalVisibilityDialog({ open: true, state, scope: 'dashboard', groups: twoAgents, chatFeatureOn: true }),
      );
    const visible = render();
    expect(visible).toContain('data-action="toggle-terminal-visibility-project"');
    expect(visible).toContain('data-project-id="project"');
    expect(visible).toContain('aria-label="Hide All Project AI Shells"');
    expect(visible).toMatch(/aria-label="Hide All Project AI Shells"[^>]*>[^]*?data-lucide="eye"/);
    const mixed = render(setTerminalVisibleInGroup(initialTerminalVisibilityState(), 'default', 'project:two', false));
    expect(mixed).toContain('aria-label="Show All Project AI Shells"');
    expect(mixed).toMatch(/aria-label="Show All Project AI Shells"[^>]*>[^]*?data-lucide="eye-off"/);
    // The heading still names the project for the outline.
    expect(mixed).toContain('>Project</');
    // The footer bulk actions stay.
    expect(mixed).toContain('hide-all-terminals-in-group');
    expect(mixed).toContain('show-all-terminals-in-group');
  });
  it('offers a per-workspace Show minimized checkbox, off by default (HS2-W1XEH7)', () => {
    const off = String(
      TerminalVisibilityDialog({
        open: true,
        state: initialTerminalVisibilityState(),
        scope: 'dashboard',
        groups,
        chatFeatureOn: true,
      }),
    );
    expect(off).toContain('data-component="checkbox"');
    expect(off).toContain('Show minimized');
    expect(off).toContain('data-action="toggle-terminal-visibility-show-minimized"');
    expect(off).not.toMatch(/data-action="toggle-terminal-visibility-show-minimized"[^>]*checked/);
    const on = String(
      TerminalVisibilityDialog({
        open: true,
        state: setShowMinimizedInGroup(initialTerminalVisibilityState(), 'default', true),
        scope: 'dashboard',
        groups,
        chatFeatureOn: true,
      }),
    );
    expect(on).toMatch(
      /<input[^>]*checked[^>]*data-action="toggle-terminal-visibility-show-minimized"|<input[^>]*data-action="toggle-terminal-visibility-show-minimized"[^>]*checked/,
    );
    // Show minimized sits in the footer beside the bulk actions.
    expect(on.indexOf('Show minimized')).toBeLessThan(on.indexOf('hide-all-terminals-in-group'));
  });
});
