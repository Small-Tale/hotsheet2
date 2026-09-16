import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { PanelHeader } from '@kerfjs/ui/panel-header';
import { ValueTable } from '@kerfjs/ui/value-table';
import { describe,expect,it } from 'vitest';

describe('dialog layout primitives',()=>{
  it('standardizes accessible title, summary, action, and value geometry',()=>{
    const header=String(PanelHeader({title:'Details',titleId:'details-title',summary:'Current state',summaryId:'details-summary',icon:'icon' as never,actions:'actions' as never}));
    expect(header).toContain('data-component="panel-header"');
    expect(header).toContain('kui-panel-header__title');
    expect(header).toContain('id="details-title"');
    expect(header).toContain('>Details<');
    expect(header).toContain('<p class="kui-panel-header__summary" id="details-summary">Current state</p>');
    expect(header).toContain('kui-panel-header__icon');
    expect(header).toContain('<div class="kui-toolbar__trailing">actions</div>');
    const table=String(ValueTable({label:'Build metadata',children:'<div><dt>Version</dt><dd>1</dd></div>' as never}));
    expect(table).toContain('data-component="value-table"');
    expect(table).toContain('aria-label="Build metadata"');
    const css=readFileSync(resolve(import.meta.dirname,'native-popover-dialog.css'),'utf8');
    expect(css).toMatch(/\.dialog-surface \.kui-panel-header \{ border-bottom: 0; \}/);
  });
});
