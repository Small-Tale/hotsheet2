import {describe,expect,it} from 'vitest';

import {activeTagPrefix,consumeSearchToken,consumeSearchTokens,dateTokenFromInput,effectiveSearch,inlineSearchParts,orderedSearchText,parseSearchDate,tokenFromRaw,tokenQuery} from './inline-search';

describe('inline advanced-search tokens',()=>{
  it('supports quoted tags and attachment wildcards',()=>{
    expect(tokenFromRaw('tag:"needs design"')).toMatchObject({kind:'tag',value:'needs design'});
    expect(tokenFromRaw('attachment:*.png')).toMatchObject({kind:'attachment',value:'*.png'});
    expect(activeTagPrefix('words tag:"needs')).toBe('needs');
    expect(tokenFromRaw('tag:"needs')).toBeUndefined();
    expect(tokenFromRaw('attachment:"screen shot')).toBeUndefined();
  });
  it('only consumes complete trailing tokens and preserves ordinary text',()=>{
    expect(consumeSearchToken('parser tag:client ')).toMatchObject({text:'parser',token:{kind:'tag',value:'client'}});
    expect(consumeSearchToken('tag:client')).toEqual({text:'tag:client'});
    expect(consumeSearchToken('tag:client',true)).toMatchObject({text:'',token:{kind:'tag'}});
    expect(consumeSearchToken('has:attachment ')).toMatchObject({text:'',token:{kind:'has',value:'attachment'}});
    expect(consumeSearchToken('is:up-next ')).toMatchObject({text:'',token:{kind:'is',value:'up-next'}});
    expect(consumeSearchToken('is:active')).toEqual({text:'is:active'});
    expect(consumeSearchToken('is:active',true)).toMatchObject({text:'',token:{kind:'is',value:'active'}});
    expect(consumeSearchToken('is:closed ')).toMatchObject({text:'',token:{kind:'is',value:'closed'}});
    expect(consumeSearchToken('is:duplicate',true)).toMatchObject({text:'',token:{kind:'is',value:'duplicate'}});
    expect(consumeSearchToken('updated-after:4h ')).toEqual({text:'updated-after:4h '});
    expect(consumeSearchToken('updated-after:4h ago ')).toMatchObject({text:'',token:{kind:'date',raw:'updated-after:4h ago'}});
    expect(consumeSearchToken('updated-after:2026/09/07',true)).toMatchObject({text:'',token:{kind:'date',raw:'updated-after:2026/09/07'}});
    expect(effectiveSearch('parser has:commit',[])).toMatchObject({text:'parser',tokens:[{kind:'has',value:'commit'}]});
    expect(effectiveSearch('',[tokenFromRaw('is:active')!])).toMatchObject({text:'is:active',tokens:[{kind:'is',value:'active'}]});
    expect(consumeSearchTokens('tag:h')).toMatchObject({text:'tag:h',tokens:[]});
    expect(consumeSearchTokens('tag:"hello ')).toMatchObject({text:'tag:"hello ',tokens:[]});
    expect(consumeSearchTokens('tag:"hello world" ')).toMatchObject({text:' ',tokens:[{kind:'tag',value:'hello world'}]});
  });
  it('keeps committed tokens ordered inside ordinary boolean text',()=>{
    expect(consumeSearchTokens('NOT tag:client',true)).toMatchObject({text:'NOT ',tokens:[{raw:'tag:client',offset:4}]});
    const parsed=consumeSearchTokens('NOT tag:client AND hello ');
    expect(parsed).toMatchObject({text:'NOT  AND hello ',tokens:[{kind:'tag',value:'client',offset:4}]});
    expect(inlineSearchParts(parsed.text,parsed.tokens)).toEqual([
      {kind:'text',value:'NOT '},
      {kind:'token',token:expect.objectContaining({raw:'tag:client',offset:4})},
      {kind:'text',value:' AND hello '},
    ]);
    expect(orderedSearchText(parsed.text,parsed.tokens)).toBe('NOT tag:client AND hello');
    expect(effectiveSearch(parsed.text,parsed.tokens).text).toBe('NOT tag:client AND hello');
    const simple=consumeSearchTokens('tag:client ',true);
    expect(effectiveSearch(simple.text,simple.tokens).text).toBe('');
    expect(inlineSearchParts('',[{...tokenFromRaw('tag:client')!,offset:0}])).toEqual([
      {kind:'text',value:''},
      {kind:'token',token:expect.objectContaining({raw:'tag:client',offset:0})},
      {kind:'text',value:''},
    ]);
  });
  it('normalizes machine-local and ISO dates and rejects impossible dates',()=>{
    expect(parseSearchDate('09/01/2026 11:05 AM','en-US')).toMatch(/^2026-09-01T/);
    expect(parseSearchDate('01/09/2026 23:05','en-GB')).toMatch(/^2026-09-01T/);
    expect(parseSearchDate('2026-09-01T11:05')).toMatch(/^2026-09-01T/);
    expect(parseSearchDate('2026-09-01T11:05:00Z')).toBe('2026-09-01T11:05:00.000Z');
    expect(parseSearchDate('2026/09/07')).toBe(new Date(2026,8,7).toISOString());
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
    expect(tokenQuery([tokenFromRaw('is:duplicate')!])).toEqual({close_reason:'duplicate'});
  });
});
