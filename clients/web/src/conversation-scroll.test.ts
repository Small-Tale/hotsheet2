import {describe,expect,it} from 'vitest';

import {syncConversationScroll} from './conversation-scroll';

interface FakeTranscript {scrollTop:number;clientHeight:number;scrollHeight:number;addEventListener:(type:string,listener:()=>void)=>void;scroll:(top:number)=>void;resize:(height:number)=>void}

function transcript(clientHeight:number,scrollHeight:number):FakeTranscript{
  const listeners:Array<()=>void>=[];
  const fake:FakeTranscript={scrollTop:0,clientHeight,scrollHeight,addEventListener:(type,listener)=>{if(type==='scroll')listeners.push(listener)},
    // Mirrors the browser: user scrolling and height clamping both dispatch `scroll`.
    scroll:top=>{fake.scrollTop=Math.max(0,Math.min(top,fake.scrollHeight-fake.clientHeight));for(const listener of listeners)listener()},
    resize:height=>{fake.scrollHeight=height;const clamped=Math.max(0,Math.min(fake.scrollTop,height-fake.clientHeight));if(clamped!==fake.scrollTop)fake.scroll(clamped)}};
  return fake;
}

function rootOf(...transcripts:FakeTranscript[]){return{querySelectorAll:()=>transcripts} as unknown as ParentNode}

describe('conversation scroll',()=>{
  it('pins the first render and follows growth while the reader stays at the latest edge',()=>{
    const view=transcript(300,1_000),root=rootOf(view);
    syncConversationScroll(root);
    expect(view.scrollTop).toBe(1_000);
    view.resize(1_200);
    syncConversationScroll(root);
    expect(view.scrollTop).toBe(1_200);
  });

  it('does not pull a scrolled-back reader away from older messages unless opening is forced',()=>{
    const view=transcript(300,1_000),root=rootOf(view);
    syncConversationScroll(root);
    view.scroll(100);
    view.resize(1_200);
    syncConversationScroll(root);
    expect(view.scrollTop).toBe(100);
    syncConversationScroll(root,true);
    expect(view.scrollTop).toBe(1_200);
    view.resize(1_400);
    syncConversationScroll(root);
    expect(view.scrollTop).toBe(1_400);
  });

  it('keeps a scrolled-back reader in place across repeated selection rerenders',()=>{
    const view=transcript(300,1_000),root=rootOf(view);
    syncConversationScroll(root);
    view.scroll(150);
    for(let pass=0;pass<3;pass++){
      view.resize(pass%2?1_000:1_040);
      syncConversationScroll(root);
      expect(view.scrollTop).toBe(150);
    }
  });

  it('pins again once the reader returns to the latest edge',()=>{
    const view=transcript(300,1_000),root=rootOf(view);
    syncConversationScroll(root);
    view.scroll(200);
    view.scroll(690);
    view.resize(1_300);
    syncConversationScroll(root);
    expect(view.scrollTop).toBe(1_300);
  });

  it('stays pinned when transient content shrinks and the browser clamps the scroll',()=>{
    const view=transcript(300,1_000),root=rootOf(view);
    syncConversationScroll(root);
    view.resize(1_200);
    syncConversationScroll(root);
    view.resize(1_050);
    expect(view.scrollTop).toBe(750);
    view.resize(1_400);
    syncConversationScroll(root);
    expect(view.scrollTop).toBe(1_400);
  });

  it('tracks each mounted transcript independently',()=>{
    const dialog=transcript(300,1_000),drawer=transcript(200,800),root=rootOf(dialog,drawer);
    syncConversationScroll(root);
    expect([dialog.scrollTop,drawer.scrollTop]).toEqual([1_000,800]);
    dialog.scroll(50);
    drawer.resize(900);
    syncConversationScroll(root);
    expect([dialog.scrollTop,drawer.scrollTop]).toEqual([50,900]);
  });

  it('is a no-op when no conversation is mounted',()=>{
    expect(()=>{syncConversationScroll(rootOf())}).not.toThrow();
  });
});
