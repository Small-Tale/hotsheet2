import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { AIConversation, type AIConversationProps, AIConversationToolbar } from './ai-conversation';
import type { TerminalDashboardSession } from './terminal-dashboard';
import { aiShellMenuItem, TerminalDrawer } from './terminal-drawer';

const sessions = [
  {
    id: 'one',
    projectId: 'project',
    projectName: 'Project',
    title: 'Terminal 1',
    alive: true,
    busy: true,
    scrollback: 'one',
  },
  {
    id: 'two',
    projectId: 'project',
    projectName: 'Project',
    title: 'Terminal 2',
    alive: true,
    busy: false,
    scrollback: 'two',
  },
];
const render = (selectedId: string = 'grid') =>
  String(
    TerminalDrawer({
      projectId: 'project',
      projectName: 'Project',
      sessions,
      width: 900,
      height: 320,
      fitAcross: 2,
      fitHigh: 2,
      selectedId,
    }),
  );
describe('TerminalDrawer', () => {
  it('offers session messaging only on a selected terminal with server capability', () => {
    expect(render('one')).not.toContain('Message AI Session');
    const markup = String(
      TerminalDrawer({
        projectId: 'project',
        projectName: 'Project',
        sessions: [{ ...sessions[0], canMessage: true }],
        width: 900,
        height: 320,
        fitAcross: 2,
        fitHigh: 2,
        selectedId: 'one',
      }),
    );
    expect(markup).toContain('aria-label="Message AI Session"');
    expect(markup).toContain('data-action="message-terminal-session"');
    expect(markup).toContain('data-terminal-id="one"');
  });
  it('keeps the grid tab fixed while the shared tab strip overflows', () => {
    // Kerf's pinned TabBarItem owns the sticky treatment; the app adds no rule or class (HS2-WF3W6A).
    const css = readFileSync(resolve(import.meta.dirname, 'terminal-drawer.css'), 'utf8');
    expect(css).toContain("@import '@kerfjs/ui/tab-bar.css'");
    expect(css).not.toContain('terminal-drawer__grid-tab');
    expect(render()).toMatch(/data-tab-id="grid"[^>]*data-pinned="true"/);
    expect(render()).not.toContain('terminal-drawer__grid-tab');
  });
  it('keeps the PopupMenu trigger on the compact pill button surface through Kerf props', () => {
    // Kerf beta.59 sizes a compact PopupMenu trigger from the group's size prop; no app ::part rule (HS2-MYVVK3).
    const css = readFileSync(resolve(import.meta.dirname, 'terminal-drawer.css'), 'utf8');
    expect(css).not.toContain('wa-button::part(base)');
    expect(render()).toMatch(
      /kui-toolbar-control-group"[^>]*data-size="compact"[^>]*>\s*<wa-dropdown|data-size="compact"[^>]*data-nested-dropdown/,
    );
    expect(render()).toContain('appearance="plain"');
  });
  it('uses the Kerf spacing scale for the rail, content, and terminal inset', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'terminal-drawer.css'), 'utf8');
    expect(css).not.toContain('--wa-space-');
    // The drawer's TabBar pads by its public gap and strip tokens; the app wrapper adds the remaining
    // inline inset so the tabs keep the xs inset.
    expect(css).toMatch(
      /\.terminal-drawer__views \{[^}]*--kui-toolbar-gap: var\(--kui-space-xs\);[^}]*padding-inline: var\(--kui-space-2xs\)/,
    );
    // The drawer grid's tighter top inset is TerminalDashboard's own `drawer` layout (HS2-DR549A).
    expect(css).not.toContain('.terminal-dashboard__content');
    // The dedicated session's inset belongs to TerminalSession, which TerminalDashboard renders (HS2-YNW0B3).
    expect(css).not.toContain('.terminal-session');
    expect(css).not.toContain('.terminal-viewport');
  });
  it('reserves a complete gutter for focus rings and selected-tab shadows inside the shared scroller', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'terminal-drawer.css'), 'utf8'),
      rule = css.match(/\.terminal-drawer__views \{([^}]+)\}/)?.[1] ?? '';
    expect(css).toContain("@import '@kerfjs/ui/tab-bar.css'");
    expect(rule).toContain('--kui-tab-bar-strip-padding: var(--kui-space-2xs)');
    expect(rule).toContain('--kui-tab-bar-strip-margin-block: calc(var(--kui-space-2xs) * -1)');
    expect(css).not.toContain('.kui-tab-bar__tabs');
  });
  it('sizes terminal names from their content instead of reserving icon-width name space', () => {
    const markup = render();
    expect(markup).toContain('class="kui-tab-bar-item terminal-tab"');
    expect(markup).toMatch(/class="kui-tab-bar-item terminal-tab"[^>]*style="[^"]*144px"/);
    expect(markup).toContain('data-size="compact"');
  });
  it('renders a manual Kerf tab bar with adjacent creation and a far-edge hide action', () => {
    const markup = render();
    expect(markup).toContain('data-mode="grid"');
    expect(markup).toContain('data-maximized="false"');
    expect(markup).toContain('data-component="tab-bar"');
    expect(markup).toContain('data-tab-bar-id="terminal-drawer"');
    expect(markup).toContain('data-tab-activation="manual"');
    expect(markup).toContain('data-trailing-placement="adjacent"');
    // Kerf beta.99 keeps snap end room inside the strip, so whole-tab snapping is on (HS2-3P7TZV).
    expect(markup).toContain('data-snap-tabs="true"');
    expect(markup).toContain('>Project grid</span>');
    expect(markup).toContain('data-lucide="layout-grid"');
    expect(markup).not.toContain('data-lucide="grid-3x3"');
    expect(markup).toMatch(/data-kui-tab-list[\s\S]*Terminal 2[\s\S]*kui-tab-bar__trailing[\s\S]*New Drawer Item/);
    expect(markup).toMatch(/kui-tab-bar__end[\s\S]*terminal-drawer__actions[\s\S]*Hide Terminal Drawer/);
    expect(markup).toContain('data-component="tab-bar-item"');
    expect(markup).toContain('data-tab-kind="terminal"');
    expect(markup).toContain('draggable="true"');
    expect(markup).toContain('tabindex="0"');
    expect(markup).toContain('class="kui-tab-bar-item');
    expect(markup).toContain('data-action="close-terminal-tab"');
    expect(markup.indexOf('Terminal 1')).toBeLessThan(markup.indexOf('aria-label="Busy"'));
    for (const action of ['select-drawer-item', 'toggle-terminal-drawer', 'toggle-terminal-drawer-maximize'])
      expect(markup).toContain(`data-action="${action}"`);
  });
  it('composes the drawer actions with Kerf PopupMenu and an accessible trigger', () => {
    const content = AIConversation({
        open: true,
        presentation: 'embedded',
        tool: 'Codex',
        messages: [],
        draft: '',
        busy: false,
        interruptible: false,
      }),
      chat = String(
        TerminalDrawer({
          projectId: 'project',
          projectName: 'Project',
          sessions,
          chatTabs: [{ id: 'chat:one', name: 'Codex chat', tool: 'Codex', content }],
          width: 900,
          height: 320,
          fitAcross: 2,
          fitHigh: 2,
          selectedId: 'chat:one',
        }),
      );
    expect(chat).toContain('data-mode="ai-chat"');
    expect(chat).toContain('data-component="tab-bar-item"');
    expect(chat).toContain('data-action="close-ai-chat-tab"');
    expect(chat).toContain('data-presentation="embedded"');
    for (const item of ['Terminal', 'AI shell', 'AI Chat', 'Saved Conversation…']) expect(chat).toContain(item);
    expect(chat).toContain('data-action="open-saved-conversation"');
    expect(chat).toContain('data-component="popup-menu"');
    expect(chat).toContain('data-terminal-drawer-create="true"');
    expect(chat).toContain('placement="top-start"');
    expect(chat).toContain('New Drawer Item');
    expect(chat).toContain('data-action="create-terminal-drawer-item"');
    expect(chat).not.toContain('terminal-drawer__create-menu');
    expect(chat).not.toContain('data-component="list-header"');
    expect(chat).not.toContain('data-lucide="chevron-right"');
    expect(chat.match(/class="kui-popup-menu__item"/g)).toHaveLength(4);
  });
  it('shows the selected chat toolbar controls in the tab toolbar, not a title bar (HS2-DMH8MX, HS2-59F7WK)', () => {
    const conversation: AIConversationProps = {
        open: true,
        presentation: 'embedded',
        tool: 'Claude',
        messages: [{ id: 'answer', role: 'assistant', content: 'Done.', status: 'completed' }],
        draft: '',
        busy: true,
        interruptible: true,
        model: 'opus',
        effort: 'medium',
        models: [{ id: 'opus', label: 'Opus' }],
        efforts: ['low', 'medium'],
        canChangeModel: true,
        canChangeEffort: true,
      },
      drawer = (selectedId: string) =>
        String(
          TerminalDrawer({
            projectId: 'project',
            projectName: 'Project',
            sessions,
            chatTabs: [
              {
                id: 'chat:one',
                name: 'Claude chat',
                tool: 'Claude',
                content: AIConversation(conversation),
                toolbar: AIConversationToolbar(conversation),
              },
            ],
            width: 900,
            height: 320,
            fitAcross: 2,
            fitHigh: 2,
            selectedId,
          }),
        );
    const chat = drawer('chat:one'),
      end = chat.slice(chat.indexOf('kui-tab-bar__end'), chat.indexOf('class="terminal-drawer__content"')),
      content = chat.slice(chat.indexOf('class="terminal-drawer__content"'));
    // The model selector, Save, and Stop sit in the tab toolbar's end zone, before Hide Terminal Drawer.
    expect(end).toMatch(
      /data-component="conversation-model-control"[\s\S]*data-conversation-model-menu[\s\S]*data-action="save-conversation"[\s\S]*data-action="stop-conversation"[\s\S]*Hide Terminal Drawer/,
    );
    expect(end).toContain('placement="top-end"');
    // The chat body carries no title bar or settings row.
    expect(content).not.toContain('Claude conversation');
    expect(content).not.toContain('data-component="conversation-model-control"');
    expect(content).not.toContain('data-action="save-conversation"');
    // The only Toolbar in the body is the MessageComposer's own action row (Kerf beta.106).
    expect(content.match(/data-component="toolbar"/g)).toHaveLength(1);
    expect(content).toMatch(
      /data-component="message-composer"[^]*?<footer class="kui-toolbar" data-component="toolbar"/,
    );
    // A terminal or the grid shows no chat controls.
    expect(drawer('one')).not.toContain('data-component="conversation-model-control"');
    expect(drawer('grid')).not.toContain('data-action="save-conversation"');
  });
  it('renders the grid tile More actions menu with Open only, and only for its own tiles (HS2-V2CCN6)', () => {
    const withMenu = (key: string) =>
      String(
        TerminalDrawer({
          projectId: 'project',
          projectName: 'Project',
          sessions,
          width: 900,
          height: 320,
          fitAcross: 2,
          fitHigh: 2,
          selectedId: 'grid',
          contextMenu: { key, x: 20, y: 30 },
        }),
      );
    const own = withMenu('project:one');
    expect(own).toContain('data-action="open-terminal-context-menu"');
    expect(own).toContain('data-context-menu="terminal"');
    expect(own.match(/data-context-menu="terminal"/g)).toHaveLength(1);
    expect(own).toContain('data-action="open-terminal-project" data-item-id="project:one"');
    expect(own).not.toContain('data-action="hide-dashboard-terminal"');
    // A menu opened for a tile in another grid (e.g. the workspace dashboard) is not duplicated here.
    expect(withMenu('elsewhere:one')).not.toContain('data-context-menu="terminal"');
    // The dedicated (non-grid) view has no tiles, so it never renders the menu.
    expect(
      String(
        TerminalDrawer({
          projectId: 'project',
          projectName: 'Project',
          sessions,
          width: 900,
          height: 320,
          fitAcross: 2,
          fitHigh: 2,
          selectedId: 'one',
          contextMenu: { key: 'project:one', x: 20, y: 30 },
        }),
      ),
    ).not.toContain('data-context-menu="terminal"');
  });
  it('renders a remembered mixed-kind order in both tabs and the project grid', () => {
    const content = AIConversation({
        open: true,
        presentation: 'embedded',
        tool: 'Codex',
        messages: [],
        draft: '',
        busy: false,
        interruptible: false,
      }),
      markup = String(
        TerminalDrawer({
          projectId: 'project',
          projectName: 'Project',
          sessions,
          chatTabs: [
            { id: 'chat:one', name: 'Codex chat', tool: 'Codex', content, summary: 'Ready to discuss the project' },
          ],
          tabOrder: ['two', 'chat:one', 'one'],
          width: 900,
          height: 320,
          fitAcross: 2,
          fitHigh: 2,
          selectedId: 'grid',
        }),
      ),
      grid = markup.slice(markup.indexOf('data-component="terminal-grid"'));
    expect(markup.indexOf('Terminal 2')).toBeLessThan(markup.indexOf('Codex chat'));
    expect(markup.indexOf('Codex chat')).toBeLessThan(markup.indexOf('Terminal 1'));
    expect(
      markup.match(/aria-keyshortcuts="Delete Backspace Alt\+Shift\+ArrowLeft Alt\+Shift\+ArrowRight"/g),
    ).toHaveLength(3);
    expect(grid.indexOf('Terminal 2')).toBeLessThan(grid.indexOf('Codex chat'));
    expect(grid.indexOf('Codex chat')).toBeLessThan(grid.indexOf('Terminal 1'));
    expect(grid).toContain('Ready to discuss the project');
    expect(grid).toContain('data-action="open-grid-ai-chat"');
  });
  it('marks failed AI chat tabs with the same accessible halted indicator as terminals (HS2-AZVE3P)', () => {
    const content = AIConversation({
      open: true,
      presentation: 'embedded',
      tool: 'Codex',
      messages: [],
      draft: '',
      busy: false,
      interruptible: false,
    });
    const draw = (error?: string) =>
      String(
        TerminalDrawer({
          projectId: 'project',
          projectName: 'Project',
          sessions: [],
          chatTabs: [{ id: 'chat:failed', name: 'Codex chat', tool: 'Codex', content, error }],
          width: 900,
          height: 320,
          fitAcross: 2,
          fitHigh: 2,
          selectedId: 'chat:failed',
        }),
      );
    expect(draw('Model at capacity')).toContain('aria-label="Stopped: Model at capacity"');
    expect(draw('Model at capacity')).toContain('data-tab-kind="ai-chat"');
    expect(draw()).not.toContain('Stopped:');
  });

  it('reserves the shared trailing tab slot whether or not a terminal has state', () => {
    const markup = render();
    expect(markup.match(/kui-tab-bar-item__trailing/g)).toHaveLength(2);
    expect(markup).toMatch(
      /Terminal 1<\/span><span class="kui-tab-bar-item__trailing"><svg[^>]*class="[^"]*terminal-drawer__busy-dot[^"]*"/,
    );
    expect(markup).toMatch(/Terminal 2<\/span><span class="kui-tab-bar-item__trailing"><\/span>/);
  });
  it('describes the rail double-click state', () => {
    const markup = String(
      TerminalDrawer({
        projectId: 'project',
        projectName: 'Project',
        sessions,
        width: 900,
        height: 700,
        fitAcross: 2,
        fitHigh: 2,
        selectedId: 'grid',
        maximized: true,
      }),
    );
    expect(markup).toContain('data-maximized="true"');
    expect(markup).toContain('Double-click to restore terminal drawer');
  });
  it('renders only the selected dedicated session for keep-alive parking', () => {
    const markup = render('one');
    expect(markup).toContain('data-mode="dedicated"');
    expect(markup).toContain('data-component="terminal-session"');
    expect(markup.match(/data-terminal-id="one"/g)?.length).toBeGreaterThanOrEqual(2);
    expect(markup.match(/data-component="terminal-viewport"/g)).toHaveLength(1);
    expect(markup).not.toMatch(/class="terminal-session"[^>]*hidden/);
    expect(markup).not.toContain('data-component="terminal-tile"');
    expect(markup).not.toContain('Workspace tile zoom');
  });
  it('offers rail Copy and Paste for a selected terminal on a touch-first desktop only (HS2-5DHHPV)', () => {
    const drawer = (selectedId: string, touchClipboard: boolean, focusMode = false) =>
      String(
        TerminalDrawer({
          projectId: 'project',
          projectName: 'Project',
          sessions,
          width: 1180,
          height: 320,
          fitAcross: 2,
          fitHigh: 2,
          selectedId,
          touchClipboard,
          focusMode,
          magnifiedKey: selectedId === 'grid' ? 'project:one' : undefined,
        }),
      );
    const touch = drawer('one', true);
    // Native buttons in the existing end group, before Hide drawer, each with a Lucide icon and a name.
    expect(touch).toMatch(
      /data-action="copy-terminal-text" aria-label="Copy Terminal Text"[^>]*>.*data-lucide="copy".*data-action="paste-terminal-text" aria-label="Paste"[^>]*>.*data-lucide="clipboard-paste".*data-action="toggle-terminal-drawer"/s,
    );
    // A fine pointer, the grid, and phone focus mode (which has its own pill) never show the rail pair.
    expect(drawer('one', false)).not.toContain('copy-terminal-text');
    const grid = drawer('grid', true);
    expect(grid).not.toMatch(/<header class="terminal-drawer__rail"[^]*copy-terminal-text[^]*<\/header>/);
    expect(drawer('one', true, true)).not.toContain('data-action="toggle-terminal-drawer"');
    // A magnified drawer-grid tile on a touch-first desktop carries the toolbar pair.
    expect(grid.match(/class="terminal-tile__clipboard"/g)).toHaveLength(2);
    expect(drawer('grid', false)).not.toContain('terminal-tile__clipboard');
  });
  it('replaces every drawer chrome control with one exit action in mobile terminal focus mode (HS2-GMTQZM)', () => {
    const markup = String(
        TerminalDrawer({
          projectId: 'project',
          projectName: 'Project',
          sessions,
          width: 390,
          height: 492,
          fitAcross: 2,
          fitHigh: 2,
          selectedId: 'one',
          focusMode: true,
          focusViewport: { left: 4, top: 18, width: 382, height: 492 },
        }),
      ),
      css = readFileSync(resolve(import.meta.dirname, 'terminal-drawer.css'), 'utf8');
    expect(markup).toContain('data-focus-mode="true"');
    // Focus mode configures the dedicated session through its `focus` prop, not drawer CSS (HS2-YNW0B3).
    expect(markup).toMatch(/class="terminal-session"[^>]*data-focus="true"/);
    expect(markup).toContain(
      'style="--terminal-focus-left:4px;--terminal-focus-top:18px;--terminal-focus-width:382px;--terminal-focus-height:492px"',
    );
    expect(markup).toContain('data-action="exit-terminal-focus-mode"');
    expect(markup).toContain('aria-label="Exit terminal focus"');
    expect(markup).not.toContain('class="terminal-drawer__rail"');
    expect(markup).not.toContain('data-action="toggle-terminal-drawer"');
    expect(markup).not.toContain('data-component="tab-bar"');
    expect(css).toMatchSource(
      ".terminal-drawer[data-focus-mode='true'] { position: fixed; z-index: 200; top: var(--terminal-focus-top); left: var(--terminal-focus-left); width: var(--terminal-focus-width); height: var(--terminal-focus-height);",
    );
    // The Exit pill floats in a Kerf FloatingToolbar that owns the safe-area inset (HS2-798JS4).
    expect(markup).toContain('aria-label="Terminal Focus Exit"');
    expect(css).not.toContain('__focus-exit');
    // Off phones (no focusTextSize) the drawer focus mode shows only the Exit pill.
    expect(markup).not.toContain('terminal-drawer__focus-controls');
    expect(markup).not.toContain('aria-label="Clipboard"');
  });
  it('exposes the phone text-size control in drawer focus mode without an inline number (HS2-ZSFAHF)', () => {
    const base = {
      projectId: 'project',
      projectName: 'Project',
      sessions,
      width: 390,
      height: 492,
      fitAcross: 2,
      fitHigh: 2,
      selectedId: 'one',
      focusMode: true,
      focusViewport: { left: 4, top: 18, width: 382, height: 492 },
    } as const;
    const markup = String(
        TerminalDrawer({
          ...base,
          focusTextSize: { viewport: base.focusViewport, keyboardVisible: false, columns: 60 },
        }),
      ),
      css = readFileSync(resolve(import.meta.dirname, 'terminal-drawer.css'), 'utf8');
    // Same control contract as the magnified terminal: cycles columns, keeps the accessible name and
    // data-columns, and reports the change with a toast (no inline number, HS2-89JZSN).
    expect(markup).toContain('terminal-drawer__focus-controls');
    expect(markup).toContain('data-action="cycle-mobile-terminal-columns"');
    expect(markup).toContain('data-columns="60"');
    expect(markup).toContain('aria-label="Text Size: 60 Columns. Change Text Size"');
    expect(markup).toContain('data-lucide="a-large-small"');
    expect(markup).not.toContain('aria-hidden="true">60<');
    // HS2-FRB545: the Copy/Paste group sits beside it in the same Kerf FloatingToolbar.
    expect(markup).toContain('aria-label="Clipboard"');
    expect(markup).toContain('data-action="copy-terminal-text"');
    expect(markup).toContain('data-action="paste-terminal-text"');
    expect(markup).toContain('data-lucide="copy"');
    expect(markup).toContain('data-lucide="clipboard-paste"');
    expect(css).not.toContain('__focus-text-size');
    // Hidden while the keyboard is presented: the key bar's Fn row carries the same actions.
    const keyboardUp = String(
      TerminalDrawer({
        ...base,
        focusTextSize: { viewport: base.focusViewport, keyboardVisible: true, columns: 60 },
      }),
    );
    expect(keyboardUp).not.toContain('terminal-drawer__focus-controls');
    expect(keyboardUp).toContain('aria-label="Exit terminal focus"');
  });
  it('keeps project-drawer visibility local and offers no grouping controls', () => {
    const markup = render();
    expect(markup).not.toContain('aria-label="Manage workspace visibility"');
    expect(markup).not.toContain('name="terminal-visibility-group"');
    expect(markup).not.toContain('data-visibility-scope');
  });
  it('uses available width for drawer grid levels above fit one', () => {
    const markup = render();
    expect(markup).toContain('--terminal-tile-width:432px');
    expect(markup).toContain('--terminal-tile-height:312px');
  });
  it('uses one horizontal row at fit one and row-major wrapping above it', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'terminal-dashboard.css'), 'utf8');
    expect(css).toMatchSource(/data-basis="high"\] \{[^}]*grid-template-columns: repeat\(auto-fill/);
    expect(css).toMatchSource(/data-basis="high"\]\[data-fit="1"\] \{[^}]*grid-auto-flow: column/);
  });

  it('names the AI shell for a single provider and nests several behind a default (HS2-3HT4PA)', () => {
    const codex = { id: 'codex', name: 'Codex' },
      claude = { id: 'claude', name: 'Claude' },
      drawer = (aiProviders: (typeof codex)[], defaultAiProvider?: string) =>
        String(
          TerminalDrawer({
            projectId: 'project',
            projectName: 'Project',
            sessions,
            width: 900,
            height: 320,
            fitAcross: 2,
            fitHigh: 2,
            selectedId: 'grid',
            aiProviders,
            defaultAiProvider,
          }),
        );
    // No provider discovered yet: the generic entry, launching the project default.
    expect(aiShellMenuItem([])).toMatchObject({ label: 'AI shell', attributes: { 'data-item-id': 'ai-shell' } });
    const single = drawer([claude], 'claude');
    expect(single).toContain('Claude shell');
    expect(single).not.toContain('>AI shell<');
    expect(single).not.toContain('data-provider=');
    expect(aiShellMenuItem([claude])).not.toHaveProperty('submenu');
    // A stale default naming an uninstalled tool still launches the only installed provider.
    expect(aiShellMenuItem([claude], 'codex')).toMatchObject({
      label: 'Claude shell',
      attributes: { 'data-item-id': 'ai-shell', 'data-provider': 'claude' },
    });
    // Several providers: a parent item with Default (<effective provider>), a divider, then each provider.
    const several = aiShellMenuItem([codex, claude], 'claude');
    expect(several).toMatchObject({ label: 'AI shell', attributes: { 'data-item-id': 'ai-shell-providers' } });
    expect(several).not.toHaveProperty('action');
    const submenu = 'submenu' in several ? (several.submenu ?? []) : [];
    expect(
      submenu.map((entry) =>
        entry.kind === 'divider' ? '---' : entry.kind === 'heading' ? entry.label : [entry.label, entry.attributes],
      ),
    ).toEqual([
      ['Default (Claude)', { 'data-item-id': 'ai-shell' }],
      '---',
      ['Codex', { 'data-item-id': 'ai-shell', 'data-provider': 'codex' }],
      ['Claude', { 'data-item-id': 'ai-shell', 'data-provider': 'claude' }],
    ]);
    // An unknown default falls back to the first provider's name; every actionable entry carries an icon.
    const fallback = aiShellMenuItem([codex, claude], 'gone');
    expect('submenu' in fallback && fallback.submenu?.[0]).toMatchObject({ label: 'Default (Codex)' });
    for (const entry of submenu)
      if (entry.kind !== 'divider' && entry.kind !== 'heading') expect(entry.icon).toBeTruthy();
    const markup = drawer([codex, claude], 'codex');
    expect(markup).toContain('Default (Codex)');
    expect(markup).toContain('data-provider="claude"');
  });
});

