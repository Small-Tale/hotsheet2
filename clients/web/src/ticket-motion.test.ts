/* eslint-disable @typescript-eslint/unbound-method -- DOM method spies are intentionally extracted for assertions. */
import {describe,expect,it,vi} from 'vitest';

import {animateTicketMotion,captureTicketMotion,type TicketMotionSnapshot} from './ticket-motion';

const rect=(left:number,top:number,width=100,height=50)=>({left,top,width,height} as DOMRect);
const pendingAnimation=()=>({finished:new Promise<void>(()=>undefined),cancel:vi.fn()}) as unknown as Animation;

interface MockRow {container:HTMLElement;visual:HTMLElement;animate:ReturnType<typeof vi.fn>;document:Document}

function mockDocument(){
  const children:HTMLElement[]=[];
  const document={
    body:{append:vi.fn((node:HTMLElement)=>children.push(node))},
    head:{append:vi.fn()},
    querySelectorAll:vi.fn(()=>children),
    createElement:vi.fn(()=>({dataset:{},textContent:'',remove:vi.fn()})),
    defaultView:{CSS:{escape:(value:string)=>value},getComputedStyle:()=>({borderRadius:'10px'}),requestAnimationFrame:vi.fn(()=>1),cancelAnimationFrame:vi.fn()},
  } as unknown as Document;
  return document;
}

function ghost(slug='HS2-A',document=mockDocument()){
  const visual={dataset:{ticketSlug:slug,component:'ticket-list-row'},style:{},removeAttribute:vi.fn(),childElementCount:0,textContent:'Ticket title'} as unknown as HTMLElement;
  const style={dataset:{},style:{},removeAttribute:vi.fn(),childElementCount:0,textContent:''} as unknown as HTMLElement;
  const ghost={
    ariaHidden:'false',inert:false,dataset:{} as DOMStringMap,style:{cssText:''},ownerDocument:document,prepend:vi.fn(),querySelectorAll:vi.fn(()=>[visual]),
    querySelector:vi.fn((selector:string)=>selector==='.ticket-list-row'?visual:null),animate:vi.fn(()=>pendingAnimation()),remove:vi.fn(),removeAttribute:vi.fn(),childElementCount:1,textContent:''
  } as unknown as HTMLElement;
  (ghost as unknown as {visual:HTMLElement;styleNode:HTMLElement}).visual=visual;(ghost as unknown as {styleNode:HTMLElement}).styleNode=style;
  return ghost;
}

function row(slug:string,parent:string,containerBox:DOMRect,visualBox=rect(containerBox.left+4,containerBox.top+4,containerBox.width-8,containerBox.height-8),document=mockDocument()):MockRow{
  const animate=vi.fn(()=>pendingAnimation());
  const visual={dataset:{ticketSlug:slug,component:'ticket-list-row'},style:{visibility:''},ownerDocument:document,getBoundingClientRect:()=>visualBox} as unknown as HTMLElement;
  const container={
    dataset:{component:'ticket-list-row-container'},style:{visibility:''},ownerDocument:document,getBoundingClientRect:()=>containerBox,
    querySelector:vi.fn(()=>visual),closest:vi.fn((selector:string)=>selector==='[data-column-id]'?{dataset:{columnId:parent}}:null),animate,
  } as unknown as HTMLElement;
  return{container,visual,animate,document};
}

function root(rows:MockRow[]){
  return{
    querySelector:()=>({dataset:{presentation:'edge-to-edge'},querySelector:()=>({dataset:{component:'ticket-board'}})}),
    querySelectorAll:(selector:string)=>selector==='[data-component="ticket-list-row-container"]'?rows.map(item=>item.container):[],
  } as unknown as ParentNode;
}

function snapshot(rows:Array<[string,MockRow,string]>):TicketMotionSnapshot{
  return{scope:'edge-to-edge:ticket-board',rows:new Map(rows.map(([slug,item,parent])=>[slug,{rect:item.container.getBoundingClientRect(),parent,container:item.container,visual:item.visual,borderRadius:'10px'}]))};
}

