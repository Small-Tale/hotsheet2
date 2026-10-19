import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

const read = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');

describe('global stylesheet ownership', () => {
  it('keeps extracted surface selectors in their component stylesheets', () => {
    const global = read('./style.css');
    const ownership = [
      ['project dialog', 'project-dialog.css', ['.project-dialog']],
      [
        'ticket-source setup',
        'ticket-source-setup-dialog.css',
        ['data-ticket-source-setup-dialog', '.ticket-source-setup-dialog', '.ticket-source-setup'],
      ],
      ['provider setup', 'provider-setup-form.css', ['.provider-setup-form']],
      ['ticket-source Settings', 'ticket-sources-settings.css', ['.ticket-provider-settings']],
    ] as const;
    // TicketPageMore composes a pill `wa-button` with no stylesheet at all (HS2-TCM7WC).
    expect(global).not.toContain('.ticket-page-more');
    expect(global).not.toContain('.settings-workspace');
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
    expect(global).toContainSource(':root');
    // Kerf's opt-in viewport lock owns the dynamic-viewport clip (KF-1XNQYP, HS2-4TN9GK).
    expect(global).not.toContain('100dvh');
    expect(read('../index.html')).toContain('<main id="app" class="kui-app-root" data-viewport="locked">');
    expect(global).not.toContain('.app-toast');
    // The application root's edge-to-edge shell is AppShell's own `viewport` presentation (HS2-DR549A).
    expect(global).not.toContain('.app-shell');
  });
});
