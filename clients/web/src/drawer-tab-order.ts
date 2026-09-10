import {applyRememberedTabOrder,reorderTabs,type TabDropPosition} from './tab-order';

export interface DrawerOrderStorage {getItem(key:string):string|null;setItem(key:string,value:string):void}

export function drawerTabOrderStorageKey(projectId:string):string{return `hotsheet.project.${projectId}.terminal-drawer-order`}

export function parseDrawerTabOrder(raw:string|null):string[]{
  if(!raw)return[];
  try{
    const parsed=JSON.parse(raw) as unknown;
    return Array.isArray(parsed)?[...new Set(parsed.filter((item):item is string=>typeof item==='string'&&item.length>0))]:[];
  }catch{return[]}
}

export function loadDrawerTabOrder(storage:DrawerOrderStorage,projectId:string):string[]{return parseDrawerTabOrder(storage.getItem(drawerTabOrderStorageKey(projectId)))}

export function saveDrawerTabOrder(storage:DrawerOrderStorage,projectId:string,ids:readonly string[]):string[]{
  const order=[...new Set(ids.filter(Boolean))];
  storage.setItem(drawerTabOrderStorageKey(projectId),JSON.stringify(order));
  return order;
}

export function orderedDrawerTabIds(terminalIds:readonly string[],chatIds:readonly string[],remembered:readonly string[]):string[]{
  return applyRememberedTabOrder([...terminalIds,...chatIds],id=>id,remembered);
}

export function reorderDrawerTabIds(ids:readonly string[],sourceId:string,targetId:string,position:TabDropPosition):string[]{return reorderTabs(ids,id=>id,sourceId,targetId,position)}

export function keyboardReorderDrawerTabIds(ids:readonly string[],sourceId:string,direction:'left'|'right'):string[]{
  const index=ids.indexOf(sourceId),targetIndex=index+(direction==='left'?-1:1);
  if(index<0||targetIndex<0||targetIndex>=ids.length)return[...ids];
  return reorderDrawerTabIds(ids,sourceId,ids[targetIndex],direction==='left'?'before':'after');
}

export function drawerTabSelectionAfterClose(ids:readonly string[],selectedId:string,closingIds:readonly string[]):string{
  const closing=new Set(closingIds);
  if(!closing.has(selectedId))return selectedId;
  const index=ids.indexOf(selectedId);
  return ids.find((id,itemIndex)=>itemIndex>index&&!closing.has(id))??[...ids].reverse().find((id,itemIndex)=>ids.length-1-itemIndex<index&&!closing.has(id))??'grid';
}
