interface TicketMotionRow {
  rect:DOMRect;
  parent:string;
  container:HTMLElement;
  visual:HTMLElement;
  borderRadius:string;
}
export interface TicketMotionSnapshot {scope:string;rows:Map<string,TicketMotionRow>}

const LAYOUT_DURATION=240,FADE_DURATION=160,MOTION_EASING='cubic-bezier(.2,.8,.2,1)';
const activeLayoutAnimations=new WeakMap<HTMLElement,Animation>();

function motionScope(root:ParentNode){const workspace=root.querySelector<HTMLElement>('.app-shell__workspace'),collection=workspace?.querySelector<HTMLElement>('[data-component="ticket-list"], [data-component="ticket-board"]');return`${workspace?.dataset.presentation??''}:${collection?.dataset.component??''}`}
function motionContainers(root:ParentNode){return [...root.querySelectorAll<HTMLElement>('[data-component="ticket-list-row-container"]')].filter(container=>Boolean(ticketVisual(container)?.dataset.ticketSlug))}
function ticketVisual(container:HTMLElement){return container.querySelector<HTMLElement>(':scope > [data-component="ticket-list-row"][data-ticket-slug]')}
function parentKey(container:HTMLElement){return container.closest<HTMLElement>('[data-column-id]')?.dataset.columnId??container.closest<HTMLElement>('[data-component="ticket-list"]')?.dataset.component??''}
function currentRow(container:HTMLElement):TicketMotionRow|undefined{
  const visual=ticketVisual(container);
  if(!visual)return undefined;
  return{rect:container.getBoundingClientRect(),parent:parentKey(container),container,visual,borderRadius:container.ownerDocument.defaultView?.getComputedStyle(visual).borderRadius??''};
}

export function captureTicketMotion(root:ParentNode):TicketMotionSnapshot{
  const rows=new Map<string,TicketMotionRow>();
  for(const container of motionContainers(root)){const row=currentRow(container),slug=row?.visual.dataset.ticketSlug;if(row&&slug)rows.set(slug,row)}
  return{scope:motionScope(root),rows};
}

export function animateTicketMotion(before:TicketMotionSnapshot,root:ParentNode,reduceMotion=matchMedia('(prefers-reduced-motion: reduce)').matches){
  if(reduceMotion||before.scope!==motionScope(root))return;
  const after=new Map<string,TicketMotionRow>();
  for(const container of motionContainers(root)){const row=currentRow(container),slug=row?.visual.dataset.ticketSlug;if(row&&slug)after.set(slug,row)}
  const removed=[...before.rows].filter(([slug])=>!after.has(slug)),incoming=[...after].filter(([slug])=>!before.rows.has(slug)),removedParents=new Set(removed.map(([,row])=>row.parent));
  const layoutMotion=new Map<string,{current:TicketMotionRow;previous:TicketMotionRow;x:number;y:number;movedColumn:boolean}>();
  for(const [slug,previous] of before.rows){
    const current=after.get(slug);
    if(!current)continue;
    const x=previous.rect.left-current.rect.left,y=previous.rect.top-current.rect.top;
    if(Math.abs(x)<.5&&Math.abs(y)<.5)continue;
    layoutMotion.set(slug,{current,previous,x,y,movedColumn:previous.parent!==current.parent});
  }
  const crossColumn=[...layoutMotion.values()].filter(item=>item.movedColumn);
  if(removed.length===0&&incoming.length===0&&crossColumn.length===0)return;
  const changedParents=new Set([...removedParents,...incoming.map(([,row])=>row.parent),...crossColumn.flatMap(item=>[item.previous.parent,item.current.parent])]);
  for(const [slug,previous] of removed)fadeRemovedTicket(slug,previous);
  for(const {current,previous,x,y,movedColumn} of layoutMotion.values()){
    if(movedColumn){animateMovedTicket(current,x,y);continue}
    if(!changedParents.has(previous.parent)||Math.abs(y)<.5)continue;
    animateLayout(current.container,y,removedParents.has(previous.parent)?FADE_DURATION:0);
  }
  for(const [slug,row] of incoming){
    const makesRoom=[...layoutMotion.values()].some(item=>!item.movedColumn&&Math.abs(item.y)>=.5&&item.current.parent===row.parent);
    fadeIncomingTicket(slug,row,makesRoom?LAYOUT_DURATION:0);
  }
}

