import {signal} from 'kerfjs';

import {type ConversationActivity,type ConversationMessage,conversationUsage} from '../ai-conversation';
import {AIConversation} from '../components/ai-conversation';
import type {PermissionItem} from '../permission-notifications';

export type AIConversationScenario='empty'|'streaming'|'permission'|'completed'|'usage-unpriced'|'failed'|'interrupted';
export const aiConversationScenario=signal<AIConversationScenario>('streaming');
export const aiConversationDemoOpen=signal(true);
export const aiConversationDraft=signal('Can you also explain the compatibility boundary?');

const completed:ConversationMessage[]=[
  {id:'question-1',role:'user',content:'Review the client connection flow.'},
  {id:'answer-1',role:'assistant',status:'completed',content:'The connection uses the project event stream, so the transcript updates without simple polling.\n\n- Turns stay in one session.\n- Permissions appear inline.',usage:{tokensIn:18_234,tokensOut:2_101,costUsd:.0423,model:'codex-5.6'}},
];
const activity:ConversationActivity[]=[{id:'activity-1',tool:'Codex',kind:'edit',summary:'Edited the conversation state boundary',importance:'normal'},{id:'activity-2',tool:'Codex',kind:'command',summary:'Ran the focused browser test',importance:'normal'}];
const permission:PermissionItem={id:42,connection:'hotsheet-sidebar',tool:'Bash',action:'npm run test',always_allow_supported:true,key:'demo:42',projectId:'demo',projectName:'Hot Sheet 2',agent:'Codex',role:'main worker',receivedAt:Date.now(),ignored:false};

function scenarioState(){
  const scenario=aiConversationScenario.value;
  if(scenario==='empty')return{messages:[] as ConversationMessage[],busy:false,interruptible:false};
  if(scenario==='completed')return{messages:completed,busy:false,interruptible:false,activity};
  if(scenario==='usage-unpriced')return{messages:completed.map(message=>message.role==='assistant'?{...message,usage:{tokensIn:8_420,tokensOut:943,model:'unpriced-model'}}:message),busy:false,interruptible:false,activity};
  if(scenario==='interrupted')return{messages:[...completed,{id:'question-2',role:'user' as const,content:'Run the full suite.'},{id:'answer-2',role:'assistant' as const,status:'interrupted' as const,content:'Stopped before the suite completed.'}],busy:false,interruptible:false};
  if(scenario==='failed')return{messages:[...completed,{id:'question-2',role:'user' as const,content:'Inspect the server.'},{id:'answer-2',role:'assistant' as const,status:'failed' as const,content:'The turn ended without output.'}],busy:false,interruptible:false,error:'The tool turn failed.'};
  const messages=[...completed,{id:'question-2',role:'user' as const,content:'Run the focused browser test.'},{id:'answer-2',role:'assistant' as const,status:'streaming' as const,content:scenario==='streaming'?'I found the production route and I’m checking its':''}];
  return{messages,busy:true,interruptible:true,progress:scenario==='permission'?'Waiting for permission…':'Running the focused browser test…',permissions:scenario==='permission'?[permission]:undefined};
}

export function AIConversationDemo(){const state=scenarioState();return <section aria-label="AIConversation demo"><wa-button appearance="accent" data-action="open-ai-conversation-demo">Open conversation</wa-button><AIConversation open={aiConversationDemoOpen.value} tool="Codex" sessionId="019-demo-session" messages={state.messages} draft={aiConversationDraft.value} busy={state.busy} progress={state.progress} interruptible={state.interruptible} permissions={state.permissions} activity={state.activity} totalUsage={conversationUsage({messages:state.messages})} error={state.error} feedbackAvailable/></section>}

export function AIConversationSettings(){return <form class="settings-form" data-settings="ai-conversation"><wa-select name="scenario" label="Public state" value={aiConversationScenario.value}><wa-option value="empty">Empty</wa-option><wa-option value="streaming">Streaming</wa-option><wa-option value="permission">Permission requested</wa-option><wa-option value="completed">Completed with priced usage</wa-option><wa-option value="usage-unpriced">Completed with unpriced usage</wa-option><wa-option value="failed">Failed</wa-option><wa-option value="interrupted">Interrupted</wa-option></wa-select><wa-button type="button" data-action="open-ai-conversation-demo">Open conversation</wa-button></form>}
