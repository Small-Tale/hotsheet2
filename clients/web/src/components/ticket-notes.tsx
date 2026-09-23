import '@kerfjs/ui/list-inset-text.css';
import './ticket-notes.css';

import { rem } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { ListInsetText } from '@kerfjs/ui/list-inset-text';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Plus } from 'lucide';

import type { AttachmentReferenceContext } from '../attachment-references';
import type { InlineFeedbackReply } from '../feedback-replies';
import { NoteCard, type NoteCardProps } from './note-card';
import { NoteComposer } from './note-composer';

export function TicketNotes({
  notes,
  editingNoteId,
  noteDraft,
  composing = false,
  composerDraft = '',
  canAdd = true,
  canEdit = true,
  canDelete = true,
  readerMode = false,
  inlineFeedbackReplies = {},
  feedbackChoiceSelections = {},
  attachmentContext,
}: {
  notes: readonly NoteCardProps[];
  editingNoteId?: string;
  noteDraft?: string;
  composing?: boolean;
  composerDraft?: string;
  canAdd?: boolean;
  canEdit?: boolean;
  canDelete?: boolean;
  readerMode?: boolean;
  inlineFeedbackReplies?: Readonly<Record<string, readonly InlineFeedbackReply[]>>;
  feedbackChoiceSelections?: Readonly<Record<string, readonly string[]>>;
  attachmentContext?: AttachmentReferenceContext;
}) {
  const latestExchangeNote = [...notes]
    .reverse()
    .find((note) => note.kind === 'regular' || note.kind === 'feedback_needed');
  const activeFeedbackNoteId = latestExchangeNote?.kind === 'feedback_needed' ? latestExchangeNote.id : undefined;
  return (
    <section class="ticket-notes" data-component="ticket-notes">
      {canAdd && !composing ? (
        <ListHeader
          label="Notes"
          count={notes.length}
          countLabel={`${notes.length} ${notes.length === 1 ? 'note' : 'notes'}`}
          action="add-ticket-note"
          actionLabel="Add note"
          actionIcon={<LucideIcon icon={Plus} name="plus" />}
        />
      ) : (
        <ListHeader
          label="Notes"
          count={notes.length}
          countLabel={`${notes.length} ${notes.length === 1 ? 'note' : 'notes'}`}
        />
      )}
      {notes.length > 0 ? (
        <List className="ticket-notes__list" gap={rem(0.55)}>
          {notes.map((note) => (
            <NoteCard
              {...note}
              editable={canEdit}
              deletable={canDelete}
              editing={note.id === editingNoteId}
              draft={note.id === editingNoteId ? noteDraft : undefined}
              readerMode={readerMode}
              respondToFeedback={!readerMode && note.id === activeFeedbackNoteId}
              inlineReplies={inlineFeedbackReplies[note.id]}
              selectedChoices={feedbackChoiceSelections[note.id]}
              attachmentContext={attachmentContext}
            />
          ))}
        </List>
      ) : (
        !composing && (
          <ListInsetText horizontalOnly className="ticket-notes__empty-inset">
            <p class="ticket-notes__empty">No notes added.</p>
          </ListInsetText>
        )
      )}
      {composing && <NoteComposer value={composerDraft} />}
      {canAdd && !composing && (
        <ListItem
          className="ticket-notes__add"
          action="add-ticket-note"
          icon={<LucideIcon icon={Plus} name="plus" />}
          label="Add note"
        />
      )}
    </section>
  );
}
