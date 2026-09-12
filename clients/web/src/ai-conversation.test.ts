import { describe,expect,it } from 'vitest';

import {applyConversationActivity,applyConversationEvent,beginConversationTurn,conversationUsage,EMPTY_CONVERSATION,formatConversationCost,formatConversationTokens} from './ai-conversation';

describe('AI conversation transcript',()=>{
  it('walks a turn through output, permission, activity, and completion',()=>{
    let state=beginConversationTurn(EMPTY_CONVERSATION,'turn-1','Please review this.');
    expect(state.messages.map(message=>message.role)).toEqual(['user','assistant']);
    state=applyConversationEvent(state,{type:'output',content:'First ',truncated:false});
    state=applyConversationEvent(state,{type:'output',content:'answer.',truncated:false});
    expect(state.messages[1].content).toBe('First answer.');expect(state.progress).toBe('Responding…');
    state=applyConversationEvent(state,{type:'permission_asked',tool:'bash',summary:'run tests'});expect(state.progress).toBe('Waiting for permission…');
    state=applyConversationEvent(state,{type:'native_activity',source:'codex',payload:{summary:'Running focused tests'}});expect(state.progress).toBe('Running focused tests');
    state=applyConversationEvent(state,{type:'usage',model:'codex-5.6',tokens_in:12_000,tokens_out:800,cost_usd:.0412});
    expect(state.messages[1].usage).toEqual({model:'codex-5.6',tokensIn:12_000,tokensOut:800,costUsd:.0412});
    state=applyConversationEvent(state,{type:'done',reason:'completed'});expect(state.messages[1].status).toBe('completed');expect(state.activeAssistantId).toBeUndefined();
    expect(conversationUsage(state)).toEqual({tokensIn:12_000,tokensOut:800,costUsd:.0412});
  });

  it('preserves prior transcript activity and clears a stale error when retrying',()=>{
    const prior={messages:[{id:'old',role:'assistant' as const,content:'Earlier answer.',status:'failed' as const}],activity:[{id:'activity-1',tool:'Codex',kind:'command',summary:'Inspected the project',importance:'normal' as const}],error:'The prior turn failed.'};
    const retried=beginConversationTurn(prior,'turn-retry','Try again.');
    expect(retried.messages.map(message=>message.id)).toEqual(['old','turn-retry','turn-retry-assistant']);
    expect(retried.activity).toBe(prior.activity);
    expect(retried.error).toBeUndefined();
  });

  it('retains stable structured file references emitted with assistant output',()=>{
    let state=beginConversationTurn(EMPTY_CONVERSATION,'turn-files','Inspect the files');
    state=applyConversationEvent(state,{type:'output',content:'Attached.',truncated:false,files:[{id:'proof-1',filename:'proof.png',mime_type:'image/png',kind:'media',url:'/files/proof.png'}]});
    state=applyConversationEvent(state,{type:'output',content:' Done.',truncated:false,files:[{id:'proof-1',filename:'proof.png',mime_type:'image/png',kind:'media',url:'/files/proof.png'}]});
    expect(state.messages[1].files).toEqual([{id:'proof-1',filename:'proof.png',mime_type:'image/png',kind:'media',url:'/files/proof.png'}]);
  });

  it('retains unknown events and gives interrupted empty turns a useful result',()=>{
    const started=beginConversationTurn(EMPTY_CONVERSATION,'turn-2','Stop soon.');
    expect(applyConversationEvent(started,{type:'future',value:1})).toBe(started);
    const stopped=applyConversationEvent(started,{type:'done',reason:'interrupted'});expect(stopped.messages[1]).toMatchObject({status:'interrupted',content:'Stopped before a response was completed.'});
  });

  it('deduplicates and bounds normalized activity while keeping AI-safe usage precision',()=>{
    let state=EMPTY_CONVERSATION;
    for(let index=0;index<55;index+=1)state=applyConversationActivity(state,{id:`event-${index}`,ts:'2026-09-08T00:00:00Z',tool:'Codex',session:'session-1',kind:'edit',summary:`Edited file ${index}`,importance:'normal'});
    const duplicate=applyConversationActivity(state,{id:'event-54',ts:'2026-09-08T00:00:00Z',tool:'Codex',kind:'edit',summary:'Duplicate',importance:'normal'});
    expect(duplicate).toBe(state);expect(state.activity).toHaveLength(50);expect(state.activity?.[0].id).toBe('event-5');
    expect(formatConversationTokens(12_840)).toBe('12.8K');expect(formatConversationCost(.0042)).toBe('≈$0.0042');expect(formatConversationCost()).toBe('Cost unavailable');
  });
});
