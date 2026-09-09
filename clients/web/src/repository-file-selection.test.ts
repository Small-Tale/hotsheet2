import {describe,expect,it} from 'vitest';

import {updateRepositoryFileSelection} from './repository-file-selection';

describe('repository file selection',()=>{
  const visible=['a.ts','b.ts','c.ts','d.ts'];
  it('selects one row without opening its menu',()=>{expect(updateRepositoryFileSelection(['a.ts'],visible,'c.ts')).toEqual(['c.ts'])});
  it('toggles additive rows',()=>{expect(updateRepositoryFileSelection(['a.ts'],visible,'c.ts',{additive:true})).toEqual(['a.ts','c.ts']);expect(updateRepositoryFileSelection(['a.ts','c.ts'],visible,'a.ts',{additive:true})).toEqual(['c.ts'])});
  it('selects contiguous ranges from the anchor',()=>{expect(updateRepositoryFileSelection([],visible,'d.ts',{range:true,anchor:'b.ts'})).toEqual(['b.ts','c.ts','d.ts'])});
});
