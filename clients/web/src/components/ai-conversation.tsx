import './ai-conversation.css';

import {Bot,MessageSquare,Send,Square,X} from 'lucide';

import type { ConversationMessage } from '../ai-conversation';
import type { PermissionItem } from '../permission-notifications';
import {LucideIcon} from './lucide-icon';
import {MarkdownPreview} from './markdown-preview';
import {PermissionRequestCard} from './permission-request-card';

export interface AIConversationProps {open:boolean;tool:string;sessionId?:string;messages:ConversationMessage[];draft:string;busy:boolean;progress?:string;interruptible:boolean;permissions?:PermissionItem[];error?:string}

export function AIConversation({open,tool,sessionId,messages,draft,busy,progress,interruptible,permissions=[],error}:AIConversationProps){
  return <wa-dialog class="ai-conversation" data-component="ai-conversation" label={`${tool} conversation`} open={open||undefined} with-footer>
    <header class="ai-conversation__header"><span><LucideIcon icon={Bot} name="bot"/><span><strong>{tool}</strong><small>{sessionId?`Session ${sessionId}`:'Project conversation'}</small></span></span><div>{busy&&interruptible&&<button type="button" data-action="stop-conversation" aria-label={`Stop ${tool}`} title={`Stop ${tool}`}><LucideIcon icon={Square} name="square"/></button>}<button type="button" data-action="close-conversation" aria-label="Close conversation" title="Close conversation"><LucideIcon icon={X} name="x"/></button></div></header>
    <section class="ai-conversation__transcript" aria-label="Conversation transcript" aria-live="polite">{messages.length===0&&<div class="ai-conversation__empty"><LucideIcon icon={MessageSquare} name="message-square"/><strong>Start a conversation</strong><p>Ask {tool} about this project or send it a task.</p></div>}{messages.map(message=><article class={`ai-conversation__message ai-conversation__message--${message.role}`} data-message-id={message.id} data-status={message.status}><strong>{message.role==='user'?'You':tool}</strong>{message.content?<MarkdownPreview source={message.content}/>:<p aria-label="Awaiting response"> </p>}</article>)}{permissions.map(item=><div data-permission-key={item.key}><PermissionRequestCard item={item} presentation="list"/></div>)}{busy&&progress&&<p class="ai-conversation__progress" role="status"><LucideIcon icon={Bot} name="bot"/>{progress}</p>}{error&&<p class="ai-conversation__error" role="alert">{error}</p>}</section>
    <form slot="footer" class="ai-conversation__composer" data-action="send-conversation-turn"><textarea name="conversation-draft" aria-label={`Message ${tool}`} placeholder={`Message ${tool}…`} rows={3} disabled={busy}>{draft}</textarea><button type="submit" disabled={busy||!draft.trim()} aria-label={`Send message to ${tool}`} title={busy?`${tool} is still working`:`Send message to ${tool}`}><LucideIcon icon={Send} name="send"/></button></form>
  </wa-dialog>;
}
