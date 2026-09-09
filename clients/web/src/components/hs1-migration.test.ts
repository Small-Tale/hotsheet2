import {describe,expect,it} from 'vitest';

import {Hs1CleanupBanner,Hs1MigrationBanner,Hs1MigrationDialog} from './hs1-migration';

describe('HS1 migration presentation',()=>{
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
  it('offers cleanup only as an explicit user action',()=>{
    const markup=String(Hs1CleanupBanner());
    expect(markup).toContain('safely backed up');
    expect(markup).toContain('data-action="remove-hs1-data"');
  });
});