describe('halted AI sessions (HS2-HJ4D1H)', () => {
  const halt = {
    error_type: 'overloaded',
    message: 'Selected model is at capacity. Please try a different model.',
    agent: 'claude',
    at: '2026-10-05T08:00:00Z',
  };
  it('marks a halted terminal tab with the error, outranking busy, and clears with the halt', () => {
    const markup = (busy: boolean, withHalt: boolean) =>
      String(
        TerminalDrawer({
          projectId: 'project',
          projectName: 'Project',
          sessions: [{ ...sessions[0], busy, ...(withHalt ? { halt } : {}) }],
          width: 900,
          height: 320,
          fitAcross: 2,
          fitHigh: 2,
          selectedId: 'grid',
        }),
      );
    const halted = markup(true, true);
    expect(halted).toContain('class="terminal-drawer__halt"');
    expect(halted).toContain(`<title>Stopped: ${halt.message}</title>`);
    expect(halted).toContain('data-lucide="triangle-alert"');
    expect(halted).not.toContain('terminal-drawer__busy-dot');
    const resumed = markup(true, false);
    expect(resumed).not.toContain('terminal-drawer__halt');
    expect(resumed).toContain('terminal-drawer__busy-dot');
  });
});

describe('AI connection to Hot Sheet (HS2-EV1XK3)', () => {
  const markup = (session: Partial<TerminalDashboardSession>) =>
    String(
      TerminalDrawer({
        projectId: 'project',
        projectName: 'Project',
        sessions: [{ ...sessions[0], ...session }],
        width: 900,
        height: 320,
        fitAcross: 2,
        fitHigh: 2,
        selectedId: 'grid',
      }),
    );
  it('marks connected and unconnected AI tabs beside busy, and nothing for an unknown state', () => {
    const connected = markup({
      busy: true,
      tool: 'codex',
      aiConnection: 'connected',
      ai_connection: { agent: 'codex', at: '2026-10-05T08:00:00Z' },
    });
    expect(connected).toContain('data-ai-connection="connected"');
    expect(connected).toContain('data-lucide="plug"');
    expect(connected).toContain('Codex is connected to Hot Sheet');
    expect(connected).toContain('terminal-drawer__busy-dot');
    const missing = markup({ busy: false, kind: 'ai', tool: 'codex', aiConnection: 'missing' });
    expect(missing).toContain('data-ai-connection="missing"');
    expect(missing).toContain('data-lucide="unplug"');
    expect(missing).toContain('Run /hooks in Codex');
    expect(missing).not.toContain('terminal-drawer__busy-dot');
    // HS2-670ADP: hooks without hotsheet-mcp fold into the same badge as not fully connected.
    const mcpMissing = markup({
      busy: false,
      tool: 'claude',
      aiConnection: 'mcp_missing',
      ai_connection: { agent: 'claude', at: '2026-10-05T08:00:00Z' },
      mcp_connected: false,
    });
    expect(mcpMissing).toContain('data-ai-connection="mcp_missing"');
    expect(mcpMissing).toContain('data-lucide="unplug"');
    expect(mcpMissing).not.toContain('data-lucide="plug"');
    expect(mcpMissing).toContain('Claude is not fully connected to Hot Sheet: MCP not connected.');
    const plain = markup({ busy: false });
    expect(plain).not.toContain('terminal-drawer__status');
    expect(plain).not.toContain('data-ai-connection');
  });
  it('lets a halt outrank the connection state', () => {
    const halted = markup({
      aiConnection: 'connected',
      halt: { error_type: 'overloaded', message: 'At capacity', at: '2026-10-05T08:00:00Z' },
    });
    expect(halted).toContain('terminal-drawer__halt');
    expect(halted).not.toContain('data-ai-connection');
  });
});

