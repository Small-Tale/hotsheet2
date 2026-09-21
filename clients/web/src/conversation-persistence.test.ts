import {describe,expect,it} from 'vitest';

import {CONVERSATION_STORAGE_KEY,loadConversationStates,saveConversationStates} from './conversation-persistence';

describe('durable AI conversation state (HS2-YHQCS2)',()=>{
  it('round-trips completed and in-progress transcript state across client reloads',()=>{
    const values=new Map<string,string>(),storage={getItem:(key:string)=>values.get(key)??null,setItem:(key:string,value:string)=>values.set(key,value)};
    const states={chat:{messages:[{id:'u1',role:'user' as const,content:'Keep this'},{id:'a1',role:'assistant' as const,content:'Still here',status:'completed' as const}],activity:[{id:'work',tool:'Codex',kind:'edit',summary:'Changed a file',importance:'normal' as const}],nextSequence:4}};
    saveConversationStates(storage,states);
    expect(loadConversationStates(storage)).toEqual(states);
    expect(values.has(CONVERSATION_STORAGE_KEY)).toBe(true);
  });

  it('drops corrupt entries without sacrificing valid conversations',()=>{
    const storage={getItem:()=>JSON.stringify({good:{messages:[{id:'u',role:'user',content:'hello'}]},bad:{messages:[{id:1,role:'robot'}]}})};
    expect(loadConversationStates(storage)).toEqual({good:{messages:[{id:'u',role:'user',content:'hello'}]}});
    expect(loadConversationStates({getItem:()=>'{broken'})).toEqual({});
  });
});
