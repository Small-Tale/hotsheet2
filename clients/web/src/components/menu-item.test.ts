import { describe,expect,it } from 'vitest';

import { MenuItem } from './menu-item';

describe('MenuItem',()=>{
  it('makes multiline rows intrinsically tall enough for wrapped content',()=>{
    const markup=String(MenuItem({action:'open',accessibleLabel:'Open item',multiline:true,icon:'icon' as never,label:'A long path that wraps'}));
    expect(markup).toContain('data-multiline="true"');
    expect(markup).toContain('class="kui-menu-item"');
    expect(markup).toContain('class="kui-menu-item__label"');
  });
});
