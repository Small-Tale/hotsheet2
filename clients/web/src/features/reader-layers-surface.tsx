import type { Signal } from 'kerfjs';

import type { CodeReview, FullTicket, TicketRow as WireTicketRow } from '../api';
import {
  type AttachmentLabelEditing,
  attachmentLabelEditingDraft,
  attachmentLabelEditingKey,
} from '../attachment-label-editing';
import type { AttachmentReferenceContext } from '../attachment-references';
import type { LiveClaimNoticeProps } from '../components/active-claim';
import { type MarkdownEditorMode } from '../components/markdown-editor';
import type {
  AttachmentContextMenuSurface,
  ChangeEvidenceSurface,
  ReaderLayersSurface,
} from '../components/reader-overlay-surfaces';
import { ReaderLayerSurface } from '../components/reader-overlay-surfaces';
import { type TicketStatus } from '../components/status-badge';
import { type InspectorTab } from '../components/ticket-inspector';
import { type TicketPriority } from '../components/ticket-row';
import { fullTicketFeedbackNeeded } from '../feedback-needed';
import { type InlineFeedbackReply } from '../feedback-replies';
import { type GallerySource, type Project } from '../interactions/types';
import { type TicketFieldConflict } from '../ticket-field-reconciliation';
import { type TicketReaderFrame } from '../ticket-reader-stack';
import { type TicketTitleEditSurface } from '../ticket-title-editing';
import type { createInspectorSurface } from './inspector-surface';

/** Live application bindings createReaderLayersSurface reads (HS2-K0VC2C). */
export interface ReaderLayersSurfaceDependencies {
  ago: (value?: string) => string;
  attachmentContext: (ticket: FullTicket, current?: Project) => AttachmentReferenceContext | undefined;
  attachmentItems: ReturnType<typeof createInspectorSurface>['attachmentItems'];
  attachmentLabelEditing: Signal<AttachmentLabelEditing | undefined>;
  attachmentMenuSurfaceProps: ({
    insideGallery,
    readerScope,
  }?: {
    insideGallery?: boolean;
    readerScope?: string;
  }) => Parameters<typeof AttachmentContextMenuSurface>[0]['menu'];
  attachmentMessage: Signal<string>;
  canAddNotes: () => boolean;
  canDeleteNotes: () => boolean;
  canEditAttachments: () => boolean;
  canEditNotes: () => boolean;
  canUpdateSelected: () => boolean;
  canUseAttachments: () => boolean;
  changeEvidenceSurfaceProps: (readerScope?: string) => Parameters<typeof ChangeEvidenceSurface>[0]['evidence'];
  codeReview: Signal<CodeReview | undefined>;
  codeReviewLoading: Signal<boolean>;
  codeReviewMessage: Signal<string>;
  composingNote: Signal<boolean>;
  duplicateTargetFor: (
    ticket: FullTicket,
  ) => { id: string; projectName: string; slug: string; title: string } | undefined;
  expandedCodeReviewCommits: Signal<string[]>;
  fieldConflict: Signal<TicketFieldConflict | undefined>;
  fieldConflictResolution: Signal<string>;
  linkedReaderStack: Signal<TicketReaderFrame[]>;
  liveClaimNotice: (ticket: WireTicketRow) => LiveClaimNoticeProps | undefined;
  newNoteDraft: Signal<string>;
  notes: (ticket: FullTicket) => {
    readonly id: string;
    readonly kind: 'regular' | 'activity' | 'feedback_needed' | 'feedback_draft' | 'status';
    readonly feedbackPrompt: boolean;
    readonly author: string;
    readonly time: string;
    readonly body: string;
    readonly aiAuthored: boolean;
    readonly humanEdited: boolean | undefined;
    readonly aiTool: string | undefined;
    readonly confidence: number | undefined;
    readonly feedbackFor: string | undefined;
    readonly ratingMetadata:
      { target: string; rating: 'helpful' | 'not_helpful' | null; explanation?: string } | undefined;
  }[];
  priority: (value?: string) => TicketPriority;
  project: () => Project | undefined;
  projects: Signal<Project[]>;
  readerBlockedReasonDraft: Signal<string>;
  readerBlockedReasonEditing: Signal<boolean>;
  readerDetailsDraft: Signal<string>;
  readerDetailsMode: Signal<MarkdownEditorMode>;
  readerEditingNoteId: Signal<string | undefined>;
  readerFeedbackChoiceSelections: Signal<Record<string, string[]>>;
  readerInlineFeedbackReplies: Signal<Record<string, InlineFeedbackReply[]>>;
  readerLargeText: Signal<boolean>;
  readerNoteDraft: Signal<string>;
  readerOpen: Signal<boolean>;
  readerTab: Signal<InspectorTab>;
  selectedTicket: Signal<FullTicket | null>;
  status: (value?: string) => TicketStatus;
  tagSuggestions: () => string[];
  timeline: (ticket: FullTicket) => {
    time: string;
    timestamp: string;
    id: string;
    title: string;
    subtitle?: string;
    durationToNext?: string;
    emphasized?: boolean;
  }[];
  titleDraft: Signal<string>;
  titleEditingSurface: Signal<TicketTitleEditSurface | undefined>;
}

