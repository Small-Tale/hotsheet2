import { describe, expect, it } from 'vitest';

import { allFeaturesOn, featureSettings, resetFeatureSettings } from '../feature-areas';
import {
  SYSTEMS_DEMO_MACHINES,
  systemsDemoInspector,
  systemsDemoSidebar,
  systemsDemoView,
} from '../ux-demo/systems-fixtures';
import { ProjectDashboardModes } from './project-strip-actions';
import { SystemsInspector, systemsInspectorPanel } from './systems-inspector';
import { SystemsSidebar, systemsSidebarPanel } from './systems-sidebar';
import { SystemsView } from './systems-view';
import { GlobalWorkspaceSurface } from './workspace-composition-surfaces';

const html = (value: unknown) => String(value);

describe('HS2-DR0ZSQ Systems strip button', () => {
  it('sits after Workspace Grid and before Cross-Project Stats with the server icon, pressed in Systems', () => {
    featureSettings.value = allFeaturesOn();
    const strip = html(ProjectDashboardModes({ mode: 'systems' }));
    const order = [...strip.matchAll(/data-shell-mode="([^"]+)"/gu)].map((match) => match[1]);
    expect(order).toEqual(['terminals', 'systems', 'stats']);
    expect(strip).toMatch(
      /data-shell-mode="systems" aria-label="Systems" aria-pressed="true"[^]*?data-lucide="server"/u,
    );
    expect(strip).toContain('aria-label="Cross-Project Stats" aria-pressed="false"');
    expect(html(ProjectDashboardModes({ mode: 'stats' }))).toContain('aria-label="Systems" aria-pressed="false"');
    resetFeatureSettings();
  });
});

describe('HS2-DR0ZSQ SystemsView', () => {
  it('shows this computer’s cards, AI tools, capabilities and toolchains', () => {
    const markup = html(SystemsView(systemsDemoView('local', 'ready')));
    expect(markup).toContain('data-component="systems-view" data-machine-id="local"');
    expect(markup).toMatch(/This Computer · macOS · aarch64/u);
    expect(markup).toContain('data-card="cpu"');
    expect(markup).toContain('data-component="cpu-core-meter"');
    expect(markup).toContain('data-systems-ai-tool="claude" data-installed="true" data-signed-in="true"');
    expect(markup).toContain('Signed in · 2 sessions running');
    // HS2-S1FEYY: each tool's sign-in badge, as the approved mock shows it.
    expect(markup).toMatch(/data-systems-ai-tool="claude"[^]*?>Signed In</u);
    expect(markup).toContain('data-systems-ai-tool="opencode" data-installed="true" data-signed-in="false"');
    expect(markup).toMatch(/data-systems-ai-tool="opencode"[^]*?>Signed Out</u);
    expect(markup).toContain('Example M-Series · 12 cores');
    expect(markup).toContain('22.1 GB free of 64.0 GB');
    expect(markup).toMatch(/>xcode<[^]*>17\.0</u);
    expect(markup).not.toContain('systems-capacity-stale');
  });

  it('names a remote as a paired remote and offers no disk choice for it', () => {
    const markup = html(SystemsView(systemsDemoView('build', 'ready')));
    expect(markup).toMatch(/Paired Remote · macOS/u);
    expect(markup).not.toContain('data-pending-disk');
    expect(markup).toContain('data-systems-ai-tool="opencode" data-installed="false" data-signed-in="unknown"');
    expect(markup).toMatch(/data-systems-ai-tool="opencode"[^]*?>Not Installed</u);
    expect(markup).toContain('Not installed on this machine');
    // A machine that cannot tell whether a tool is signed in shows it as Installed.
    expect(markup).toContain('data-systems-ai-tool="codex" data-installed="true" data-signed-in="unknown"');
    expect(markup).toMatch(/data-systems-ai-tool="codex"[^]*?>Installed</u);
  });

  it('says how to connect a machine without readings', () => {
    const offline = html(SystemsView(systemsDemoView('offline', 'ready')));
    expect(offline).toContain('Travel Laptop Is Not Connected');
    expect(offline).not.toContain('data-component="stats-this-mac"');
    expect(html(SystemsView(systemsDemoView('revoked', 'ready')))).toContain('Lab Box Revoked This Computer');
  });

  it('shows loading, a failed first read, and stale readings after a failure', () => {
    expect(html(SystemsView(systemsDemoView('local', 'loading')))).toContain('Loading…');
    const failed = html(SystemsView(systemsDemoView('local', 'failed')));
    expect(failed).toContain('AI tool information is not available. The Hot Sheet server could not be reached.');
    expect(failed).toContain('Toolchain information is not available.');
    const stale = html(SystemsView(systemsDemoView('local', 'stale')));
    expect(stale).toContain('data-key="systems-capacity-stale"');
    expect(stale).toContain('data-systems-ai-tool="claude"');
  });

  it('says when no declared toolchain is installed, and when the machine does not report its tools', () => {
    const view = systemsDemoView('local', 'ready');
    const capacity = view.capacity.capacity!;
    const none = html(
      SystemsView({
        ...view,
        capacity: {
          capacity: {
            ...capacity,
            tools: [],
            capabilities: { ...capacity.capabilities!, ai_tools: [], toolchains: [{ name: 'go', installed: false }] },
          },
        },
      }),
    );
    expect(none).toContain('None of the declared toolchains is installed.');
    expect(none).toContain('AI tool information is not reported.');
  });

  it('is the global workspace for the systems kind', () => {
    const markup = html(GlobalWorkspaceSurface({ kind: 'systems', systems: systemsDemoView('local', 'ready') }));
    expect(markup).toContain('data-component="systems-view"');
    expect(markup).toContain('aria-label="Systems: Studio"');
  });
});

