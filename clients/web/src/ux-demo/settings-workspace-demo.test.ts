import { describe, expect, it } from 'vitest';

import {
  resetSettingsWorkspaceDemo,
  SETTINGS_WORKSPACE_DEMO_CATEGORIES,
  SettingsWorkspaceSettings,
  settingsWorkspaceSettings,
} from './settings-workspace-demo';

describe('SettingsWorkspace demo settings (HS2-PS9BQV)', () => {
  it('offers every settings category, the permission automation states, and a reset', () => {
    expect(SETTINGS_WORKSPACE_DEMO_CATEGORIES).toEqual([
      'sources',
      'ai',
      'commands',
      'lifecycle',
      'terminals',
      'permissions',
      'columns',
      'general',
      'accounts',
      'keyboard',
    ]);
    const markup = String(SettingsWorkspaceSettings());
    expect(markup).toContain('name="workspace-category"');
    expect(markup).toContain('name="workspace-permission-action"');
    expect(markup).toContain('data-action="reset-settings"');
  });

  it('resets state and the live controls to the sources category with automation off', () => {
    settingsWorkspaceSettings.category.value = 'permissions';
    settingsWorkspaceSettings.permissionAction.value = 'deny';
    const controls: Record<string, { value: string; checked: boolean }> = {
      'workspace-category': { value: 'permissions', checked: false },
      'workspace-permission-action': { value: 'deny', checked: false },
    };
    const root = {
      querySelector: (selector: string) => {
        const match = /\[name="([^"]+)"\]/.exec(selector);
        return match ? (controls[match[1]] ?? null) : null;
      },
    } as unknown as ParentNode;
    resetSettingsWorkspaceDemo(root);
    expect(settingsWorkspaceSettings.category.value).toBe('sources');
    expect(settingsWorkspaceSettings.permissionAction.value).toBe('off');
    expect(controls['workspace-category'].value).toBe('sources');
    expect(controls['workspace-permission-action'].value).toBe('off');
  });
});
