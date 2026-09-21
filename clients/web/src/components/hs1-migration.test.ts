import {readFileSync} from 'node:fs';

import {describe,expect,it} from 'vitest';

import {Hs1CleanupBanner,Hs1MigrationBanner,Hs1MigrationDialog} from './hs1-migration';

describe('HS1 migration presentation',()=>{
  it('uses canonical dialog, banner, and connected-copy spacing',()=>{
    const css=readFileSync(new URL('./hs1-migration.css',import.meta.url),'utf8');
    expect(css).not.toContain('--wa-space-');
    expect(css).toMatch(/\.hs1-migration-dialog \{[^}]*gap:var\(--kui-space-l\)/);
    expect(css).toMatch(/__intro \{[^}]*gap:var\(--kui-space-m\)/);
    expect(css).toMatch(/__destination \{[^}]*gap:var\(--kui-space-xs\)/);
    expect(css).toMatch(/\.hs1-cleanup-banner,\.hs1-migration-banner \{[^}]*padding:var\(--kui-space-xs\) var\(--kui-space-m\)[^}]*gap:var\(--kui-space-m\)/);
    expect(css).toMatch(/__copy,\.hs1-migration-banner > div \{[^}]*gap:var\(--kui-space-2xs\)/);
  });

  it('asks only where to store tickets and describes the complete import',()=>{
    const markup=String(Hs1MigrationDialog({projectName:'Demo',projectRoot:'/work/demo',sourcePath:'/work/demo/.hotsheet',databasePath:'/work/demo/.hotsheet/db',postgresVersion:'17',defaultStore:'/work/demo.hs2',open:true,busy:false}));
    expect(markup).toContain('Hot Sheet 1 data found in Demo');
    expect(markup).toContain('tickets, notes, attachments, settings, and AI-tool setup');
    expect(markup).toContain('name="hs1-ticket-store"');
    expect(markup).toContain('/work/demo/.hotsheet/db');
    expect(markup).toContain('PostgreSQL');
    expect(markup).not.toContain('provider');
  });
  it('keeps a dismissed import available from a non-modal source banner',()=>{const markup=String(Hs1MigrationBanner({databasePath:'/work/demo/.hotsheet/db'}));expect(markup).toContain('/work/demo/.hotsheet/db');expect(markup).toContain('data-action="open-hs1-migration"')});
  it('offers cleanup or dismissal as explicit user actions',()=>{
    const markup=String(Hs1CleanupBanner());
    expect(markup).toContain('safely backed up');
    expect(markup).toContain('data-action="remove-hs1-data"');
    expect(markup).toContain('data-action="dismiss-hs1-cleanup"');
  });
});
