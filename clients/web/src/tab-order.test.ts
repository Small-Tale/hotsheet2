import {describe,expect,it} from 'vitest';

import {applyRememberedTabOrder,reorderTabs,replaceTabInPlace} from './tab-order';

const items=['one','two','three'].map(id=>({id})),identity=(item:{id:string})=>item.id;

describe('tab order',()=>{
  it('moves tabs before or after a target without losing identity',()=>{
    expect(reorderTabs(items,identity,'three','one','before').map(identity)).toEqual(['three','one','two']);
    expect(reorderTabs(items,identity,'one','two','after').map(identity)).toEqual(['two','one','three']);
    expect(reorderTabs(items,identity,'missing','two','after')).toEqual(items);
  });

  it('restores a partial remembered order and appends newly discovered tabs stably',()=>{
    expect(applyRememberedTabOrder(items,identity,['three','one']).map(identity)).toEqual(['three','one','two']);
    expect(applyRememberedTabOrder(items,identity,['removed','two']).map(identity)).toEqual(['two','one','three']);
  });

  it('updates a reopened tab in place and appends a genuinely new tab',()=>{
    expect(replaceTabInPlace(items,identity,{id:'two',name:'updated'} as {id:string}).map(identity)).toEqual(['one','two','three']);
    expect(replaceTabInPlace(items,identity,{id:'four'}).map(identity)).toEqual(['one','two','three','four']);
  });
});
