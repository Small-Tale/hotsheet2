import '@awesome.me/webawesome/dist/components/button/button.js';
import './note-card.css';

import { Activity, CircleAlert, FilePenLine, MessageSquareText, RefreshCw, Trash2, X } from 'lucide';

import type {AttachmentReferenceContext} from '../attachment-references';
import { type InlineFeedbackReply,splitFeedbackPrompt } from '../feedback-replies';
import { LucideIcon } from './lucide-icon';
import { MarkdownPreview } from './markdown-preview';

export type NoteKind = 'regular' | 'status' | 'feedback_needed' | 'feedback_draft' | 'activity';
export interface NoteCardProps { id: string; kind: NoteKind; author: string; time: string; body: string; title?: string; editable?: boolean; deletable?: boolean; editing?: boolean; draft?: string; readerMode?: boolean; respondToFeedback?: boolean; inlineReplies?: readonly InlineFeedbackReply[];attachmentContext?:AttachmentReferenceContext }

const presentations = {
  regular: { label: 'Note', icon: MessageSquareText, iconName: 'message-square-text' },
  status: { label: 'Status update', icon: RefreshCw, iconName: 'refresh-cw' },
  feedback_needed: { label: 'Feedback needed', icon: CircleAlert, iconName: 'circle-alert' },
  feedback_draft: { label: 'Feedback draft', icon: FilePenLine, iconName: 'file-pen-line' },
  activity: { label: 'Activity', icon: Activity, iconName: 'activity' },
} as const;

export function NoteCard({ id, kind, author, time, body, title, editable = true, deletable = true, editing = false, draft, readerMode = false, respondToFeedback = false, inlineReplies = [],attachmentContext }: NoteCardProps) {
  const presentation = presentations[kind];
  const feedbackEditor = readerMode && (kind === 'feedback_needed' || kind === 'feedback_draft');
  const feedbackResponse = readerMode && kind === 'feedback_needed';
  const editorOpen = editing || feedbackEditor;
  const source = draft ?? (feedbackResponse ? '' : body);
  const editAttributes=editable&&!editorOpen?{'data-edit-on-double-click':'true',role:'button',tabIndex:0,'aria-label':'Edit note',title:'Double-click to edit'}:{};
  const acknowledgement=kind==='regular'&&body.trim()==='No response needed';
  return <article class={`note-card${editorOpen ? ' note-card--editing' : ''}`} data-component="note-card" data-note-id={id} data-kind={kind} data-acknowledgement={acknowledgement?'true':undefined} data-edit-on-double-click={editable&&!editorOpen?'true':undefined} title={editable&&!editorOpen?'Double-click to edit':undefined}>
    <header class="note-card__header">
      <span class="note-card__kind"><LucideIcon icon={presentation.icon} name={presentation.iconName} />{title ?? presentation.label}</span>
      <span class="note-card__header-end">{!editorOpen && deletable && <span class="note-card__actions"><button type="button" data-action="delete-note" data-note-id={id} aria-label="Delete note"><LucideIcon icon={Trash2} name="trash-2" /></button></span>}<time>{time}</time></span>
    </header>
    {feedbackResponse && <div class="note-card__feedback-prompt">{splitFeedbackPrompt(body, inlineReplies).map(segment => <div class="note-card__feedback-section">{segment.markdown && <div class="note-card__feedback-block" data-action="add-inline-feedback-reply" data-note-id={id} data-segment-start={segment.start} data-segment-end={segment.end} role="button" tabIndex={0} aria-label="Add response at a character position"><MarkdownPreview source={segment.markdown} attachmentContext={attachmentContext} /></div>}{segment.reply && <div class="note-card__inline-reply-row"><textarea class="note-card__inline-reply" name="inline-feedback-response" data-note-id={id} data-offset={segment.reply.offset} aria-label={`Response at character ${segment.reply.offset}`}>{segment.reply.text}</textarea><button type="button" data-action="remove-inline-feedback-reply" data-note-id={id} data-offset={segment.reply.offset} aria-label={`Remove response at character ${segment.reply.offset}`}><LucideIcon icon={X} name="x" /></button></div>}</div>)}</div>}
    {editorOpen ? <div class="note-card__editor"><textarea name="note-body" data-note-id={id} data-note-response={feedbackResponse ? 'true' : undefined} aria-label={feedbackResponse ? 'Feedback response' : 'Note body'} placeholder={feedbackResponse && inlineReplies.length ? 'General response (optional)' : undefined}>{source}</textarea>{feedbackEditor && <div>{feedbackResponse&&<wa-button size="small" appearance="outlined" data-action="dismiss-feedback" data-note-id={id} title="Clear this feedback request without replying">No response needed</wa-button>}<wa-button size="small" appearance="accent" data-action="save-note-edit" data-note-id={id} data-note-response={feedbackResponse ? 'true' : undefined}>{feedbackResponse ? 'Respond' : 'Submit'}</wa-button></div>}</div> : <div class="note-card__body" {...editAttributes}><MarkdownPreview source={body} attachmentContext={attachmentContext} /></div>}
    {respondToFeedback && !readerMode && <wa-button class="note-card__respond" appearance="outlined" data-action="respond-to-feedback" data-note-id={id}>Respond to Feedback</wa-button>}
    <footer>{author}</footer>
  </article>;
}
