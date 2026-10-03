import '@kerfjs/ui/list-inset-text.css';
import './ticket-notes.css';

import { pct, rem } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { ListInsetText } from '@kerfjs/ui/list-inset-text';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Skeleton } from '@kerfjs/ui/skeleton';
import { Activity, MessageSquareText, Plus } from 'lucide';

import type { AttachmentReferenceContext } from '../attachment-references';
import type { InlineFeedbackReply } from '../feedback-replies';
import { NoteCard, type NoteCardProps } from './note-card';
import { NoteComposer } from './note-composer';

/** One value-free note entry: the note-card header (kind + time skeleton) over a skeleton body. */
function PlaceholderNote({ kind }: { kind: 'activity' | 'regular' }) {
  const presentation =
    kind === 'activity'
      ? { label: 'Activity', icon: Activity, iconName: 'activity' }
      : { label: 'Note', icon: MessageSquareText, iconName: 'message-square-text' };
  return (
    <div class="ticket-notes__placeholder-note" data-kind={kind}>
      <div class="ticket-notes__placeholder-header">
        <span class="ticket-notes__placeholder-kind">
          <LucideIcon icon={presentation.icon} name={presentation.iconName} size={14.4} />
          {presentation.label}
        </span>
        <Skeleton width={rem(2.5)} height={rem(0.6875)} />
      </div>
      <div class="ticket-notes__placeholder-body">
        <Skeleton />
        <Skeleton width={pct(45)} />
      </div>
    </div>
  );
}

/**
 * The value-free loading state of {@link TicketNotes} (HS2-XBHADT): the Notes header with its add
 * action disabled over two activity entries and one regular note card, each a skeleton.
 */
function TicketNotesPlaceholder() {
  return (
    <section class="ticket-notes" data-component="ticket-notes" data-placeholder="true">
      <ListHeader
        label="Notes"
        actionDisabled
        action="add-ticket-note"
        actionLabel="Add note"
        actionIcon={<LucideIcon icon={Plus} name="plus" />}
      />
      <div class="ticket-notes__placeholder">
        <PlaceholderNote kind="activity" />
        <PlaceholderNote kind="activity" />
        <PlaceholderNote kind="regular" />
      </div>
    </section>
  );
}

export function TicketNotes(props: ({ placeholder?: false } & TicketNotesProps) | { placeholder: true }) {
  return props.placeholder ? <TicketNotesPlaceholder /> : <LoadedTicketNotes {...props} />;
}

interface TicketNotesProps {
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
}

function LoadedTicketNotes({
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
}: TicketNotesProps) {
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
              density="compact"
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
          <div class="ticket-notes__empty-inset">
            <ListInsetText sides="rl">
              <p class="ticket-notes__empty">No notes added.</p>
            </ListInsetText>
          </div>
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
