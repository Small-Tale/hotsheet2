import '@kerfjs/ui/list-inset-control.css';
import './ticket-info-panel.css';

import { foregroundColorVar, rem } from '@kerfjs/ui/css-values';
import { Grid } from '@kerfjs/ui/grid';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { ListInsetControl } from '@kerfjs/ui/list-inset-control';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Skeleton } from '@kerfjs/ui/skeleton';
import { Text } from '@kerfjs/ui/text';
import { CircleAlert, Plus } from 'lucide';

import type { PhaseEstimatePresentation } from '../active-ticket-work';
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
import { TicketTagEditor, ticketTagSuggestionsId } from './ticket-tag-editor';

/** The warning foreground the Feedback needed header shares with its tinted surface. */
const WARNING_TEXT = foregroundColorVar('--wa-color-warning-on-quiet');

export interface TicketInfoPanelProps {
  status: TicketStatus;
  startedPhase?: StartedPhase;
  /** Estimated time left in the current testing phase (HS2-YDGM04). */
  phaseEstimate?: PhaseEstimatePresentation;
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
  /** The row-backed partial variant (HS2-15833W): details and notes are placeholders until they load. */
  contentLoading?: boolean;
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
      <section class="ticket-info-panel__metadata" aria-label="Ticket Metadata">
        <List gap="xs">
          <Grid columns={2} gap="xs">
            <TicketCategorySelect name="inspector-category" value="" placeholder />
            <TicketPrioritySelect name="inspector-priority" value="default" placeholder />
          </Grid>
          <div class="ticket-info-panel__status-field">
            <ListHeader label="Status" />
            <ListInsetControl>
              <Row wrap gap="xs" vAlign="middle" rootAttributes={{ 'data-status-line': 'true' }}>
                <Skeleton width={rem(5)} height={rem(1.5)} />
              </Row>
            </ListInsetControl>
          </div>
        </List>
      </section>
      <List gap="xs" className="ticket-info-panel__section ticket-info-panel__blocked-section">
        <ListItem
          action="edit-blocked-reason"
          icon={<LucideIcon icon={Plus} name="plus" />}
          label="Block ticket"
          tabIndex={-1}
        />
      </List>
      <section class="ticket-info-panel__section ticket-info-panel__details-section">
        <ListHeader label="Details" />
        <div class="ticket-info-panel__details-surface">
          <div class="ticket-info-panel__details-placeholder">
            <Skeleton lines={3} />
          </div>
        </div>
      </section>
      <List gap="xs" className="ticket-info-panel__section">
        <ListHeader
          label="Tags"
          actionDisabled
          action="open-ticket-tag-popover"
          actionLabel="Add tag"
          actionIcon={<LucideIcon icon={Plus} name="plus" />}
        />
      </List>
      <TicketNotes placeholder />
      <Row hAlign="full" gap="2xs" textInsets="rl" rootAttributes={{ 'data-provenance': 'true' }}>
        <Skeleton width={rem(6)} height={rem(0.6875)} />
        <Skeleton width={rem(4)} height={rem(0.6875)} />
      </Row>
    </TicketInspectorPanel>
  );
}

function LoadedTicketInfoPanel({
  status,
  startedPhase,
  phaseEstimate,
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
  providerName = 'Hot Sheet Git',
  updatedLabel = 'Updated now',
  attachmentContext,
  contentLoading = false,
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
      <section class="ticket-info-panel__metadata" aria-label="Ticket Metadata">
        <List gap="xs">
          <Grid columns={2} gap="xs">
            <TicketCategorySelect name="inspector-category" value={category} disabled={!canUpdate} />
            <TicketPrioritySelect name="inspector-priority" value={priority} disabled={!canUpdate} />
          </Grid>
          <div class="ticket-info-panel__status-field">
            <ListHeader label="Status" />
            <ListInsetControl>
              <Row wrap gap="xs" vAlign="middle" rootAttributes={{ 'data-status-line': 'true' }}>
                <>
                  {status === 'deleted' ? (
                    <StatusBadge status="deleted" />
                  ) : (
                    <TicketStatusMenu
                      value={status}
                      startedPhase={startedPhase}
                      phaseEstimate={phaseEstimate}
                      disabled={!canUpdate}
                      canEditStartedPhase={canEditStartedPhase}
                    />
                  )}
                  {blockedReason && <BlockedBadge />}
                </>
              </Row>
            </ListInsetControl>
          </div>
        </List>
      </section>
      <List gap="xs" className="ticket-info-panel__section ticket-info-panel__blocked-section">
        {blockedReasonEditing ? (
          <>
            <ListHeader label="Blocked Reason" />
            <div class="ticket-info-panel__blocked-editor">
              <textarea name="blocked-reason" aria-label="Blocked Reason">
                {blockedReasonDraft}
              </textarea>
            </div>
          </>
        ) : blockedReason ? (
          <>
            <ListHeader label="Blocked Reason" />
            <div
              class="ticket-info-panel__blocked-surface"
              data-edit-blocked-reason={canEditText ? 'true' : undefined}
              role={canEditText ? 'button' : undefined}
              tabIndex={canEditText ? 0 : undefined}
              aria-label={canEditText ? 'Edit blocked reason' : undefined}
              title={canEditText ? 'Double-click to edit' : undefined}
            >
              <Text variant="p" wrap="anywhere" flush>
                {blockedReason}
              </Text>
            </div>
          </>
        ) : canEditText ? (
          <ListItem action="edit-blocked-reason" icon={<LucideIcon icon={Plus} name="plus" />} label="Block ticket" />
        ) : undefined}
      </List>
      <section class="ticket-info-panel__section ticket-info-panel__details-section">
        <ListHeader label="Details" />
        <div class="ticket-info-panel__details-surface" data-feedback-needed={detailsFeedback ? 'true' : undefined}>
          {contentLoading ? (
            <div class="ticket-info-panel__details-placeholder" data-details-loading="true">
              <Skeleton lines={3} />
            </div>
          ) : detailsFeedback && readerPresentation && canAddNotes ? (
            <List gap="xs" rootAttributes={{ 'data-details-feedback': 'true' }}>
              <Row gap="2xs" vAlign="middle" rootAttributes={{ 'data-details-feedback-header': 'true' }}>
                <LucideIcon size={15.2} icon={CircleAlert} name="circle-alert" color={WARNING_TEXT} />
                <Text variant="span" size="compact" color={WARNING_TEXT}>
                  <strong>Feedback needed</strong>
                </Text>
              </Row>
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
            </List>
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
      <List gap="xs" className="ticket-info-panel__section">
        {canUpdate ? (
          <ListHeader
            label="Tags"
            action="open-ticket-tag-popover"
            actionLabel="Add tag"
            actionIcon={<LucideIcon icon={Plus} name="plus" />}
            triggerAttributes={{
              'data-tag-suggestions': ticketTagSuggestionsId(resolvedTagPopoverId),
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
      </List>
      {contentLoading ? (
        <TicketNotes placeholder />
      ) : (
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
      )}
      <Row hAlign="full" gap="2xs" textInsets="rl" rootAttributes={{ 'data-provenance': 'true' }}>
        <Text variant="span" tone="quiet" size="compact">
          {providerName}
        </Text>
        <Text variant="span" tone="quiet" size="compact">
          {updatedLabel}
        </Text>
      </Row>
    </TicketInspectorPanel>
  );
}
