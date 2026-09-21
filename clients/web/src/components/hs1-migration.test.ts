import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { Hs1CleanupBanner, Hs1MigrationBanner, Hs1MigrationDialog } from './hs1-migration';

describe('HS1 migration presentation', () => {
  it('uses canonical dialog spacing and StateBanner customization tokens', () => {
    const css = readFileSync(new URL('./hs1-migration.css', import.meta.url), 'utf8');
    expect(css).not.toContain('--wa-space-');
    expect(css).toMatchSource(/\.hs1-migration-dialog \{[^}]*gap:var\(--kui-space-l\)/);
    expect(css).toMatchSource(/__intro \{[^}]*gap:var\(--kui-space-m\)/);
    expect(css).toMatchSource(/__destination \{[^}]*gap:var\(--kui-space-xs\)/);
    expect(css).toMatchSource(
      /\.hs1-cleanup-banner,\.hs1-migration-banner \{[^}]*--kui-layout-item-gap:var\(--kui-space-m\)[^}]*--kui-layout-item-padding:var\(--kui-space-xs\) var\(--kui-space-m\)/,
    );
    expect(css).toMatchSource(/\.kui-state-banner__copy[^}]*gap:var\(--kui-space-2xs\)/);
    expect(css).not.toMatch(/\.hs1-cleanup-banner,\.hs1-migration-banner \{[^}]*display:grid/);
  });

  it('asks only where to store tickets and describes the complete import', () => {
    const markup = String(
      Hs1MigrationDialog({
        projectName: 'Demo',
        projectRoot: '/work/demo',
        sourcePath: '/work/demo/.hotsheet',
        databasePath: '/work/demo/.hotsheet/db',
        postgresVersion: '17',
        defaultStore: '/work/demo.hs2',
        open: true,
        busy: false,
      }),
    );
    expect(markup).toContain('Hot Sheet 1 data found in Demo');
    expect(markup).toContain('tickets, notes, attachments, settings, and AI-tool setup');
    expect(markup).toContain('name="hs1-ticket-store"');
    expect(markup).toContain('/work/demo/.hotsheet/db');
    expect(markup).toContain('PostgreSQL');
    expect(markup).not.toContain('provider');
  });
  it('keeps a dismissed import available from a polite informational StateBanner', () => {
    const markup = String(Hs1MigrationBanner({ databasePath: '/work/demo/.hotsheet/db' }));
    expect(markup).toContain('data-component="state-banner"');
    expect(markup).toContain('class="kui-state-banner hs1-migration-banner"');
    expect(markup).toContain('data-tone="info"');
    expect(markup).toContain('role="status"');
    expect(markup).toContain('aria-live="polite"');
    expect(markup).toContain('/work/demo/.hotsheet/db');
    expect(markup).toContain('data-action="open-hs1-migration"');
  });
  it('offers cleanup or dismissal from one StateBanner action slot', () => {
    const markup = String(Hs1CleanupBanner());
    expect(markup).toContain('data-component="state-banner"');
    expect(markup).toContain('class="kui-state-banner hs1-cleanup-banner"');
    expect(markup).toContain('data-tone="success"');
    expect(markup).toContain('safely backed up');
    expect(markup).toContain('class="kui-state-banner__action"><div class="hs1-cleanup-banner__actions"');
    expect(markup).toContain('data-action="remove-hs1-data"');
    expect(markup).toContain('data-action="dismiss-hs1-cleanup"');
  });
});
