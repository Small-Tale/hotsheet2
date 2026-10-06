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
      // TicketPageMore owns the paged list/column continuation button (HS2-WP69TD).
      ['ticket page continuation', 'ticket-page-more.css', ['.ticket-page-more']],
      ['app loading indicator', 'app-loading-indicator.css', ['.app-loading']],
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
    expect(global).toContain("@import '@kerfjs/ui/document.css'");
    expect(global).not.toContain('font-family:');
    expect(global).not.toContain('box-sizing:');
    for (const selector of [':root', 'html, body, #app']) expect(global).toContainSource(selector);
    expect(global).not.toContain('.app-toast');
    // The application root's edge-to-edge shell is AppShell's own `viewport` presentation (HS2-DR549A).
    expect(global).not.toContain('.app-shell');
  });
});
