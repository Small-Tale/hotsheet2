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
      'data-component="floating-toolbar" data-position="bottom-end" data-placement="floating" role="toolbar" aria-label="Workspace tile zoom"',
    );
    expect(markup).toContain('data-component="toolbar-control-group"');
    expect(markup).toContain('data-tone="default"');
    expect(markup).not.toContain('data-tone="dark"');
    expect(markup).toContain('data-action="zoom-terminal-grid"');
    for (const action of ['magnify-terminal', 'dedicate-terminal'])
      expect(markup).not.toContain(`data-action="${action}"`);
  });
  it('renders AI chats beside terminals with the same fixed-natural-size scaling contract', () => {
    const markup = String(TerminalDashboard({ groups, width: 1200, height: 700, fitAcross: 4, fitHigh: 2 }));
    expect(markup).toContain('aria-label="Workspace grid"');
    expect(markup).toContain('data-component="workspace-chat-tile"');
    expect(markup).toContain('data-chat-id="chat:review"');
    expect(markup).toContain('data-action="open-grid-ai-chat"');
    expect(markup).toContain('Claude AI chat');
    expect(markup).toContain('Reviewing the workspace changes');
    expect(markup).toContain('aria-label="Working" title="Working"');
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
    expect(grouped).toMatch(/<h2>Project One<span>2<\/span>/);
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
  });
  it('makes the footer identity actionable, explains terminal state, and omits the snapshot working directory', () => {
    const markup = String(TerminalDashboard({ groups, width: 1200, height: 700, fitAcross: 4, fitHigh: 2 }));
    expect(markup).toContain('aria-label="Open Codex in Project One"');
    expect(markup).toContain('aria-label="Busy" title="Busy"');
    expect(markup).not.toContain('/work/one');
  });
  it('reserves fixed 80x24 sizing for dashboard grid previews', () => {
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
    expect(magnified).toContain('data-mobile-grid-policy="80xm"');
    expect(magnified).not.toContain('data-grid-policy="dashboard-80x24"');
  });
  it('shows the phone text-size control without an inline column number (HS2-89JZSN)', () => {
    const session = groups[0].sessions[0],
      mobile = { viewport: { left: 0, top: 0, width: 360, height: 640 }, keyboardVisible: false, columns: 60 },
      markup = String(FixedAspectTerminalCard({ session, mode: 'magnified', mobile }));
    // The control stays present and accessible; a toast reports the change instead of an inline number.
    expect(markup).toContain('data-action="cycle-mobile-terminal-columns"');
    expect(markup).toContain('data-columns="60"');
    expect(markup).toContain('aria-label="Text size: 60 columns. Change text size"');
    expect(markup).not.toContain('aria-hidden="true">60<');
    // HS2-FRB545: the phone toolbar carries Copy and Paste beside text size.
    expect(markup).toContain('data-action="copy-terminal-text"');
    expect(markup).toContain('data-action="paste-terminal-text"');
    expect(String(FixedAspectTerminalCard({ session, mode: 'magnified' }))).not.toContain('copy-terminal-text');
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
    expect(magnified).toContain('Open Codex in project terminal drawer');
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
    expect(markup).toContain('data-component="terminal-context-menu"');
    expect(markup.match(/<wa-dropdown-item/g)).toHaveLength(2);
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
    expect(openOnly.match(/<wa-dropdown-item/g)).toHaveLength(1);
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
    ).not.toContain('data-component="terminal-context-menu"');
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
    expect(markup).not.toContain('visibility-toolbar');
    // The group Select is wrapped by the app; its root carries no app class (KUI-L022).
    expect(markup).toContain('<div class="terminal-dashboard-controls__visibility-select"><wa-select');
    expect(markup).toContain('>2</span>');
    expect(markup).toContain('name="terminal-visibility-group"');
    expect(markup).toContain(
      'class="kui-select__custom-selected"><span class="kui-select__custom-selected-content"><span>Focus</span>',
    );
    expect(markup).toContain('<wa-option value="focus"');
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
  it('styles only its own controls, configuring Kerf controls through props (HS2-DR549A)', () => {
    // No blanket button rule reaches into Kerf control groups (the zoom toolbar, the key bar).
    expect(css).not.toMatch(/\.terminal-dashboard button/);
    expect(css).not.toContain('.terminal-key-bar');
    expect(css).not.toContain('.app-shell');
    expect(css).not.toContain('.kui-');
    expect(css).not.toContain('::part(');
    // The one temporary rule on the app's own zoom buttons is tied to its Kerf fix (KF-FTADQT).
    expect(css).toContainSource(
      "/* Temporary until KF-FTADQT: ToolbarControlGroup's native-button rule keeps a pointer cursor on a",
    );
    expect(css).toContainSource('.terminal-dashboard__zoom button:disabled { cursor: not-allowed; }');
    expect(css).toMatchSource(
      /:is\( \.terminal-tile__identity, \.terminal-tile__menu, \.terminal-tile__open, \.terminal-tile__close, \.terminal-tile__text-size, \.terminal-tile__clipboard \) \{ border: 0; color: var\(--wa-color-neutral-on-quiet\); background: transparent; \}/,
    );
    const controls = String(
      TerminalVisibilityControls({ groups: [{ id: 'default', name: 'Default', hiddenKeys: [] }], activeId: 'default' }),
    );
    expect(controls).toContain('data-presentation="toolbar-borderless"');
    expect(controls).toContain('data-size="compact"');
    expect(controls).toContain('data-focus-ring-owner="group"');
  });
  it('clips a phone dedicated terminal through the session it renders (HS2-DR549A)', () => {
    const session = { id: 't1', projectId: 'p', projectName: 'P', alive: true, busy: false, scrollback: '' };
    expect(String(TerminalSession({ session }))).toContain(
      'class="terminal-session" data-key="p:t1" data-mobile="false"',
    );
    expect(String(TerminalSession({ session, mobile: true }))).toContain('data-mobile="true"');
    const drawerCss = readFileSync(new URL('./terminal-drawer.css', import.meta.url), 'utf8');
    expect(drawerCss).toContainSource(".terminal-session[data-mobile='true'] .terminal-viewport { overflow: clip; }");
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
});
