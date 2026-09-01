import '@awesome.me/webawesome/dist/components/button/button.js';
import './note-card.css';

import { Activity, CircleAlert, FilePenLine, MessageSquareText, Pencil, RefreshCw, Trash2 } from 'lucide';

import { LucideIcon } from './lucide-icon';

export type NoteKind = 'regular' | 'status' | 'feedback_needed' | 'feedback_draft' | 'activity';
export interface NoteCardProps { id: string; kind: NoteKind; author: string; time: string; body: string; title?: string; editable?: boolean; deletable?: boolean; editing?: boolean; draft?: string; readerMode?: boolean; readerEditing?: boolean }

const presentations = {
  regular: { label: 'Note', icon: MessageSquareText, iconName: 'message-square-text' },
  status: { label: 'Status update', icon: RefreshCw, iconName: 'refresh-cw' },
  feedback_needed: { label: 'Feedback needed', icon: CircleAlert, iconName: 'circle-alert' },
  feedback_draft: { label: 'Feedback draft', icon: FilePenLine, iconName: 'file-pen-line' },
  activity: { label: 'Activity', icon: Activity, iconName: 'activity' },
} as const;

export function NoteCard({ id, kind, author, time, body, title, editable: editCapability = true, deletable = true, editing = false, draft, readerMode = false, readerEditing = false }: NoteCardProps) {
  const presentation = presentations[kind];
  const feedbackEditor = readerMode && (kind === 'feedback_needed' || kind === 'feedback_draft');
  const editorOpen = editing || feedbackEditor;
  const editable = editCapability && (!readerMode || readerEditing);
  const source = draft ?? (kind === 'feedback_needed' ? '' : body);
  return <article class={`note-card${editorOpen ? ' note-card--editing' : ''}`} data-component="note-card" data-note-id={id} data-kind={kind} data-edit-on-double-click={editable && !editorOpen ? 'true' : undefined} title={editable && !editorOpen ? 'Double-click to edit' : undefined}>
    <header class="note-card__header">
      <span class="note-card__kind"><LucideIcon icon={presentation.icon} name={presentation.iconName} />{title ?? presentation.label}</span>
      <span class="note-card__header-end">{!editorOpen && (editable || deletable) && <span class="note-card__actions">{editable && <button type="button" data-action="edit-note" data-note-id={id} aria-label="Edit note"><LucideIcon icon={Pencil} name="pencil" /></button>}{deletable && <button type="button" data-action="delete-note" data-note-id={id} aria-label="Delete note"><LucideIcon icon={Trash2} name="trash-2" /></button>}</span>}<time>{time}</time></span>
    </header>
    {kind === 'feedback_needed' && feedbackEditor && <p class="note-card__body">{body}</p>}
    {editorOpen ? <div class="note-card__editor"><textarea name="note-body" data-note-id={id} data-note-response={kind === 'feedback_needed' ? 'true' : undefined} aria-label={kind === 'feedback_needed' ? 'Feedback response' : 'Note body'}>{source}</textarea>{feedbackEditor && <div><wa-button appearance="accent" data-action="save-note-edit" data-note-id={id} data-note-response={kind === 'feedback_needed' ? 'true' : undefined}>{kind === 'feedback_needed' ? 'Respond' : 'Submit'}</wa-button></div>}</div> : <p class="note-card__body">{body}</p>}
    <footer>{author}</footer>
  </article>;
}
