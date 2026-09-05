export interface TicketScrollPosition { top:number; left:number }
export type TicketScrollState=Map<string,TicketScrollPosition>;

const SCROLL_OWNER='[data-ticket-scroll-owner]';

export function captureTicketScrollState(root:ParentNode=document):TicketScrollState{
  return new Map([...root.querySelectorAll<HTMLElement>(SCROLL_OWNER)].flatMap(element=>{
    const key=element.dataset.ticketScrollOwner;
    return key?[[key,{top:element.scrollTop,left:element.scrollLeft}] as const]:[];
  }));
}

export function restoreTicketScrollState(state:TicketScrollState,root:ParentNode=document):void{
  for(const element of root.querySelectorAll<HTMLElement>(SCROLL_OWNER)){
    const position=state.get(element.dataset.ticketScrollOwner??'');
    if(position)element.scrollTo(position.left,position.top);
  }
}
