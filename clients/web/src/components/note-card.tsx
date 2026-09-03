import '@awesome.me/webawesome/dist/components/button/button.js';
import './note-card.css';

import { Activity, CircleAlert, FilePenLine, MessageSquareText, RefreshCw, Trash2 } from 'lucide';

import { type InlineFeedbackReply,parseFeedbackBlocks } from '../feedback-replies';
import { LucideIcon } from './lucide-icon';
import { MarkdownPreview } from './markdown-preview';

export type NoteKind = 'regular' | 'status' | 'feedback_needed' | 'feedback_draft' | 'activity';
export interface NoteCardProps { id: string; kind: NoteKind; author: string; time: string; body: string; title?: string; editable?: boolean; deletable?: boolean; editing?: boolean; draft?: string; readerMode?: boolean; inlineReplies?: readonly InlineFeedbackReply[] }

const presentations = {
  regular: { label: 'Note', icon: MessageSquareText, iconName: 'message-square-text' },
  status: { label: 'Status update', icon: RefreshCw, iconName: 'refresh-cw' },
  feedback_needed: { label: 'Feedback needed', icon: CircleAlert, iconName: 'circle-alert' },
  feedback_draft: { label: 'Feedback draft', icon: FilePenLine, iconName: 'file-pen-line' },
  activity: { label: 'Activity', icon: Activity, iconName: 'activity' },
} as const;

export function NoteCard({ id, kind, author, time, body, title, editable = true, deletable = true, editing = false, draft, readerMode = false, inlineReplies = [] }: NoteCardProps) {
  const presentation = presentations[kind];
  const feedbackEditor = readerMode && (kind === 'feedback_needed' || kind === 'feedback_draft');
  const feedbackResponse = readerMode && kind === 'feedback_needed';
  const editorOpen = editing || feedbackEditor;
  const source = draft ?? (feedbackResponse ? '' : body);
  const editAttributes=editable&&!editorOpen?{'data-edit-on-double-click':'true',role:'button',tabIndex:0,'aria-label':'Edit note',title:'Double-click to edit'}:{};
  return <article class={`note-card${editorOpen ? ' note-card--editing' : ''}`} data-component="note-card" data-note-id={id} data-kind={kind} data-edit-on-double-click={editable&&!editorOpen?'true':undefined} title={editable&&!editorOpen?'Double-click to edit':undefined}>
    <header class="note-card__header">
      <span class="note-card__kind"><LucideIcon icon={presentation.icon} name={presentation.iconName} />{title ?? presentation.label}</span>
      <span class="note-card__header-end">{!editorOpen && deletable && <span class="note-card__actions"><button type="button" data-action="delete-note" data-note-id={id} aria-label="Delete note"><LucideIcon icon={Trash2} name="trash-2" /></button></span>}<time>{time}</time></span>
    </header>
    {feedbackResponse && <div class="note-card__feedback-prompt">{parseFeedbackBlocks(body).map((block, blockIndex) => <div class="note-card__feedback-section"><div class="note-card__feedback-block" data-action="add-inline-feedback-reply" data-note-id={id} data-block-index={blockIndex} role="button" tabIndex={0} aria-label={`Add response after section ${blockIndex + 1}`}><MarkdownPreview source={block.markdown} /></div>{inlineReplies.find(reply => reply.blockIndex === blockIndex) && <textarea class="note-card__inline-reply" name="inline-feedback-response" data-note-id={id} data-block-index={blockIndex} aria-label={`Response after section ${blockIndex + 1}`}>{inlineReplies.find(reply => reply.blockIndex === blockIndex)!.text}</textarea>}</div>)}</div>}
    {editorOpen ? <div class="note-card__editor"><textarea name="note-body" data-note-id={id} data-note-response={feedbackResponse ? 'true' : undefined} aria-label={feedbackResponse ? 'Feedback response' : 'Note body'} placeholder={feedbackResponse && inlineReplies.length ? 'General response (optional)' : undefined}>{source}</textarea>{feedbackEditor && <div><wa-button appearance="accent" data-action="save-note-edit" data-note-id={id} data-note-response={feedbackResponse ? 'true' : undefined}>{feedbackResponse ? 'Respond' : 'Submit'}</wa-button></div>}</div> : <div class="note-card__body" {...editAttributes}><MarkdownPreview source={body} /></div>}
    <footer>{author}</footer>
  </article>;
}