function animateLayout(container:HTMLElement,y:number,delay:number){
  activeLayoutAnimations.get(container)?.cancel();
  const animation=container.animate([{transform:`translate(0px, ${y}px)`},{transform:'translate(0, 0)'}],{delay,duration:LAYOUT_DURATION,easing:MOTION_EASING,fill:'backwards'});
  activeLayoutAnimations.set(container,animation);
  animation.finished.finally(()=>{if(activeLayoutAnimations.get(container)===animation)activeLayoutAnimations.delete(container)}).catch(()=>undefined);
}

function fadeRemovedTicket(slug:string,previous:TicketMotionRow){
  if(hasGhost(previous.container.ownerDocument,'outgoing',slug))return;
  const ghost=previous.container.cloneNode(true) as HTMLElement;
  prepareGhost(ghost,slug,'outgoing',previous.rect,previous.borderRadius);appendGhost(previous.container.ownerDocument,ghost);
  removeAfter(ghost.animate([{opacity:1,transform:'scale(1)'},{opacity:0,transform:'scale(.985)'}],{duration:FADE_DURATION,easing:'ease-in'}),ghost);
}

function animateMovedTicket(current:TicketMotionRow,x:number,y:number){
  const slug=current.visual.dataset.ticketSlug??'';
  if(hasGhost(current.container.ownerDocument,'move',slug))return;
  const ghost=current.container.cloneNode(true) as HTMLElement,{visibility,hideRule}=hideRealTicket(current.container,slug);
  prepareGhost(ghost,slug,'move',current.rect,current.borderRadius);appendGhost(current.container.ownerDocument,ghost);
  const animation=ghost.animate([{transform:`translate(${x}px, ${y}px)`,zIndex:'1300'},{transform:'translate(0, 0)',zIndex:'1300'}],{duration:LAYOUT_DURATION,easing:MOTION_EASING});
  animation.finished.finally(()=>{ghost.remove();hideRule.remove();current.container.style.visibility=visibility}).catch(()=>{ghost.remove();hideRule.remove();current.container.style.visibility=visibility});
}

function fadeIncomingTicket(slug:string,row:TicketMotionRow,delay:number){
  if(hasGhost(row.container.ownerDocument,'incoming',slug))return;
  const ghost=row.container.cloneNode(true) as HTMLElement,{visibility,hideRule}=hideRealTicket(row.container,slug);
  prepareGhost(ghost,slug,'incoming',row.rect,row.borderRadius);appendGhost(row.container.ownerDocument,ghost);
  const stopTracking=trackTicketPosition(row.container.ownerDocument,ghost,slug);
  const animation=ghost.animate([{opacity:0},{opacity:1}],{delay,duration:FADE_DURATION,easing:'ease-out',fill:'backwards'});
  animation.finished.finally(()=>{stopTracking();ghost.remove();hideRule.remove();row.container.style.visibility=visibility}).catch(()=>{stopTracking();ghost.remove();hideRule.remove();row.container.style.visibility=visibility});
}

function hideRealTicket(container:HTMLElement,slug:string){
  const visibility=container.style.visibility,hideRule=container.ownerDocument.createElement('style'),escaped=container.ownerDocument.defaultView?.CSS.escape(slug)??slug;
  hideRule.dataset.ticketMotionHide=slug;hideRule.textContent=`[data-component="ticket-list-row-container"]:has(> [data-component="ticket-list-row"][data-ticket-slug="${escaped}"]){visibility:hidden!important}`;container.ownerDocument.head.append(hideRule);container.style.visibility='hidden';
  return{visibility,hideRule};
}

