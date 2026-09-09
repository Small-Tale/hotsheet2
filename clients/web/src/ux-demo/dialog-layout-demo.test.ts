import { beforeEach, describe, expect, it } from 'vitest';

import { closeHs1MigrationDialogDemo, Hs1MigrationDialogDemo, hs1MigrationDialogDemoOpen, openHs1MigrationDialogDemo } from './dialog-layout-demo';

describe('dialog UX demos',()=>{
  beforeEach(()=>{openHs1MigrationDialogDemo()});

  it('keeps the HS1 migration dialog explicitly reopenable',()=>{
    expect(hs1MigrationDialogDemoOpen.value).toBe(true);
    expect(String(Hs1MigrationDialogDemo())).not.toContain('data-action="open-hs1-migration-demo"');
    closeHs1MigrationDialogDemo();
    expect(hs1MigrationDialogDemoOpen.value).toBe(false);
    expect(String(Hs1MigrationDialogDemo())).toContain('data-action="open-hs1-migration-demo"');
    openHs1MigrationDialogDemo();
    expect(hs1MigrationDialogDemoOpen.value).toBe(true);
  });
});
