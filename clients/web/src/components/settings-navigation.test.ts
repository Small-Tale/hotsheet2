import { describe, expect, it } from 'vitest';

import { settingsCategoryTitle,SettingsNavigation } from './settings-navigation';

describe('SettingsNavigation', () => {
  it('renders every available category and exposes the current one', () => {
    const markup = String(SettingsNavigation({ selected: 'permissions', collapseControl: true }));
    expect(markup).toContain('aria-label="Settings categories"');
    expect(markup).toContain('data-item-id="sources"');
    expect(markup).toContain('data-item-id="commands"');
    expect(markup).toContain('data-item-id="permissions" aria-current="page"');
    expect(markup).toContain('data-item-id="columns"');
    expect(markup).toContain('aria-label="Hide settings sidebar"');
    for (const icon of ['database', 'terminal-square', 'shield-check', 'columns-3']) expect(markup).toContain(`data-lucide="${icon}"`);
  });

  it('uses the same category labels for navigation and workspace headings', () => {
    expect(settingsCategoryTitle('sources')).toBe('Ticket sources');
    expect(settingsCategoryTitle('commands')).toBe('Commands');
    expect(settingsCategoryTitle('permissions')).toBe('Permissions');
    expect(settingsCategoryTitle('columns')).toBe('Column view');
  });
});
