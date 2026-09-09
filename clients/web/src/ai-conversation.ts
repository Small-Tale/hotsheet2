import type {ActivityEvent, ClientTurnEvent} from './api';

export type ConversationMessageRole = 'user' | 'assistant';
export type ConversationMessageStatus = 'streaming' | 'completed' | 'failed' | 'interrupted';
export interface ConversationUsage {tokensIn:number;tokensOut:number;costUsd?:number;model?:string}
export interface ConversationMessage { id:string; role:ConversationMessageRole; content:string; status?:ConversationMessageStatus;usage?:ConversationUsage }
export interface ConversationActivity {id:string;tool:string;kind:string;summary:string;importance:'low'|'normal'|'high'}
export interface ConversationState { messages:ConversationMessage[]; activity?:ConversationActivity[]; activeAssistantId?:string; progress?:string; error?:string }

export const EMPTY_CONVERSATION:ConversationState={messages:[]};

export function beginConversationTurn(state:ConversationState,id:string,content:string):ConversationState{
  const assistantId=`${id}-assistant`;
  return{...state,messages:[...state.messages,{id,role:'user',content},{id:assistantId,role:'assistant',content:'',status:'streaming'}],activeAssistantId:assistantId,progress:'Reviewing the project and planning the next steps…',error:undefined};
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
  if(event.type==='usage'){
    if(index<0)return state;
    const tokensIn=typeof event.tokens_in==='number'?event.tokens_in:0,tokensOut=typeof event.tokens_out==='number'?event.tokens_out:0,costUsd=typeof event.cost_usd==='number'?event.cost_usd:undefined,model=typeof event.model==='string'?event.model:undefined;
    const messages=state.messages.map((message,messageIndex)=>messageIndex===index?{...message,usage:{tokensIn:(message.usage?.tokensIn??0)+tokensIn,tokensOut:(message.usage?.tokensOut??0)+tokensOut,...(costUsd!==undefined?{costUsd:(message.usage?.costUsd??0)+costUsd}:message.usage?.costUsd!==undefined?{costUsd:message.usage.costUsd}:{}),model:model??message.usage?.model}}:message);
    return{...state,messages};
  }
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

export function applyConversationActivity(state:ConversationState,event:ActivityEvent):ConversationState{
  const current=state.activity??[];
  if(current.some(item=>item.id===event.id))return state;
  return{...state,activity:[...current,{id:event.id,tool:event.tool,kind:event.kind,summary:event.summary,importance:event.importance}].slice(-50)};
}

export function conversationUsage(state:ConversationState):ConversationUsage|undefined{
  const events=state.messages.flatMap(message=>message.usage?[message.usage]:[]);
  if(!events.length)return;
  const priced=events.filter(event=>event.costUsd!==undefined);
  return{tokensIn:events.reduce((sum,event)=>sum+event.tokensIn,0),tokensOut:events.reduce((sum,event)=>sum+event.tokensOut,0),...(priced.length===events.length?{costUsd:priced.reduce((sum,event)=>sum+event.costUsd!,0)}:{})};
}

export function formatConversationTokens(value:number):string{return new Intl.NumberFormat('en-US',{notation:value>=10_000?'compact':'standard',maximumFractionDigits:1}).format(value)}
export function formatConversationCost(value?:number):string{return value===undefined?'Cost unavailable':`≈$${value<0.01?value.toFixed(4):value.toFixed(2)}`}