describe('HS2-DR0ZSQ SystemsSidebar', () => {
  it('lists every machine with its kind, a CPU gauge or why it has none, and selects one', () => {
    const markup = html(SystemsSidebar(systemsDemoSidebar('build', 'ready')));
    const rows = [...markup.matchAll(/data-machine-id="([^"]+)"/gu)].map((match) => match[1]);
    expect(rows).toEqual(Object.values(SYSTEMS_DEMO_MACHINES).map((machine) => machine.id));
    expect(markup).toContain('This Computer · 3 AI sessions');
    expect(markup).toContain(`data-systems-cpu="local"`);
    expect(markup).toMatch(
      /data-tone="warning"[^>]*data-systems-cpu="remote:|data-systems-cpu="remote:[^>]*data-tone="warning"/u,
    );
    expect(markup).toContain('>Offline<');
    expect(markup).toContain('>Revoked<');
    expect(markup.match(/data-item-id="([^"]+)"[^>]*aria-current="page"/gu)).toEqual([
      'data-item-id="remote:01JSYNTHREMTE0000000000001" data-has-icon="true" data-has-description="true" data-density="default" data-divider="none" aria-current="page"',
    ]);
    expect(markup).toContain('data-action="select-systems-machine"');
  });

  it('shows AI Tool Capacity rows, and each other state', () => {
    const ready = html(SystemsSidebar(systemsDemoSidebar('local', 'ready')));
    expect(ready).toContain('Claude · Work');
    expect(ready).toContain('Up to 20 sessions · $14 of $40 today');
    expect(ready).toContain('>Limited<');
    const loading = html(SystemsSidebar(systemsDemoSidebar('local', 'ready', { status: 'loading' })));
    expect(loading).toContain('data-systems-tool-capacity="loading"');
    expect(html(SystemsSidebar(systemsDemoSidebar('local', 'ready', { status: 'none' })))).toContain(
      'No open orchestration project sets AI tool capacity.',
    );
    expect(
      html(SystemsSidebar(systemsDemoSidebar('local', 'ready', { status: 'error', message: 'refused' }))),
    ).toContain('AI tool capacity is not available. refused');
    expect(html(SystemsSidebar(systemsDemoSidebar('local', 'ready', { status: 'ready', rows: [] })))).toContain(
      'The orchestration policy configures no AI tools.',
    );
  });

  it('is a left-rail panel with the standard sidebar toggle', () => {
    const panel = systemsSidebarPanel(systemsDemoSidebar('local', 'ready'));
    expect(panel.toggle).toEqual({ action: 'toggle-project-sidebar', name: 'Systems Sidebar' });
    expect(html(panel.content)).toContain('data-component="systems-sidebar"');
  });
});

describe('HS2-DR0ZSQ SystemsInspector', () => {
  it('lists the machine’s AI sessions with whether each is working', () => {
    const markup = html(SystemsInspector(systemsDemoInspector('local', 'ready')));
    expect(markup).toContain('data-systems-session="t1"');
    expect(markup).toContain('Worker · Claude');
    expect(markup).toContain('>Working<');
    expect(markup).toContain('>Idle<');
  });

  it('says why there are no sessions to show', () => {
    expect(html(SystemsInspector(systemsDemoInspector('offline', 'ready')))).toContain(
      'Sessions show while the machine is connected.',
    );
    expect(html(SystemsInspector(systemsDemoInspector('local', 'loading')))).toContain('Loading…');
    expect(html(SystemsInspector(systemsDemoInspector('local', 'failed')))).toContain(
      'Sessions are not available. The Hot Sheet server could not be reached.',
    );
    expect(html(SystemsInspector({ machine: SYSTEMS_DEMO_MACHINES.local, sessions: [] }))).toContain(
      'No AI session is running here.',
    );
  });

  it('titles the right rail with the machine and its kind', () => {
    const panel = systemsInspectorPanel(systemsDemoInspector('build', 'ready'));
    expect(html(panel.toolbar.title)).toContain('Build Host');
    expect(html(panel.toolbar.title)).toContain('Paired Remote');
    expect(panel.toggle.action).toBe('toggle-ticket-inspector');
  });
});
