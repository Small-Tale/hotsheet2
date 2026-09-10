import {describe,expect,it} from 'vitest';

import {drawerTabOrderStorageKey,drawerTabSelectionAfterClose,keyboardReorderDrawerTabIds,loadDrawerTabOrder,orderedDrawerTabIds,parseDrawerTabOrder,reorderDrawerTabIds,saveDrawerTabOrder} from './drawer-tab-order';

describe('drawer tab ordering',()=>{
  it('applies one remembered order across terminal and AI-chat tabs and appends new tabs',()=>{
    expect(orderedDrawerTabIds(['terminal-a','terminal-b','terminal-c'],['chat-a'],['terminal-b','chat-a','terminal-a'])).toEqual(['terminal-b','chat-a','terminal-a','terminal-c']);
  });

  it('reorders across kinds for pointer and keyboard interactions',()=>{
    const ids=['terminal-a','terminal-b','chat-a'];
    expect(reorderDrawerTabIds(ids,'chat-a','terminal-a','before')).toEqual(['chat-a','terminal-a','terminal-b']);
    expect(keyboardReorderDrawerTabIds(ids,'chat-a','left')).toEqual(['terminal-a','chat-a','terminal-b']);
    expect(keyboardReorderDrawerTabIds(ids,'terminal-a','left')).toEqual(ids);
  });

  it('selects the nearest live tab after a close, preferring the right neighbor',()=>{
    const ids=['terminal-a','chat-a','terminal-b','chat-b'];
    expect(drawerTabSelectionAfterClose(ids,'chat-a',['chat-a'])).toBe('terminal-b');
    expect(drawerTabSelectionAfterClose(ids,'terminal-b',['terminal-b','chat-b'])).toBe('chat-a');
    expect(drawerTabSelectionAfterClose(ids,'chat-a',['terminal-a','chat-a','terminal-b','chat-b'])).toBe('grid');
    expect(drawerTabSelectionAfterClose(ids,'terminal-a',['chat-a'])).toBe('terminal-a');
  });

  it('persists a deduplicated device-local order and tolerates invalid data',()=>{
    const values=new Map<string,string>(),storage={getItem:(key:string)=>values.get(key)??null,setItem:(key:string,value:string)=>{values.set(key,value)}};
    expect(saveDrawerTabOrder(storage,'project',['terminal-a','chat-a','terminal-a'])).toEqual(['terminal-a','chat-a']);
    expect(values.get(drawerTabOrderStorageKey('project'))).toBe('["terminal-a","chat-a"]');
    expect(loadDrawerTabOrder(storage,'project')).toEqual(['terminal-a','chat-a']);
    expect(parseDrawerTabOrder('{')).toEqual([]);
    expect(parseDrawerTabOrder('["one",1,"one",""]')).toEqual(['one']);
  });
});
