import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import {
  FixedAspectTerminalCard,
  TerminalDashboard,
  TerminalDashboardControls,
  type TerminalDashboardGroup,
  TerminalPreview,
  TerminalSession,
  TerminalVisibilityControls,
} from './terminal-dashboard';

const groups: TerminalDashboardGroup[] = [
  {
    projectId: 'one',
    projectName: 'Project One',
    sessions: [
      {
        id: 'term-1',
        projectId: 'one',
        projectName: 'Project One',
        title: 'Codex',
        alive: true,
        busy: true,
        cwd: '/work/one',
        progress: 42,
        scrollback: 'Working\nRunning tests',
      },
    ],
    chats: [
      {
        id: 'chat:review',
        projectId: 'one',
        projectName: 'Project One',
        name: 'Review chat',
        tool: 'Claude',
        busy: true,
        summary: 'Reviewing the workspace changes',
      },
    ],
  },
];
const css = readFileSync(new URL('./terminal-dashboard.css', import.meta.url), 'utf8');

describe('TerminalDashboard', () => {
  it('offers a clear action only for a halted terminal in its More actions menu (HS2-6J79SA)', () => {
    const props = {
        groups,
        width: 1200,
        height: 700,
        fitAcross: 4,
        fitHigh: 2,
        contextMenu: { key: 'one:term-1', x: 100, y: 100 },
      },
      ordinary = String(TerminalDashboard(props)),
      halted = String(
        TerminalDashboard({
          ...props,
          groups: [
            {
              ...groups[0],
              sessions: [
                {
                  ...groups[0].sessions[0],
                  halt: { error_type: 'rate_limit', message: 'Rate limit reached.', at: '2026-10-06T01:00:00Z' },
                },
              ],
            },
          ],
        }),
      );
    expect(ordinary).not.toContain('Clear stopped state');
    expect(halted).toContain('Clear stopped state');
    expect(halted).toContain('data-action="clear-terminal-halt"');
  });
  it('renders every dashboard terminal as a fixed 80x24 scaled non-interactive viewport with Kerf floating zoom controls', () => {
    const markup = String(TerminalDashboard({ groups, width: 1200, height: 700, fitAcross: 4, fitHigh: 2 }));
    expect(markup).toContain('data-basis="across"');
    expect(markup).toContain('data-fit="4"');
    expect(markup).toContain('Project One');
    expect(markup).toContain('Running tests');
    expect(markup).toContain('42%');
    expect(markup).toContain('data-component="terminal-viewport"');
    expect(markup).toContain('data-display-mode="scaled-preview"');
    expect(markup).toContain('data-grid-policy="dashboard-80x24"');
    expect(markup).toContain('aria-hidden="true"');
    expect(markup).toContain('data-action="preview-terminal"');
    expect(markup).toContain('<div class="terminal-dashboard__zoom"><div class="kui-floating-toolbar"');
    expect(markup).toContain(
      'data-component="floating-toolbar" data-position="bottom-end" data-placement="floating" role="toolbar" aria-label="Workspace Grid Controls"',
    );
    expect(markup).toContain('data-component="toolbar-control-group"');
    expect(markup).toContain('data-tone="default"');
    expect(markup).toContain('data-action="zoom-terminal-grid"');
    expect(markup).toContain('data-action="toggle-terminal-grid-fullscreen"');
    expect(markup).toContain('aria-label="Enter workspace grid full screen" aria-pressed="false"');
    const expanded = String(
      TerminalDashboard({ groups, width: 1200, height: 700, fitAcross: 4, fitHigh: 2, fullscreen: true }),
    );
    expect(expanded).toContain('aria-label="Exit workspace grid full screen" aria-pressed="true"');
    const drawer = String(
      TerminalDashboard({ groups, width: 1200, height: 700, fitAcross: 4, fitHigh: 2, layoutMode: 'drawer' }),
    );
    expect(drawer).not.toContain('data-action="toggle-terminal-grid-fullscreen"');
    for (const action of ['magnify-terminal', 'dedicate-terminal'])
      expect(markup).not.toContain(`data-action="${action}"`);
  });
  it('renders AI chats beside terminals with the same fixed-natural-size scaling contract', () => {
    const markup = String(TerminalDashboard({ groups, width: 1200, height: 700, fitAcross: 4, fitHigh: 2 }));
    expect(markup).toContain('aria-label="Workspace Grid"');
    expect(markup).toContain('data-component="workspace-chat-tile"');
    expect(markup).toContain('data-chat-id="chat:review"');
    expect(markup).toContain('data-action="open-grid-ai-chat"');
    expect(markup).toContain('Claude AI Chat');
    expect(markup).toContain('Reviewing the workspace changes');
    expect(markup).toMatch(/data-component="text"[^>]*data-size="medium"[^>]*><strong>Claude AI Chat<\/strong>/);
    expect(markup).toContain('data-max-lines="3"');
    expect(markup).toMatch(/data-component="text"[^>]*data-size="compact"[^>]*>42%<\/span>/);
    expect(markup).toMatch(
      /class="terminal-tile__state" title="Working"><span class="kui-badge"[^>]*aria-label="Working"/,
    );
    expect(markup).toContain('data-preview-scale="0.8877777777777779"');
    expect(markup).toContain(
      '--workspace-chat-preview-natural-width:300px;--workspace-chat-preview-natural-height:180px',
    );
    expect(markup.match(/data-component="terminal-tile"/g)).toHaveLength(1);
    expect(markup.match(/data-component="workspace-chat-tile"/g)).toHaveLength(1);
  });
  it('counts and filters both item kinds in project-grouped grids', () => {
    const grouped = String(
      TerminalDashboard({ groups, width: 1200, height: 700, fitAcross: 4, fitHigh: 2, grouping: 'project' }),
    );
    // Kerf ListHeader names each project section with its item count (HS2-GWDD1Z).
    expect(grouped).toContain('data-component="list-header"');
    expect(grouped).toContain('Project One');
    expect(grouped).toContain('2 workspace items');
    const hidden = String(
      TerminalDashboard({
        groups,
        width: 1200,
        height: 700,
        fitAcross: 4,
        fitHigh: 2,
        hiddenKeys: ['one:chat:review'],
      }),
    );
    expect(hidden).not.toContain('data-component="workspace-chat-tile"');
    expect(hidden).toContain('data-component="terminal-tile"');
    const chatOnly = String(
      TerminalDashboard({ groups, width: 1200, height: 700, fitAcross: 4, fitHigh: 2, visibleTypes: ['chat'] }),
    );
    expect(chatOnly).toContain('data-component="workspace-chat-tile"');
    expect(chatOnly).not.toContain('data-component="terminal-tile"');
    const terminalOnly = String(
      TerminalDashboard({ groups, width: 1200, height: 700, fitAcross: 4, fitHigh: 2, visibleTypes: ['shell'] }),
    );
    expect(terminalOnly).not.toContain('data-component="workspace-chat-tile"');
    expect(terminalOnly).toContain('data-component="terminal-tile"');
  });
  it('makes the footer identity actionable, explains terminal state, and omits the snapshot working directory', () => {
    const markup = String(TerminalDashboard({ groups, width: 1200, height: 700, fitAcross: 4, fitHigh: 2 }));
    expect(markup).toContain('aria-label="Open Codex in Project One"');
    expect(markup).toMatch(/class="terminal-tile__state" title="Busy"><span class="kui-badge"[^>]*aria-label="Busy"/);
    expect(markup).not.toContain('/work/one');
  });
  it('keeps fixed 80x24 sizing in dashboard previews and magnified terminals', () => {
    const session = groups[0].sessions[0],
      preview = String(FixedAspectTerminalCard({ session })),
      magnified = String(FixedAspectTerminalCard({ session, mode: 'magnified' }));
    for (const markup of [preview, magnified]) {
      expect(markup).toContain('terminal-tile__viewport-frame');
      expect(markup).toContain('data-geometry-ready="false"');
      expect(markup).toContain('data-action="open-terminal-context-menu"');
      expect(markup).toContain('data-lucide="ellipsis"');
    }
    expect(preview).toContain('data-grid-policy="dashboard-80x24"');
    expect(preview).toContain('data-fixed-aspect-terminal-card="preview"');
    expect(preview).toContain('data-display-mode="scaled-preview"');
    expect(preview).toContain('data-mount-policy="visible-progressive"');
    expect(magnified).toContain('data-fixed-aspect-terminal-card="magnified"');
    expect(magnified).toContain('data-display-mode="interactive"');
    expect(magnified).toContain('data-mount-policy="immediate"');
    expect(magnified).not.toContain('data-grid-policy="dashboard-80x24"');
    expect(magnified).not.toContain('data-mobile-grid-policy="80xm"');
  });
  it('keeps the phone magnified toolbar compatible with its fixed grid', () => {
    const session = groups[0].sessions[0],
      mobile = { viewport: { left: 0, top: 0, width: 360, height: 640 }, keyboardVisible: false, columns: 60 },
      markup = String(FixedAspectTerminalCard({ session, mode: 'magnified', mobile }));
    expect(markup).not.toContain('data-action="cycle-mobile-terminal-columns"');
    expect(markup).not.toContain('data-grid-policy="dashboard-80x24"');
    expect(markup).not.toContain('aria-hidden="true">60<');
    // HS2-FRB545: the phone toolbar carries Copy and Paste beside text size.
    expect(markup).toContain('data-action="copy-terminal-text"');
    expect(markup).toContain('data-action="paste-terminal-text"');
    expect(String(FixedAspectTerminalCard({ session, mode: 'magnified' }))).not.toContain('copy-terminal-text');
    // HS2-5DHHPV: a touch-first desktop's magnified toolbar adds Copy and Paste but keeps the desktop
    // identity and open-in-drawer button; previews never carry them.
    const tablet = String(FixedAspectTerminalCard({ session, mode: 'magnified', touchClipboard: true }));
    expect(tablet.match(/class="terminal-tile__clipboard"/g)).toHaveLength(2);
    expect(tablet).toContain('Open Codex in Project Terminal Drawer');
    // HS2-XHA5QA: the icon actions sit in one borderless Kerf group, compact on desktop and default on the
    // phone toolbar, whose Close button gets its own single group.
    const actions = (html: string) =>
      /<[^>]*data-component="toolbar-control-group"[^>]*aria-label="Terminal Actions"[^>]*>/.exec(html)?.[0] ?? '';
    expect(actions(tablet)).toContain('data-appearance="borderless"');
    expect(actions(tablet)).toContain('data-size="compact"');
    expect(actions(markup)).toContain('data-size="default"');
    // The footer slot hosts a Kerf footer Toolbar, tuned only through public toolbar tokens.
    expect(tablet).toMatch(/<div class="terminal-tile__footer">\s*<footer[^>]*class="kui-toolbar"/);
    expect(css).toMatchSource(
      /\.terminal-tile__footer \{[^}]*--kui-toolbar-group-size: [^;]+;[^}]*--kui-toolbar-inset:/,
    );
    expect(markup).toMatch(/data-appearance="borderless"[^>]*>\s*<button[^>]*class="terminal-tile__close"/);
    expect(tablet).not.toContain('terminal-tile__identity-project');
    expect(tablet).not.toContain('cycle-mobile-terminal-columns');
    expect(String(FixedAspectTerminalCard({ session, touchClipboard: true }))).not.toContain('copy-terminal-text');
    expect(
      String(
        TerminalDashboard({
          groups,
          width: 1180,
          height: 700,
          fitAcross: 4,
          fitHigh: 2,
          magnifiedKey: 'one:term-1',
          touchClipboard: true,
        }),
      ).match(/data-action="paste-terminal-text"/g),
    ).toHaveLength(1);
    // HS2-8NQRJB: the phone toolbar drops the separate open-in-drawer button (More actions → Open and the
    // identity keep that action) so the identity is not truncated; the desktop toolbar keeps it.
    expect(markup).not.toContain('in project Terminal Drawer');
    expect(markup).toContain('data-action="open-terminal-context-menu"');
    expect(markup).toContain('data-action="open-terminal-project"');
    expect(String(FixedAspectTerminalCard({ session, mode: 'magnified' }))).toContain(
      'Open Codex in Project Terminal Drawer',
    );
  });
  it('keeps the smallest tile keyboard-focusable and makes its fitted magnified copy interactive', () => {
    const compact = String(TerminalDashboard({ groups, width: 900, height: 600, fitAcross: 7, fitHigh: 3 }));
    expect(compact).toContain('data-basis="high"');
    expect(compact).toContain('data-fit="3"');
    expect(compact).toContain('data-preview-only="true"');
    expect(compact).toContain('data-display-mode="scaled-preview"');
    const magnified = String(
      TerminalDashboard({ groups, width: 900, height: 600, fitAcross: 7, fitHigh: 3, magnifiedKey: 'one:term-1' }),
    );
    expect(magnified).toContain('role="dialog"');
    expect(magnified).toContain('data-display-mode="interactive"');
    expect(magnified).toContain('data-component="terminal-preview-placeholder"');
    expect(magnified).toContain('data-preview-paused="true"');
    expect(magnified).not.toContain('data-grid-policy="dashboard-80x24"');
    expect(magnified.match(/data-component="terminal-viewport"/g)).toHaveLength(1);
    expect(magnified).toContain('data-action="open-terminal-project"');
    expect(magnified).toContain('Open Codex in Project Terminal Drawer');
    expect(magnified).not.toContain('Restore terminal grid');
  });
  it('pauses only the matching grid preview while a terminal is magnified', () => {
    const second = { ...groups[0].sessions[0], id: 'term-2', title: 'Shell' },
      markup = String(
        TerminalDashboard({
          groups: [{ ...groups[0], sessions: [groups[0].sessions[0], second] }],
          width: 1200,
          height: 700,
          fitAcross: 4,
          fitHigh: 2,
          magnifiedKey: 'one:term-1',
        }),
      );
    expect(markup.match(/data-component="terminal-preview-placeholder"/g)).toHaveLength(1);
    expect(markup.match(/data-display-mode="scaled-preview"/g)).toHaveLength(1);
    expect(markup).toContain('data-terminal-id="term-1" aria-hidden="true"');
    expect(markup).toContain('data-terminal-id="term-2" data-display-mode="scaled-preview"');
  });
  it('uses the standard context-menu items for both right-click and the footer action', () => {
    const markup = String(
      TerminalDashboard({
        groups,
        width: 900,
        height: 600,
        fitAcross: 7,
        fitHigh: 3,
        contextMenu: { key: 'one:term-1', x: 20, y: 30 },
      }),
    );
    expect(markup).toContain('data-action="open-terminal-context-menu"');
    expect(markup).toContain('data-context-menu="terminal"');
    expect(markup.match(/class="kui-popup-menu__item"/g)).toHaveLength(2);
    expect(markup).toContain('data-action="open-terminal-project"');
    expect(markup).toContain('data-action="hide-dashboard-terminal"');
    const openOnly = String(
      TerminalDashboard({
        groups,
        width: 900,
        height: 600,
        fitAcross: 7,
        fitHigh: 3,
        contextMenu: { key: 'one:term-1', x: 20, y: 30 },
        contextMenuActions: ['open'],
      }),
    );
    expect(openOnly.match(/class="kui-popup-menu__item"/g)).toHaveLength(1);
    expect(openOnly).not.toContain('data-action="hide-dashboard-terminal"');
    expect(
      String(
        TerminalDashboard({
          groups,
          width: 900,
          height: 600,
          fitAcross: 7,
          fitHigh: 3,
          contextMenu: { key: 'other:term-9', x: 20, y: 30 },
        }),
      ),
    ).not.toContain('data-context-menu="terminal"');
  });
  it('uses a compact label select with a content-sized popup for visibility groups and omits global layout grouping', () => {
    const visibilityGroups = [
        { id: 'default', name: 'Default', hiddenKeys: [] },
        { id: 'focus', name: 'Focus', hiddenKeys: ['one:term-1'] },
      ],
      markup = String(
        TerminalDashboardControls({ hiddenCount: 2, visibilityGroups, activeVisibilityGroupId: 'focus' }),
      );
    expect(markup).toContain('data-action="open-terminal-visibility"');
    // The nested Toolbar carries no app class; its wrapper configures it through public tokens (HS2-3J2PX3).
    expect(markup).toContain(
      '<div class="terminal-dashboard-controls__visibility-group" data-visibility-scope="dashboard"><header class="kui-toolbar"',
    );
    expect(markup).toMatch(/<div class="kui-toolbar__trailing"><div class="kui-toolbar-control-group"/);
    expect(markup).not.toContain('visibility-toolbar');
    // Kerf caps the Select's renderSelected trigger without an app width wrapper.
    expect(markup).toContain('terminal-dashboard-controls__visibility-select"');
    expect(markup).toContain('data-trigger-max="" data-trigger-min=""');
    expect(markup).not.toContain('<div class="terminal-dashboard-controls__visibility-select"');
    expect(markup).toContain('data-component="badge" data-tone="warning" data-appearance="solid"');
    expect(markup).toContain('data-placement="corner" aria-hidden="true">2</span>');
    expect(markup).toContain('name="terminal-visibility-group"');
    expect(markup).toContain(
      'class="kui-select__custom-selected"><span class="kui-select__custom-selected-content"><span>Focus</span>',
    );
    expect(markup).toMatch(/class="kui-select__option"[^>]*data-value="focus"/);
    expect(markup).not.toContain('kui-select--fit-menu');
    expect(markup).not.toContain('name="terminal-grouping"');
  });
  it('uses width-based wrapping after drawer fit one', () => {
    const markup = String(
      TerminalDashboard({ groups, width: 876, height: 296, fitAcross: 2, fitHigh: 2, layoutMode: 'drawer' }),
    );
    expect(markup).toContain('data-layout-mode="drawer"');
    expect(markup).toContain('--terminal-tile-width:432px');
    expect(markup).toContain('--terminal-tile-height:312px');
  });
  it('keeps card chrome borderless and scales the fixed-size chat body from the top-left', () => {
    expect(css).toMatch(/\.terminal-tile \{[^}]*border-radius:/);
    expect(css).not.toMatch(/\.terminal-tile \{[^}]*border:/);
    expect(css).toMatch(/\.terminal-tile__preview pre \{[^}]*font-family: var\(--wa-font-family-code\)/);
    expect(css).toMatch(/\.workspace-chat-tile__preview \{[^}]*background: var\(--wa-color-surface-default\)/);
    expect(css).toMatch(
      /\.workspace-chat-tile__preview-surface \{[^}]*width: var\(--workspace-chat-preview-natural-width\)[^}]*height: var\(--workspace-chat-preview-natural-height\)[^}]*transform: scale\(var\(--workspace-chat-preview-scale\)\)[^}]*transform-origin: top left/,
    );
    expect(css).toMatchSource(/\.workspace-chat-tile\[data-preview-only="true"\] \{ cursor: pointer;/);
  });
  it('anchors the Kerf floating zoom toolbar at the mobile safe-area corner', () => {
    // A zero-size positioned anchor sits at the safe-area corner; the FloatingToolbar keeps its own
    // bottom-end inset relative to it, so no app CSS touches the toolbar root (HS2-QCSPDH, HS2-VABS08).
    expect(css).toMatchSource(
      /\.terminal-dashboard__zoom \{[^}]*position: absolute;[^}]*width: 0;[^}]*height: 0;[^}]*inset-inline-end: var\(--hotsheet-safe-area-right, 0px\);[^}]*inset-block-end: var\(--hotsheet-safe-area-bottom, 0px\)/,
    );
    expect(css).not.toContain('.terminal-dashboard__zoom.kui-floating-toolbar');
    // The drawer layout's Workbench drawer already pads by the home-indicator inset (HS2-ZEC4QV), so the
    // dashboard's own drawer layout drops it; no other component reaches in to do so (HS2-DR549A).
    expect(css).toContainSource(
      ".terminal-dashboard[data-layout-mode='drawer'] .terminal-dashboard__zoom { inset-block-end: 0; }",
    );
    expect(css).toContainSource(
      ".terminal-dashboard[data-layout-mode='drawer'] .terminal-dashboard__content { padding-top: var(--kui-space-xs); }",
    );
    const drawerCss = readFileSync(new URL('./terminal-drawer.css', import.meta.url), 'utf8');
    expect(drawerCss).not.toContain('terminal-dashboard');
    expect(drawerCss).not.toContain('.app-shell');
  });
  it('renders empty, loading, and message copy as a centered Kerf List of Text (HS2-1Q6GER)', () => {
    const render = (props: Partial<Parameters<typeof TerminalDashboard>[0]>) =>
      String(TerminalDashboard({ groups: [], width: 640, height: 240, fitAcross: 3, fitHigh: 3, ...props }));
    const empty = render({}),
      loading = render({ loading: true }),
      message = render({ message: 'Terminals are unavailable.' });
    for (const markup of [empty, loading, message]) {
      expect(markup).toMatch(
        /data-terminal-dashboard-feedback="true"[^>]*class="kui-list" data-component="list"[^>]*data-fill="true"[^>]*data-h-align="center"[^>]*data-v-align="middle"/u,
      );
      expect(markup).not.toContain('terminal-dashboard__empty');
    }
    expect(empty).toMatch(/<strong>Nothing open yet<\/strong>/u);
    expect(empty).toContain('Open a project terminal or AI chat to add it to this grid.');
    expect(empty).not.toContain('role="status"');
    expect(loading).toContain('role="status"');
    expect(loading).toContain('Loading workspace items…');
    expect(message).toContain('role="status"');
    expect(message).toContain('Terminals are unavailable.');
    expect(message).not.toContain('Nothing open yet');
    expect(css).not.toContain('.terminal-dashboard__empty');
    // The controls row is a Kerf Row; the app keeps no wrapper rule for it.
    expect(String(TerminalDashboardControls({}))).toMatch(
      /^<div data-terminal-dashboard-controls="true" class="kui-row" data-component="row"/u,
    );
    expect(css).not.toMatch(/^\.terminal-dashboard-controls \{/mu);
  });
  it('styles only its own controls, configuring Kerf controls through props (HS2-DR549A)', () => {
    // No blanket button rule reaches into Kerf control groups (the zoom toolbar, the key bar).
    expect(css).not.toMatch(/\.terminal-dashboard button/);
    expect(css).not.toContain('.terminal-key-bar');
    expect(css).not.toContain('.app-shell');
    expect(css).not.toContain('.kui-');
    expect(css).not.toContain('::part(');
    // Kerf's ToolbarControlGroup owns the disabled zoom buttons' cursor, dimming, and hover (KF-FTADQT,
    // HS2-T44PFW), so the dashboard no longer styles its own group buttons.
    expect(css).not.toContain('.terminal-dashboard__zoom button');
    // HS2-KKP8YJ: iOS must not raise its own callout over a long-pressed interactive terminal.
    expect(css).toContainSource(".terminal-viewport[data-display-mode='interactive'] { -webkit-touch-callout: none; }");
    // The identity button's reset is merged into its one rule (HS2-GWDD1Z).
    expect(css).toContainSource(
      '.terminal-tile__identity { display: flex; min-width: 0; padding: 0; border: 0; border-radius: var(--wa-border-radius-s); color: var(--wa-color-neutral-on-quiet); background: transparent; flex: 1; flex-direction: column; text-align: left; }',
    );
    expect(css.match(/^\.terminal-tile__identity \{/gm)).toHaveLength(1);
    expect(css.match(/^\.terminal-dashboard-controls__visibility-select \{/gm)).toHaveLength(1);
    expect(css).not.toContain('terminal-dashboard__project-title');
    // The tile's icon actions sit in Kerf borderless ToolbarControlGroups, which own their chrome (HS2-XHA5QA).
    for (const action of ['menu', 'open', 'close', 'text-size', 'clipboard'])
      expect(css, action).not.toContain(`.terminal-tile__${action}`);
    const controls = String(
      TerminalVisibilityControls({ groups: [{ id: 'default', name: 'Default', hiddenKeys: [] }], activeId: 'default' }),
    );
    expect(controls).toContain('data-presentation="toolbar-borderless"');
    expect(controls).toContain('data-size="compact"');
    expect(controls).toContain('data-focus-ring-owner="group"');
    expect(css).not.toContain('.terminal-dashboard-controls__count');
    expect(css).not.toContain('.terminal-tile__progress');
  });
  it('clips a phone dedicated terminal through the session it renders (HS2-DR549A)', () => {
    const session = { id: 't1', projectId: 'p', projectName: 'P', alive: true, busy: false, scrollback: '' };
    expect(String(TerminalSession({ session }))).toContain(
      'class="terminal-session" data-key="p:t1" data-mobile="false"',
    );
    expect(String(TerminalSession({ session, mobile: true }))).toContain('data-mobile="true"');
    expect(css).toContainSource(".terminal-session[data-mobile='true'] .terminal-viewport { overflow: clip; }");
  });
  it('owns the dedicated session presentation and its focus-mode variant (HS2-YNW0B3)', () => {
    const session = { id: 't1', projectId: 'p', projectName: 'P', alive: true, busy: false, scrollback: '' };
    expect(String(TerminalSession({ session }))).toContain('data-focus="false"');
    expect(String(TerminalSession({ session, focus: true }))).toContain('data-focus="true"');
    expect(String(TerminalSession({ session, active: false }))).toMatch(/class="terminal-session"[^>]*hidden/);
    expect(css).toMatchSource(/\.terminal-session \.terminal-viewport \{[^}]*padding: var\(--kui-space-xs\)/);
    expect(css).toContainSource(".terminal-session[data-focus='true'] .terminal-viewport { padding: 0; }");
    expect(css).toContainSource('.terminal-session .xterm { background: var(--hs-terminal-background); }');
    // The dedicated rules follow the shared viewport rules so they win in every connection state.
    expect(css.indexOf('.terminal-session .terminal-viewport {')).toBeGreaterThan(
      css.indexOf(".terminal-viewport[data-connection='error'] {"),
    );
    const drawerCss = readFileSync(new URL('./terminal-drawer.css', import.meta.url), 'utf8');
    expect(drawerCss).not.toContain('.terminal-session');
  });
  it('owns the shared scaled preview frame, canvas, and connecting fallback (HS2-148B5C)', () => {
    const markup = String(TerminalPreview({ projectId: 'p', terminalId: 't1', viewportKey: 'close:t1' }));
    expect(markup).toContain('class="terminal-preview" data-component="terminal-preview"');
    // The scaled viewport is the frame's direct child: the runtime scales it to its parent box.
    expect(markup).toMatch(
      /<div class="terminal-preview__frame"><div class="terminal-viewport terminal-viewport--scaled-preview" data-key="close:t1"/,
    );
    expect(markup).toContain('data-display-mode="scaled-preview"');
    expect(markup).toContain('<p class="terminal-preview__fallback">Connecting to the live terminal…</p>');
    expect(
      String(TerminalPreview({ projectId: 'p', terminalId: 't1', viewportKey: 'k', fallback: 'Waiting' })),
    ).toContain('Waiting');
    expect(css).toMatchSource(
      /\.terminal-preview__frame \{[^}]*inset: var\(--terminal-preview-inset, var\(--kui-space-l\)\);[^}]*border-radius: var\(--terminal-preview-radius, var\(--wa-border-radius-m\)\)/,
    );
    expect(css).toMatchSource(
      /\.terminal-preview:has\( \.terminal-viewport\[data-connection='connected'\], \.terminal-viewport\[data-connection='reconnecting'\] \) \.terminal-preview__fallback \{ display: none; \}/,
    );
    expect(css).toMatchSource(
      /\.terminal-viewport\.terminal-viewport--scaled-preview \{[^}]*width: 1280px;[^}]*height: 768px;/,
    );
  });
  it('renders the tile independently of a dashboard ancestor, with grid and aspect fits (HS2-0X36TX)', () => {
    const session = { id: 't1', projectId: 'p', projectName: 'P', alive: true, busy: false, scrollback: 'ready' };
    expect(String(FixedAspectTerminalCard({ session }))).toContain('data-fit="grid"');
    expect(String(FixedAspectTerminalCard({ session, fit: 'aspect' }))).toContain('data-fit="aspect"');
    expect(String(FixedAspectTerminalCard({ session, mode: 'magnified', fit: 'aspect' }))).toMatch(
      /data-fixed-aspect-terminal-card="magnified" data-fit="aspect"/,
    );
    // The tile's tokens and preview styling hang off its own root, not a .terminal-dashboard ancestor.
    expect(css).toMatchSource(
      /\.terminal-tile \{ --terminal-tile-frame-inset: var\(--wa-space-s\); --terminal-tile-footer-height: remify\(43\.2px\);/,
    );
    expect(css).not.toMatch(/\.terminal-dashboard \{[^}]*--terminal-tile-/);
    expect(css).not.toContain('.terminal-dashboard .terminal-tile');
    expect(css).not.toContain('.terminal-dashboard .workspace-chat-tile');
    expect(css).toMatchSource(/\.terminal-tile\[data-fit='aspect'\] \{ width: 100%; height: auto; \}/);
    expect(css).toMatchSource(
      /\.terminal-tile\[data-fit='aspect'\] \.terminal-tile__viewport-frame \{[^}]*aspect-ratio: 5 \/ 3;/,
    );
    const demoCss = readFileSync(new URL('../ux-demo/style.css', import.meta.url), 'utf8');
    expect(demoCss).not.toMatch(/\.fixed-aspect-terminal-card-demo[^{]*\.terminal-/);
  });
});

describe('halted AI sessions on dashboard tiles (HS2-HJ4D1H)', () => {
  it('projects connection transitions beside activity, with halt precedence (HS2-8WEY8X)', () => {
    const session = {
      id: 't1',
      projectId: 'p',
      projectName: 'P',
      alive: true,
      busy: true,
      scrollback: '',
      tool: 'codex',
    };
    for (const aiConnection of ['connected', 'missing', 'mcp_missing'] as const) {
      const markup = String(FixedAspectTerminalCard({ session: { ...session, aiConnection } }));
      expect(markup).toContain(`data-ai-connection="${aiConnection}"`);
      expect(markup).toContain(`data-lucide="${aiConnection === 'connected' ? 'plug' : 'unplug'}"`);
      expect(markup).toMatch(
        /class="terminal-tile__state" title="Busy"><span class="kui-badge"[^>]*data-tone="success"[^>]*aria-label="Busy"/,
      );
      expect(markup).toContain(
        aiConnection === 'connected'
          ? 'its permission prompts come to the app'
          : aiConnection === 'missing'
            ? 'Run /hooks in Codex'
            : 'MCP not connected',
      );
      const halted = String(
        FixedAspectTerminalCard({
          session: {
            ...session,
            aiConnection,
            halt: { message: 'Rate limit reached.', at: '2026-10-05T08:00:00Z', error_type: 'rate_limit' },
          },
        }),
      );
      expect(halted).toContain('Stopped: Rate limit reached.');
      expect(halted).not.toContain('data-ai-connection');
      expect(halted).not.toContain('terminal-tile__state');
    }
    expect(String(FixedAspectTerminalCard({ session }))).not.toContain('data-ai-connection');
  });

  it('replaces the state dot with the halt warning while the session is halted', () => {
    const session = { id: 't1', projectId: 'p', projectName: 'P', alive: true, busy: false, scrollback: '' };
    const halted = String(
      FixedAspectTerminalCard({
        session: {
          ...session,
          halt: { error_type: 'rate_limit', message: 'Rate limit reached.', at: '2026-10-05T08:00:00Z' },
        },
      }),
    );
    expect(halted).toContain('data-halted="true"');
    expect(halted).toContain('class="terminal-tile__halt"');
    expect(halted).toContain('title="Stopped: Rate limit reached."');
    expect(halted).not.toContain('terminal-tile__state');
    const running = String(FixedAspectTerminalCard({ session }));
    expect(running).not.toContain('data-halted');
    expect(running).toContain('terminal-tile__state');
  });
});
