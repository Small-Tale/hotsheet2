import { Pencil, X } from 'lucide';
import { LucideIcon } from './lucide-icon';
import { MarkdownPreview } from './markdown-preview';
import { NoteCard, type NoteCardProps } from './note-card';
import './ticket-reader.css';

export interface TicketReaderProps { slug: string; title: string; details: string; notes: readonly NoteCardProps[] }

export function TicketReader({ slug, title, details, notes }: TicketReaderProps) {
  return <article class="ticket-reader" data-component="ticket-reader">
    <header class="ticket-reader__header">
      <div><span>{slug}</span><h1>{title}</h1></div>
      <div class="ticket-reader__actions">
        <wa-button appearance="plain" data-action="edit-ticket-reader" aria-label="Edit ticket"><LucideIcon icon={Pencil} name="pencil" /></wa-button>
        <wa-button appearance="plain" data-action="close-ticket-reader" aria-label="Close ticket reader"><LucideIcon icon={X} name="x" /></wa-button>
      </div>
    </header>
    <div class="ticket-reader__scroll">
      <section aria-labelledby="ticket-reader-details"><h2 id="ticket-reader-details">Details</h2><MarkdownPreview source={details} /></section>
      <section aria-labelledby="ticket-reader-notes"><h2 id="ticket-reader-notes">Notes <span>{notes.length}</span></h2><div class="ticket-reader__notes">{notes.map(note => <NoteCard {...note} />)}</div></section>
    </div>
  </article>;
}
