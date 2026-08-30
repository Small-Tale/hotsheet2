import '@awesome.me/webawesome/dist/components/button/button.js';
import './note-card.css';

import { Activity, CircleAlert, MessageSquareText, Pencil, RefreshCw } from 'lucide';

import { LucideIcon } from './lucide-icon';

export type NoteKind = 'regular' | 'status' | 'feedback_needed' | 'activity';
export interface NoteCardProps { id: string; kind: NoteKind; author: string; time: string; body: string; title?: string; editing?: boolean; draft?: string }

const presentations = {
  regular: { label: 'Note', icon: MessageSquareText, iconName: 'message-square-text' },
  status: { label: 'Status update', icon: RefreshCw, iconName: 'refresh-cw' },
  feedback_needed: { label: 'Feedback needed', icon: CircleAlert, iconName: 'circle-alert' },
  activity: { label: 'Activity', icon: Activity, iconName: 'activity' },
} as const;

export function NoteCard({ id, kind, author, time, body, title, editing = false, draft = body }: NoteCardProps) {
  const presentation = presentations[kind];
  return <article class={`note-card${editing ? ' note-card--editing' : ''}`} data-component="note-card" data-note-id={id} data-kind={kind} data-edit-on-double-click={editing ? undefined : 'true'} title={editing ? undefined : 'Double-click to edit'}>
    <header class="note-card__header">
      <span class="note-card__kind"><LucideIcon icon={presentation.icon} name={presentation.iconName} />{title ?? presentation.label}</span>
      <span class="note-card__header-end"><span class="note-card__actions"><button type="button" data-action="edit-note" data-note-id={id} aria-label="Edit note"><LucideIcon icon={Pencil} name="pencil" /></button></span><time>{time}</time></span>
    </header>
    {editing ? <div class="note-card__editor"><textarea name="note-body" data-note-id={id} aria-label="Note body">{draft}</textarea><div><wa-button appearance="plain" data-action="cancel-note-edit" data-note-id={id}>Cancel</wa-button><wa-button appearance="accent" data-action="save-note-edit" data-note-id={id}>Save</wa-button></div></div> : <p class="note-card__body">{body}</p>}
    <footer>{author}</footer>
  </article>;
}
