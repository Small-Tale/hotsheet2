import { ListItem } from '@kerfjs/ui/list-item';
import { describe,expect,it } from 'vitest';

describe('ListItem',()=>{
  it('makes multiline rows intrinsically tall enough for wrapped content',()=>{
    const markup=String(ListItem({action:'open',accessibleLabel:'Open item',multiline:true,icon:'icon' as never,label:'A long path that wraps'}));
    expect(markup).toContain('data-multiline="true"');
    expect(markup).toContain('class="kui-list-item"');
    expect(markup).toContain('class="kui-list-item__label"');
  });
});