describe('TerminalDrawer chat permission indicator (HS2-NHRDMW)', () => {
  it('shows the active permission mode on the chat tab, as a warning for a risky mode', () => {
    const draw = (permissionMode?: { label: string; warning?: string }, error?: string) =>
      String(
        TerminalDrawer({
          projectId: 'project',
          projectName: 'Project',
          sessions: [],
          chatTabs: [
            { id: 'chat:one', name: 'Claude chat', tool: 'Claude', content: '' as never, permissionMode, error },
          ],
          width: 900,
          height: 320,
          fitAcross: 2,
          fitHigh: 2,
          selectedId: 'chat:one',
        }),
      );
    const safe = draw({ label: 'Accept edits' });
    expect(safe).toContain('aria-label="Permissions: Accept edits"');
    expect(safe).toContain('data-permission-warning="false"');
    const risky = draw({ label: 'Bypass permissions', warning: 'Runs everything.' });
    expect(risky).toContain('data-permission-warning="true"');
    expect(risky).toContain('<title>Permissions: Bypass permissions. Runs everything.</title>');
    // A stopped chat's error takes the trailing slot; no mode means no indicator.
    expect(draw({ label: 'Accept edits' }, 'Model at capacity')).not.toContain('Permissions:');
    expect(draw()).not.toContain('terminal-drawer__permission-mode');
  });
});