function prepareGhost(ghost:HTMLElement,slug:string,kind:'move'|'incoming'|'outgoing',rect:DOMRect,borderRadius:string){
  ghost.ariaHidden='true';ghost.inert=true;ghost.dataset.ticketMotionGhost=kind;ghost.dataset.ticketMotionSlug=slug;
  isolateGhostText(ghost);
  for(const element of [ghost,...ghost.querySelectorAll<HTMLElement>('*')]){
    delete element.dataset.ticketSlug;delete element.dataset.component;delete element.dataset.action;delete element.dataset.attachmentDropTarget;delete element.dataset.key;
    element.removeAttribute('role');element.removeAttribute('tabindex');element.removeAttribute('aria-selected');element.removeAttribute('aria-label');element.removeAttribute('draggable');element.removeAttribute('id');
  }
  const visual=ghost.querySelector<HTMLElement>('.ticket-list-row');if(visual&&borderRadius)visual.style.borderRadius=borderRadius;
  ghost.style.cssText=`position:fixed;z-index:1300;pointer-events:none;margin:0;left:${rect.left}px;top:${rect.top}px;width:${rect.width}px;height:${rect.height}px;will-change:transform,opacity`;
}

function isolateGhostText(ghost:HTMLElement){
  const createWalker=(ghost.ownerDocument as unknown as {createTreeWalker?:Document['createTreeWalker']}).createTreeWalker?.bind(ghost.ownerDocument);
  if(createWalker){
    const walker=createWalker(ghost,NodeFilter.SHOW_TEXT),nodes:Text[]=[];let node=walker.nextNode();
    while(node){if(node.textContent?.trim())nodes.push(node as Text);node=walker.nextNode()}
    for(const textNode of nodes){const visualText=ghost.ownerDocument.createElement('span');visualText.dataset.ticketMotionVisualText=textNode.data;textNode.replaceWith(visualText)}
  }else for(const element of ghost.querySelectorAll<HTMLElement>('*'))if(element.childElementCount===0&&element.textContent){element.dataset.ticketMotionVisualText=element.textContent;element.textContent=''}
  const style=ghost.ownerDocument.createElement('style');style.textContent='[data-ticket-motion-ghost] [data-ticket-motion-visual-text]::before{content:attr(data-ticket-motion-visual-text)}';ghost.prepend(style);
}

function hasGhost(document:Document,kind:string,slug:string){return[...document.querySelectorAll<HTMLElement>('[data-ticket-motion-ghost]')].some(existing=>existing.dataset.ticketMotionGhost===kind&&existing.dataset.ticketMotionSlug===slug)}

function trackTicketPosition(document:Document,ghost:HTMLElement,slug:string){
  const view=document.defaultView;
  if(!view?.requestAnimationFrame)return()=>undefined;
  let frame=0;
  const update=()=>{const visual=[...document.querySelectorAll<HTMLElement>('[data-component="ticket-list-row"][data-ticket-slug]')].find(candidate=>candidate.dataset.ticketSlug===slug),container=visual?.closest<HTMLElement>('[data-component="ticket-list-row-container"]');if(container){const rect=container.getBoundingClientRect();ghost.style.left=`${rect.left}px`;ghost.style.top=`${rect.top}px`;ghost.style.width=`${rect.width}px`;ghost.style.height=`${rect.height}px`}frame=view.requestAnimationFrame(update)};
  frame=view.requestAnimationFrame(update);return()=>{view.cancelAnimationFrame(frame)};
}

function appendGhost(document:Document,ghost:HTMLElement){
  for(const existing of document.querySelectorAll<HTMLElement>('[data-ticket-motion-ghost]'))if(existing.dataset.ticketMotionGhost===ghost.dataset.ticketMotionGhost&&existing.dataset.ticketMotionSlug===ghost.dataset.ticketMotionSlug)existing.remove();
  document.body.append(ghost);
}

function removeAfter(animation:Animation,element:HTMLElement){animation.finished.finally(()=>{element.remove()}).catch(()=>{element.remove()})}
