import type { ConversationActivity, ConversationMessage, ConversationState } from './ai-conversation';

export const CONVERSATION_STORAGE_KEY='hotsheet.ai-conversations.v1';

interface StorageLike {getItem(key:string):string|null;setItem(key:string,value:string):void}

function message(value:unknown):value is ConversationMessage{
  if(!value||typeof value!=='object')return false;
  const item=value as Partial<ConversationMessage>;
  return typeof item.id==='string'&&(item.role==='user'||item.role==='assistant')&&typeof item.content==='string';
}

function activity(value:unknown):value is ConversationActivity{
  if(!value||typeof value!=='object')return false;
  const item=value as Partial<ConversationActivity>;
  return typeof item.id==='string'&&typeof item.tool==='string'&&typeof item.kind==='string'&&typeof item.summary==='string'&&['low','normal','high'].includes(item.importance??'');
}

function state(value:unknown):value is ConversationState{
  if(!value||typeof value!=='object')return false;
  const item=value as Partial<ConversationState>;
  return Array.isArray(item.messages)&&item.messages.every(message)&&(!item.activity||Array.isArray(item.activity)&&item.activity.every(activity));
}

export function loadConversationStates(storage:Pick<StorageLike,'getItem'>):Record<string,ConversationState>{
  try{
    const parsed:unknown=JSON.parse(storage.getItem(CONVERSATION_STORAGE_KEY)??'{}');
    if(!parsed||typeof parsed!=='object'||Array.isArray(parsed))return{};
    const valid:Record<string,ConversationState>={};
    for(const[key,value]of Object.entries(parsed))if(key&&state(value))valid[key]=value;
    return valid;
  }catch{return{}}
}

export function saveConversationStates(storage:Pick<StorageLike,'setItem'>,states:Record<string,ConversationState>):void{
  storage.setItem(CONVERSATION_STORAGE_KEY,JSON.stringify(states));
}
