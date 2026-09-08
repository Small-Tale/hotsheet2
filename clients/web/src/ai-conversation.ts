import type { ClientTurnEvent } from './api';

export type ConversationMessageRole = 'user' | 'assistant';
export type ConversationMessageStatus = 'streaming' | 'completed' | 'failed' | 'interrupted';
export interface ConversationMessage { id:string; role:ConversationMessageRole; content:string; status?:ConversationMessageStatus }
export interface ConversationState { messages:ConversationMessage[]; activeAssistantId?:string; progress?:string; error?:string }

export const EMPTY_CONVERSATION:ConversationState={messages:[]};

export function beginConversationTurn(state:ConversationState,id:string,content:string):ConversationState{
  const assistantId=`${id}-assistant`;
  return{messages:[...state.messages,{id,role:'user',content},{id:assistantId,role:'assistant',content:'',status:'streaming'}],activeAssistantId:assistantId,progress:'Reviewing the project and planning the next steps…'};
}

function nativeProgress(payload:unknown):string|undefined{
  if(!payload||typeof payload!=='object')return;
  const value=payload as Record<string,unknown>;
  for(const key of ['summary','message','title','kind'])if(typeof value[key]==='string'&&value[key].trim())return value[key].trim().slice(0,160);
}

export function applyConversationEvent(state:ConversationState,event:ClientTurnEvent):ConversationState{
  const active=state.activeAssistantId,index=active?state.messages.findIndex(message=>message.id===active):-1;
  if(event.type==='permission_asked')return{...state,progress:'Waiting for permission…'};
  if(event.type==='native_activity')return{...state,progress:nativeProgress(event.payload)??'Working through the requested changes…'};
  if(event.type==='output'){
    if(index<0)return state;
    const content=typeof event.content==='string'?event.content:'';
    const messages=state.messages.map((message,messageIndex)=>messageIndex===index?{...message,content:`${message.content}${content}`,status:'streaming' as const}:message);
    return{...state,messages,progress:'Responding…'};
  }
  if(event.type==='done'){
    if(index<0)return{...state,activeAssistantId:undefined,progress:undefined};
    const reason=event.reason==='failed'||event.reason==='interrupted'?event.reason:'completed';
    const status:ConversationMessageStatus=reason;
    const messages=state.messages.map((message,messageIndex)=>messageIndex===index?{...message,status,content:message.content||(status==='interrupted'?'Stopped before a response was completed.':'The turn ended without output.')}:message);
    return{...state,messages,activeAssistantId:undefined,progress:undefined,error:reason==='failed'?'The tool turn failed.':undefined};
  }
  return state;
}
