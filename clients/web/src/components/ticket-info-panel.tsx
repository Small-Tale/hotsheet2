import '@kerfjs/ui/list-inset-control.css';
import './ticket-info-panel.css';

import { rem } from '@kerfjs/ui/css-values';
import { ListHeader } from '@kerfjs/ui/list-header';
import { ListInsetControl } from '@kerfjs/ui/list-inset-control';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Skeleton } from '@kerfjs/ui/skeleton';
import { CircleAlert, Plus } from 'lucide';

import type { StartedPhase } from '../api';
import type { AttachmentReferenceContext } from '../attachment-references';
import { DETAILS_FEEDBACK_ID, textRequestsFeedback } from '../feedback-needed';
import type { InlineFeedbackReply } from '../feedback-replies';
import { MarkdownEditor, type MarkdownEditorMode } from './markdown-editor';
import { FeedbackPrompt, type NoteCardProps, NoteEditor, RespondToFeedbackButton } from './note-card';
import { BlockedBadge, StatusBadge, type TicketStatus } from './status-badge';
import { TicketCategorySelect } from './ticket-category-select';
import { TicketInspectorPanel } from './ticket-inspector-panel';
import { TicketNotes } from './ticket-notes';
import { TicketPrioritySelect } from './ticket-priority-select';
import type { TicketPriority } from './ticket-row';
import { TicketStatusMenu } from './ticket-status-menu';
import { TicketTagEditor } from './ticket-tag-editor';

export interface TicketInfoPanelProps {
  status: TicketStatus;
  startedPhase?: StartedPhase;
  priority: TicketPriority;
  category: string;
  tags: string[];
  tagSuggestions?: readonly string[];
  tagPopoverId?: string;
  canUpdate?: boolean;
  canEditStartedPhase?: boolean;
  canEditText?: boolean;
  canAddNotes?: boolean;
  canEditNotes?: boolean;
  canDeleteNotes?: boolean;
  composingNote?: boolean;
  composerDraft?: string;
  details: string;
  detailsMode?: MarkdownEditorMode;
  detailsDirty?: boolean;
  readerPresentation?: boolean;
  feedbackNeeded?: boolean;
  notes?: readonly NoteCardProps[];
  editingNoteId?: string;
  noteDraft?: string;
  inlineFeedbackReplies?: Readonly<Record<string, readonly InlineFeedbackReply[]>>;
  feedbackChoiceSelections?: Readonly<Record<string, readonly string[]>>;
  blockedReason?: string;
  blockedReasonEditing?: boolean;
  blockedReasonDraft?: string;
  providerName?: string;
  updatedLabel?: string;
  attachmentContext?: AttachmentReferenceContext;
}
/**
 * The info panel, or its value-free loading state (HS2-XBHADT). The `placeholder` variant keeps the
 * real chrome (metadata controls in their Kerf `placeholder` mode, Block ticket, section headers, and
 * the notes placeholder) and shows unknown per-ticket values as Skeleton blocks. It is inert and
 * hidden from assistive technology; the inspector skeleton composes it instead of borrowing classes.
 */
export function TicketInfoPanel(
  props: ({ placeholder?: false } & TicketInfoPanelProps) | { placeholder: true; readerPresentation?: boolean },
) {
  return props.placeholder ? (
    <TicketInfoPanelPlaceholder readerPresentation={props.readerPresentation} />
  ) : (
    <LoadedTicketInfoPanel {...props} />
  );
}

