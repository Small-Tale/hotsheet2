import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { sourceTokens } from '../source-format-matchers';
import { AIConversation } from './ai-conversation';
import { TerminalDrawer } from './terminal-drawer';

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
    const css = readFileSync(resolve(import.meta.dirname, 'terminal-drawer.css'), 'utf8');
    expect(css).toContain("@import '@kerfjs/ui/tab-bar.css'");
    expect(css).toMatch(/\.terminal-drawer__grid-tab \{[^}]*position: sticky/);
  });
  it('keeps the wrapped create action on the shared transparent pill button surface', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'terminal-drawer.css'), 'utf8'),
      base =
        sourceTokens(css).match(
          /\.terminal-drawer__rail>button,\.terminal-drawer__create,\.terminal-drawer__actions>button\{([^}]+)\}/,
        )?.[1] ?? '';
    expect(base).toContainSource('height: remify(32px)');
    expect(base).toContainSource('border-radius: var(--wa-border-radius-pill)');
    expect(base).toContainSource('background: transparent');
    expect(css).toContain('.terminal-drawer__create:hover');
    expect(css).toContain('.terminal-drawer__create:focus-visible');
  });
  it('uses the Kerf spacing scale for the rail, connected menu rows, content, and terminal inset', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'terminal-drawer.css'), 'utf8');
    expect(css).not.toContain('--wa-space-');
    expect(css).toContain('padding: var(--kui-space-xs) var(--kui-space-m)');
    expect(css).toContain('gap: var(--kui-space-none)');
    expect(css).toContain('padding-top: var(--kui-space-xs)');
    expect(css).toMatch(/\.terminal-session \.terminal-viewport \{[^}]*padding: var\(--kui-space-xs\)/);
  });
  it('reserves a complete gutter for focus rings and selected-tab shadows inside the shared scroller', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'terminal-drawer.css'), 'utf8'),
      rule = css.match(/\.terminal-drawer__views \.kui-tab-bar__tabs \{([^}]+)\}/)?.[1] ?? '';
    expect(css).toContain("@import '@kerfjs/ui/tab-bar.css'");
    expect(rule).toContain('padding: var(--kui-space-2xs)');
    expect(rule).toContain('margin: calc(var(--kui-space-2xs) * -1)');
  });
  it('sizes terminal names from their content instead of reserving icon-width name space', () => {
    const markup = render();
    expect(markup).toContain('class="kui-app-tab terminal-tab"');
    expect(markup).toContain('style="--kui-app-tab-label-max-width:144px"');
    expect(markup).toContain('data-size="compact"');
  });
  it('renders one automatic Kerf tab bar with draggable terminal tabs before its trailing actions', () => {
    const markup = render();
    expect(markup).toContain('data-mode="grid"');
    expect(markup).toContain('data-maximized="false"');
    expect(markup).toContain('data-component="tab-bar"');
    expect(markup).toContain('data-tab-bar-id="terminal-drawer"');
    expect(markup).toContain('data-tab-activation="manual"');
    expect(markup).toContain('>Project grid</span>');
    expect(markup).toContain('data-lucide="layout-grid"');
    expect(markup).not.toContain('data-lucide="grid-3x3"');
    expect(markup).toMatch(
      /data-kui-tab-list[\s\S]*Terminal 2[\s\S]*kui-tab-bar__trailing[\s\S]*terminal-drawer__create/,
    );
    expect(markup).toContain('data-component="app-tab"');
    expect(markup).toContain('data-tab-kind="terminal"');
    expect(markup).toContain('draggable="true"');
    expect(markup).toContain('tabindex="0"');
    expect(markup).toContain('class="kui-app-tab');
    expect(markup).toContain('data-action="close-terminal-tab"');
    expect(markup.indexOf('Terminal 1')).toBeLessThan(markup.indexOf('aria-label="Busy"'));
    for (const action of [
      'select-drawer-item',
      'toggle-terminal-create-menu',
      'toggle-terminal-drawer',
      'toggle-terminal-drawer-maximize',
    ])
      expect(markup).toContain(`data-action="${action}"`);
  });
  it('opens a direct action menu without a redundant heading or false submenu disclosures', () => {
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
          createMenuOpen: true,
        }),
      );
    expect(chat).toContain('data-mode="ai-chat"');
    expect(chat).toContain('data-component="app-tab"');
    expect(chat).toContain('data-action="close-ai-chat-tab"');
    expect(chat).toContain('data-presentation="embedded"');
    for (const item of ['Default shell', 'AI shell', 'AI chat', 'Saved conversation…']) expect(chat).toContain(item);
    expect(chat).toContain('data-action="open-saved-conversation"');
    expect(chat).toContain('role="menu"');
    expect(chat).not.toContain('data-component="list-header"');
    expect(chat).not.toContain('data-lucide="chevron-right"');
    expect(chat.match(/<wa-dropdown-item/g)).toHaveLength(4);
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
    expect(markup).toMatch(/Terminal 1<\/span><span class="kui-app-tab__trailing"><i aria-label="Busy"/);
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
    expect(css).toMatchSource(".terminal-drawer__focus-text-size[data-keyboard-visible='true'] { display: none; }");
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
});
