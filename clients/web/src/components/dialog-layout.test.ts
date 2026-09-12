import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { DialogHeader } from '@kerfjs/ui/dialog-header';
import { ValueTable } from '@kerfjs/ui/value-table';
import { describe,expect,it } from 'vitest';

describe('dialog layout primitives',()=>{
  it('standardizes accessible title, summary, action, and value geometry',()=>{
    const header=String(DialogHeader({title:'Details',titleId:'details-title',summary:'Current state',summaryId:'details-summary',icon:'icon' as never,actions:'actions' as never}));
    expect(header).toContain('data-component="dialog-header"');
    expect(header).toContain('<h2 id="details-title">Details</h2>');
    expect(header).toContain('<p id="details-summary">Current state</p>');
    expect(header).toContain('kui-dialog-header__icon');
    expect(header).toContain('kui-dialog-header__actions');
    const table=String(ValueTable({label:'Build metadata',children:'<div><dt>Version</dt><dd>1</dd></div>' as never}));
    expect(table).toContain('data-component="value-table"');
    expect(table).toContain('aria-label="Build metadata"');
    const css=readFileSync(resolve(import.meta.dirname,'dialog-layout.css'),'utf8');
    expect(css).toMatch(/\.dialog-surface \.kui-dialog-header \{ border-bottom: 0; \}/);
  });
});