describe('ticket motion',()=>{
  it('does not clone or animate rows when the ticket collection view changes',()=>{
    const previous=row('HS2-A','ticket-list',rect(10,20)),incoming=row('HS2-B','ticket-list',rect(10,20));
    const before=captureTicketMotion(root([previous]),'queue');
    animateTicketMotion(before,root([incoming]),false,'archive');
    expect(incoming.animate).not.toHaveBeenCalled();
  });

  it('captures keyed outer-container geometry instead of the inset article geometry',()=>{
    const item=row('HS2-A','started',rect(10,20,180,72),rect(14,24,172,64));
    const captured=captureTicketMotion(root([item])).rows.get('HS2-A');
    expect(captured?.rect).toEqual(rect(10,20,180,72));expect(captured?.container).toBe(item.container);expect(captured?.visual).toBe(item.visual);
  });

  it('lifts a cross-column move as a container-stable fixed overlay',()=>{
    const document=mockDocument(),previous=row('HS2-A','started',rect(10,20,180,72),undefined,document),current=row('HS2-A','completed',rect(210,80,220,88),undefined,document),overlay=ghost('HS2-A',document);
    (current.container as unknown as {cloneNode:()=>HTMLElement}).cloneNode=()=>overlay;
    animateTicketMotion(snapshot([['HS2-A',previous,'started']]),root([current]),false);
    expect(current.container.style.visibility).toBe('hidden');expect(document.body.append).toHaveBeenCalledWith(overlay);expect(overlay.dataset.ticketMotionGhost).toBe('move');expect(overlay.dataset.ticketMotionSlug).toBe('HS2-A');expect(overlay.style.cssText).toContain('width:220px');expect(overlay.style.cssText).toContain('height:88px');expect(overlay.style.cssText).toContain('z-index:1300');
    expect(overlay.animate).toHaveBeenCalledWith([{transform:'translate(-200px, -60px)',zIndex:'1300'},{transform:'translate(0, 0)',zIndex:'1300'}],expect.objectContaining({duration:240}));
    const clonedVisual=(overlay as unknown as {visual:HTMLElement}).visual;expect(clonedVisual.dataset.ticketSlug).toBeUndefined();expect(clonedVisual.dataset.component).toBeUndefined();expect(clonedVisual.style.borderRadius).toBe('10px');
  });

  it('uses exact outer-wrapper FLIP distance before fading an incoming row',()=>{
    const document=mockDocument(),previous=row('HS2-A','not-started',rect(10,20,190,53),undefined,document),retained=row('HS2-A','not-started',rect(10,91,190,53),undefined,document),incoming=row('HS2-B','not-started',rect(10,20,190,63),undefined,document),overlay=ghost('HS2-B',document);
    (incoming.container as unknown as {cloneNode:()=>HTMLElement}).cloneNode=()=>overlay;
    animateTicketMotion(snapshot([['HS2-A',previous,'not-started']]),root([incoming,retained]),false);
    expect(retained.animate).toHaveBeenCalledWith([{transform:'translate(0px, -71px)'},{transform:'translate(0, 0)'}],expect.objectContaining({delay:0,duration:240,fill:'backwards'}));expect(incoming.container.style.visibility).toBe('hidden');expect(overlay.dataset.ticketMotionGhost).toBe('incoming');expect(overlay.animate).toHaveBeenCalledWith([{opacity:0},{opacity:1}],expect.objectContaining({delay:240,duration:160,fill:'backwards'}));
  });

  it('fades the first incoming row when the visible collection was empty',()=>{
    const document=mockDocument(),incoming=row('HS2-FIRST','not-started',rect(10,20,190,63),undefined,document),overlay=ghost('HS2-FIRST',document);
    (incoming.container as unknown as {cloneNode:()=>HTMLElement}).cloneNode=()=>overlay;
    animateTicketMotion({scope:'edge-to-edge:ticket-board',rows:new Map()},root([incoming]),false);
    expect(incoming.container.style.visibility).toBe('hidden');expect(overlay.dataset.ticketMotionGhost).toBe('incoming');expect(overlay.animate).toHaveBeenCalledWith([{opacity:0},{opacity:1}],expect.objectContaining({delay:0,duration:160,fill:'backwards'}));
  });

  it('fades an outgoing container ghost before closing its exact source space',()=>{
    const document=mockDocument(),removed=row('HS2-A','started',rect(10,20,190,67),undefined,document),overlay=ghost('HS2-A',document),previousSibling=row('HS2-B','started',rect(10,95,190,44),undefined,document),retained=row('HS2-B','started',rect(10,20,190,44),undefined,document);
    (removed.container as unknown as {cloneNode:()=>HTMLElement}).cloneNode=()=>overlay;
    animateTicketMotion(snapshot([['HS2-A',removed,'started'],['HS2-B',previousSibling,'started']]),root([retained]),false);
    expect(overlay.dataset.ticketMotionGhost).toBe('outgoing');expect(overlay.animate).toHaveBeenCalledWith([{opacity:1,transform:'scale(1)'},{opacity:0,transform:'scale(.985)'}],expect.objectContaining({duration:160}));expect(retained.animate).toHaveBeenCalledWith([{transform:'translate(0px, 75px)'},{transform:'translate(0, 0)'}],expect.objectContaining({delay:160,duration:240}));
  });

  it('cancels a stale FLIP animation before replacing it on the same keyed wrapper',()=>{
    const document=mockDocument(),old=row('HS2-A','started',rect(10,20),undefined,document),retained=row('HS2-A','started',rect(10,80),undefined,document),incoming=row('HS2-B','started',rect(10,20),undefined,document),incomingGhost=ghost('HS2-B',document);
    (incoming.container as unknown as {cloneNode:()=>HTMLElement}).cloneNode=()=>incomingGhost;
    animateTicketMotion(snapshot([['HS2-A',old,'started']]),root([incoming,retained]),false);
    const first=retained.animate.mock.results[0].value as Animation;
    const secondIncoming=row('HS2-D','started',rect(10,80),undefined,document),secondGhost=ghost('HS2-D',document),beforeSecond=row('HS2-A','started',rect(10,80),undefined,document);
    (secondIncoming.container as unknown as {cloneNode:()=>HTMLElement}).cloneNode=()=>secondGhost;
    (retained.container as unknown as {getBoundingClientRect:()=>DOMRect}).getBoundingClientRect=()=>rect(10,140);
    animateTicketMotion(snapshot([['HS2-A',beforeSecond,'started']]),root([secondIncoming,retained]),false);
    expect(first.cancel).toHaveBeenCalled();
  });

  it('does not animate reduced motion or unrelated layout changes',()=>{
    const previous=row('HS2-A','started',rect(10,20)),current=row('HS2-A','started',rect(210,80));
    animateTicketMotion(snapshot([['HS2-A',previous,'started']]),root([current]),true);expect(current.animate).not.toHaveBeenCalled();
    animateTicketMotion(snapshot([['HS2-A',previous,'started']]),root([current]),false);expect(current.animate).not.toHaveBeenCalled();
  });
});
