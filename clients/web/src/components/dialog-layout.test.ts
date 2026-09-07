import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe,expect,it } from 'vitest';

import { DialogHeader,ValueTable } from './dialog-layout';

describe('dialog layout primitives',()=>{
  it('standardizes accessible title, summary, action, and value geometry',()=>{
    const header=String(DialogHeader({title:'Details',titleId:'details-title',summary:'Current state',summaryId:'details-summary',icon:'icon' as never,actions:'actions' as never}));
    expect(header).toContain('data-component="dialog-header"');
    expect(header).toContain('<h2 id="details-title">Details</h2>');
    expect(header).toContain('<p id="details-summary">Current state</p>');
    expect(header).toContain('dialog-header__icon');
    expect(header).toContain('dialog-header__actions');
    const table=String(ValueTable({label:'Build metadata',children:'<div><dt>Version</dt><dd>1</dd></div>' as never}));
    expect(table).toContain('data-component="value-table"');
    expect(table).toContain('aria-label="Build metadata"');
    const css=readFileSync(resolve(import.meta.dirname,'dialog-layout.css'),'utf8');
    expect(css).toMatch(/\.dialog-header \{[^}]*border-bottom:/);
    expect(css).toMatch(/\.value-table \{[^}]*background:/);
    expect(css).not.toMatch(/\.value-table \{[^}]*border:/);
    expect(css).toMatch(/\.value-table > div \+ div::before \{[^}]*right: var\(--wa-space-m\);[^}]*left: var\(--wa-space-m\);/);
  });
});
