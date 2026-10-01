import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import type { SettingsWorkspaceProps } from './settings-workspace';
import { SettingsWorkspace } from './settings-workspace';

const props: SettingsWorkspaceProps = {
  category: 'general',
  sources: { sources: [] },
  connections: { connections: [] },
  ai: { tools: [], selection: { tool: 'codex' }, loading: false, message: '' },
  commands: { commands: [] },
  lifecycle: { days: 30, message: '' },
  terminals: { inheritGlobalShellHistory: false, message: '' },
  permissions: { automation: { action: 'off', delayMs: 60_000 }, delays: [15_000, 60_000] },
  columns: { hideVerified: false },
  general: { showLoadingActivity: true },
  keyboard: { overrides: {}, apple: true },
};

describe('SettingsWorkspace', () => {
  it('renders the selected category with its stable delegated action', () => {
    const general = String(SettingsWorkspace(props));
    expect(general).toContain('data-component="settings-workspace"');
    expect(general).toContain('aria-label="General settings"');
    expect(general).toContain('data-action="toggle-loading-activity"');
    const terminals = String(
      SettingsWorkspace({
        ...props,
        category: 'terminals',
        terminals: { inheritGlobalShellHistory: true, message: 'Saved locally.' },
      }),
    );
    expect(terminals).toContain('data-action="toggle-global-shell-history"');
    expect(terminals).toContain('Saved locally.');
  });

  it('renders permission transitions and source composition from typed inputs', () => {
    const permissions = String(
      SettingsWorkspace({
        ...props,
        category: 'permissions',
        permissions: { automation: { action: 'allow', delayMs: 60_000 }, delays: [15_000, 60_000, 120_000] },
      }),
    );
    expect(permissions).toContain('name="permission-automation-action"');
    expect(permissions).toContain('value="allow"');
    expect(permissions).toContain('2 minutes');
    // HS2-EBGCGW: immediate (0 s) is an Auto-allow-only choice; Auto-deny never offers it.
    const delays = [0, 15_000, 60_000],
      allowImmediately = String(
        SettingsWorkspace({
          ...props,
          category: 'permissions',
          permissions: { automation: { action: 'allow', delayMs: 0 }, delays },
        }),
      ),
      deny = String(
        SettingsWorkspace({
          ...props,
          category: 'permissions',
          permissions: { automation: { action: 'deny', delayMs: 15_000 }, delays },
        }),
      );
    expect(allowImmediately).toContain('0 seconds');
    expect(allowImmediately).toContain('without showing the popup');
    expect(deny).not.toContain('>0 seconds');
    expect(deny).toContain('15 seconds');
    const sources = String(
      SettingsWorkspace({
        ...props,
        category: 'sources',
        sources: {
          sources: [
            { connectionId: 'git', name: 'Hot Sheet git', provider: 'git', locator: '/work/demo.hs2', default: true },
          ],
        },
      }),
    );
    expect(sources).toContain('data-component="ticket-sources-settings"');
    expect(sources).toContain('/work/demo.hs2');
  });

  it('owns workspace layout styles outside the global stylesheet', () => {
    const global = readFileSync(new URL('../style.css', import.meta.url), 'utf8'),
      owned = readFileSync(new URL('./settings-workspace.css', import.meta.url), 'utf8');
    expect(global).not.toContain('.project-settings');
    expect(owned).toContain('.project-settings');
    expect(owned).toContain('.project-settings__permission-grid');
  });
});
