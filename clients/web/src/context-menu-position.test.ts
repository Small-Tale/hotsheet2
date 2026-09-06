import {describe,expect,it} from 'vitest';

import {viewportSafeContextMenuPosition} from './context-menu-position';

describe('viewportSafeContextMenuPosition',()=>{
  it('keeps an ordinary pointer position unchanged',()=>{expect(viewportSafeContextMenuPosition(200,150,1000,700,{width:224,height:208})).toEqual({x:200,y:150})});
  it('flips the menu inward at the bottom-right viewport edge',()=>{expect(viewportSafeContextMenuPosition(998,698,1000,700,{width:224,height:208})).toEqual({x:768,y:484})});
  it('honors the safe margin at negative coordinates and tiny viewports',()=>{expect(viewportSafeContextMenuPosition(-20,-30,1000,700,{width:224,height:208})).toEqual({x:8,y:8});expect(viewportSafeContextMenuPosition(40,30,180,120,{width:224,height:208})).toEqual({x:8,y:8})});
});
