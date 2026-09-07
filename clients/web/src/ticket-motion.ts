interface TicketMotionRow {rect:DOMRect;parent:string;element:HTMLElement}
export interface TicketMotionSnapshot {scope:string;rows:Map<string,TicketMotionRow>}

const LAYOUT_DURATION=240,FADE_DURATION=160,MOTION_EASING='cubic-bezier(.2,.8,.2,1)';

function motionScope(root:ParentNode){const workspace=root.querySelector<HTMLElement>('.app-shell__workspace'),collection=workspace?.querySelector<HTMLElement>('[data-component="ticket-list"], [data-component="ticket-board"]');return`${workspace?.dataset.presentation??''}:${collection?.dataset.component??''}`}
function motionRows(root:ParentNode){return [...root.querySelectorAll<HTMLElement>('[data-component="ticket-list-row"][data-ticket-slug]')]}
function parentKey(row:HTMLElement){return row.closest<HTMLElement>('[data-column-id]')?.dataset.columnId??row.closest<HTMLElement>('[data-component="ticket-list"]')?.dataset.component??''}

export function captureTicketMotion(root:ParentNode):TicketMotionSnapshot{
  return{scope:motionScope(root),rows:new Map(motionRows(root).map(row=>[row.dataset.ticketSlug!,{rect:row.getBoundingClientRect(),parent:parentKey(row),element:row}]))}
}

export function animateTicketMotion(before:TicketMotionSnapshot,root:ParentNode,reduceMotion=matchMedia('(prefers-reduced-motion: reduce)').matches){
  if(reduceMotion||before.rows.size===0||before.scope!==motionScope(root))return;
  const after=new Map(motionRows(root).map(row=>[row.dataset.ticketSlug!,row]));
  const removed=[...before.rows].filter(([slug])=>!after.has(slug)),incoming=[...after].filter(([slug])=>!before.rows.has(slug)),removedParents=new Set(removed.map(([,row])=>row.parent));
  const layoutMotion=new Map<string,{row:HTMLElement;previous:TicketMotionRow;rect:DOMRect;x:number;y:number;movedColumn:boolean}>();
  for(const [slug,previous] of before.rows){
    const row=after.get(slug);
    if(!row)continue;
    const rect=row.getBoundingClientRect(),x=previous.rect.left-rect.left,y=previous.rect.top-rect.top;
    if(Math.abs(x)<.5&&Math.abs(y)<.5)continue;
    const movedColumn=previous.parent!==parentKey(row);
    layoutMotion.set(slug,{row,previous,rect,x,y,movedColumn});
  }
  const crossColumn=[...layoutMotion.values()].filter(item=>item.movedColumn);
  if(removed.length===0&&incoming.length===0&&crossColumn.length===0)return;
  const changedParents=new Set([...removedParents,...incoming.map(([,row])=>parentKey(row)),...crossColumn.flatMap(item=>[item.previous.parent,parentKey(item.row)])]);
  for(const [,previous] of removed)fadeRemovedTicket(previous);
  for(const {row,previous,rect,x,y,movedColumn} of layoutMotion.values()){
    if(movedColumn){animateMovedTicket(previous,row,rect,x,y);continue}
    if(!changedParents.has(previous.parent)||Math.abs(y)<.5)continue;
    const delay=removedParents.has(previous.parent)?FADE_DURATION:0;
    row.animate([{transform:`translate(0px, ${y}px)`},{transform:'translate(0, 0)'}],{delay,duration:LAYOUT_DURATION,easing:MOTION_EASING,fill:'backwards'});
  }
  for(const [,row] of incoming){
    const parent=parentKey(row),makesRoom=[...layoutMotion.values()].some(item=>!item.movedColumn&&Math.abs(item.y)>=.5&&parentKey(item.row)===parent);
    fadeIncomingTicket(row,makesRoom?LAYOUT_DURATION:0);
  }
}

function fadeRemovedTicket(previous:TicketMotionRow){
  const ghost=previous.element.cloneNode(true) as HTMLElement,rect=previous.rect;
  if(hasGhost(previous.element.ownerDocument,'outgoing',previous.element.dataset.ticketSlug??''))return;
  prepareGhost(ghost,'outgoing',rect);appendGhost(previous.element.ownerDocument,ghost);
  removeAfter(ghost.animate([{opacity:1,transform:'scale(1)'},{opacity:0,transform:'scale(.985)'}],{duration:FADE_DURATION,easing:'ease-in'}),ghost);
}

function animateMovedTicket(previous:TicketMotionRow,row:HTMLElement,rect:DOMRect,x:number,y:number){
  const slug=row.dataset.ticketSlug??'';
  if(hasGhost(row.ownerDocument,'move',slug))return;
  const ghost=row.cloneNode(true) as HTMLElement,{visibility,hideRule}=hideRealTicket(row);
  prepareGhost(ghost,'move',rect);appendGhost(row.ownerDocument,ghost);
  const animation=ghost.animate([{transform:`translate(${x}px, ${y}px)`,zIndex:'1300'},{transform:'translate(0, 0)',zIndex:'1300'}],{duration:LAYOUT_DURATION,easing:MOTION_EASING});
  animation.finished.finally(()=>{ghost.remove();hideRule.remove();row.style.visibility=visibility}).catch(()=>{ghost.remove();hideRule.remove();row.style.visibility=visibility});
}

