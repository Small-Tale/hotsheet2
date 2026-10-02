import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

const read = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');

describe('global stylesheet ownership', () => {
  it('keeps extracted surface selectors in their component stylesheets', () => {
    const global = read('./style.css');
    const ownership = [
      ['project dialog', 'project-dialog.css', ['data-project-dialog', '.project-dialog', '.remote-project-dialog']],
      [
        'ticket-source setup',
        'ticket-source-setup-dialog.css',
        ['data-ticket-source-setup-dialog', '.ticket-source-setup'],
      ],
      ['provider setup', 'provider-setup-form.css', ['.provider-setup-form']],
      ['ticket-source settings', 'ticket-sources-settings.css', ['.ticket-provider-settings']],
      ['settings workspace', 'settings-workspace.css', ['.project-settings']],
      ['notification inspector', 'notification-inspector.css', ['.notification-inspector-empty']],
      // AppEmptyState owns its full-surface message presentation (HS2-EWYDH7).
      ['app empty state', 'app-empty-state.css', ['.app-empty']],
    ] as const;
    for (const [name, file, selectors] of ownership) {
      const owned = read(`./components/${file}`);
      for (const selector of selectors) {
        expect(global, `${name} leaked ${selector} into style.css`).not.toContain(selector);
        expect(owned, `${name} does not own ${selector}`).toContain(selector);
      }
    }
  });

  it('keeps the intentionally global app-shell rules in style.css', () => {
    const global = read('./style.css');
    for (const selector of [':root', 'html, body, #app', '.app-loading, .app-toast', '.ticket-page-more'])
      expect(global).toContainSource(selector);
    // The application root's edge-to-edge shell is AppShell's own `viewport` presentation (HS2-DR549A).
    expect(global).not.toContain('.app-shell');
  });
});