function TicketInfoPanelPlaceholder({ readerPresentation = false }: { readerPresentation?: boolean }) {
  return (
    <TicketInspectorPanel
      component="ticket-info-panel"
      className="ticket-info-panel"
      presentation={readerPresentation ? 'reader' : 'sidebar'}
      attributes={{ 'data-placeholder': 'true', 'aria-hidden': 'true', inert: '' }}
    >
      <section class="ticket-info-panel__metadata" aria-label="Ticket metadata">
        <TicketCategorySelect name="inspector-category" value="" placeholder />
        <TicketPrioritySelect name="inspector-priority" value="default" placeholder />
        <div class="ticket-info-panel__status-field">
          <ListHeader label="Status" />
          <ListInsetControl>
            <div class="ticket-info-panel__status-line">
              <Skeleton width={rem(5)} height={rem(1.5)} />
            </div>
          </ListInsetControl>
        </div>
      </section>
      <section class="ticket-info-panel__section ticket-info-panel__blocked-section">
        <ListItem
          action="edit-blocked-reason"
          icon={<LucideIcon icon={Plus} name="plus" />}
          label="Block ticket"
          tabIndex={-1}
        />
      </section>
      <section class="ticket-info-panel__section ticket-info-panel__details-section">
        <ListHeader label="Details" />
        <div class="ticket-info-panel__details-surface">
          <div class="ticket-info-panel__details-placeholder">
            <Skeleton lines={3} />
          </div>
        </div>
      </section>
      <section class="ticket-info-panel__section">
        <ListHeader
          label="Tags"
          actionDisabled
          action="open-ticket-tag-popover"
          actionLabel="Add tag"
          actionIcon={<LucideIcon icon={Plus} name="plus" />}
        />
      </section>
      <TicketNotes placeholder />
      <footer class="ticket-info-panel__provenance">
        <Skeleton width={rem(6)} height={rem(0.6875)} />
        <Skeleton width={rem(4)} height={rem(0.6875)} />
      </footer>
    </TicketInspectorPanel>
  );
}