function fadeIncomingTicket(row:HTMLElement,delay:number){
  const slug=row.dataset.ticketSlug??'';
  if(hasGhost(row.ownerDocument,'incoming',slug))return;
  const ghost=row.cloneNode(true) as HTMLElement,{visibility,hideRule}=hideRealTicket(row);
  prepareGhost(ghost,'incoming',row.getBoundingClientRect());appendGhost(row.ownerDocument,ghost);
  const stopTracking=trackTicketPosition(row.ownerDocument,ghost,slug);
  const animation=ghost.animate([{opacity:0},{opacity:1}],{delay,duration:FADE_DURATION,easing:'ease-out',fill:'backwards'});
  animation.finished.finally(()=>{stopTracking();ghost.remove();hideRule.remove();row.style.visibility=visibility}).catch(()=>{stopTracking();ghost.remove();hideRule.remove();row.style.visibility=visibility});
}

function hideRealTicket(row:HTMLElement){
  const visibility=row.style.visibility,hideRule=row.ownerDocument.createElement('style'),slug=row.dataset.ticketSlug??'';
  hideRule.dataset.ticketMotionHide=slug;hideRule.textContent=`[data-component="ticket-list-row"][data-ticket-slug="${row.ownerDocument.defaultView?.CSS.escape(slug)??slug}"]:not([data-ticket-motion-ghost]){visibility:hidden!important}`;row.ownerDocument.head.append(hideRule);row.style.visibility='hidden';
  return{visibility,hideRule};
}

function prepareGhost(ghost:HTMLElement,kind:'move'|'incoming'|'outgoing',rect:DOMRect){
  const slug=ghost.dataset.ticketSlug??'';
  ghost.ariaHidden='true';ghost.dataset.ticketMotionGhost=kind;ghost.dataset.ticketMotionSlug=slug;delete ghost.dataset.ticketSlug;delete ghost.dataset.component;delete ghost.dataset.action;delete ghost.dataset.attachmentDropTarget;ghost.removeAttribute('role');ghost.removeAttribute('tabindex');ghost.removeAttribute('aria-selected');ghost.removeAttribute('aria-label');ghost.removeAttribute('draggable');ghost.removeAttribute('id');isolateGhostText(ghost);ghost.style.cssText=`position:fixed;z-index:1300;pointer-events:none;margin:0;left:${rect.left}px;top:${rect.top}px;width:${rect.width}px;height:${rect.height}px;will-change:transform,opacity`;
}

function isolateGhostText(ghost:HTMLElement){
  for(const element of ghost.querySelectorAll<HTMLElement>('*'))if(element.childElementCount===0&&element.textContent){element.dataset.ticketMotionVisualText=element.textContent;element.textContent=''}
  const style=ghost.ownerDocument.createElement('style');style.textContent='[data-ticket-motion-ghost] [data-ticket-motion-visual-text]::before{content:attr(data-ticket-motion-visual-text)}';ghost.prepend(style);
}

function hasGhost(document:Document,kind:string,slug:string){return[...document.querySelectorAll<HTMLElement>('[data-ticket-motion-ghost]')].some(existing=>existing.dataset.ticketMotionGhost===kind&&existing.dataset.ticketMotionSlug===slug)}

function trackTicketPosition(document:Document,ghost:HTMLElement,slug:string){
  const view=document.defaultView;
  if(!view?.requestAnimationFrame)return()=>undefined;
  let frame=0;
  const update=()=>{const row=[...document.querySelectorAll<HTMLElement>('[data-component="ticket-list-row"][data-ticket-slug]:not([data-ticket-motion-ghost])')].find(candidate=>candidate.dataset.ticketSlug===slug);if(row){const rect=row.getBoundingClientRect();ghost.style.left=`${rect.left}px`;ghost.style.top=`${rect.top}px`;ghost.style.width=`${rect.width}px`;ghost.style.height=`${rect.height}px`}frame=view.requestAnimationFrame(update)};
  frame=view.requestAnimationFrame(update);return()=>{view.cancelAnimationFrame(frame)};
}

function appendGhost(document:Document,ghost:HTMLElement){
  for(const existing of document.querySelectorAll<HTMLElement>('[data-ticket-motion-ghost]'))if(existing.dataset.ticketMotionGhost===ghost.dataset.ticketMotionGhost&&existing.dataset.ticketMotionSlug===ghost.dataset.ticketMotionSlug)existing.remove();
  document.body.append(ghost);
}

function removeAfter(animation:Animation,element:HTMLElement){
  animation.finished.finally(()=>{element.remove()}).catch(()=>{element.remove()});
}
