import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { AIConversation } from './ai-conversation';
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
  it('keeps the grid tab fixed while the shared tab strip overflows', () => {
    // Kerf's pinned AppTab owns the sticky treatment; the app adds no rule or class (HS2-WF3W6A).
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
    expect(render()).toMatch(/terminal-drawer__create-wrap"[^>]*data-size="compact"/);
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
    expect(markup).toContain('class="kui-app-tab terminal-tab"');
    expect(markup).toMatch(/class="kui-app-tab terminal-tab"[^>]*style="[^"]*144px"/);
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
    // Peers settle at whole-tab starts beside the pinned grid tab, never as a sliver (HS2-6Y8HSH).
    expect(markup).toMatch(
      /class="kui-tab-bar__tabs"[^>]*data-snap-tabs="true"|data-snap-tabs="true"[^>]*class="kui-tab-bar__tabs"/,
    );
    expect(markup).toContain('>Project grid</span>');
    expect(markup).toContain('data-lucide="layout-grid"');
    expect(markup).not.toContain('data-lucide="grid-3x3"');
    expect(markup).toMatch(
      /data-kui-tab-list[\s\S]*Terminal 2[\s\S]*kui-tab-bar__trailing[\s\S]*terminal-drawer__create/,
    );
    expect(markup).toMatch(/kui-tab-bar__end[\s\S]*terminal-drawer__actions[\s\S]*Hide terminal drawer/);
    expect(markup).toContain('data-component="app-tab"');
    expect(markup).toContain('data-tab-kind="terminal"');
    expect(markup).toContain('draggable="true"');
    expect(markup).toContain('tabindex="0"');
    expect(markup).toContain('class="kui-app-tab');
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
    expect(chat).toContain('data-component="app-tab"');
    expect(chat).toContain('data-action="close-ai-chat-tab"');
    expect(chat).toContain('data-presentation="embedded"');
    for (const item of ['Terminal', 'AI shell', 'AI chat', 'Saved conversation…']) expect(chat).toContain(item);
    expect(chat).toContain('data-action="open-saved-conversation"');
    expect(chat).toContain('data-component="popup-menu"');
    expect(chat).toContain('data-terminal-drawer-create="true"');
    expect(chat).toContain('placement="top-start"');
    expect(chat).toContain('New drawer item');
    expect(chat).toContain('data-action="create-terminal-drawer-item"');
    expect(chat).not.toContain('terminal-drawer__create-menu');
    expect(chat).not.toContain('data-component="list-header"');
    expect(chat).not.toContain('data-lucide="chevron-right"');
    expect(chat.match(/<wa-dropdown-item/g)).toHaveLength(4);
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
    expect(own).toContain('data-component="terminal-context-menu"');
    expect(own.match(/data-component="terminal-context-menu"/g)).toHaveLength(1);
    expect(own).toContain('data-action="open-terminal-project" data-item-id="project:one"');
    expect(own).not.toContain('data-action="hide-dashboard-terminal"');
    // A menu opened for a tile in another grid (e.g. the workspace dashboard) is not duplicated here.
    expect(withMenu('elsewhere:one')).not.toContain('data-component="terminal-context-menu"');
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
    ).not.toContain('data-component="terminal-context-menu"');
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
  it('reserves the shared trailing tab slot whether or not a terminal has state', () => {
    const markup = render();
    expect(markup.match(/kui-app-tab__trailing/g)).toHaveLength(2);
    expect(markup).toMatch(
      /Terminal 1<\/span><span class="kui-app-tab__trailing"><i class="terminal-drawer__busy-dot" aria-label="Busy"/,
    );
    expect(markup).toMatch(/Terminal 2<\/span><span class="kui-app-tab__trailing"><\/span>/);
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
  it('retains dedicated terminal sessions while presenting one without grid chrome or zoom', () => {
    const markup = render('one');
    expect(markup).toContain('data-mode="dedicated"');
    expect(markup).toContain('data-component="terminal-session"');
    expect(markup.match(/data-terminal-id="one"/g)?.length).toBeGreaterThanOrEqual(2);
    expect(markup.match(/data-component="terminal-viewport"/g)).toHaveLength(2);
    expect(markup.match(/class="terminal-session"[^>]*hidden/g)).toHaveLength(1);
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
      /data-action="copy-terminal-text" aria-label="Copy terminal text"[^>]*>.*data-lucide="copy".*data-action="paste-terminal-text" aria-label="Paste"[^>]*>.*data-lucide="clipboard-paste".*data-action="toggle-terminal-drawer"/s,
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
    expect(css).toContain('env(safe-area-inset-top, 0px)');
    // Off phones (no focusTextSize) the drawer focus mode shows only the Exit pill.
    expect(markup).not.toContain('terminal-drawer__focus-text-size');
    expect(markup).not.toContain('terminal-drawer__focus-clipboard');
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
    expect(markup).toContain('class="terminal-drawer__focus-text-size"');
    expect(markup).toContain('data-action="cycle-mobile-terminal-columns"');
    expect(markup).toContain('data-columns="60"');
    expect(markup).toContain('aria-label="Text size: 60 columns. Change text size"');
    expect(markup).toContain('data-lucide="a-large-small"');
    expect(markup).not.toContain('aria-hidden="true">60<');
    // Hidden while the keyboard is presented.
    expect(markup).toContain('data-keyboard-visible="false"');
    expect(css).toMatchSource(".terminal-drawer__focus-text-size[data-keyboard-visible='true'] { display: none; }"); // HS2-FRB545: the Copy/Paste pill sits beside it and hides with it while the keyboard is up.
    expect(markup).toContain('class="terminal-drawer__focus-clipboard" role="group" aria-label="Clipboard"');
    expect(markup).toContain('data-action="copy-terminal-text"');
    expect(markup).toContain('data-action="paste-terminal-text"');
    expect(markup).toContain('data-lucide="copy"');
    expect(markup).toContain('data-lucide="clipboard-paste"');
    expect(css).toMatchSource(".terminal-drawer__focus-clipboard[data-keyboard-visible='true'] { display: none; }");
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
    expect(halted).toContain(`title="Stopped: ${halt.message}"`);
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