function LoadedTicketInfoPanel({
  status,
  startedPhase,
  priority,
  category,
  tags,
  tagSuggestions,
  tagPopoverId,
  canUpdate = true,
  canEditStartedPhase = false,
  canEditText = canUpdate,
  canAddNotes = true,
  canEditNotes = true,
  canDeleteNotes = true,
  composingNote = false,
  composerDraft = '',
  details,
  detailsMode = 'preview',
  detailsDirty = false,
  readerPresentation = false,
  feedbackNeeded = false,
  notes = [],
  editingNoteId,
  noteDraft,
  inlineFeedbackReplies,
  feedbackChoiceSelections,
  blockedReason = '',
  blockedReasonEditing = false,
  blockedReasonDraft = blockedReason,
  providerName = 'Hot Sheet git',
  updatedLabel = 'Updated now',
  attachmentContext,
}: TicketInfoPanelProps) {
  const detailsFeedback =
    feedbackNeeded && textRequestsFeedback(details) && !notes.some((note) => note.kind === 'feedback_needed');
  const resolvedTagPopoverId = tagPopoverId ?? 'ticket-tag-popover';
  return (
    <TicketInspectorPanel
      component="ticket-info-panel"
      className="ticket-info-panel"
      presentation={readerPresentation ? 'reader' : 'sidebar'}
    >
      <section class="ticket-info-panel__metadata" aria-label="Ticket metadata">
        <TicketCategorySelect name="inspector-category" value={category} disabled={!canUpdate} />
        <TicketPrioritySelect name="inspector-priority" value={priority} disabled={!canUpdate} />
        <div class="ticket-info-panel__status-field">
          <ListHeader label="Status" />
          <ListInsetControl>
            <div class="ticket-info-panel__status-line">
              <>
                {status === 'deleted' ? (
                  <StatusBadge status="deleted" />
                ) : (
                  <TicketStatusMenu
                    value={status}
                    startedPhase={startedPhase}
                    disabled={!canUpdate}
                    canEditStartedPhase={canEditStartedPhase}
                  />
                )}
                {blockedReason && <BlockedBadge />}
              </>
            </div>
          </ListInsetControl>
        </div>
      </section>
      <section class="ticket-info-panel__section ticket-info-panel__blocked-section">
        {blockedReasonEditing ? (
          <>
            <ListHeader label="Blocked reason" />
            <div class="ticket-info-panel__blocked-editor">
              <textarea name="blocked-reason" aria-label="Blocked reason">
                {blockedReasonDraft}
              </textarea>
            </div>
          </>
        ) : blockedReason ? (
          <>
            <ListHeader label="Blocked reason" />
            <div
              class="ticket-info-panel__blocked-surface"
              data-edit-blocked-reason={canEditText ? 'true' : undefined}
              role={canEditText ? 'button' : undefined}
              tabIndex={canEditText ? 0 : undefined}
              aria-label={canEditText ? 'Edit blocked reason' : undefined}
              title={canEditText ? 'Double-click to edit' : undefined}
            >
              <p>{blockedReason}</p>
            </div>
          </>
        ) : canEditText ? (
          <ListItem action="edit-blocked-reason" icon={<LucideIcon icon={Plus} name="plus" />} label="Block ticket" />
        ) : undefined}
      </section>
      <section class="ticket-info-panel__section ticket-info-panel__details-section">
        <ListHeader label="Details" />
        <div class="ticket-info-panel__details-surface" data-feedback-needed={detailsFeedback ? 'true' : undefined}>
          {detailsFeedback && readerPresentation && canAddNotes ? (
            <div class="ticket-info-panel__details-feedback" data-details-feedback="true">
              <header class="ticket-info-panel__details-feedback-header">
                <LucideIcon size={15.2} icon={CircleAlert} name="circle-alert" />
                Feedback needed
              </header>
              <FeedbackPrompt
                source={details}
                id={DETAILS_FEEDBACK_ID}
                inlineReplies={inlineFeedbackReplies?.[DETAILS_FEEDBACK_ID]}
                selectedChoices={feedbackChoiceSelections?.[DETAILS_FEEDBACK_ID]}
                attachmentContext={attachmentContext}
              />
              <NoteEditor
                id={DETAILS_FEEDBACK_ID}
                source={noteDraft ?? ''}
                response
                actions
                placeholder="Additional response (optional)"
              />
            </div>
          ) : (
            <>
              <MarkdownEditor
                value={details}
                mode={detailsMode}
                dirty={detailsDirty}
                appearance="embedded"
                inset="flush"
                showExpand={false}
                label="Ticket details"
                editable={canEditText}
                attachmentContext={attachmentContext}
              />
              {detailsFeedback && canAddNotes && <RespondToFeedbackButton id={DETAILS_FEEDBACK_ID} />}
            </>
          )}
        </div>
      </section>
      <section class="ticket-info-panel__section">
        {canUpdate ? (
          <ListHeader
            label="Tags"
            action="open-ticket-tag-popover"
            actionLabel="Add tag"
            actionIcon={<LucideIcon icon={Plus} name="plus" />}
            triggerAttributes={{
              popoverTarget: resolvedTagPopoverId,
              'aria-controls': resolvedTagPopoverId,
              'aria-haspopup': 'dialog',
            }}
          />
        ) : (
          <ListHeader label="Tags" />
        )}
        <TicketTagEditor
          tags={tags}
          suggestions={tagSuggestions}
          editable={canUpdate}
          popoverId={resolvedTagPopoverId}
        />
      </section>
      <TicketNotes
        notes={notes}
        editingNoteId={editingNoteId}
        noteDraft={noteDraft}
        composing={composingNote}
        composerDraft={composerDraft}
        canAdd={canAddNotes}
        canEdit={canEditNotes}
        canDelete={canDeleteNotes}
        readerMode={readerPresentation}
        inlineFeedbackReplies={inlineFeedbackReplies}
        feedbackChoiceSelections={feedbackChoiceSelections}
        attachmentContext={attachmentContext}
      />
      <footer class="ticket-info-panel__provenance">
        <span>{providerName}</span>
        <span>{updatedLabel}</span>
      </footer>
    </TicketInspectorPanel>
  );
}
