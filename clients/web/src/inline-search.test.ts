import {describe,expect,it} from 'vitest';

import {activeTagPrefix,consumeSearchToken,parseSearchDate,tokenFromRaw,tokenQuery} from './inline-search';

describe('inline advanced-search tokens',()=>{
  it('supports quoted tags and attachment wildcards',()=>{
    expect(tokenFromRaw('tag:"needs design"')).toMatchObject({kind:'tag',value:'needs design'});
    expect(tokenFromRaw('attachment:*.png')).toMatchObject({kind:'attachment',value:'*.png'});
    expect(activeTagPrefix('words tag:"needs')).toBe('needs');
  });
  it('only consumes complete trailing tokens and preserves ordinary text',()=>{
    expect(consumeSearchToken('parser tag:client ')).toMatchObject({text:'parser',token:{kind:'tag',value:'client'}});
    expect(consumeSearchToken('tag:client')).toEqual({text:'tag:client'});
    expect(consumeSearchToken('tag:client',true)).toMatchObject({text:'',token:{kind:'tag'}});
  });
  it('normalizes valid local dates and rejects impossible dates',()=>{
    expect(parseSearchDate('09-01-2026 11:05 AM')).toMatch(/^2026-09-01T/);
    expect(parseSearchDate('02-31-2026')).toBeUndefined();
  });
  it('maps lifecycle and attachment tokens to structured server queries',()=>{
    const tokens=[tokenFromRaw('has-attachment')!,tokenFromRaw('attachment:*.png')!,tokenFromRaw('tag:"needs design"')!,tokenFromRaw('completed-after:09-01-2026')!];
    expect(tokenQuery(tokens)).toMatchObject({has_attachment:true,attachment:'*.png',tags:'needs design',completed_after:parseSearchDate('09-01-2026')});
    expect(tokenQuery([tokenFromRaw('started-before:09-01-2026')!])).toMatchObject({status:'started',updated_before:expect.any(String)});
  });
});
