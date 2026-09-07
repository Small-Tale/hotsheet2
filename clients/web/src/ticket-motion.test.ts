/* eslint-disable @typescript-eslint/unbound-method -- DOM method spies are intentionally extracted for assertions. */
import {describe,expect,it,vi} from 'vitest';

import {animateTicketMotion,type TicketMotionSnapshot} from './ticket-motion';

const rect=(left:number,top:number,width=100,height=50)=>({left,top,width,height} as DOMRect);
const pendingAnimation=()=>({finished:new Promise<void>(()=>undefined)}) as unknown as Animation;
function root(rows:HTMLElement[]){return{querySelector:()=>({dataset:{presentation:'edge-to-edge'},querySelector:()=>({dataset:{component:'ticket-board'}})}),querySelectorAll:()=>rows} as unknown as ParentNode}
function row(slug:string,parent:string,box:DOMRect,animate=vi.fn(()=>pendingAnimation())){return{dataset:{ticketSlug:slug},style:{visibility:''},ownerDocument:{body:{append:vi.fn()},head:{append:vi.fn()},querySelectorAll:()=>[],createElement:()=>({dataset:{},textContent:'',remove:vi.fn()}),defaultView:{CSS:{escape:(value:string)=>value}}},getBoundingClientRect:()=>box,closest:()=>({dataset:{columnId:parent}}),animate} as unknown as HTMLElement}
function snapshot(rows:Array<[string,HTMLElement,string]>):TicketMotionSnapshot{return{scope:'edge-to-edge:ticket-board',rows:new Map(rows.map(([slug,element,parent])=>[slug,{rect:element.getBoundingClientRect(),parent,element}]))}}
function ghost(slug='HS2-A'){return{ariaHidden:'false',dataset:{ticketSlug:slug} as DOMStringMap,style:{cssText:''},ownerDocument:{createElement:()=>({textContent:''})},prepend:vi.fn(),querySelectorAll:()=>[],animate:vi.fn(()=>pendingAnimation()),remove:vi.fn(),removeAttribute:vi.fn()} as unknown as HTMLElement}

describe('ticket motion',()=>{
  it('lifts a cross-column move into a fixed overlay instead of clipping it in the destination',()=>{
    const previous=row('HS2-A','started',rect(10,20)),current=row('HS2-A','completed',rect(210,80)),overlay=ghost(),append=vi.fn();
    (current as unknown as {cloneNode:()=>HTMLElement}).cloneNode=()=>overlay;(current as unknown as {ownerDocument:{body:{append:(node:HTMLElement)=>void}}}).ownerDocument.body.append=append;
    animateTicketMotion(snapshot([['HS2-A',previous,'started']]),root([current]),false);
    expect(current.style.visibility).toBe('hidden');expect(append).toHaveBeenCalledWith(overlay);expect(overlay.dataset.ticketMotionGhost).toBe('move');expect(overlay.dataset.ticketMotionSlug).toBe('HS2-A');expect(overlay.dataset.ticketSlug).toBeUndefined();expect(overlay.style.cssText).toContain('position:fixed');expect(overlay.style.cssText).toContain('z-index:1300');expect(overlay.animate).toHaveBeenCalledWith([{transform:'translate(-200px, -60px)',zIndex:'1300'},{transform:'translate(0, 0)',zIndex:'1300'}],expect.objectContaining({duration:240}));
  });

  it('slides retained rows to make room before fading an incoming row',()=>{
    const previous=row('HS2-A','not-started',rect(10,20)),retained=row('HS2-A','not-started',rect(10,80)),incoming=row('HS2-B','not-started',rect(10,20)),overlay=ghost();
    (incoming as unknown as {cloneNode:()=>HTMLElement}).cloneNode=()=>overlay;
    animateTicketMotion(snapshot([['HS2-A',previous,'not-started']]),root([incoming,retained]),false);
    expect(retained.animate).toHaveBeenCalledWith([{transform:'translate(0px, -60px)'},{transform:'translate(0, 0)'}],expect.objectContaining({delay:0,duration:240,fill:'backwards'}));expect(incoming.style.visibility).toBe('hidden');expect(overlay.dataset.ticketMotionGhost).toBe('incoming');expect(overlay.animate).toHaveBeenCalledWith([{opacity:0},{opacity:1}],expect.objectContaining({delay:240,duration:160,fill:'backwards'}));
  });

  it('fades an outgoing ghost before closing its source-column space',()=>{
    const removed=row('HS2-A','started',rect(10,20)),overlay=ghost(),append=vi.fn(),previousSibling=row('HS2-B','started',rect(10,80)),retained=row('HS2-B','started',rect(10,20));
    (removed as unknown as {cloneNode:()=>HTMLElement}).cloneNode=()=>overlay;(removed as unknown as {ownerDocument:{body:{append:(node:HTMLElement)=>void}}}).ownerDocument.body.append=append;
    animateTicketMotion(snapshot([['HS2-A',removed,'started'],['HS2-B',previousSibling,'started']]),root([retained]),false);
    expect(overlay.dataset.ticketMotionGhost).toBe('outgoing');expect(overlay.animate).toHaveBeenCalledWith([{opacity:1,transform:'scale(1)'},{opacity:0,transform:'scale(.985)'}],expect.objectContaining({duration:160}));expect(retained.animate).toHaveBeenCalledWith([{transform:'translate(0px, 60px)'},{transform:'translate(0, 0)'}],expect.objectContaining({delay:160,duration:240}));
  });

  it('does not animate when reduced motion is requested',()=>{const previous=row('HS2-A','started',rect(10,20)),current=row('HS2-A','started',rect(10,80));animateTicketMotion(snapshot([['HS2-A',previous,'started']]),root([current]),true);expect(current.animate).not.toHaveBeenCalled()});

  it('ignores pure layout shifts that do not change collection membership or status',()=>{const previous=row('HS2-A','started',rect(10,20)),current=row('HS2-A','started',rect(210,20));animateTicketMotion(snapshot([['HS2-A',previous,'started']]),root([current]),false);expect(current.animate).not.toHaveBeenCalled()});
});
