import { Activity, CircleAlert, MessageSquareText, RefreshCw } from 'lucide';
import { LucideIcon } from './lucide-icon';
import './note-card.css';

export type NoteKind = 'regular' | 'status' | 'feedback_needed' | 'activity';
export interface NoteCardProps { id: string; kind: NoteKind; author: string; time: string; body: string; title?: string }

const presentations = {
  regular: { label: 'Note', icon: MessageSquareText, iconName: 'message-square-text' },
  status: { label: 'Status update', icon: RefreshCw, iconName: 'refresh-cw' },
  feedback_needed: { label: 'Feedback needed', icon: CircleAlert, iconName: 'circle-alert' },
  activity: { label: 'Activity', icon: Activity, iconName: 'activity' },
} as const;

export function NoteCard({ id, kind, author, time, body, title }: NoteCardProps) {
  const presentation = presentations[kind];
  return <article class="note-card" data-component="note-card" data-note-id={id} data-kind={kind}>
    <header class="note-card__header">
      <span class="note-card__kind"><LucideIcon icon={presentation.icon} name={presentation.iconName} />{title ?? presentation.label}</span>
      <time>{time}</time>
    </header>
    <p class="note-card__body">{body}</p>
    <footer>{author}</footer>
  </article>;
}
