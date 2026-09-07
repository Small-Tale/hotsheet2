interface TicketMotionRow {rect:DOMRect;parent:string;element:HTMLElement}
export interface TicketMotionSnapshot {scope:string;rows:Map<string,TicketMotionRow>}

function motionScope(root:ParentNode){const workspace=root.querySelector<HTMLElement>('.app-shell__workspace'),collection=workspace?.querySelector<HTMLElement>('[data-component="ticket-list"], [data-component="ticket-board"]');return`${workspace?.dataset.presentation??''}:${collection?.dataset.component??''}`}
function motionRows(root:ParentNode){return [...root.querySelectorAll<HTMLElement>('[data-component="ticket-list-row"][data-ticket-slug]')]}
function parentKey(row:HTMLElement){return row.closest<HTMLElement>('[data-column-id]')?.dataset.columnId??row.closest<HTMLElement>('[data-component="ticket-list"]')?.dataset.component??''}

export function captureTicketMotion(root:ParentNode):TicketMotionSnapshot{
  return{scope:motionScope(root),rows:new Map(motionRows(root).map(row=>[row.dataset.ticketSlug!,{rect:row.getBoundingClientRect(),parent:parentKey(row),element:row}]))}
}

export function animateTicketMotion(before:TicketMotionSnapshot,root:ParentNode,reduceMotion=matchMedia('(prefers-reduced-motion: reduce)').matches){
  if(reduceMotion||before.rows.size===0||before.scope!==motionScope(root))return;
  const after=new Map(motionRows(root).map(row=>[row.dataset.ticketSlug!,row]));
  const hasRemovedRows=[...before.rows.keys()].some(slug=>!after.has(slug));
  for(const [slug,previous] of before.rows){
    const row=after.get(slug);
    if(!row){fadeRemovedTicket(previous);continue}
    const rect=row.getBoundingClientRect(),x=previous.rect.left-rect.left,y=previous.rect.top-rect.top;
    if(Math.abs(x)<.5&&Math.abs(y)<.5)continue;
    const movedColumn=previous.parent!==parentKey(row);
    row.animate([{transform:`translate(${x}px, ${y}px)`,zIndex:movedColumn?'30':'2'},{transform:'translate(0, 0)',zIndex:movedColumn?'30':'2'}],{delay:hasRemovedRows?130:0,duration:240,easing:'cubic-bezier(.2,.8,.2,1)',fill:'backwards'});
  }
  for(const [slug,row] of after)if(!before.rows.has(slug))row.animate([{opacity:0},{opacity:0,offset:.45},{opacity:1}],{duration:260,easing:'ease-out'});
}

function fadeRemovedTicket(previous:TicketMotionRow){
  const ghost=previous.element.cloneNode(true) as HTMLElement,rect=previous.rect;
  ghost.ariaHidden='true';ghost.style.cssText=`position:fixed;z-index:1300;pointer-events:none;margin:0;left:${rect.left}px;top:${rect.top}px;width:${rect.width}px;height:${rect.height}px`;
  document.body.append(ghost);
  const animation=ghost.animate([{opacity:1,transform:'scale(1)'},{opacity:0,transform:'scale(.985)'}],{duration:150,easing:'ease-in'});
  animation.finished.finally(()=>{ghost.remove()}).catch(()=>{ghost.remove()});
}
