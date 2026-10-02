import '@awesome.me/webawesome/dist/components/button/button.js';
import '@kerfjs/ui/tab-bar.css';
import './ticket-inspector.css';

import { AppTab } from '@kerfjs/ui/app-tab';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { TabBar } from '@kerfjs/ui/tab-bar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import {
  ALargeSmall,
  BookOpen,
  CircleAlert,
  CopyX,
  Info,
  ListTree,
  MessageSquareCode,
  Paperclip,
  Star,
  X,
} from 'lucide';

import type { CodeReview, DuplicateBacklink, TicketCloseReason } from '../api';
import type { AttachmentReferenceContext } from '../attachment-references';
import type { InlineFeedbackReply } from '../feedback-replies';
import { ticketCloseReasonLabel } from '../ticket-close';
import type { TicketFieldConflict as TicketFieldConflictState } from '../ticket-field-reconciliation';
import { LiveClaimNotice, type LiveClaimNoticeProps } from './active-claim';
import { ConfidenceBadge } from './confidence-badge';
import type { MarkdownEditorMode } from './markdown-editor';
import type { NoteCardProps } from './note-card';
import { inspectorToggle, SidebarPane, type SidebarPanelParts } from './sidebar-panel';
import type { TicketStatus } from './status-badge';
import { type TicketAttachmentItem, TicketAttachments } from './ticket-attachments';
import { TicketCodeReview } from './ticket-code-review';
import {
  type DuplicateTargetSummary,
  TicketDuplicateBacklinks,
  TicketDuplicateTarget,
} from './ticket-duplicate-backlinks';
import { TicketFieldConflict } from './ticket-field-conflict';
import { TicketInfoPanel } from './ticket-info-panel';
import type { TicketPriority } from './ticket-row';
import { TicketTimeline, type TicketTimelineEntry } from './ticket-timeline';

export type InspectorTab = 'info' | 'timeline' | 'code-review' | 'attachments';

export interface TicketInspectorProps {
  slug: string;
  title: string;
  titleEditing?: boolean;
  titleDraft?: string;
  canUpdate?: boolean;
  canEditText?: boolean;
  canAddNotes?: boolean;
  canEditNotes?: boolean;
  canDeleteNotes?: boolean;
  composingNote?: boolean;
  composerDraft?: string;
  status: TicketStatus;
  priority: TicketPriority;
  category: string;
  tags: string[];
  tagSuggestions?: readonly string[];
  details: string;
  detailsMode?: MarkdownEditorMode;
  detailsDirty?: boolean;
  activeTab?: InspectorTab;
  upNext?: boolean;
  upNextEligible?: boolean;
  /** The ticket has an unresolved `feedback_needed` note — it is waiting on the user. */
  feedbackNeeded?: boolean;
  closeReason?: TicketCloseReason;
  /** Derived AI completion confidence of a completed/verified ticket (HS2-DWTJ43). */
  latestConfidence?: number;
  /** A live claim lease: who is actively working on the ticket and its ETA progress (HS2-QKNQXC). */
  liveClaim?: LiveClaimNoticeProps;
  duplicateTarget?: DuplicateTargetSummary;
  duplicateBacklinks?: readonly DuplicateBacklink[];
  duplicateBacklinkInaccessibleProjects?: readonly string[];
  timelineEntries?: readonly TicketTimelineEntry[];
  attachments?: readonly TicketAttachmentItem[];
  codeReview?: CodeReview;
  codeReviewLoading?: boolean;
  codeReviewMessage?: string;
  expandedCodeReviewCommits?: readonly string[];
  attachmentsEnabled?: boolean;
  /** Existing attachments can be edited (the provider reports `attachment_edit`, HS2-HSA64D). */
  attachmentsEditable?: boolean;
  attachmentMessage?: string;
  attachmentContext?: AttachmentReferenceContext;
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
  presentation?: 'sidebar' | 'reader';
  largeText?: boolean;
  fieldConflict?: TicketFieldConflictState;
  fieldConflictResolution?: string;
}

