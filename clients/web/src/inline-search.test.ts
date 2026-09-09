import {describe,expect,it} from 'vitest';

import {activeTagPrefix,consumeSearchToken,dateTokenFromInput,effectiveSearch,parseSearchDate,tokenFromRaw,tokenQuery} from './inline-search';

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
    expect(consumeSearchToken('has:attachment ')).toMatchObject({text:'',token:{kind:'has',value:'attachment'}});
    expect(effectiveSearch('parser has:commit',[])).toMatchObject({text:'parser',tokens:[{kind:'has',value:'commit'}]});
  });
  it('normalizes machine-local and ISO dates and rejects impossible dates',()=>{
    expect(parseSearchDate('09/01/2026 11:05 AM','en-US')).toMatch(/^2026-09-01T/);
    expect(parseSearchDate('01/09/2026 23:05','en-GB')).toMatch(/^2026-09-01T/);
    expect(parseSearchDate('2026-09-01T11:05')).toMatch(/^2026-09-01T/);
    expect(parseSearchDate('2026-09-01T11:05:00Z')).toBe('2026-09-01T11:05:00.000Z');
    expect(parseSearchDate('4h ago','en-US',new Date('2026-09-01T12:00:00Z'))).toBe('2026-09-01T08:00:00.000Z');
    expect(parseSearchDate('31/02/2026','en-GB')).toBeUndefined();
  });
  it('labels date-picker tokens in the machine locale while keeping editable ISO syntax',()=>{
    expect(dateTokenFromInput('created-after','2026-09-01','23:05','en-GB')).toMatchObject({kind:'date',raw:'created-after:2026-09-01T23:05',label:'created after 01/09/2026, 23:05'});
    expect(dateTokenFromInput('created-after','2026-09-01','','en-US')).toMatchObject({raw:'created-after:2026-09-01',label:'created after 9/1/26'});
  });
  it('maps lifecycle and attachment tokens to structured server queries',()=>{
    const tokens=[tokenFromRaw('has:attachment')!,tokenFromRaw('has:media-annotation')!,tokenFromRaw('has:commit')!,tokenFromRaw('attachment:*.png')!,tokenFromRaw('tag:"needs design"')!,tokenFromRaw('completed-after:2026-09-01')!];
    expect(tokenQuery(tokens)).toMatchObject({has_attachment:true,has_media_annotation:true,has_commit:true,attachment:'*.png',tags:'needs design',completed_after:parseSearchDate('2026-09-01')});
    expect(tokenQuery([tokenFromRaw('started-before:2026-09-01')!])).toMatchObject({status:'started',updated_before:expect.any(String)});
  });
});
