import { describe,expect,it } from 'vitest';

import {applyConversationEvent,beginConversationTurn,EMPTY_CONVERSATION} from './ai-conversation';

describe('AI conversation transcript',()=>{
  it('walks a turn through output, permission, activity, and completion',()=>{
    let state=beginConversationTurn(EMPTY_CONVERSATION,'turn-1','Please review this.');
    expect(state.messages.map(message=>message.role)).toEqual(['user','assistant']);
    state=applyConversationEvent(state,{type:'output',content:'First ',truncated:false});
    state=applyConversationEvent(state,{type:'output',content:'answer.',truncated:false});
    expect(state.messages[1].content).toBe('First answer.');expect(state.progress).toBe('Responding…');
    state=applyConversationEvent(state,{type:'permission_asked',tool:'bash',summary:'run tests'});expect(state.progress).toBe('Waiting for permission…');
    state=applyConversationEvent(state,{type:'native_activity',source:'codex',payload:{summary:'Running focused tests'}});expect(state.progress).toBe('Running focused tests');
    state=applyConversationEvent(state,{type:'done',reason:'completed'});expect(state.messages[1].status).toBe('completed');expect(state.activeAssistantId).toBeUndefined();
  });

  it('retains unknown events and gives interrupted empty turns a useful result',()=>{
    const started=beginConversationTurn(EMPTY_CONVERSATION,'turn-2','Stop soon.');
    expect(applyConversationEvent(started,{type:'future',value:1})).toBe(started);
    const stopped=applyConversationEvent(started,{type:'done',reason:'interrupted'});expect(stopped.messages[1]).toMatchObject({status:'interrupted',content:'Stopped before a response was completed.'});
  });
});
