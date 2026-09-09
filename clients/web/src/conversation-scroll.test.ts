import {describe,expect,it} from 'vitest';

import {syncConversationScroll} from './conversation-scroll';

function fixture(scrollTop:number,clientHeight:number,scrollHeight:number,previousHeight?:number){
  const transcript={scrollTop,clientHeight,scrollHeight,dataset:previousHeight===undefined?{}:{conversationScrollHeight:String(previousHeight)}};
  const root={querySelector:()=>transcript} as unknown as ParentNode;
  return{root,transcript};
}

describe('conversation scroll',()=>{
  it('pins the first render and subsequent growth when the reader was at the bottom',()=>{
    const first=fixture(0,300,1_000);
    syncConversationScroll(first.root);
    expect(first.transcript.scrollTop).toBe(1_000);
    expect(first.transcript.dataset.conversationScrollHeight).toBe('1000');

    const growing=fixture(700,300,1_200,1_000);
    syncConversationScroll(growing.root);
    expect(growing.transcript.scrollTop).toBe(1_200);
  });

  it('does not pull a reader away from older messages unless opening is forced',()=>{
    const reading=fixture(100,300,1_200,1_000);
    syncConversationScroll(reading.root);
    expect(reading.transcript.scrollTop).toBe(100);
    syncConversationScroll(reading.root,true);
    expect(reading.transcript.scrollTop).toBe(1_200);
  });

  it('is a no-op when no conversation is mounted',()=>{
    expect(()=>{syncConversationScroll({querySelector:()=>null} as unknown as ParentNode)}).not.toThrow();
  });
});
