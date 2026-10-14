import type { Signal } from 'kerfjs';

import type {
  Api,
  Capabilities,
  CodeReview,
  DuplicateBacklink,
  FullTicket,
  ProviderOutboxOperation,
  TicketRow as WireTicketRow,
} from '../api';
import { attachmentRoundNumbers } from '../attachment-grouping';
import {
  type AttachmentLabelEditing,
  attachmentLabelEditingDraft,
  attachmentLabelEditingKey,
  INSPECTOR_ATTACHMENT_LABEL_SCOPE,
} from '../attachment-label-editing';
import type { AttachmentReferenceContext } from '../attachment-references';
import type { LiveClaimNoticeProps } from '../components/active-claim';
import { type MarkdownEditorMode } from '../components/markdown-editor';
import type { NotWorkingSurface } from '../components/reader-overlay-surfaces';
import { type TicketStatus } from '../components/status-badge';
import { type InspectorTab, type TicketInspectorProps } from '../components/ticket-inspector';
import { type TicketPriority } from '../components/ticket-row';
import { fullTicketFeedbackNeeded } from '../feedback-needed';
import { type NotWorkingTarget, type PendingEvidence, type Project } from '../interactions/types';
import { type ProjectTicketSources } from '../new-ticket-source';
import { type TicketFieldConflict } from '../ticket-field-reconciliation';
import { type TicketTitleEditSurface } from '../ticket-title-editing';

