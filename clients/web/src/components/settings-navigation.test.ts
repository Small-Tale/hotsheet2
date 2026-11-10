import { describe, expect, it } from 'vitest';

import { settingsCategoryTitle, SettingsNavigation } from './settings-navigation';

describe('SettingsNavigation', () => {
  it('renders every available category and exposes the current one', () => {
    const markup = String(SettingsNavigation({ selected: 'permissions', collapseControl: true }));
    expect(markup).toContain('aria-label="Settings categories"');
    expect(markup).toContain('data-component="pane"');
    // Pane content, both category lists, and the Project Settings command list (HS2-C7GMQN).
    expect(markup.match(/data-component="list"/g)).toHaveLength(4);
    expect(markup).toContain('<nav aria-label="Project Settings">');
    expect(markup).toContain('<nav aria-label="App Settings">');
    expect(markup).not.toContain('divider-sides');
    expect(markup).toContain('data-item-id="sources"');
    expect(markup).toContain('data-item-id="ai"');
    expect(markup).toContain('data-item-id="development-strategy"');
    expect(markup).toContain('data-item-id="guidance"');
    expect(markup).toContain('data-item-id="commands"');
    expect(markup).toContain('data-item-id="lifecycle"');
    expect(markup).toContain('data-item-id="terminals"');
    expect(markup).toMatch(/data-item-id="permissions"[^>]*aria-current="page"/);
    expect(markup).toContain('data-item-id="columns"');
    expect(markup).toContain('aria-label="Hide Settings Sidebar"');
    for (const icon of [
      'database',
      'bot',
      'git-branch',
      'book-open',
      'terminal-square',
      'archive-restore',
      'shield-check',
      'columns-3',
    ])
      expect(markup).toContain(`data-lucide="${icon}"`);
    // Project-scoped categories and the app-scoped Keyboard item live under distinct headings (HS2-QT6PGR).
    expect(markup).toContain('class="kui-text" data-component="text"');
    expect(markup).toContain('data-font="default" data-border="none">Project Settings</h2>');
    expect(markup).toContain('data-font="default" data-border="none">App Settings</h2>');
    expect(markup).toContain('data-item-id="general"');
    expect(markup).toContain('data-item-id="keyboard"');
    expect(markup).toContain('data-lucide="keyboard"');
    // The keyboard heading precedes the project one? No — project first, then app.
    expect(markup.indexOf('Project Settings')).toBeLessThan(markup.indexOf('App Settings'));
    expect(markup.indexOf('data-item-id="columns"')).toBeLessThan(markup.indexOf('data-item-id="general"'));
    expect(markup.indexOf('data-item-id="general"')).toBeLessThan(markup.indexOf('data-item-id="keyboard"'));
  });

  it('offers Close Project… under Project Settings, outside the category landmark (HS2-C7GMQN)', () => {
    const markup = String(SettingsNavigation({ selected: 'sources' })),
      close = markup.indexOf('data-action="close-project-from-settings"');
    expect(close).toBeGreaterThan(-1);
    expect(markup.match(/data-action="close-project-from-settings"/g)).toHaveLength(1);
    expect(markup).toContain('Close Project…');
    expect(markup.slice(close)).toContain('data-lucide="x"');
    // After the Project Settings navigation closes, before the App Settings heading.
    expect(close).toBeGreaterThan(markup.indexOf('</nav>'));
    expect(close).toBeLessThan(markup.indexOf('App Settings'));
  });

  it('uses the same category labels for navigation and workspace headings', () => {
    expect(settingsCategoryTitle('sources')).toBe('Ticket sources');
    expect(settingsCategoryTitle('ai')).toBe('AI tools');
    expect(settingsCategoryTitle('development-strategy')).toBe('Development strategy');
    expect(settingsCategoryTitle('guidance')).toBe('Project guidance');
    expect(settingsCategoryTitle('commands')).toBe('Commands');
    expect(settingsCategoryTitle('lifecycle')).toBe('Lifecycle');
    expect(settingsCategoryTitle('terminals')).toBe('Terminals');
    expect(settingsCategoryTitle('permissions')).toBe('Permissions');
    expect(settingsCategoryTitle('columns')).toBe('Column view');
    expect(settingsCategoryTitle('general')).toBe('General');
    expect(settingsCategoryTitle('keyboard')).toBe('Keyboard Shortcuts');
  });
});
