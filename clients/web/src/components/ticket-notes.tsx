import { Plus } from 'lucide';
import { LucideIcon } from './lucide-icon';
import { NoteCard, type NoteCardProps } from './note-card';
import './ticket-notes.css';

export function TicketNotes({ notes, editingNoteId, noteDraft, readerAvailable = false }: { notes: readonly NoteCardProps[]; editingNoteId?: string; noteDraft?: string; readerAvailable?: boolean }) {
  return <section class="ticket-notes" data-component="ticket-notes">
    <header class="ticket-inspector__section-header"><h2>Notes <span>{notes.length}</span></h2><wa-button appearance="plain" data-action="add-ticket-note" aria-label="Add note"><LucideIcon icon={Plus} name="plus" /></wa-button></header>
    {notes.length > 0 ? <div class="ticket-notes__list">{notes.map(note => <NoteCard {...note} editing={note.id === editingNoteId} draft={note.id === editingNoteId ? noteDraft : undefined} readerAvailable={readerAvailable} />)}</div> : <p class="ticket-notes__empty">No notes added.</p>}
    {notes.length > 0 && <button type="button" class="ticket-notes__add" data-action="add-ticket-note"><LucideIcon icon={Plus} name="plus" />Add note</button>}
  </section>;
}
