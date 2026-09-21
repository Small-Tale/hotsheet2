import {describe,expect,it,vi} from 'vitest';

import {applyAfterCatalogPopupsClose} from './catalog-update';

const popup=(open:boolean)=>Object.assign(new EventTarget(),{open});
const root=(popups:EventTarget[])=>({querySelectorAll:()=>popups}) as unknown as ParentNode;

describe('catalog metadata updates',()=>{
  it('applies immediately when no catalog popup is open',()=>{const apply=vi.fn();applyAfterCatalogPopupsClose(root([popup(false)]),apply);expect(apply).toHaveBeenCalledOnce()});

  it('waits through each open select or dropdown before applying once',()=>{
    const select=popup(true),dropdown=popup(true),apply=vi.fn();
    applyAfterCatalogPopupsClose(root([select,dropdown]),apply);expect(apply).not.toHaveBeenCalled();
    Object.assign(select,{open:false});select.dispatchEvent(new Event('wa-after-hide'));expect(apply).not.toHaveBeenCalled();
    Object.assign(dropdown,{open:false});dropdown.dispatchEvent(new Event('wa-after-hide'));expect(apply).toHaveBeenCalledOnce();
    dropdown.dispatchEvent(new Event('wa-after-hide'));expect(apply).toHaveBeenCalledOnce();
  });
});
