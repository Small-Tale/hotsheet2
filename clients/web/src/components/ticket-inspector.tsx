import '@awesome.me/webawesome/dist/styles/utilities/visually-hidden.css';
import '@kerfjs/ui/tab-bar.css';
import './ticket-inspector.css';

import { Badge } from '@kerfjs/ui/badge';
import { Button } from '@kerfjs/ui/button';
import { ContentItem } from '@kerfjs/ui/content-item';
import { foregroundColorVar, pct, rem } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Skeleton } from '@kerfjs/ui/skeleton';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { TabBar } from '@kerfjs/ui/tab-bar';
import { TabBarItem } from '@kerfjs/ui/tab-bar-item';
import { Text } from '@kerfjs/ui/text';
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

import type { PhaseEstimatePresentation } from '../active-ticket-work';
import type { CodeReview, DuplicateBacklink, ProviderOutboxOperation, StartedPhase, TicketCloseReason } from '../api';
import type { AttachmentFilenameEditing } from '../attachment-filename-editing';
import type { AttachmentReferenceContext } from '../attachment-references';
import type { InlineFeedbackReply } from '../feedback-replies';
import { INSPECTOR_AND_EDITOR_ACTIONS } from '../interaction-attrs/inspector-and-editor';
import { ticketCloseReasonLabel } from '../ticket-close';
import { ticketDisplayNumber } from '../ticket-display-number';
import type { TicketFieldConflict as TicketFieldConflictState } from '../ticket-field-reconciliation';
import { isTicketTitleExpanded } from '../ticket-title-disclosure';
import type { TimelineUsageRow } from '../ticket-usage-timeline';
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
import { TicketInspectorPanel } from './ticket-inspector-panel';
import type { TicketPriority } from './ticket-row';
import { TicketSourceIcon, type TicketSourceIdentity } from './ticket-source-icon';
import { TicketTimeline, type TicketTimelineEntry } from './ticket-timeline';

export type InspectorTab = 'info' | 'timeline' | 'code-review' | 'attachments';