const tabs = [
  { id: 'info', label: 'Info', icon: Info, iconName: 'info' },
  { id: 'timeline', label: 'Timeline', icon: ListTree, iconName: 'list-tree' },
  { id: 'code-review', label: 'Code Review', icon: MessageSquareCode, iconName: 'message-square-code' },
  { id: 'attachments', label: 'Attachments', icon: Paperclip, iconName: 'paperclip' },
] as const;

/**
 * The ticket inspector's panel parts (HS2-QQW6CT): the toolbar carries the ticket number and actions,
 * the fixed header the title, status notices (live claim, needs review, confidence, close outcome,
 * duplicates, field conflict), and section tabs, and the content the active section. The application
 * shell hands them to the Workbench's right rail; {@link TicketInspector} renders them standalone for
 * the reader modal, the terminal rail's pushed detail, and the UX catalog.
 */
export function ticketInspectorPanel({
  slug,
  title,
  titleEditing = false,
  titleDraft = title,
  canUpdate = true,
  canEditText = canUpdate,
  canAddNotes = true,
  canEditNotes = true,
  canDeleteNotes = true,
  composingNote = false,
  composerDraft = '',
  status,
  priority,
  category,
  tags,
  tagSuggestions,
  details,
  detailsMode,
  detailsDirty,
  activeTab = 'info',
  upNext = false,
  upNextEligible = status === 'not_started' || status === 'started',
  feedbackNeeded = false,
  closeReason,
  latestConfidence,
  liveClaim,
  duplicateTarget,
  duplicateBacklinks = [],
  duplicateBacklinkInaccessibleProjects = [],
  timelineEntries,
  attachments,
  codeReview,
  codeReviewLoading = false,
  codeReviewMessage = '',
  expandedCodeReviewCommits,
  attachmentsEnabled = true,
  attachmentsEditable = attachmentsEnabled,
  attachmentMessage = '',
  attachmentContext,
  notes,
  editingNoteId,
  noteDraft,
  inlineFeedbackReplies,
  feedbackChoiceSelections,
  blockedReason,
  blockedReasonEditing,
  blockedReasonDraft,
  providerName,
  updatedLabel,
  presentation = 'sidebar',
  largeText = false,
  fieldConflict,
  fieldConflictResolution = fieldConflict?.mine ?? '',
}: TicketInspectorProps): SidebarPanelParts {
  const star = (
    <>
      {upNextEligible && (
        <button
          type="button"
          class={`ticket-inspector__star${upNext ? ' ticket-inspector__star--active' : ''}`}
          data-action="toggle-inspector-up-next"
          aria-label={upNext ? 'Remove from Up Next' : 'Add to Up Next'}
        >
          <LucideIcon icon={Star} name="star" />
        </button>
      )}
    </>
  );
  // The reader modal is not a Workbench panel: it keeps its own close control (HS2-QQW6CT).
  const close = (
    <button type="button" data-dialog="close" data-action="close-ticket-reader" aria-label="Close ticket reader">
      <LucideIcon icon={X} name="x" />
    </button>
  );
  const actions =
    presentation === 'reader' ? (
      <>
        {upNextEligible && (
          <ToolbarControlGroup appearance="borderless" single>
            {star}
          </ToolbarControlGroup>
        )}
        <ToolbarControlGroup appearance="borderless" buttonAppearance="push" label="Reader text size" single>
          <button
            type="button"
            data-action="toggle-reader-text-size"
            aria-label={largeText ? 'Use standard reader text size' : 'Use large reader text size'}
            aria-pressed={String(largeText)}
            title={largeText ? 'Standard text size' : 'Large text size'}
          >
            <LucideIcon icon={ALargeSmall} name="a-large-small" />
          </button>
        </ToolbarControlGroup>
        <ToolbarControlGroup appearance="borderless" single>
          {close}
        </ToolbarControlGroup>
      </>
    ) : (
      <ToolbarControlGroup appearance="borderless" label="Ticket actions">
        {star}
        <button
          type="button"
          data-action="open-ticket-reader"
          aria-label="Open ticket reader"
          title="Open ticket reader"
        >
          <LucideIcon icon={BookOpen} name="book-open" />
        </button>
      </ToolbarControlGroup>
    );
  // The ticket number leads the toolbar for the sidebar inspector (HS2-9MCJ2B), the reader modal
  // (HS2-FZ5HB2), and the terminal rail's pushed detail, where it follows Kerf's back control
  // (HS2-FY06N4).
  const slugButton = (
    <button
      type="button"
      class="ticket-inspector__slug"
      data-action="copy-ticket-slug"
      // The Workbench composes this toolbar outside the app-owned wrappers, so the button names its
      // own ticket (HS2-QQW6CT).
      data-ticket-slug={slug}
      aria-label={`Copy ticket number ${slug}`}
      title="Copy ticket number"
    >
      <ToolbarText text={slug} size="small" />
    </button>
  );
  // The Workbench owns the panel's Pane root, so the ticket identity, review state, and attachment drop
  // target live on the app-owned header and body wrappers (HS2-QQW6CT).
  const identity = {
    'data-ticket-slug': slug,
    'data-presentation': presentation,
    'data-needs-review': String(feedbackNeeded),
    'data-attachment-drop-target': 'true',
  };
  const header = (
    <div class="ticket-inspector__header" data-component="ticket-inspector-header" {...identity}>
      {titleEditing ? (
        <input class="ticket-inspector__title-input" name="ticket-title" aria-label="Ticket title" value={titleDraft} />
      ) : (
        <h1
          data-action={canUpdate ? 'edit-ticket-title' : undefined}
          data-editable={String(canUpdate)}
          tabIndex={canUpdate ? 0 : undefined}
          title={canUpdate ? 'Double-click to edit title' : undefined}
        >
          {title}
        </h1>
      )}
      {liveClaim && <LiveClaimNotice {...liveClaim} />}
      {feedbackNeeded && (
        <div class="ticket-inspector__feedback" role="status">
          <span class="ticket-inspector__feedback-icon">
            <LucideIcon icon={CircleAlert} name="circle-alert" />
          </span>
          <span>Needs review</span>
        </div>
      )}
      {(status === 'completed' || status === 'verified') && latestConfidence !== undefined && (
        <div class="ticket-inspector__confidence" role="status" data-confidence={String(latestConfidence)}>
          <ConfidenceBadge value={latestConfidence} appearance="labeled" />
          <span>Reported by the completing AI</span>
        </div>
      )}
      {closeReason && closeReason !== 'duplicate' && (
        <div class="ticket-inspector__close-outcome" role="status" data-close-reason={closeReason}>
          <span>Closed as {ticketCloseReasonLabel(closeReason)?.toLowerCase() ?? closeReason}</span>
        </div>
      )}
      {closeReason === 'duplicate' &&
        (duplicateTarget ? (
          <TicketDuplicateTarget target={duplicateTarget} />
        ) : (
          <div class="ticket-inspector__close-outcome" role="status" data-close-reason="duplicate">
            <LucideIcon icon={CopyX} name="copy-x" />
            <span>Duplicate of another ticket</span>
          </div>
        ))}
      <TicketDuplicateBacklinks
        backlinks={duplicateBacklinks}
        inaccessibleProjects={duplicateBacklinkInaccessibleProjects}
      />
      {fieldConflict && <TicketFieldConflict conflict={fieldConflict} resolution={fieldConflictResolution} />}
      {/* The app owns the strip's outer inset; Kerf's TabBar is a full-width block, so an inset must
          come from this frame's padding rather than a margin that would overflow (HS2-7DJPSG). */}
      <div class="ticket-inspector__tabs-frame">
        <TabBar
          id={`ticket-inspector-${presentation}-${slug}`}
          label="Ticket inspector sections"
          className="ticket-inspector__tabs"
          activation="automatic"
          allocation="fill"
          presentation="inspector"
          // The reader keeps segmented names and lets Kerf switch them to icon-only below 832px.
          iconOnlyAt={presentation === 'reader' ? 'wide' : undefined}
        >
          {tabs.map((tab) => (
            <AppTab
              id={tab.id}
              name={tab.label}
              selected={activeTab === tab.id}
              closable={false}
              selectAction="set-inspector-tab"
              // Sidebar and rail inspectors are too narrow for names; icon-only keeps `name` accessible.
              presentation={presentation === 'reader' ? 'segmented' : 'icon-only'}
              size="compact"
              rootAttributes={{ 'data-inspector-tab': tab.id }}
              leading={<LucideIcon icon={tab.icon} name={tab.iconName} />}
              trailing={
                tab.id === 'attachments' && attachments?.length ? (
                  <span class="ticket-inspector__tab-count">
                    <span aria-hidden="true">{attachments.length}</span>
                    <span class="ticket-inspector__tab-count-label">{attachments.length} attachments</span>
                  </span>
                ) : undefined
              }
            />
          ))}
        </TabBar>
      </div>
    </div>
  );
  const content = (
    <div class="ticket-inspector__body" data-component="ticket-inspector-body" {...identity}>
      {activeTab === 'info' && (
        <TicketInfoPanel
          status={status}
          priority={priority}
          category={category}
          tags={tags}
          tagSuggestions={tagSuggestions}
          tagPopoverId={`ticket-tag-${presentation}-${slug.toLowerCase()}`}
          canUpdate={canUpdate}
          canEditText={canEditText}
          canAddNotes={canAddNotes}
          canEditNotes={canEditNotes}
          canDeleteNotes={canDeleteNotes}
          composingNote={composingNote}
          composerDraft={composerDraft}
          details={details}
          detailsMode={detailsMode}
          detailsDirty={detailsDirty}
          readerPresentation={presentation === 'reader'}
          feedbackNeeded={feedbackNeeded}
          notes={notes}
          editingNoteId={editingNoteId}
          noteDraft={noteDraft}
          inlineFeedbackReplies={inlineFeedbackReplies}
          feedbackChoiceSelections={feedbackChoiceSelections}
          blockedReason={blockedReason}
          blockedReasonEditing={blockedReasonEditing}
          blockedReasonDraft={blockedReasonDraft}
          providerName={providerName}
          updatedLabel={updatedLabel}
          attachmentContext={attachmentContext}
        />
      )}
      {activeTab === 'timeline' && <TicketTimeline entries={timelineEntries} />}
      {activeTab === 'code-review' && (
        <TicketCodeReview
          review={codeReview}
          loading={codeReviewLoading}
          message={codeReviewMessage}
          expandedCommits={expandedCodeReviewCommits}
        />
      )}
      {activeTab === 'attachments' && (
        <TicketAttachments
          attachments={attachments}
          enabled={attachmentsEnabled}
          editable={attachmentsEditable}
          message={attachmentMessage}
        />
      )}
    </div>
  );
  return {
    label: `${slug} inspector`,
    toolbar: {
      label: 'Ticket inspector toolbar',
      dividerSides: '',
      leading: slugButton,
      trailing: actions,
    },
    toggle: inspectorToggle(),
    header,
    content,
    pane: {},
  };
}

/**
 * The ticket inspector rendered standalone from its {@link ticketInspectorPanel} parts: the reader
 * modal (its own close and text-size controls, no rail toggle), the terminal rail's pushed detail
 * (the rail's panel toolbar holds the toggle), and the UX catalog (`collapseControl` mirrors the
 * Workbench's standard toggle).
 */
export function TicketInspector({
  collapseControl = false,
  ...props
}: TicketInspectorProps & { collapseControl?: boolean }) {
  const presentation = props.presentation ?? 'sidebar',
    parts = ticketInspectorPanel(props);
  return (
    <aside
      class={presentation === 'reader' ? 'ticket-inspector ticket-inspector--reader' : 'ticket-inspector'}
      data-component="ticket-inspector"
      data-presentation={presentation}
      data-large-text={presentation === 'reader' ? String(props.largeText ?? false) : undefined}
      data-ticket-slug={props.slug}
      data-needs-review={String(props.feedbackNeeded ?? false)}
      data-attachment-drop-target="true"
      aria-label={parts.label}
    >
      <SidebarPane
        parts={parts}
        element="div"
        side="right"
        collapseControl={presentation !== 'reader' && collapseControl}
      />
    </aside>
  );
}