/**
 * Projects the ticket reader stack: each reader layer's props, the gallery source lookup, and the
 * reader-layers surface. Extracted from the application runtime without behavior changes
 * (HS2-K0VC2C).
 */
export function createReaderLayersSurface(dependencies: ReaderLayersSurfaceDependencies) {
  const {
    ago,
    attachmentContext,
    attachmentItems,
    attachmentLabelEditing,
    attachmentMenuSurfaceProps,
    attachmentMessage,
    canAddNotes,
    canDeleteNotes,
    canEditAttachments,
    canEditNotes,
    canUpdateSelected,
    canUseAttachments,
    changeEvidenceSurfaceProps,
    codeReview,
    codeReviewLoading,
    codeReviewMessage,
    composingNote,
    duplicateTargetFor,
    expandedCodeReviewCommits,
    fieldConflict,
    fieldConflictResolution,
    linkedReaderStack,
    liveClaimNotice,
    newNoteDraft,
    notes,
    priority,
    project,
    projects,
    readerBlockedReasonDraft,
    readerBlockedReasonEditing,
    readerDetailsDraft,
    readerDetailsMode,
    readerEditingNoteId,
    readerFeedbackChoiceSelections,
    readerInlineFeedbackReplies,
    readerLargeText,
    readerNoteDraft,
    readerOpen,
    readerTab,
    selectedTicket,
    status,
    tagSuggestions,
    timeline,
    titleDraft,
    titleEditingSurface,
  } = dependencies;
  function readerLayerSurface({
    ticket,
    current,
    active,
    open,
    editable,
    frame,
    activeTab,
    stackPosition,
    stackSize,
  }: {
    ticket: FullTicket;
    current: Project;
    active: boolean;
    open: boolean;
    editable: boolean;
    frame?: TicketReaderFrame;
    activeTab: InspectorTab;
    stackPosition: number;
    stackSize: number;
  }) {
    const linked = frame?.edit,
      capabilities = frame ? { ...frame.capabilities, notes: false, note_delete: false } : undefined,
      canUpdate = editable && canUpdateSelected();
    const readerScope = frame?.id ?? 'workspace-reader';
    return (
      <ReaderLayerSurface
        slug={ticket.slug}
        title={ticket.title}
        titleEditing={editable && titleEditingSurface.value === 'reader'}
        titleDraft={editable ? titleDraft.value : ticket.title}
        canUpdate={canUpdate}
        canEditText={frame ? capabilities!.update : canUpdate}
        canAddNotes={frame ? capabilities!.notes : editable && canAddNotes()}
        canEditNotes={frame ? capabilities!.note_edit : editable && canEditNotes()}
        canDeleteNotes={frame ? capabilities!.note_delete : editable && canDeleteNotes()}
        composingNote={editable && composingNote.value}
        composerDraft={editable ? newNoteDraft.value : ''}
        status={status(ticket.status)}
        priority={priority(ticket.priority)}
        category={ticket.category ?? 'issue'}
        tags={ticket.tags}
        tagSuggestions={editable ? tagSuggestions() : undefined}
        details={
          linked?.detailsMode === 'write'
            ? linked.detailsDraft
            : editable && readerDetailsMode.value === 'write'
              ? readerDetailsDraft.value
              : ticket.details
        }
        liveClaim={liveClaimNotice(ticket)}
        detailsMode={linked?.detailsMode ?? (editable ? readerDetailsMode.value : 'preview')}
        detailsDirty={
          Boolean(linked && linked.detailsDraft !== ticket.details) ||
          (editable && readerDetailsDraft.value !== ticket.details)
        }
        notes={notes(ticket)}
        editingNoteId={linked?.editingNoteId ?? (editable ? readerEditingNoteId.value : undefined)}
        noteDraft={linked?.noteDraft ?? (editable ? readerNoteDraft.value : '')}
        inlineFeedbackReplies={editable ? readerInlineFeedbackReplies.value : {}}
        feedbackChoiceSelections={editable ? readerFeedbackChoiceSelections.value : {}}
        blockedReason={ticket.blocked_reason}
        blockedReasonEditing={linked?.blockedReasonEditing ?? (editable && readerBlockedReasonEditing.value)}
        blockedReasonDraft={
          linked?.blockedReasonDraft ?? (editable ? readerBlockedReasonDraft.value : (ticket.blocked_reason ?? ''))
        }
        providerName="Hot Sheet Git"
        updatedLabel={`Updated ${ago(ticket.updated_at)}`}
        activeTab={activeTab}
        upNext={ticket.up_next}
        upNextEligible={editable && (ticket.status === 'not_started' || ticket.status === 'started')}
        feedbackNeeded={fullTicketFeedbackNeeded(ticket)}
        closeReason={ticket.close_reason}
        latestConfidence={ticket.latest_confidence}
        duplicateTarget={duplicateTargetFor(ticket)}
        timelineEntries={timeline(ticket)}
        attachments={attachmentItems(ticket, current)}
        attachmentContext={attachmentContext(ticket, current)}
        codeReview={editable ? codeReview.value : undefined}
        codeReviewLoading={editable && codeReviewLoading.value}
        codeReviewMessage={editable ? codeReviewMessage.value : ''}
        expandedCodeReviewCommits={editable ? expandedCodeReviewCommits.value : []}
        attachmentsEnabled={editable && canUseAttachments()}
        attachmentsEditable={editable && canEditAttachments()}
        attachmentMessage={editable ? attachmentMessage.value : 'Attachments are view-only in linked ticket readers.'}
        attachmentLabelEditing={attachmentLabelEditingKey(attachmentLabelEditing.value, readerScope)}
        attachmentLabelDraft={attachmentLabelEditingDraft(attachmentLabelEditing.value, readerScope)}
        largeText={readerLargeText.value}
        fieldConflict={editable ? fieldConflict.value : undefined}
        fieldConflictResolution={editable ? fieldConflictResolution.value : ''}
        active={active}
        frameId={readerScope}
        open={open}
        projectName={current.name}
        stackPosition={stackPosition}
        stackSize={stackSize}
        style={stackPosition < stackSize ? `--ticket-reader-offset:${(stackSize - stackPosition) * -10}px` : undefined}
        attachmentMenu={attachmentMenuSurfaceProps({ readerScope })}
        evidence={changeEvidenceSurfaceProps(readerScope)}
        readOnly={frame ? !capabilities!.update && !capabilities!.note_edit : !editable}
      />
    );
  }
  // Media opened from a stacked (linked) reader belongs to that reader's ticket and project (HS2-97E0QR).
  function gallerySourceFor(target: Element): GallerySource | undefined {
    const frameId = target.closest<HTMLElement>('[data-component="ticket-reader"]')?.dataset.readerFrameId,
      frame = frameId ? linkedReaderStack.value.find((item) => item.id === frameId) : undefined,
      owner = frame ? projects.value.find((item) => item.id === frame.projectId) : undefined;
    if (!frame || !owner) return undefined;
    return {
      ticket: frame.ticket,
      project: owner,
      // Linked readers never manage attachments (only the workspace reader is editable).
      readOnly: true,
      update: (ticket) => {
        linkedReaderStack.value = linkedReaderStack.value.map((item) =>
          item.id === frame.id ? { ...item, ticket } : item,
        );
      },
    };
  }
  function readerLayersSurfaceProps(): Parameters<typeof ReaderLayersSurface>[0] {
    const current = project(),
      base =
        selectedTicket.value && current
          ? {
              id: 'workspace-reader',
              open: readerOpen.value,
              project: current,
              ticket: selectedTicket.value,
              activeTab: readerTab.value,
              frame: undefined,
            }
          : undefined,
      linked = linkedReaderStack.value.flatMap((frame) => {
        const project = projects.value.find((item) => item.id === frame.projectId);
        return project
          ? [{ id: frame.id, open: frame.open, project, ticket: frame.ticket, activeTab: frame.activeTab, frame }]
          : [];
      }),
      openLayers = [...(base?.open ? [base] : []), ...linked.filter((layer) => layer.open)],
      top = openLayers.at(-1)?.id,
      position = new Map(openLayers.map((layer, index) => [layer.id, index + 1]));
    return {
      layers: [...(base ? [base] : []), ...linked].map((layer) => {
        const stackPosition = position.get(layer.id) ?? 1,
          active = layer.id === top;
        return readerLayerSurface({
          ticket: layer.ticket,
          current: layer.project,
          active,
          open: layer.open,
          editable: active && layer.id === 'workspace-reader',
          frame: layer.frame,
          activeTab: layer.activeTab,
          stackPosition,
          stackSize: Math.max(1, openLayers.length),
        });
      }),
    };
  }
  return { readerLayerSurface, gallerySourceFor, readerLayersSurfaceProps };
}
