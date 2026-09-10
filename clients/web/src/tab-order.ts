export type TabDropPosition='before'|'after';

export function reorderTabs<T>(items:readonly T[],id:(item:T)=>string,sourceId:string,targetId:string,position:TabDropPosition):T[]{
  if(sourceId===targetId)return[...items];
  const source=items.find(item=>id(item)===sourceId),targetIndex=items.findIndex(item=>id(item)===targetId);
  if(!source||targetIndex<0)return[...items];
  const remaining=items.filter(item=>id(item)!==sourceId),adjustedTarget=remaining.findIndex(item=>id(item)===targetId);
  remaining.splice(adjustedTarget+(position==='after'?1:0),0,source);
  return remaining;
}

/** Apply a remembered partial order while keeping newly discovered tabs in source order. */
export function applyRememberedTabOrder<T>(items:readonly T[],id:(item:T)=>string,remembered:readonly string[]):T[]{
  const rank=new Map(remembered.map((value,index)=>[value,index]));
  return items.map((item,index)=>({item,index,rank:rank.get(id(item))})).sort((left,right)=>{
    if(left.rank===undefined&&right.rank===undefined)return left.index-right.index;
    if(left.rank===undefined)return 1;
    if(right.rank===undefined)return-1;
    return left.rank-right.rank;
  }).map(entry=>entry.item);
}

export function replaceTabInPlace<T>(items:readonly T[],id:(item:T)=>string,next:T):T[]{
  const index=items.findIndex(item=>id(item)===id(next));
  if(index<0)return[...items,next];
  return items.map((item,itemIndex)=>itemIndex===index?next:item);
}
