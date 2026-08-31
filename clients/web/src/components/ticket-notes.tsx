import './ticket-notes.css';

import { Plus } from 'lucide';

import { LucideIcon } from './lucide-icon';
import { NoteCard, type NoteCardProps } from './note-card';
import { NoteComposer } from './note-composer';

export function TicketNotes({ notes, editingNoteId, noteDraft, composing = false, composerDraft = '', canAdd = true, canEdit = true, canDelete = true, readerMode = false, readerEditing = false }: { notes: readonly NoteCardProps[]; editingNoteId?: string; noteDraft?: string; composing?: boolean; composerDraft?: string; canAdd?: boolean; canEdit?: boolean; canDelete?: boolean; readerMode?: boolean; readerEditing?: boolean }) {
  return <section class="ticket-notes" data-component="ticket-notes">
    <header class="ticket-inspector__section-header"><h2>Notes <span>{notes.length}</span></h2>{canAdd && !composing && <wa-button appearance="plain" data-action="add-ticket-note" aria-label="Add note"><LucideIcon icon={Plus} name="plus" /></wa-button>}</header>
    {composing && <NoteComposer value={composerDraft} />}
    {notes.length > 0 ? <div class="ticket-notes__list">{notes.map(note => <NoteCard {...note} editable={canEdit} deletable={canDelete} editing={note.id === editingNoteId} draft={note.id === editingNoteId ? noteDraft : undefined} readerMode={readerMode} readerEditing={readerEditing} />)}</div> : !composing && <p class="ticket-notes__empty">No notes added.</p>}
    {canAdd && !composing && notes.length > 0 && <button type="button" class="ticket-notes__add" data-action="add-ticket-note"><LucideIcon icon={Plus} name="plus" />Add note</button>}
  </section>;
}
