import './ticket-notes.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { MenuHeader } from '@kerfjs/ui/menu-header';
import { MenuItem } from '@kerfjs/ui/menu-item';
import { Plus } from 'lucide';

import type {AttachmentReferenceContext} from '../attachment-references';
import type { InlineFeedbackReply } from '../feedback-replies';
import { NoteCard, type NoteCardProps } from './note-card';
import { NoteComposer } from './note-composer';

export function TicketNotes({ notes, editingNoteId, noteDraft, composing = false, composerDraft = '', canAdd = true, canEdit = true, canDelete = true, readerMode = false, inlineFeedbackReplies = {}, feedbackChoiceSelections = {},attachmentContext }: { notes: readonly NoteCardProps[]; editingNoteId?: string; noteDraft?: string; composing?: boolean; composerDraft?: string; canAdd?: boolean; canEdit?: boolean; canDelete?: boolean; readerMode?: boolean; inlineFeedbackReplies?: Readonly<Record<string, readonly InlineFeedbackReply[]>>; feedbackChoiceSelections?: Readonly<Record<string, readonly string[]>>;attachmentContext?:AttachmentReferenceContext }) {
  const latestExchangeNote = [...notes].reverse().find(note => note.kind === 'regular' || note.kind === 'feedback_needed');
  const activeFeedbackNoteId = latestExchangeNote?.kind === 'feedback_needed' ? latestExchangeNote.id : undefined;
  return <section class="ticket-notes" data-component="ticket-notes">
    <MenuHeader label="Notes" count={notes.length} countLabel={`${notes.length} ${notes.length===1?'note':'notes'}`} action={canAdd&&!composing?'add-ticket-note':undefined} actionLabel="Add note" actionIcon={canAdd&&!composing?<LucideIcon icon={Plus} name="plus"/>:undefined}/>
    {notes.length > 0 ? <div class="ticket-notes__list">{notes.map(note => <NoteCard {...note} editable={canEdit} deletable={canDelete} editing={note.id === editingNoteId} draft={note.id === editingNoteId ? noteDraft : undefined} readerMode={readerMode} respondToFeedback={!readerMode && note.id === activeFeedbackNoteId} inlineReplies={inlineFeedbackReplies[note.id]} selectedChoices={feedbackChoiceSelections[note.id]} attachmentContext={attachmentContext} />)}</div> : !composing && <p class="ticket-notes__empty">No notes added.</p>}
    {composing && <NoteComposer value={composerDraft} />}
    {canAdd && !composing && <MenuItem className="ticket-notes__add" action="add-ticket-note" icon={<LucideIcon icon={Plus} name="plus"/>} label="Add note"/>}
  </section>;
}
