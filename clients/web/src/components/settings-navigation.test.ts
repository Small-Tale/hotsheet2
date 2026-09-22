import { describe, expect, it } from 'vitest';

import { settingsCategoryTitle, SettingsNavigation } from './settings-navigation';

describe('SettingsNavigation', () => {
  it('renders every available category and exposes the current one', () => {
    const markup = String(SettingsNavigation({ selected: 'permissions', collapseControl: true }));
    expect(markup).toContain('aria-label="Settings categories"');
    expect(markup).toContain('data-component="pane"');
    expect(markup.match(/data-component="list"/g)).toHaveLength(2);
    expect(markup).toContain('<nav aria-label="Project Settings">');
    expect(markup).toContain('<nav aria-label="App Settings">');
    expect(markup).not.toContain('divider-sides');
    expect(markup).toContain('data-item-id="sources"');
    expect(markup).toContain('data-item-id="ai"');
    expect(markup).toContain('data-item-id="commands"');
    expect(markup).toContain('data-item-id="lifecycle"');
    expect(markup).toContain('data-item-id="terminals"');
    expect(markup).toMatch(/data-item-id="permissions"[^>]*aria-current="page"/);
    expect(markup).toContain('data-item-id="columns"');
    expect(markup).toContain('aria-label="Hide settings sidebar"');
    for (const icon of ['database', 'bot', 'terminal-square', 'archive-restore', 'shield-check', 'columns-3'])
      expect(markup).toContain(`data-lucide="${icon}"`);
    // Project-scoped categories and the app-scoped Keyboard item live under distinct headings (HS2-QT6PGR).
    expect(markup).toContain('class="kui-list-header__label">Project Settings</h2>');
    expect(markup).toContain('class="kui-list-header__label">App Settings</h2>');
    expect(markup).toContain('data-item-id="general"');
    expect(markup).toContain('data-item-id="keyboard"');
    expect(markup).toContain('data-lucide="keyboard"');
    // The keyboard heading precedes the project one? No — project first, then app.
    expect(markup.indexOf('Project Settings')).toBeLessThan(markup.indexOf('App Settings'));
    expect(markup.indexOf('data-item-id="columns"')).toBeLessThan(markup.indexOf('data-item-id="general"'));
    expect(markup.indexOf('data-item-id="general"')).toBeLessThan(markup.indexOf('data-item-id="keyboard"'));
  });

  it('uses the same category labels for navigation and workspace headings', () => {
    expect(settingsCategoryTitle('sources')).toBe('Ticket sources');
    expect(settingsCategoryTitle('ai')).toBe('AI tools');
    expect(settingsCategoryTitle('commands')).toBe('Commands');
    expect(settingsCategoryTitle('lifecycle')).toBe('Lifecycle');
    expect(settingsCategoryTitle('terminals')).toBe('Terminals');
    expect(settingsCategoryTitle('permissions')).toBe('Permissions');
    expect(settingsCategoryTitle('columns')).toBe('Column view');
    expect(settingsCategoryTitle('general')).toBe('General');
    expect(settingsCategoryTitle('keyboard')).toBe('Keyboard shortcuts');
  });
});
