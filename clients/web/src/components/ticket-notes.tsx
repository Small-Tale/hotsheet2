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
import { isAiThumbsFeedback } from '../feedback-needed';
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

/** Legacy and current thumbs notes keep their source id in the first line. Only a
 * known rating and an existing parent are nested; orphaned/other-target notes stay
 * visible so an edit or provider sync cannot silently discard them. */
export function nestAiFeedback(notes: readonly NoteCardProps[]): NoteCardProps[] {
  const target = (note: NoteCardProps) => {
    const first = note.body.split(/\r?\n/, 1)[0] ?? '';
    const match =
      /^AI feedback for note:([^:\r\n]+): (Helpful — keep suggestions like this\.|Not helpful — stop suggestions like this\.)$/u.exec(
        first,
      );
    const structuredParent = note.ratingMetadata?.target.startsWith('note:')
      ? note.ratingMetadata.target.slice('note:'.length)
      : undefined;
    return structuredParent
      ? { parent: structuredParent, rating: match?.[2], first: match ? first : undefined }
      : note.feedbackFor
        ? { parent: note.feedbackFor, rating: match?.[2], first: match ? first : undefined }
        : match
          ? { parent: match[1], rating: match[2], first }
          : undefined;
  };
  const parents = new Set(notes.filter((note) => !target(note)).map((note) => note.id));
  const feedback = new Map<string, NoteCardProps[]>();
  const visible: NoteCardProps[] = [];
  for (const note of notes) {
    const link = target(note);
    if (!link || link.parent === note.id || !parents.has(link.parent)) {
      visible.push(note);
      continue;
    }
    const entries = feedback.get(link.parent) ?? [];
    entries.push({
      ...note,
      body: link.first
        ? [link.rating, note.body.slice(link.first.length).trim()].filter(Boolean).join('\n\n')
        : note.body,
    });
    feedback.set(link.parent, entries);
  }
  return visible.map((note) => ({ ...note, aiFeedback: feedback.get(note.id) }));
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
  const visibleNotes = nestAiFeedback(
    notes.filter((note) => !(note.kind === 'activity' && note.body.startsWith('Started phase changed from '))),
  );
  const latestExchangeNote = [...visibleNotes]
    .reverse()
    .find(
      (note) =>
        (note.kind === 'regular' || note.kind === 'feedback_needed') &&
        !isAiThumbsFeedback({ text: note.body, feedback_for: note.feedbackFor, ai_feedback: note.ratingMetadata }),
    );
  const activeFeedbackNoteId = latestExchangeNote?.kind === 'feedback_needed' ? latestExchangeNote.id : undefined;
  return (
    <section class="ticket-notes" data-component="ticket-notes">
      {canAdd && !composing ? (
        <ListHeader
          label="Notes"
          count={visibleNotes.length}
          countLabel={`${visibleNotes.length} ${visibleNotes.length === 1 ? 'note' : 'notes'}`}
          action="add-ticket-note"
          actionLabel="Add note"
          actionIcon={<LucideIcon icon={Plus} name="plus" />}
        />
      ) : (
        <ListHeader
          label="Notes"
          count={visibleNotes.length}
          countLabel={`${visibleNotes.length} ${visibleNotes.length === 1 ? 'note' : 'notes'}`}
        />
      )}
      {visibleNotes.length > 0 ? (
        <List className="ticket-notes__list" gap={rem(0.55)}>
          {visibleNotes.map((note) => (
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
