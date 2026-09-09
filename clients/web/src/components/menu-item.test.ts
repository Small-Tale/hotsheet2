import { readFileSync } from 'node:fs';

import { describe,expect,it } from 'vitest';

import { MenuItem } from './menu-item';

describe('MenuItem',()=>{
  it('makes multiline rows intrinsically tall enough for wrapped content',()=>{
    const markup=String(MenuItem({action:'open',accessibleLabel:'Open item',multiline:true,icon:'icon' as never,label:'A long path that wraps'}));
    expect(markup).toContain('data-multiline="true"');
    const css=readFileSync(new URL('./menu-item.css',import.meta.url),'utf8');
    expect(css).toContain('.menu-item[data-multiline="true"] { height: fit-content; white-space: normal; }');
    expect(css).toContain('.menu-item[data-multiline="true"] .menu-item__label { overflow: visible; white-space: normal; }');
  });
});