/** Live application bindings createInspectorSurface reads (HS2-K0VC2C). */
export interface InspectorSurfaceDependencies {
  ago: (value?: string) => string;
  api: () => Api;
  attachmentContext: (ticket: FullTicket, current?: Project) => AttachmentReferenceContext | undefined;
  attachmentLabelEditing: Signal<AttachmentLabelEditing | undefined>;
  attachmentMessage: Signal<string>;
  attachmentsEditableFor: (connectionId: string) => boolean;
  blockedReasonDraft: Signal<string>;
  blockedReasonEditing: Signal<boolean>;
  canAddNotes: () => boolean;
  canDeleteNotes: () => boolean;
  canEditAttachments: () => boolean;
  canEditNotes: () => boolean;
  canEditStartedPhaseSelected: () => boolean;
  canUpdateSelected: () => boolean;
  canUseAttachments: () => boolean;
  capabilitiesFor: (connectionId: string) => Capabilities;
  codeReview: Signal<CodeReview | undefined>;
  codeReviewLoading: Signal<boolean>;
  codeReviewMessage: Signal<string>;
  composingNote: Signal<boolean>;
  defaultProviders: Signal<Record<string, ProjectTicketSources | undefined>>;
  detailsDraft: Signal<string>;
  detailsMode: Signal<MarkdownEditorMode>;
  duplicateBacklinkState: Signal<{ key: string; backlinks: DuplicateBacklink[]; inaccessibleProjects: string[] }>;
  duplicateTargetFor: (
    ticket: FullTicket,
  ) => { id: string; projectName: string; slug: string; title: string } | undefined;
  editingNoteId: Signal<string | undefined>;
  expandedCodeReviewCommits: Signal<string[]>;
  fieldConflict: Signal<TicketFieldConflict | undefined>;
  fieldConflictResolution: Signal<string>;
  inspectorTab: Signal<InspectorTab>;
  liveClaimNotice: (ticket: WireTicketRow) => LiveClaimNoticeProps | undefined;
  newNoteDraft: Signal<string>;
  noteDraft: Signal<string>;
  notes: (ticket: FullTicket) => {
    readonly id: string;
    readonly kind: 'regular' | 'activity' | 'feedback_needed' | 'feedback_draft' | 'status';
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
  providerOutboxOperations: Signal<Record<string, ProviderOutboxOperation | undefined>>;
  readerOpen: Signal<boolean>;
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
 * Projects the selected ticket's inspector props, the not-working dialog props, and ticket
 * attachment items. Extracted from the application runtime without behavior changes (HS2-K0VC2C).
 */
export function createInspectorSurface(dependencies: InspectorSurfaceDependencies) {
  const {
    ago,
    api,
    attachmentContext,
    attachmentLabelEditing,
    attachmentMessage,
    attachmentsEditableFor,
    blockedReasonDraft,
    blockedReasonEditing,
    canAddNotes,
    canDeleteNotes,
    canEditAttachments,
    canEditNotes,
    canEditStartedPhaseSelected,
    canUpdateSelected,
    canUseAttachments,
    capabilitiesFor,
    codeReview,
    codeReviewLoading,
    codeReviewMessage,
    composingNote,
    defaultProviders,
    detailsDraft,
    detailsMode,
    duplicateBacklinkState,
    duplicateTargetFor,
    editingNoteId,
    expandedCodeReviewCommits,
    fieldConflict,
    fieldConflictResolution,
    inspectorTab,
    liveClaimNotice,
    newNoteDraft,
    noteDraft,
    notes,
    priority,
    project,
    providerOutboxOperations,
    readerOpen,
    selectedTicket,
    status,
    tagSuggestions,
    timeline,
    titleDraft,
    titleEditingSurface,
  } = dependencies;
  function notWorkingSurfaceProps(
    target: NotWorkingTarget,
    note: string,
    files: PendingEvidence[],
    submitting: boolean,
    submissionError: string,
  ): Parameters<typeof NotWorkingSurface>[0] {
    const capabilities = target.slug ? capabilitiesFor(target.connectionId) : undefined;
    return {
      slug: target.slug || 'ticket',
      mode: target.mode,
      open: Boolean(target.slug),
      note,
      attachments: files,
      notesEnabled: capabilities?.notes ?? true,
      attachmentsEnabled: capabilities?.attachments ?? true,
      submitting,
      error: submissionError,
    };
  }
  function attachmentItems(ticket: FullTicket, current: Project) {
    const rounds = attachmentRoundNumbers(ticket.attachments, ticket.notes),
      // Generated video posters are stored beside git attachments; a provider that cannot
      // edit attachments has no poster to fetch or upload (HS2-HSA64D).
      posters = attachmentsEditableFor(ticket.connection_id);
    return ticket.attachments.map((item) => ({
      id: item.id,
      name: item.filename,
      url: api().checkoutAttachmentUrl(current.id, ticket.qualified_id, item.id),
      thumbnailUrl: posters
        ? api().checkoutAttachmentThumbnailUrl(current.id, ticket.qualified_id, item.id)
        : undefined,
      manageVideoPoster: posters,
      annotationCount: item.annotations?.length ?? 0,
      round: rounds.get(item.id),
      batch_id: item.batch_id,
      batch_label: item.batch_label,
      actor: item.actor,
      purpose: item.purpose,
    }));
  }
  function selectedInspectorProps(): TicketInspectorProps | undefined {
    const ticket = selectedTicket.value,
      current = project();
    if (!ticket || !current) return;
    const backlinks =
      duplicateBacklinkState.value.key === `${current.id}:${ticket.qualified_id}`
        ? duplicateBacklinkState.value
        : undefined;
    return {
      slug: ticket.slug,
      source: defaultProviders.value[current.id]?.sources.find(
        (source) => source.connectionId === ticket.connection_id,
      ),
      title: ticket.title,
      liveClaim: liveClaimNotice(ticket),
      pendingOperations: Object.values(providerOutboxOperations.value)
        .filter((operation): operation is ProviderOutboxOperation =>
          Boolean(
            operation && operation.connection_id === ticket.connection_id && operation.native_id === ticket.native_id,
          ),
        )
        .slice(0, 5),
      titleEditing: titleEditingSurface.value === 'inspector',
      titleDraft: titleDraft.value,
      canUpdate: canUpdateSelected(),
      canEditStartedPhase: canEditStartedPhaseSelected(),
      canAddNotes: canAddNotes(),
      canEditNotes: canEditNotes(),
      canDeleteNotes: canDeleteNotes(),
      composingNote: composingNote.value,
      composerDraft: newNoteDraft.value,
      status: status(ticket.status),
      startedPhase: ticket.started_phase,
      priority: priority(ticket.priority),
      category: ticket.category ?? 'issue',
      tags: ticket.tags,
      tagSuggestions: tagSuggestions(),
      details: detailsMode.value === 'write' ? detailsDraft.value : ticket.details,
      detailsMode: detailsMode.value,
      detailsDirty: detailsDraft.value !== ticket.details,
      notes: notes(ticket),
      editingNoteId: editingNoteId.value,
      noteDraft: noteDraft.value,
      blockedReason: ticket.blocked_reason,
      blockedReasonEditing: blockedReasonEditing.value,
      blockedReasonDraft: blockedReasonDraft.value,
      providerName: 'Hot Sheet Git',
      updatedLabel: `Updated ${ago(ticket.updated_at)}`,
      activeTab: inspectorTab.value,
      upNext: ticket.up_next,
      upNextEligible: ticket.status === 'not_started' || ticket.status === 'started',
      feedbackNeeded: fullTicketFeedbackNeeded(ticket),
      closeReason: ticket.close_reason,
      latestConfidence: ticket.latest_confidence,
      duplicateTarget: duplicateTargetFor(ticket),
      duplicateBacklinks: backlinks?.backlinks,
      duplicateBacklinkInaccessibleProjects: backlinks?.inaccessibleProjects,
      timelineEntries: timeline(ticket),
      attachments: attachmentItems(ticket, current),
      attachmentContext: attachmentContext(ticket),
      codeReview: codeReview.value,
      codeReviewLoading: codeReviewLoading.value,
      codeReviewMessage: codeReviewMessage.value,
      expandedCodeReviewCommits: expandedCodeReviewCommits.value,
      attachmentsEnabled: canUseAttachments(),
      attachmentsEditable: canEditAttachments(),
      attachmentMessage: attachmentMessage.value,
      attachmentLabelEditing: attachmentLabelEditingKey(attachmentLabelEditing.value, INSPECTOR_ATTACHMENT_LABEL_SCOPE),
      attachmentLabelDraft: attachmentLabelEditingDraft(attachmentLabelEditing.value, INSPECTOR_ATTACHMENT_LABEL_SCOPE),
      fieldConflict: readerOpen.value ? undefined : fieldConflict.value,
      fieldConflictResolution: fieldConflictResolution.value,
    };
  }
  return { notWorkingSurfaceProps, attachmentItems, selectedInspectorProps };
}