export interface TicketInspectorProps {
  slug: string;
  source?: TicketSourceIdentity;
  title: string;
  titleEditing?: boolean;
  titleDraft?: string;
  canUpdate?: boolean;
  canEditStartedPhase?: boolean;
  canEditText?: boolean;
  canAddNotes?: boolean;
  canEditNotes?: boolean;
  canDeleteNotes?: boolean;
  composingNote?: boolean;
  composerDraft?: string;
  status: TicketStatus;
  startedPhase?: StartedPhase;
  /** Estimated time left in the current testing phase (HS2-YDGM04). */
  phaseEstimate?: PhaseEstimatePresentation;
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
  /** Recent Jira write-behind states for this ticket, including confirmed history. */
  pendingOperations?: readonly ProviderOutboxOperation[];
  duplicateTarget?: DuplicateTargetSummary;
  duplicateBacklinks?: readonly DuplicateBacklink[];
  duplicateBacklinkInaccessibleProjects?: readonly string[];
  timelineEntries?: readonly TicketTimelineEntry[];
  /** The ticket's AI usage by model and in total, shown under the timeline (HS2-J14225). */
  timelineUsage?: readonly TimelineUsageRow[];
  attachments?: readonly TicketAttachmentItem[];
  codeReview?: CodeReview;
  codeReviewLoading?: boolean;
  codeReviewMessage?: string;
  expandedCodeReviewCommits?: readonly string[];
  attachmentsEnabled?: boolean;
  /** Existing attachments can be edited (the provider reports `attachment_edit`, HS2-HSA64D). */
  attachmentsEditable?: boolean;
  /** Existing attachments can be deleted (`attachment_delete`, HS2-51ADSG); defaults to `attachmentsEditable`. */
  attachmentsRemovable?: boolean;
  attachmentMessage?: string;
  /** The attachment batch key whose label editor is open (HS2-SG0AZY). */
  attachmentLabelEditing?: string;
  /** The controlled draft of that open label editor (HS2-0QQHSZ). */
  attachmentLabelDraft?: string;
  attachmentFilenameEdit?: AttachmentFilenameEditing;
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
  /**
   * Partial-detail variant (HS2-15833W): the props come from the ticket's list row while its full detail
   * loads. The row's summary (title, status, priority, tags, notices) renders normally; details, notes,
   * the timeline, and attachments show placeholders, and the whole surface is inert until the detail
   * read lands and replaces it.
   */
  contentLoading?: boolean;
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
  canEditStartedPhase = false,
  canEditText = canUpdate,
  canAddNotes = true,
  canEditNotes = true,
  canDeleteNotes = true,
  composingNote = false,
  composerDraft = '',
  status,
  startedPhase,
  phaseEstimate,
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
  pendingOperations = [],
  duplicateTarget,
  duplicateBacklinks = [],
  duplicateBacklinkInaccessibleProjects = [],
  timelineEntries,
  timelineUsage,
  attachments,
  codeReview,
  codeReviewLoading = false,
  codeReviewMessage = '',
  expandedCodeReviewCommits,
  attachmentsEnabled = true,
  attachmentsEditable = attachmentsEnabled,
  attachmentsRemovable = attachmentsEditable,
  attachmentMessage = '',
  attachmentLabelEditing,
  attachmentLabelDraft,
  attachmentFilenameEdit,
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
  source,
  updatedLabel,
  presentation = 'sidebar',
  largeText = false,
  fieldConflict,
  fieldConflictResolution = fieldConflict?.mine ?? '',
  contentLoading = false,
}: TicketInspectorProps): SidebarPanelParts {
  // While the detail loads nothing is actionable: the row-backed chrome is shown for continuity only.
  const loadingAttributes = contentLoading ? { inert: true, 'aria-busy': 'true', 'data-content-loading': 'true' } : {};
  const star = (
    <>
      {upNextEligible && (
        <button
          type="button"
          class={`ticket-inspector__star${upNext ? ' ticket-inspector__star--active' : ''}`}
          {...INSPECTOR_AND_EDITOR_ACTIONS.toggleInspectorUpNext.attrs}
          disabled={contentLoading}
          aria-label={upNext ? 'Remove from Up Next' : 'Add to Up Next'}
        >
          <LucideIcon
            icon={Star}
            name="star"
            appearance={upNext ? 'solid' : 'outline'}
            color={upNext ? foregroundColorVar('--hs-ticket-state-up-next') : undefined}
          />
        </button>
      )}
    </>
  );
  // The reader modal is not a Workbench panel: it keeps its own close control (HS2-QQW6CT).
  const close = (
    <button type="button" data-dialog-close="" data-action="close-ticket-reader" aria-label="Close Ticket Reader">
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
        <ToolbarControlGroup appearance="borderless" buttonAppearance="push" label="Reader Text Size" single>
          <button
            type="button"
            {...INSPECTOR_AND_EDITOR_ACTIONS.toggleReaderTextSize.attrs}
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
      <ToolbarControlGroup appearance="borderless" label="Ticket Actions">
        {star}
        <button
          type="button"
          {...INSPECTOR_AND_EDITOR_ACTIONS.openTicketReader.attrs}
          aria-label="Open Ticket Reader"
          title="Open ticket reader"
          disabled={contentLoading}
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
      {...INSPECTOR_AND_EDITOR_ACTIONS.copyTicketSlug.attrs}
      // The Workbench composes this toolbar outside the app-owned wrappers, so the button names its
      // own ticket (HS2-QQW6CT).
      data-ticket-slug={slug}
      aria-label={`Copy ticket number ${slug}`}
      title="Copy ticket number"
    >
      <ToolbarText text={ticketDisplayNumber(slug, source?.provider)} size="small" />
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
    <div class="ticket-inspector__header" data-component="ticket-inspector-header" {...identity} {...loadingAttributes}>
      {titleEditing ? (
        // A one-row textarea wraps a long title the way the static heading does (HS2-98ZVPE). The frame
        // mirrors the controlled draft in a hidden grid twin, so the editor grows with its wrapped lines
        // in every engine; Enter and pasted line breaks keep the title on one logical line.
        <div
          class={
            presentation === 'reader'
              ? 'ticket-inspector__title-frame ticket-inspector__title-frame--reader'
              : 'ticket-inspector__title-frame'
          }
          data-title-mirror={titleDraft}
        >
          <textarea
            class={
              presentation === 'reader'
                ? 'ticket-inspector__title-input ticket-inspector__title-input--reader'
                : 'ticket-inspector__title-input'
            }
            name="ticket-title"
            aria-label="Ticket Title"
            rows={1}
            spellcheck="true"
          >
            {titleDraft}
          </textarea>
        </div>
      ) : (
        <div
          class="ticket-inspector__title-disclosure"
          data-ticket-title-disclosure="true"
          data-ticket-slug={slug}
          data-presentation={presentation}
          data-title-expanded={String(isTicketTitleExpanded(slug, presentation))}
        >
          <h1
            class={
              presentation === 'reader'
                ? 'ticket-inspector__title ticket-inspector__title--reader'
                : 'ticket-inspector__title'
            }
            data-action={canUpdate ? 'edit-ticket-title' : undefined}
            data-editable={String(canUpdate)}
            tabIndex={canUpdate ? 0 : undefined}
            title={canUpdate ? 'Double-click to edit title' : undefined}
          >
            {title}
          </h1>
          <button
            type="button"
            class="ticket-inspector__title-toggle"
            {...INSPECTOR_AND_EDITOR_ACTIONS.toggleTicketTitle.attrs}
            aria-expanded={String(isTicketTitleExpanded(slug, presentation))}
          >
            {isTicketTitleExpanded(slug, presentation) ? 'Collapse title' : 'Show full title'}
          </button>
        </div>
      )}
      {liveClaim && <LiveClaimNotice {...liveClaim} />}
      {feedbackNeeded && (
        <div class="ticket-inspector__notice ticket-inspector__feedback">
          <StateBanner
            title="Needs Review"
            tone="warning"
            icon={<LucideIcon icon={CircleAlert} name="circle-alert" size="s" />}
          />
        </div>
      )}
      {(status === 'completed' || status === 'verified') && latestConfidence !== undefined && (
        <div
          class="ticket-inspector__notice ticket-inspector__confidence"
          role="status"
          data-confidence={String(latestConfidence)}
        >
          <Row wrap gap="xs" vAlign="middle" controlInsets="rl">
            <ConfidenceBadge value={latestConfidence} appearance="labeled" />
            <Text variant="span" tone="quiet" size="compact" border="none" flush>
              Reported by the completing AI
            </Text>
          </Row>
        </div>
      )}
      {closeReason && closeReason !== 'duplicate' && (
        <div class="ticket-inspector__notice" data-close-reason={closeReason}>
          <StateBanner title={`Closed as ${ticketCloseReasonLabel(closeReason)?.toLowerCase() ?? closeReason}`} />
        </div>
      )}
      {closeReason === 'duplicate' &&
        (duplicateTarget ? (
          <TicketDuplicateTarget target={duplicateTarget} />
        ) : (
          <div class="ticket-inspector__notice" data-close-reason="duplicate">
            <StateBanner
              title="Duplicate of Another Ticket"
              icon={<LucideIcon size="s" icon={CopyX} name="copy-x" />}
            />
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
          label="Ticket Inspector Sections"
          className="ticket-inspector__tabs"
          activation="automatic"
          allocation="fill"
          presentation="inspector"
          // The reader keeps segmented names and lets Kerf switch them to icon-only below 832px.
          iconOnlyAt={presentation === 'reader' ? 'wide' : undefined}
        >
          {tabs.map((tab) => (
            <TabBarItem
              id={tab.id}
              name={tab.label}
              selected={activeTab === tab.id}
              closable={false}
              selectAction="set-inspector-tab"
              // Sidebar and rail inspectors are too narrow for names; icon-only keeps `name` accessible.
              presentation={presentation === 'reader' ? 'segmented' : 'icon-only'}
              size="compact"
              rootAttributes={{ 'data-inspector-tab': tab.id }}
              leading={<LucideIcon icon={tab.icon} name={tab.iconName} size={14.4} />}
              trailing={
                tab.id === 'attachments' && attachments?.length ? (
                  <span class="ticket-inspector__tab-count">
                    {/* A generic span cannot carry an accessible name, so the badge is decorative and a
                        visually hidden label speaks the count (HS2-MMHS28). */}
                    <Badge size="compact" ariaHidden>
                      {attachments.length}
                    </Badge>
                    <span class="wa-visually-hidden">{attachments.length} attachments</span>
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
    <div class="ticket-inspector__body" data-component="ticket-inspector-body" {...identity} {...loadingAttributes}>
      {activeTab === 'info' && pendingOperations.length > 0 && (
        <ContentItem appearance="warning" ariaLabel="Jira Sync Status" className="ticket-inspector__outbox">
          <List gap="xs">
            <strong>Jira sync</strong>
            {pendingOperations.map((operation) => (
              <div class="ticket-inspector__outbox-operation" data-state={operation.state}>
                <span>
                  {operation.state === 'needs_attention'
                    ? 'Needs attention'
                    : operation.state === 'rate_limited'
                      ? 'Waiting for Jira rate limit'
                      : operation.state === 'sending'
                        ? 'Sending…'
                        : operation.state === 'queued'
                          ? 'Queued locally'
                          : operation.state === 'confirmed'
                            ? 'Confirmed by Jira'
                            : 'Local edit discarded'}
                </span>
                {operation.last_error && <small>{operation.last_error}</small>}
                {operation.conflict && (
                  <small>Remote values changed while this edit was pending; local fields take precedence.</small>
                )}
                {operation.state === 'needs_attention' && (
                  <Button
                    type="button"
                    size="default"
                    emphasis="secondary"
                    action={INSPECTOR_AND_EDITOR_ACTIONS.retryProviderOutbox.value}
                    rootAttributes={{
                      'data-connection-id': operation.connection_id,
                      'data-operation-id': operation.operation_id,
                    }}
                  >
                    Retry
                  </Button>
                )}
                {(operation.state === 'queued' ||
                  operation.state === 'rate_limited' ||
                  operation.state === 'needs_attention') && (
                  <Button
                    type="button"
                    size="default"
                    emphasis="secondary"
                    action={INSPECTOR_AND_EDITOR_ACTIONS.discardProviderOutbox.value}
                    rootAttributes={{
                      'data-connection-id': operation.connection_id,
                      'data-operation-id': operation.operation_id,
                    }}
                  >
                    Discard Local Edit
                  </Button>
                )}
              </div>
            ))}
          </List>
        </ContentItem>
      )}
      {activeTab === 'info' && (
        <TicketInfoPanel
          status={status}
          startedPhase={startedPhase}
          phaseEstimate={phaseEstimate}
          priority={priority}
          category={category}
          tags={tags}
          tagSuggestions={tagSuggestions}
          tagPopoverId={`ticket-tag-${presentation}-${slug.toLowerCase()}`}
          canUpdate={canUpdate}
          canEditStartedPhase={canEditStartedPhase}
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
          contentLoading={contentLoading}
        />
      )}
      {contentLoading && activeTab !== 'info' && (
        <TicketInspectorPanel
          component="ticket-inspector-section-loading"
          presentation={presentation}
          attributes={{ 'data-inspector-section': activeTab }}
        >
          <Skeleton lines={3} />
        </TicketInspectorPanel>
      )}
      {!contentLoading && activeTab === 'timeline' && (
        <TicketTimeline entries={timelineEntries} presentation={presentation} usage={timelineUsage} />
      )}
      {!contentLoading && activeTab === 'code-review' && (
        <TicketCodeReview
          review={codeReview}
          loading={codeReviewLoading}
          message={codeReviewMessage}
          expandedCommits={expandedCodeReviewCommits}
          presentation={presentation}
        />
      )}
      {!contentLoading && activeTab === 'attachments' && (
        <TicketAttachments
          attachments={attachments}
          enabled={attachmentsEnabled}
          editable={attachmentsEditable}
          removable={attachmentsRemovable}
          message={attachmentMessage}
          editingLabelBatch={attachmentLabelEditing}
          editingLabelDraft={attachmentLabelDraft}
          filenameEdit={attachmentFilenameEdit}
          presentation={presentation}
        />
      )}
    </div>
  );
  return {
    label: `${slug} inspector`,
    toolbar: {
      label: 'Ticket Inspector Toolbar',
      dividerSides: '',
      leading: source ? (
        <Row gap="2xs" vAlign="middle" rootAttributes={{ 'data-source-identity': 'true' }}>
          <TicketSourceIcon source={source} size="compact" />
          {slugButton}
        </Row>
      ) : (
        slugButton
      ),
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

/**
 * The inspector's loading placeholder parts (HS2-REG3A2): the real inspector chrome (toolbar, header,
 * Kerf tab bar, and the {@link TicketInfoPanel} `placeholder` variant) with the unknown ticket values as
 * unanimated Skeleton blocks. It lives with the inspector so only the inspector renders its own class
 * contract, and the info panel and notes render their own placeholder variants (HS2-XBHADT). The tabs
 * use the loaded sidebar inspector's icon-only TabBar presentation so they fit any rail width
 * (HS2-MYS1MR). Like the inspector it exposes Workbench panel parts (HS2-QQW6CT).
 */
export function ticketInspectorSkeletonPanel({ slug }: { slug?: string } = {}): SidebarPanelParts {
  return {
    label: 'Loading ticket',
    toolbar: {
      label: 'Ticket Inspector Toolbar',
      dividerSides: '',
      center: slug ? <ToolbarText text={slug} size="small" /> : <ToolbarText text="" size="small" placeholder />,
      trailing: (
        <ToolbarControlGroup appearance="borderless" label="Ticket Actions">
          <button type="button" aria-label="Open Ticket Reader" title="Open ticket reader" tabIndex={-1}>
            <LucideIcon icon={BookOpen} name="book-open" />
          </button>
        </ToolbarControlGroup>
      ),
    },
    toggle: inspectorToggle(),
    header: (
      // The chrome is shown for continuity only; `inert` keeps all of it non-interactive while loading.
      <div class="ticket-inspector__header" data-component="ticket-inspector-header" aria-hidden="true" inert>
        <div class="ticket-inspector__title-placeholder">
          <Skeleton height={rem(1.25)} />
          <Skeleton width={pct(62)} height={rem(1.25)} />
        </div>
        <div class="ticket-inspector__tabs-frame">
          <TabBar
            id="ticket-inspector-loading"
            label="Ticket Inspector Sections"
            className="ticket-inspector__tabs"
            allocation="fill"
            presentation="inspector"
          >
            {tabs.map((tab) => (
              <TabBarItem
                id={tab.id}
                name={tab.label}
                selected={tab.id === 'info'}
                closable={false}
                placeholder
                presentation="icon-only"
                size="compact"
                leading={<LucideIcon icon={tab.icon} name={tab.iconName} size={14.4} />}
              />
            ))}
          </TabBar>
        </div>
      </div>
    ),
    content: (
      <div class="ticket-inspector__body" data-component="ticket-inspector-skeleton-body" aria-busy="true">
        <TicketInfoPanel placeholder />
      </div>
    ),
    pane: {},
  };
}

/** The skeleton rendered standalone (the terminal rail's pushed detail and the UX catalog). */
export function TicketInspectorSkeleton({
  slug,
  collapseControl = false,
}: { slug?: string; collapseControl?: boolean } = {}) {
  return (
    <aside
      class="ticket-inspector ticket-inspector--placeholder"
      data-component="ticket-inspector-skeleton"
      aria-busy="true"
      aria-label="Loading Ticket"
    >
      <SidebarPane
        parts={ticketInspectorSkeletonPanel({ slug })}
        element="div"
        side="right"
        collapseControl={collapseControl}
      />
    </aside>
  );
}
