import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { ProjectTabBar } from './project-tab-bar';

describe('ProjectTabBar actions (HS2-402AXQ, HS2-PNCDAE)', () => {
  it('renders a drag target from tab state so a later render keeps it (HS2-6KNWYT)', () => {
    const tab = (dropTarget?: 'ticket' | 'file') =>
      String(ProjectTabBar({ tabs: [{ id: 'other', name: 'other', location: 'local', dropTarget }] }));
    expect(tab('file')).toMatch(/data-drop-target="true"[^>]*/);
    expect(tab('file')).toContain('data-dragging-file="true"');
    expect(tab('file')).not.toContain('data-dragging-ticket');
    expect(tab('ticket')).toMatch(/data-drop-target="true"/);
    expect(tab('ticket')).toContain('data-dragging-ticket="true"');
    expect(tab('ticket')).not.toContain('data-dragging-file');
    expect(tab()).not.toMatch(/data-drop-target|data-dragging-/);
  });

  it('leaves the far-edge zone to ticket actions after the announcer moves to the sidebar (HS2-6PZ35D)', () => {
    const tabs = [{ id: 'demo', name: 'demo', location: 'local' as const, selected: true }];
    const desktop = String(ProjectTabBar({ tabs }));
    const phone = String(ProjectTabBar({ tabs, mobile: true }));
    expect(desktop).not.toContain('kui-tab-bar__end');
    expect(desktop).not.toContain('data-announcer-action');
    expect(phone).not.toContain('data-announcer-action');
    expect(String(ProjectTabBar({ tabs, workspaceAction: { kind: 'new-ticket' } }))).toContain('kui-tab-bar__end');
  });

  it('keeps the selected project tab active for project Stats without selecting global Stats (HS2-JK3DXP)', () => {
    const tabs = [{ id: 'demo', name: 'demo', location: 'local' as const, selected: true }];
    const projectStats = String(ProjectTabBar({ tabs, mode: 'stats', projectStats: true }));
    expect(projectStats).toContain('aria-label="Cross-Project Stats" aria-pressed="false"');
    expect(projectStats).toMatch(/role="tab"[^>]*aria-selected="true"/);

    const globalStats = String(ProjectTabBar({ tabs, mode: 'stats' }));
    expect(globalStats).toContain('aria-label="Cross-Project Stats" aria-pressed="true"');
    expect(globalStats).not.toMatch(/role="tab"[^>]*aria-selected="true"/);
  });

  it('renders Add project as a native button in a borderless Kerf control group the strip never styles', () => {
    const markup = String(ProjectTabBar({ tabs: [{ id: 'demo', name: 'demo', location: 'local', selected: true }] }));
    expect(markup).toMatch(
      /<div class="kui-toolbar-control-group" data-component="toolbar-control-group" data-appearance="borderless"[^>]*data-single="true"[^>]*data-size="compact"[^>]*data-content="icon"[^>]*><button type="button" data-action="choose-project" aria-label="Add Project">/,
    );
    expect(markup).not.toContain('<wa-button');
    const css = readFileSync(new URL('./project-tab-bar.css', import.meta.url), 'utf8');
    expect(css).not.toMatch(/wa-button|wa-dropdown/);
    // The strip owns no control chrome: the groups' buttons are Kerf-styled (HS2-PNCDAE).
    expect(css).not.toMatch(/project-tab-bar__(?:action|actions|modes)\b/);
    expect(css).not.toMatch(/(?:^|[\s,])button\b/m);
  });

  it('renders the phone strip as a Kerf Toolbar of control groups with the same actions', () => {
    const markup = String(
      ProjectTabBar({
        mobile: true,
        mode: 'stats',
        tabs: [
          { id: 'demo', name: 'demo', location: 'local', selected: true },
          { id: 'opening', name: 'opening', location: 'local', pending: true },
        ],
      }),
    );
    expect(markup).toContain('data-component="toolbar"');
    expect(markup).toMatch(
      /kui-toolbar__leading"><div class="kui-toolbar-control-group"[^>]*aria-label="Global Dashboards"[^]*data-shell-mode="stats" aria-label="Cross-Project Stats" aria-pressed="true"[^]*name="mobile-project"[^]*data-action="choose-project" aria-label="Add Project"/,
    );
    // Still-opening projects are selectable choices labeled as opening (HS2-XGSSK1).
    expect(markup).toContain('value="opening"');
    expect(markup).toContain('opening — Opening…');
    const empty = String(
      ProjectTabBar({ mobile: true, tabs: [{ id: 'opening', name: 'opening', location: 'local', pending: true }] }),
    );
    expect(empty).toContain('mobile-project');
    expect(empty).toContain('data-action="choose-project"');
  });

  it('projects attention into phone options and the selected value, then removes it on recovery (HS2-34VG07)', () => {
    const tabs = [
      { id: 'healthy', name: 'Healthy', location: 'local' as const },
      {
        id: 'halted',
        name: 'Halted',
        location: 'local' as const,
        attention: true,
        attentionLabel: 'An AI session stopped on an error',
      },
    ];
    const inactive = String(
      ProjectTabBar({ mobile: true, tabs: tabs.map((tab) => ({ ...tab, selected: tab.id === 'healthy' })) }),
    );
    expect(inactive).toContain('Halted — Needs attention');
    expect(inactive.match(/data-lucide="circle-alert"/g)).toHaveLength(1);
    const selected = String(
      ProjectTabBar({ mobile: true, tabs: tabs.map((tab) => ({ ...tab, selected: tab.id === 'halted' })) }),
    );
    expect(selected.match(/data-lucide="circle-alert"/g)).toHaveLength(2);
    expect(selected).toContain('aria-label="An AI session stopped on an error"');
    expect(selected).toContain('var(--kui-color-danger-on-quiet)');
    const recovered = String(
      ProjectTabBar({
        mobile: true,
        tabs: tabs.map((tab) => ({ ...tab, attention: false, selected: tab.id === 'halted' })),
      }),
    );
    expect(recovered).not.toContain('circle-alert');
    expect(recovered).not.toContain('stopped on an error');
  });

  it('HS2-7V2EEV closes the selected mobile project, including a failed remembered root, but not a pending root', () => {
    const failed = String(
      ProjectTabBar({
        mobile: true,
        tabs: [
          { id: 'healthy', name: 'Healthy', location: 'local' },
          {
            id: 'project-restore-error:/work/missing',
            name: 'Missing',
            location: 'local',
            selected: true,
            restoreFailure: true,
            closable: false,
          },
        ],
      }),
    );
    expect(failed).toContain('data-action="close-project-tab"');
    expect(failed).toContain('data-project-id="project-restore-error:/work/missing"');
    expect(failed).toContain('aria-label="Close Missing Project"');
    expect(failed).toContain('data-lucide="x"');
    const healthy = String(
      ProjectTabBar({ mobile: true, tabs: [{ id: 'healthy', name: 'Healthy', location: 'local', selected: true }] }),
    );
    expect(healthy).toContain('data-project-id="healthy"');
    expect(healthy).toContain('aria-label="Close Healthy Project"');
    const pending = String(
      ProjectTabBar({
        mobile: true,
        tabs: [{ id: 'pending:/work/missing', name: 'Missing', location: 'local', pending: true }],
      }),
    );
    expect(pending).not.toContain('data-action="close-project-tab"');
  });
});
