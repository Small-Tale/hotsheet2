import { batch, type Signal, signal } from 'kerfjs';

import {
  Api,
  ApiHttpError,
  type AttachmentMetadata,
  type Capabilities,
  type CheckoutTicketCounts,
  type CodeReview,
  type DuplicateBacklink,
  type FeedbackDelivery,
  type FullTicket,
  type TicketCloseReason,
  type TicketRow as WireTicketRow,
  type TicketUsage,
} from '../api';
import { describeUnreadableAttachments, screenAttachmentFiles } from '../attachment-files';
import { attachmentUploadBatchId } from '../attachment-grouping';
import { type AttachmentReferenceContext, isVideoAttachment } from '../attachment-references';
import { browserOutboxId, browserRandomId } from '../browser-id';
import type { BulkUpdateHandle } from '../bulk-update-progress';
import type { BulkTicketDialogState } from '../components/bulk-ticket-dialog';
import type { MarkdownEditorMode } from '../components/markdown-editor';
import {
  focusQuickTicketComposerTitle,
  showQuickTicketComposer,
  STRANDED_ATTACHMENTS_MESSAGE,
  strandedNewTicketAttachments,
} from '../components/quick-ticket-composer';
import type { TicketCloseDialogState } from '../components/ticket-close-dialog';
import type { InspectorTab } from '../components/ticket-inspector';
import type { TicketLinkChoice } from '../components/ticket-link-choice-dialog';
import { isPlainTicketReselection, updateTicketSelection } from '../components/ticket-selection';
import type { Confirm } from '../confirmation';
import { copyWithSelection } from '../copy-text';
import {
  createDebouncedAutosave,
  createFocusLossAutosave,
  type FailedSaveRetryOptions,
  type FocusLossAutosave,
  type InFlightRecovery,
} from '../debounced-autosave';
import { draftFiles } from '../draft-file-recovery';
import { flushDraftInputs } from '../draft-input-coalescing';
import { finishDraftSubmission } from '../draft-submission-state';
import { featureOn } from '../feature-areas';
import {
  DETAILS_FEEDBACK_ID,
  isStaleDescriptionFeedbackDismissal,
  isStaleFeedbackDismissal,
  NO_RESPONSE_NEEDED_NOTE,
  presentedNoteKind,
} from '../feedback-needed';
import type { InlineFeedbackReply } from '../feedback-replies';
import { syncFocusedDraftControl } from '../focused-draft-sync';
import { SEARCH_AND_COMPOSER_TARGETS } from '../interaction-attrs/search-and-composer';
import { beginInteractionTiming } from '../interaction-performance';
import { data } from '../interactions/dom';
import type { Control, NotWorkingTarget, PendingEvidence, Project } from '../interactions/types';
import type { LocalTicketChangeAcknowledgements } from '../local-ticket-changes';
import { createTicketWithAttachments, describeNewTicketAttachmentFailures } from '../new-ticket-attachments';
import {
  newTicketCreationSourceId,
  newTicketUpNextDisabledReason,
  type ProjectTicketSources,
  rememberNewTicketSource,
  type TicketSourceChoice,
} from '../new-ticket-source';
import { submitNotWorkingReport } from '../not-working-workflow';
import { noteAuthorship } from '../note-authorship';
import { responseLinkUnavailable } from '../note-response-link';
import { type PendingCreatedTickets, prependCreatedTicketRow } from '../pending-created-tickets';
import { parseRemoteProjectId } from '../remote-projects/identity';
import {
  bulkTagChoices,
  type BulkTicketAction,
  type BulkTicketMutationSequencer,
  bulkTicketPatch,
  canAtomicallyBulkUpdate,
  canBulkUpdate,
  projectBulkTicketCounts,
  ticketUpNextDisabledReason,
} from '../ticket-bulk-operations';
import {
  duplicateReference,
  type DuplicateTarget,
  duplicateTargetKey,
  parseDuplicateReference,
  relationTargetProject,
  resolveDuplicateReferenceTarget,
  ticketCloseReasonLabel,
  validateTicketClose,
} from '../ticket-close';
import { createTicketDetailCache, sameTicketRevision } from '../ticket-detail-cache';
import {
  captureTicketDraft,
  clearTicketDraft,
  loadRestorableTicketDraft,
  planTicketDraftRestore,
  prepareCommittedDraftCleanup,
  saveTicketDraft,
  type TicketDraftField,
  ticketDraftKey,
} from '../ticket-draft-store';
import {
  conflictedEditorQueue,
  isTicketConcurrencyConflict,
  isWriteBehindUnavailable,
  mergedEditNotice,
  noteEditPatch,
  rebaseDraftValue,
  reconcileTicketPatch,
  resolvedConflictPatch,
  type TicketFieldConflict,
  ticketFieldLabel,
  ticketFieldText,
} from '../ticket-field-reconciliation';
import { ticketGroupCollapsed } from '../ticket-group-state';
import { withoutCollapsedGroupMembers } from '../ticket-groups';
import { bestTicketCopies, type TicketLinkMatch, ticketLinkMatchKey } from '../ticket-link-resolution';
import { projectTicketPatch, reportMutationTiming, ticketRowFromFull } from '../ticket-mutation';
import {
  type ClipboardTicket,
  deduplicateTitle,
  TicketHistory,
  type TicketPatch,
  type TicketSnapshot,
} from '../ticket-operations';
import {
  adoptCommittedReaderDrafts,
  linkedReaderPatchAllowed,
  reconcileTicketReaderFrame,
  resolveReaderFieldConflict,
  settleReaderFieldConflict,
  showReaderFieldConflict,
  type TicketReaderFrame,
  updateTicketReaderFrame,
} from '../ticket-reader-stack';
import { searchTicketReferences } from '../ticket-reference-search';
import { ticketTimelineEntries, timelineShares } from '../ticket-timeline-data';
import type { TicketTitleEditSurface } from '../ticket-title-editing';
import { copiedTicketPlacement, ticketClipboardText } from '../ticket-transfer';
import { timelineUsageLabels } from '../ticket-usage-timeline';
import {
  createdTicketVisibleInView,
  isTrashedTicket,
  newTicketCreationPlacement,
  newTicketStatusForView,
  selectionVisibleInView,
  type TicketView,
} from '../ticket-views';
import { timelineStageEstimate, typicalPhaseDurations } from '../timeline-stage-estimate';
import { trashEmptyLeftTickets, trashEmptySummary } from '../trash-empty-summary';
import { ensureVideoPoster } from '../video-posters';
import { quietWebAwesomeNativeDialog } from '../wa-dialog-semantics';
import { reconcileWorkerTicketSelection } from '../worker-ticket-refresh';
import type { WorkerTicketDetails } from '../worker-view-model';
import type { StoredPendingAttachment } from '../workspace-session';
import { pruneSelectionSavingEdits } from './selection-prune';

interface TicketWorkflowMutableState {
  blockedReasonDraftBase: string;
  bulkTicketSlugs: string[];
  clipboard: { tickets: ClipboardTicket[]; cut: boolean; source: Project } | undefined;
  detailsDraftBase: string;
  detailsEditGeneration: number;
  noteDraftBase: string;
  readerBlockedReasonDraftBase: string;
  readerDetailsDraftBase: string;
  readerDetailsEditGeneration: number;
  readerNoteDraftBase: string;
  ticketSelectionAnchor: string | undefined;
  titleDraftBase: string;
}

export interface TicketWorkflowDependencies {
  /** The shared in-app confirmation, offering to open a link's unopened remote project (HS2-4JTETR). */
  confirm?: Confirm;
  /** Opens a paired remote host's checkout as a tab (HS2-4JTETR). */
  openRemoteHostCheckout?: (bindingId: string, checkoutId: string) => Promise<Project>;
  /** A paired host's name for one of its checkouts; `null` when it lists none (HS2-4JTETR). */
  remoteHostCheckoutName?: (bindingId: string, checkoutId: string) => Promise<string | null | undefined>;
  state: TicketWorkflowMutableState;
  CLOSED_NOT_WORKING_TARGET: NotWorkingTarget;
  projects: Signal<Project[]>;
  selectedProjectId: Signal<string>;
  tickets: Signal<WireTicketRow[]>;
  workerDetails: Signal<WorkerTicketDetails>;
  ticketRowsByProject: Signal<Record<string, WireTicketRow[]>>;
  ticketCountsByProject: Signal<Record<string, CheckoutTicketCounts>>;
  selectedTicket: Signal<FullTicket | null>;
  selectedTicketSlugs: Signal<string[]>;
  selectedCorruptKey: Signal<string | undefined>;
  selectedView: Signal<TicketView>;
  ticketCollectionState: Signal<{ projectId: string; view: TicketView; status: 'loading' | 'error' } | undefined>;
  loading: Signal<boolean>;
  error: Signal<string>;
  attachmentMessage: Signal<string>;
  inspectorTab: Signal<InspectorTab>;
  inspectorVisible: Signal<boolean>;
  readerTab: Signal<InspectorTab>;
  readerOpen: Signal<boolean>;
  linkedReaderStack: Signal<TicketReaderFrame[]>;
  detailsMode: Signal<MarkdownEditorMode>;
  detailsDraft: Signal<string>;
  readerDetailsMode: Signal<MarkdownEditorMode>;
  readerDetailsDraft: Signal<string>;
  /** The one surface whose title editor is open (HS2-2M5BBN). */
  titleEditingSurface: Signal<TicketTitleEditSurface | undefined>;
  titleDraft: Signal<string>;
  blockedReasonEditing: Signal<boolean>;
  blockedReasonDraft: Signal<string>;
  readerBlockedReasonEditing: Signal<boolean>;
  readerBlockedReasonDraft: Signal<string>;
  editingNoteId: Signal<string | undefined>;
  readerEditingNoteId: Signal<string | undefined>;
  noteDraft: Signal<string>;
  readerNoteDraft: Signal<string>;
  fieldConflict: Signal<TicketFieldConflict | undefined>;
  fieldConflictResolution: Signal<string>;
  readerInlineFeedbackReplies: Signal<Record<string, InlineFeedbackReply[]>>;
  readerFeedbackChoiceSelections: Signal<Record<string, string[]>>;
  readerFeedbackChoiceAnchors: Map<string, string>;
  codeReview: Signal<CodeReview | undefined>;
  codeReviewLoading: Signal<boolean>;
  codeReviewMessage: Signal<string>;
  expandedCodeReviewCommits: Signal<string[]>;
  duplicateBacklinkState: Signal<{ key: string; backlinks: DuplicateBacklink[]; inaccessibleProjects: string[] }>;
  /** The selected ticket's AI usage history, keyed like the backlinks (HS2-ZA7W6E). */
  ticketUsageState: Signal<{ key: string; usage?: TicketUsage }>;
  resolvedDuplicateTargets: Signal<Record<string, DuplicateTarget>>;
  ticketCloseDialog: Signal<TicketCloseDialogState | undefined>;
  ticketLinkChoice: Signal<TicketLinkChoice | undefined>;
  bulkTicketDialog: Signal<BulkTicketDialogState | undefined>;
  notWorkingTarget: Signal<NotWorkingTarget>;
  notWorkingNote: Signal<string>;
  notWorkingFiles: Signal<PendingEvidence[]>;
  /** References whose recovery copy could not be read; kept until submitted or discarded (HS2-791W19). */
  notWorkingUnrestoredFiles: Signal<StoredPendingAttachment[]>;
  notWorkingScreening: Signal<boolean>;
  notWorkingSubmitting: Signal<boolean>;
  notWorkingError: Signal<string>;
  composerExpanded: Signal<boolean>;
  composerTitle: Signal<string>;
  composerDetails: Signal<string>;
  composerCategory: Signal<string>;
  composerUpNext: Signal<boolean>;
  composerPriority: Signal<string>;
  composerBacklog: Signal<boolean>;
  /** The source picked in the open composer, if any (HS2-NZMJBJ). */
  composerSource: Signal<string | undefined>;
  /** The source each project last created a ticket in, in memory only (HS2-NZMJBJ). */
  lastTicketSourceByProject: Signal<Record<string, string>>;
  composerAttachments: Signal<PendingEvidence[]>;
  composerUnrestoredAttachments: Signal<StoredPendingAttachment[]>;
  composerAttachmentMessage: Signal<string>;
  composerAttachmentError: Signal<boolean>;
  composerScreening: Signal<boolean>;
  composerSubmitting: Signal<boolean>;
  histories: Map<string, TicketHistory>;
  mutationGenerations: Map<string, number>;
  committedTickets: Map<string, FullTicket>;
  singleTicketMutationSequencer: BulkTicketMutationSequencer;
  bulkTicketMutationSequencer: BulkTicketMutationSequencer;
  localTicketChangeAcknowledgements: LocalTicketChangeAcknowledgements;
  pendingCreatedTickets: PendingCreatedTickets;
  project: () => Project | undefined;
  api: () => Api;
  defaultProvider: () => ProjectTicketSources | undefined;
  /** The source a new ticket targets: the composer pick, last-used, or default source. */
  newTicketSource: () => TicketSourceChoice | undefined;
  capabilitiesFor: (connectionId: string) => Capabilities | undefined;
  canUseAttachments: () => boolean;
  canStageNewTicketAttachments: () => boolean;
  ticketSnapshot: (slug: string) => TicketSnapshot | undefined;
  visibleTickets: () => WireTicketRow[];
  projectTabTicketRows: (projectId: string) => WireTicketRow[];
  projectTicketCounts: (projectId: string) => CheckoutTicketCounts;
  suppressBulkTicketMotion: (count: number) => void;
  beginBulkBoardRefill: (
    projectId: string,
    before: readonly WireTicketRow[],
    after: readonly WireTicketRow[],
    counts: CheckoutTicketCounts | undefined,
  ) => void;
  finishBulkBoardRefill: (projectId: string) => Promise<void>;
  publishOptimisticTicketRows: (projectId: string) => void;
  beginLocalTicketMutation: () => () => void;
  beginBulkUpdateProgress: (total: number) => BulkUpdateHandle;
  beginLocalTicketCreation: () => () => Promise<void>;
  refreshProject: (options?: { showLoading?: boolean }) => Promise<unknown>;
  refreshTicketCollection: (view: TicketView) => Promise<unknown>;
  refreshCodeReview: () => Promise<unknown>;
  selectTicketView: (view: TicketView, options?: { refresh?: boolean }) => void;
  scheduleClaimLeaseExpiry: () => void;
  scheduleProjectSessionPersistence: () => void;
  persistWorkspacePreferences: () => void;
  showToast: (message: string) => void;
  showFieldConflict: (conflict: TicketFieldConflict) => void;
  reconcileRefreshedSelected: (previous: FullTicket, refreshed: FullTicket) => void;
  openTicketLinkMatch: (match: TicketLinkMatch) => Promise<void>;
  presentTicketReaderDialog: (
    id: string,
    trigger: HTMLElement | undefined,
    onOpen: () => void,
    afterOpen?: () => void,
  ) => void;
  beginDetailsEdit: (reader?: boolean, frame?: TicketReaderFrame) => void;
  /** Bring the inspector into view where it is not always visible (the phone overlay). */
  revealTicketInspector: () => void;
  activeTicketSurface: () => ParentNode;
  draftScope: (kind: 'composer' | 'not-working', projectId?: string) => string;
  ago: (value?: string) => string;
  /** The composer opened in this project, so a minimized composer there is restored (HS2-3JR0ZH). */
  composerOpened?: (projectId: string | undefined) => void;
}

/** A saved human note waiting on the app-level Choose/Start an AI Session prompt (HS2-6RXBS9). */
export interface PendingFeedbackDelivery {
  ticketId: string;
  projectId: string;
  slug: string;
  title: string;
  delivery: FeedbackDelivery;
}

/** Ticket mutation, selection, transfer, attachment, close, and composer workflows. */
export function createTicketWorkflows(dependencies: TicketWorkflowDependencies) {
  /**
   * A saved human note still waiting for the person to choose, or start, the AI session to notify. It
   * keeps its own project and ticket so the app-level prompt survives selecting another ticket,
   * project, or view (HS2-6RXBS9).
   */
  const feedbackDelivery = signal<PendingFeedbackDelivery | undefined>();
  /** Show the saved note's notification outcome; `choose` and `start` wait in the app-level prompt. */
  function announceFeedbackDelivery(
    owner: Project,
    ticket: Pick<WireTicketRow, 'qualified_id' | 'slug' | 'title'>,
    delivery: FeedbackDelivery,
  ) {
    feedbackDelivery.value =
      delivery.status === 'choose' || delivery.status === 'start'
        ? { ticketId: ticket.qualified_id, projectId: owner.id, slug: ticket.slug, title: ticket.title, delivery }
        : undefined;
    if (delivery.status === 'sent') showToast('The AI session was notified.');
    if (delivery.status === 'queued') showToast('The AI session will be notified after its current turn.');
    // HS2-H9Z9KZ: delivery is best-effort; the note stays on the ticket for the AI's next read.
    if (delivery.status === 'not_shared') showToast('Saved, but the AI session could not be told right now.');
  }
  async function chooseFeedbackSession(ticketId: string, connectionId: string) {
    const pending = feedbackDelivery.value;
    if (pending?.ticketId !== ticketId || pending.delivery.status !== 'choose') return;
    const owner = projects.value.find((item) => item.id === pending.projectId);
    if (!owner) return;
    try {
      const delivery = await new Api(owner.apiPath).notifyTicketNote(
        owner.id,
        ticketId,
        pending.delivery.note_id,
        connectionId,
      );
      if (feedbackDelivery.value === pending) feedbackDelivery.value = undefined;
      showToast(
        delivery.status === 'queued'
          ? 'The AI session will be notified after its current turn.'
          : 'The AI session was notified.',
      );
    } catch (reason) {
      showToast(reason instanceof Error ? reason.message : String(reason));
    }
  }
  /** The person dismissed the prompt; the note stays on the ticket for the AI's next read. */
  function dismissFeedbackDelivery() {
    feedbackDelivery.value = undefined;
  }
  const {
    state,
    CLOSED_NOT_WORKING_TARGET,
    projects,
    selectedProjectId,
    tickets,
    workerDetails,
    ticketRowsByProject,
    ticketCountsByProject,
    selectedTicket,
    selectedTicketSlugs,
    selectedCorruptKey,
    selectedView,
    ticketCollectionState,
    loading,
    error,
    attachmentMessage,
    inspectorTab,
    inspectorVisible,
    readerTab,
    readerOpen,
    linkedReaderStack,
    detailsMode,
    detailsDraft,
    readerDetailsMode,
    readerDetailsDraft,
    titleEditingSurface,
    titleDraft,
    blockedReasonEditing,
    blockedReasonDraft,
    readerBlockedReasonEditing,
    readerBlockedReasonDraft,
    editingNoteId,
    readerEditingNoteId,
    noteDraft,
    readerNoteDraft,
    fieldConflict,
    fieldConflictResolution,
    readerInlineFeedbackReplies,
    readerFeedbackChoiceSelections,
    readerFeedbackChoiceAnchors,
    codeReview,
    codeReviewLoading,
    codeReviewMessage,
    expandedCodeReviewCommits,
    duplicateBacklinkState,
    ticketUsageState,
    resolvedDuplicateTargets,
    ticketCloseDialog,
    ticketLinkChoice,
    bulkTicketDialog,
    notWorkingTarget,
    notWorkingNote,
    notWorkingFiles,
    notWorkingUnrestoredFiles,
    notWorkingScreening,
    notWorkingSubmitting,
    notWorkingError,
    composerExpanded,
    composerTitle,
    composerDetails,
    composerCategory,
    composerUpNext,
    composerPriority,
    composerBacklog,
    composerSource,
    lastTicketSourceByProject,
    composerAttachments,
    composerUnrestoredAttachments,
    composerAttachmentMessage,
    composerAttachmentError,
    composerScreening,
    composerSubmitting,
    histories,
    mutationGenerations,
    committedTickets,
    singleTicketMutationSequencer,
    bulkTicketMutationSequencer,
    localTicketChangeAcknowledgements,
    pendingCreatedTickets,
    project,
    api,
    defaultProvider,
    newTicketSource,
    capabilitiesFor,
    canUseAttachments,
    canStageNewTicketAttachments,
    ticketSnapshot,
    visibleTickets,
    projectTabTicketRows,
    projectTicketCounts,
    suppressBulkTicketMotion,
    beginBulkBoardRefill,
    finishBulkBoardRefill,
    publishOptimisticTicketRows,
    beginLocalTicketMutation,
    beginBulkUpdateProgress,
    beginLocalTicketCreation,
    refreshProject,
    refreshTicketCollection,
    refreshCodeReview,
    selectTicketView,
    scheduleClaimLeaseExpiry,
    scheduleProjectSessionPersistence,
    persistWorkspacePreferences,
    showToast,
    showFieldConflict,
    reconcileRefreshedSelected,
    openTicketLinkMatch,
    presentTicketReaderDialog,
    beginDetailsEdit,
    revealTicketInspector,
    activeTicketSurface,
    draftScope,
    ago,
    composerOpened,
  } = dependencies;
  /** Each project's median phase durations for the in-progress timeline stage, loaded once (HS2-ST6RN1). */
  const typicalPhasesByProject = signal<Record<string, Readonly<Record<string, number>>>>({});
  const typicalPhaseLoads = new Set<string>();
  /** Rebase-and-retry attempts after a stale-token rejection before an edit is reported as failed. */
  const MAX_CONCURRENT_EDIT_RETRIES = 4;
  const FEEDBACK_ALREADY_RESOLVED = 'This feedback request was already resolved, so no response was sent.';
  let ticketCloseSearchGeneration = 0,
    ticketCloseSearchTimer: number | undefined,
    composerAttachmentEpoch = 0,
    notWorkingAttachmentEpoch = 0;
  let composerFollowUpTo: { qualified: string; slug: string } | undefined;
  const activeComposerScreenings = new Set<symbol>();
  const activeNotWorkingScreenings = new Set<symbol>();
  /** Recently viewed ticket details rendered without a loading view on reopen (HS2-4Q04Z9). */
  const detailCache = createTicketDetailCache();
  function reconcileWorkerSelection(projectId: string, ticket: FullTicket) {
    reconcileWorkerTicketSelection({
      projectId,
      ticket,
      selectedProjectId: project()?.id,
      selectedTicket: selectedTicket.value,
      cache: detailCache,
      reconcile: reconcileRefreshedSelected,
    });
  }
  interface DraftSlot {
    base: () => string;
    draft: Signal<string>;
  }
  /** Open editors whose draft saves into `field` (a note's text for `note`), with the value each draft began from. */
  function draftSlots(field: string, noteId?: string): DraftSlot[] {
    const slots: Array<DraftSlot | false> =
      field === 'details'
        ? [
            detailsMode.value === 'write' && { base: () => state.detailsDraftBase, draft: detailsDraft },
            readerDetailsMode.value === 'write' && {
              base: () => state.readerDetailsDraftBase,
              draft: readerDetailsDraft,
            },
          ]
        : field === 'title'
          ? [titleEditingSurface.value !== undefined && { base: () => state.titleDraftBase, draft: titleDraft }]
          : field === 'blocked_reason'
            ? [
                blockedReasonEditing.value && { base: () => state.blockedReasonDraftBase, draft: blockedReasonDraft },
                readerBlockedReasonEditing.value && {
                  base: () => state.readerBlockedReasonDraftBase,
                  draft: readerBlockedReasonDraft,
                },
              ]
            : field === 'note' && noteId
              ? [
                  editingNoteId.value === noteId && { base: () => state.noteDraftBase, draft: noteDraft },
                  readerEditingNoteId.value === noteId && {
                    base: () => state.readerNoteDraftBase,
                    draft: readerNoteDraft,
                  },
                ]
              : [];
    return slots.filter((slot): slot is DraftSlot => slot !== false);
  }
  const DRAFT_FIELDS = ['details', 'title', 'blocked_reason', 'note'] as const;
  const patchText = (value: unknown) => (typeof value === 'string' ? value : '');
  /** Title and blocked-reason saves trim the draft, so compare drafts to a saved value the same way. */
  const draftMatches = (field: string, draft: string, value: string) =>
    field === 'title' || field === 'blocked_reason' ? draft.trim() === value.trim() : draft === value;
  /**
   * Put merged values into the open drafts that produced `sent` (while their text is still what was sent), and
   * return the values to record as those drafts' new bases.
   */
  function adoptMergedDrafts(sent: TicketPatch, merged: TicketPatch): TicketPatch {
    const recorded: TicketPatch = {};
    for (const field of DRAFT_FIELDS) {
      if (!Object.hasOwn(sent, field) || !Object.hasOwn(merged, field)) continue;
      const mine = patchText(sent[field]),
        value = patchText(merged[field]);
      if (mine === value) continue;
      const noteId = typeof sent.note_id === 'string' ? sent.note_id : undefined;
      for (const slot of draftSlots(field, noteId))
        if (draftMatches(field, slot.draft.value, mine)) {
          slot.draft.value = value;
          syncFocusedDraftControl(document.activeElement, mine, value, (live) => draftMatches(field, live, mine));
          recorded[field] = merged[field];
          if (noteId) recorded.note_id = noteId;
        }
    }
    return recorded;
  }
  /** Rebase draft-backed fields onto `base` before sending; see `rebaseDraftValue`. */
  function rebaseDraftPatch(
    patch: TicketPatch,
    base: FullTicket,
  ): { patch: TicketPatch; recorded: TicketPatch; conflict?: TicketFieldConflict; merged: string[] } {
    const next: TicketPatch = { ...patch },
      merged: string[] = [],
      noteId = typeof patch.note_id === 'string' ? patch.note_id : undefined;
    for (const field of DRAFT_FIELDS) {
      if (!Object.hasOwn(patch, field)) continue;
      const mine = patchText(patch[field]),
        slot = draftSlots(field, noteId).find((item) => draftMatches(field, item.draft.value, mine));
      if (!slot) continue;
      const draftBase = slot.base(),
        current = ticketFieldText(base, field, noteId),
        result = rebaseDraftValue(draftBase, mine, current);
      if (result.kind === 'conflict')
        return {
          patch,
          recorded: patch,
          merged: [],
          conflict: {
            key: field === 'note' ? `note:${noteId}` : field,
            field,
            label: ticketFieldLabel(field),
            base: draftBase,
            mine,
            theirs: current,
          },
        };
      if (result.kind === 'merged') {
        next[field] = field === 'blocked_reason' && !result.value.trim() ? null : result.value;
        // The saved value combines both edits, so the user is told once it commits (HS2-JJ3R1G).
        if (result.value !== mine && result.value !== current) merged.push(field);
      }
    }
    return { patch: next, recorded: { ...patch, ...adoptMergedDrafts(patch, next) }, merged };
  }
  async function applyTicketPatch(slug: string, patch: TicketPatch, dismissedRequestId?: string) {
    const current = project(),
      ticket = tickets.value.find((item) => item.slug === slug),
      // A response appends a new note; its draft must never rebase as an edit of the prompt (HS2-VBFSZJ).
      responseAppend =
        (typeof patch.response_to === 'string' || dismissedRequestId === DETAILS_FEEDBACK_ID) &&
        typeof patch.note === 'string' &&
        !Object.hasOwn(patch, 'note_id');
    if (!current || !ticket) return false;
    const client = api();
    // HS2-E0H7S0: queued/retried origin writes cannot rebase another selection's editor recovery.
    const ownsDrafts = () => project()?.id === current.id && selectedTicket.value?.qualified_id === ticket.qualified_id;
    const interaction = Object.hasOwn(patch, 'up_next')
        ? 'ticket-up-next-change'
        : Object.hasOwn(patch, 'status')
          ? 'ticket-status-change'
          : 'ticket-change',
      finishTiming = beginInteractionTiming(interaction, { slug }),
      releaseRefresh = beginLocalTicketMutation();
    const selectedBefore = selectedTicket.value?.slug === slug ? selectedTicket.value : null,
      started = performance.now(),
      generation = (mutationGenerations.get(slug) ?? 0) + 1;
    let rollbackRow = ticket,
      rollbackSelected = selectedBefore;
    mutationGenerations.set(slug, generation);
    batch(() => {
      tickets.value = tickets.value.map((item) => (item.slug === slug ? projectTicketPatch(item, patch) : item));
      publishOptimisticTicketRows(current.id);
      if (selectedBefore) selectedTicket.value = projectTicketPatch(selectedBefore, patch);
    });
    const optimistic = performance.now() - started;
    finishTiming();
    // Serialize the network section per ticket so the user's own rapid sequential edits each base off the
    // previous edit's committed token (last-write-wins) instead of self-conflicting (HS2-K9SG2R).
    const queuedAt = performance.now();
    return singleTicketMutationSequencer.enqueue(slug, async () => {
      const queueWait = performance.now() - queuedAt;
      try {
        // Base off the last edit this client committed for the ticket (its up-to-date token), falling back to
        // the pre-edit selection or a fresh fetch. A real external write still fails the token check below.
        let base =
          committedTickets.get(slug) ??
          selectedBefore ??
          (await client.checkoutTicket(current.id, ticket.qualified_id)).ticket;
        // A draft typed on top of an older value must merge with what the ticket holds now, or the save would
        // silently overwrite a concurrent edit to the same field (HS2-A4XCXE).
        const queuedFields = new Set(['title', 'details', 'category', 'priority', 'tags']);
        let useQueue = Boolean(
          capabilitiesFor(ticket.connection_id)?.write_behind &&
          Object.keys(patch).length > 0 &&
          Object.keys(patch).every((field) => queuedFields.has(field)),
        );
        const rebased =
          useQueue || !ownsDrafts() || responseAppend
            ? { patch, recorded: patch, merged: [] as string[] }
            : rebaseDraftPatch(patch, base);
        if (rebased.conflict) {
          if (mutationGenerations.get(slug) === generation) {
            tickets.value = tickets.value.map((item) => (item.slug === slug ? ticketRowFromFull(item, base) : item));
            publishOptimisticTicketRows(current.id);
            if (selectedTicket.value?.slug === slug) selectedTicket.value = base;
            showFieldConflict(rebased.conflict);
            reportMutationTiming({
              slug,
              optimistic_ms: optimistic,
              request_ms: performance.now() - started,
              queue_ms: queueWait,
              outcome: 'rolled_back',
            });
          }
          return false;
        }
        let pending = rebased.patch,
          recorded = rebased.recorded,
          updated: FullTicket | undefined;
        /** Fields saved as a three-way merge with a newer change, announced once the save commits (HS2-JJ3R1G). */
        const mergedFields = new Set(rebased.merged);
        const operationId = browserOutboxId();
        // Token drift from unrelated writes (an AI's notes, lease renewals) can land between a refetch and the
        // retry; rebase and retry a bounded number of times instead of failing the edit (HS2-A4XCXE).
        for (let attempt = 0; !updated; attempt += 1) {
          if (
            isStaleFeedbackDismissal(dismissedRequestId, pending, base.notes) ||
            isStaleDescriptionFeedbackDismissal(dismissedRequestId, pending, base)
          ) {
            showToast(FEEDBACK_ALREADY_RESOLVED);
            updated = base;
            break;
          }
          try {
            if (useQueue) {
              try {
                updated = (
                  await client.queueProviderUpdates(ticket.connection_id, [
                    {
                      operation_id: operationId,
                      native_id: ticket.native_id,
                      patch: pending,
                    },
                  ])
                )[0]?.ticket;
              } catch (reason) {
                if (isWriteBehindUnavailable(reason)) {
                  useQueue = false;
                } else if (!(reason instanceof ApiHttpError)) {
                  const accepted = (await client.providerOutbox(ticket.connection_id).catch(() => [])).some(
                    (operation) => operation.operation_id === operationId,
                  );
                  if (accepted) updated = (await client.checkoutTicket(current.id, ticket.qualified_id)).ticket;
                  else throw reason;
                } else {
                  throw reason;
                }
              }
            }
            if (!updated) {
              const result = await client.updateCheckoutTicket(
                current.id,
                ticket.qualified_id,
                base.concurrency_token ? { ...pending, expected_token: base.concurrency_token } : pending,
              );
              updated = result.ticket;
              if (result.feedbackDelivery) announceFeedbackDelivery(current, ticket, result.feedbackDelivery);
            }
            localTicketChangeAcknowledgements.acknowledge(current.id, {
              store: updated.connection_id,
              id: updated.id,
              kind: 'updated',
            });
          } catch (reason) {
            if (!isTicketConcurrencyConflict(reason) || attempt >= MAX_CONCURRENT_EDIT_RETRIES) throw reason;
            const remote = (await client.checkoutTicket(current.id, ticket.qualified_id)).ticket,
              reconciled = reconcileTicketPatch(base, remote, pending);
            committedTickets.set(slug, remote);
            rollbackRow = ticketRowFromFull(ticket, remote);
            if (!responseAppend) rollbackSelected = remote;
            if (mutationGenerations.get(slug) !== generation) return true;
            tickets.value = tickets.value.map((item) => (item.slug === slug ? ticketRowFromFull(item, remote) : item));
            publishOptimisticTicketRows(current.id);
            if (!responseAppend && selectedTicket.value?.slug === slug) reconcileRefreshedSelected(base, remote);
            const conflict = reconciled.conflicts[0];
            // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type. (HS2-W3RDCB)
            if (conflict) {
              showFieldConflict(conflict);
              reportMutationTiming({
                slug,
                optimistic_ms: optimistic,
                request_ms: performance.now() - started,
                queue_ms: queueWait,
                outcome: 'rolled_back',
              });
              return false;
            }
            if (!responseAppend && ownsDrafts())
              recorded = { ...recorded, ...adoptMergedDrafts(pending, reconciled.retry) };
            for (const field of reconciled.merged) mergedFields.add(field);
            if (
              isStaleFeedbackDismissal(dismissedRequestId, pending, remote.notes) ||
              isStaleDescriptionFeedbackDismissal(dismissedRequestId, pending, remote)
            ) {
              showToast(FEEDBACK_ALREADY_RESOLVED);
              updated = remote;
            } else if (Object.keys(reconciled.retry).length === 0) updated = remote;
            else {
              base = remote;
              pending = reconciled.retry;
            }
          }
        }
        // Record the committed token even when this edit is stale for UI purposes: its server write advanced
        // the token, so the next queued edit must base off it.
        committedTickets.set(slug, updated);
        if (mutationGenerations.get(slug) !== generation) {
          reportMutationTiming({
            slug,
            optimistic_ms: optimistic,
            request_ms: performance.now() - started,
            queue_ms: queueWait,
            outcome: 'stale',
          });
          return true;
        }
        tickets.value = tickets.value.map((item) => (item.slug === slug ? ticketRowFromFull(item, updated) : item));
        publishOptimisticTicketRows(current.id);
        if (selectedTicket.value?.slug === slug && (!responseAppend || ownsDrafts())) selectedTicket.value = updated;
        if (!responseAppend && ownsDrafts()) recordCommittedDraftBases(recorded);
        error.value = updated.warnings?.join('\n') ?? '';
        if (mergedFields.size) showToast(mergedEditNotice([...mergedFields]));
        reportMutationTiming({
          slug,
          optimistic_ms: optimistic,
          request_ms: performance.now() - started,
          queue_ms: queueWait,
          outcome: 'committed',
        });
        return true;
      } catch (reason) {
        if (mutationGenerations.get(slug) === generation) {
          tickets.value = tickets.value.map((item) => (item.slug === slug ? rollbackRow : item));
          publishOptimisticTicketRows(current.id);
          if (rollbackSelected && selectedTicket.value?.slug === slug && (!responseAppend || ownsDrafts()))
            selectedTicket.value = rollbackSelected;
          error.value = reason instanceof Error ? reason.message : String(reason);
          reportMutationTiming({
            slug,
            optimistic_ms: optimistic,
            request_ms: performance.now() - started,
            queue_ms: queueWait,
            outcome: 'rolled_back',
          });
        }
        return false;
      } finally {
        if (mutationGenerations.get(slug) === generation) mutationGenerations.delete(slug);
        releaseRefresh();
      }
    });
  }
  async function updateSelected(patch: Record<string, unknown>) {
    const linked = linkedReaderStack.value.at(-1);
    return linked
      ? updateLinkedReader(linked.id, patch)
      : selectedTicket.value
        ? applyTicketPatch(selectedTicket.value.slug, patch)
        : false;
  }
  /** Capture dismissal intent separately from ordinary user-authored responses (HS2-VBFSZJ). */
  function dismissFeedbackRequest(requestId: string, frameId?: string): Promise<boolean> {
    const patch = {
      note: NO_RESPONSE_NEEDED_NOTE,
      note_kind: 'regular',
      ...(requestId === DETAILS_FEEDBACK_ID ? {} : { response_to: requestId }),
    };
    if (frameId) return updateLinkedReader(frameId, patch, requestId);
    const ticket = selectedTicket.value;
    return ticket ? applyTicketPatch(ticket.slug, patch, requestId) : Promise.resolve(false);
  }
  function history(projectId = project()?.id ?? '') {
    let value = histories.get(projectId);
    if (!value) {
      value = new TicketHistory(ticketSnapshot, applyTicketPatch);
      histories.set(projectId, value);
    }
    return value;
  }
  function recordCommittedDraftBases(patch: TicketPatch) {
    if (typeof patch.details === 'string' && detailsMode.value === 'write') state.detailsDraftBase = patch.details;
    if (typeof patch.details === 'string' && readerDetailsMode.value === 'write')
      state.readerDetailsDraftBase = patch.details;
    if (typeof patch.title === 'string' && titleEditingSurface.value !== undefined) state.titleDraftBase = patch.title;
    if (Object.hasOwn(patch, 'blocked_reason') && blockedReasonEditing.value)
      state.blockedReasonDraftBase = typeof patch.blocked_reason === 'string' ? patch.blocked_reason : '';
    if (Object.hasOwn(patch, 'blocked_reason') && readerBlockedReasonEditing.value)
      state.readerBlockedReasonDraftBase = typeof patch.blocked_reason === 'string' ? patch.blocked_reason : '';
    if (typeof patch.note_id === 'string' && patch.note_id === editingNoteId.value && typeof patch.note === 'string')
      state.noteDraftBase = patch.note;
    if (
      typeof patch.note_id === 'string' &&
      patch.note_id === readerEditingNoteId.value &&
      typeof patch.note === 'string'
    )
      state.readerNoteDraftBase = patch.note;
  }
  async function updateSelectedTracked(patch: TicketPatch) {
    const linked = linkedReaderStack.value.at(-1);
    return linked
      ? updateLinkedReader(linked.id, patch)
      : selectedTicket.value
        ? history().execute(selectedTicket.value.slug, patch)
        : false;
  }
  // Text edits stay local while the user types and reach the server once, when focus leaves the
  // editing surface (HS2-RE1PS6). The debounced step only refreshes a localStorage recovery copy
  // keyed by project, ticket, and field, together with the value the edit started from.
  const draftStorage = (): Storage | undefined => (typeof localStorage === 'undefined' ? undefined : localStorage);
  function selectedDraftKey(field: TicketDraftField, noteId?: string): string | undefined {
    // A top linked reader owns the edit, matching updateSelected's routing (HS2-KRVWKF).
    const linked = linkedReaderStack.value.at(-1);
    if (linked) return ticketDraftKey(linked.projectId, linked.ticket.qualified_id, field, noteId);
    const current = project(),
      ticket = selectedTicket.value;
    return current && ticket ? ticketDraftKey(current.id, ticket.qualified_id, field, noteId) : undefined;
  }
  function persistSelectedDraft(field: TicketDraftField, base: string, draft: string, noteId?: string) {
    const storage = draftStorage(),
      key = selectedDraftKey(field, noteId);
    if (storage && key) saveTicketDraft(storage, key, { base, draft });
  }
  function prepareSelectedDraft(
    field: TicketDraftField,
    base: string,
    draft: string,
    noteId?: string,
    previous?: InFlightRecovery<string>,
  ) {
    const key = selectedDraftKey(field, noteId),
      predecessor =
        key !== undefined && previous?.destination === key
          ? field === 'title' || field === 'blocked_reason'
            ? previous.value.trim()
            : previous.value
          : undefined;
    return {
      destination: key,
      capture: () => {
        captureTicketDraft(draftStorage(), key, {
          base,
          draft,
          ...(predecessor !== undefined && predecessor !== draft ? { pendingIntent: { predecessor } } : {}),
        });
      },
    };
  }
  /**
   * The recovery copy for a field the user is about to edit, rebased onto the field's current value:
   * the stored draft continues on the same base when nothing changed and merges when the remote
   * change is disjoint. When it cannot be merged, the field-conflict card offers both versions and
   * the copy stays until the card is resolved (HS2-7D3MPP); the editor opens on the current value.
   * `undefined` when there is nothing to restore into the editor.
   */
  function restoreTicketDraft(
    field: TicketDraftField,
    current: string,
    noteId?: string,
  ): { draft: string; base: string } | undefined {
    const storage = draftStorage(),
      key = selectedDraftKey(field, noteId);
    if (!storage || !key) return undefined;
    const plan = planTicketDraftRestore(loadRestorableTicketDraft(storage, key), current);
    if (plan.kind === 'none') {
      if (plan.clear) clearTicketDraft(storage, key);
      return undefined;
    }
    if (plan.kind === 'conflict') {
      showFieldConflict({
        key: field === 'note' ? `note:${noteId ?? ''}` : field,
        field,
        label: ticketFieldLabel(field),
        base: plan.base,
        mine: plan.mine,
        theirs: plan.theirs,
      });
      return undefined;
    }
    // A merged restore opens on the combined text with the current value as its base, so the focus-loss
    // save writes it against the current token without a conflict (HS2-JJ3R1G).
    showToast(plan.merged ? mergedEditNotice([field], { restored: true }) : 'Restored an unsaved edit.');
    return { draft: plan.draft, base: plan.base };
  }
  /** The recovery destination a field conflict owns, in the edited ticket or `ticketKey`'s ticket. */
  function conflictDestination(
    conflict: TicketFieldConflict,
    ticketKey: (field: TicketDraftField, noteId?: string) => string | undefined = selectedDraftKey,
  ): string | undefined {
    const field = DRAFT_FIELDS.find((item) => item === conflict.field);
    return field ? ticketKey(field, field === 'note' ? conflict.key.slice('note:'.length) : undefined) : undefined;
  }
  /** A resolved field conflict settles the field's recovery copy: the user chose what to keep (HS2-7D3MPP). */
  function clearConflictRecoveryCopy(conflict: TicketFieldConflict) {
    const storage = draftStorage(),
      key = conflictDestination(conflict);
    if (storage && key) clearTicketDraft(storage, key);
  }
  /** The recovery destination a linked reader frame's conflict card owns (HS2-JT2AFG). */
  const linkedConflictDestination = (frame: TicketReaderFrame) =>
    frame.fieldConflict &&
    conflictDestination(frame.fieldConflict.conflict, (field, noteId) =>
      ticketDraftKey(frame.projectId, frame.ticket.qualified_id, field, noteId),
    );
  /**
   * A field conflict owns the edit at `destination`, so a failed save there is neither kept nor retried:
   * the inspector's card, or a linked reader frame's card (HS2-Y0VRXR, HS2-JT2AFG). The frame now holds
   * the remote ticket, so resending the unchanged draft would overwrite the other edit unreviewed.
   */
  const conflictOwns = (destination: string) =>
    linkedReaderStack.value.some((frame) => linkedConflictDestination(frame) === destination) ||
    (fieldConflict.value !== undefined && conflictDestination(fieldConflict.value) === destination);
  /** A linked reader conflict resolved by the user settles its recovery copy, as the inspector's does (HS2-JT2AFG). */
  function clearLinkedConflictRecoveryCopy(frame: TicketReaderFrame) {
    const storage = draftStorage(),
      key = linkedConflictDestination(frame);
    if (storage && key) clearTicketDraft(storage, key);
  }
  /** The field text a failed save is retried against; undefined while that ticket or note is not loaded. */
  const committedFieldText = (ticket: FullTicket | null | undefined, field: TicketDraftField, noteId?: string) =>
    !ticket || (field === 'note' && !ticket.notes.some((note) => note.id === noteId))
      ? undefined
      : ticketFieldText(ticket, field, noteId);
  /** Retry a failed save of the edited ticket (the top linked reader's, matching `selectedDraftKey`) on focus loss. */
  const selectedRetry = <T>(
    field: TicketDraftField,
    text: (value: T) => string,
    noteId?: (value: T) => string,
  ): FailedSaveRetryOptions<T> => ({
    text,
    committed: (value) =>
      committedFieldText(linkedReaderStack.value.at(-1)?.ticket ?? selectedTicket.value, field, noteId?.(value)),
    conflictOwns,
  });
  const trimmed = (value: string) => value.trim(),
    noteValueId = ({ id }: { id: string; value: string }) => id;
  const savingDraft =
    <T>(
      field: TicketDraftField,
      save: (value: T) => Promise<boolean>,
      text: (value: T) => string,
      noteId?: (value: T) => string | undefined,
    ) =>
    async (value: T) => {
      const clearCommitted = prepareCommittedDraftCleanup(
        draftStorage(),
        selectedDraftKey(field, noteId?.(value)),
        field,
        text(value),
      );
      const saved = await save(value);
      if (saved) clearCommitted();
      return saved;
    };
  const plain = (value: string) => value,
    noteText = ({ value }: { id: string; value: string }) => value;
  const detailsAutosave = createFocusLossAutosave(
    savingDraft('details', (value: string) => updateSelectedTracked({ details: value }), plain),
    {
      persist: (value) => {
        persistSelectedDraft('details', state.detailsDraftBase, value);
      },
      prepareRecovery: (value, predecessor) =>
        prepareSelectedDraft('details', state.detailsDraftBase, value, undefined, predecessor),
      retry: selectedRetry('details', plain),
    },
  );
  const readerDetailsAutosave = createFocusLossAutosave(
    savingDraft('details', (value: string) => updateSelectedTracked({ details: value }), plain),
    {
      persist: (value) => {
        persistSelectedDraft('details', state.readerDetailsDraftBase, value);
      },
      prepareRecovery: (value, predecessor) =>
        prepareSelectedDraft('details', state.readerDetailsDraftBase, value, undefined, predecessor),
      retry: selectedRetry('details', plain),
    },
  );
  const noteAutosave = createFocusLossAutosave(
    savingDraft(
      'note',
      ({ id, value }: { id: string; value: string }) => updateSelected(noteEditPatch(selectedTicket.value, id, value)),
      noteText,
      ({ id }) => id,
    ),
    {
      persist: ({ id, value }) => {
        persistSelectedDraft('note', state.noteDraftBase, value, id);
      },
      prepareRecovery: ({ id, value }, predecessor) =>
        prepareSelectedDraft(
          'note',
          state.noteDraftBase,
          value,
          id,
          predecessor?.value.id === id
            ? { destination: predecessor.destination, value: predecessor.value.value }
            : undefined,
        ),
      retry: selectedRetry('note', noteText, noteValueId),
    },
  );
  const readerNoteAutosave = createFocusLossAutosave(
    savingDraft(
      'note',
      ({ id, value }: { id: string; value: string }) => updateSelected(noteEditPatch(selectedTicket.value, id, value)),
      noteText,
      ({ id }) => id,
    ),
    {
      persist: ({ id, value }) => {
        persistSelectedDraft('note', state.readerNoteDraftBase, value, id);
      },
      prepareRecovery: ({ id, value }, predecessor) =>
        prepareSelectedDraft(
          'note',
          state.readerNoteDraftBase,
          value,
          id,
          predecessor?.value.id === id
            ? { destination: predecessor.destination, value: predecessor.value.value }
            : undefined,
        ),
      retry: selectedRetry('note', noteText, noteValueId),
    },
  );
  const blockedReasonAutosave = createFocusLossAutosave(
    savingDraft('blocked_reason', (value: string) => updateSelected({ blocked_reason: value.trim() || null }), plain),
    {
      persist: (value) => {
        persistSelectedDraft('blocked_reason', state.blockedReasonDraftBase, value);
      },
      prepareRecovery: (value, predecessor) =>
        prepareSelectedDraft('blocked_reason', state.blockedReasonDraftBase, value, undefined, predecessor),
      retry: selectedRetry('blocked_reason', trimmed),
    },
  );
  const readerBlockedReasonAutosave = createFocusLossAutosave(
    savingDraft('blocked_reason', (value: string) => updateSelected({ blocked_reason: value.trim() || null }), plain),
    {
      persist: (value) => {
        persistSelectedDraft('blocked_reason', state.readerBlockedReasonDraftBase, value);
      },
      prepareRecovery: (value, predecessor) =>
        prepareSelectedDraft('blocked_reason', state.readerBlockedReasonDraftBase, value, undefined, predecessor),
      retry: selectedRetry('blocked_reason', trimmed),
    },
  );
  const titleAutosave = createFocusLossAutosave(
    savingDraft('title', (value: string) => updateSelectedTracked({ title: value.trim() }), plain),
    {
      persist: (value) => {
        persistSelectedDraft('title', state.titleDraftBase, value);
      },
      prepareRecovery: (value, predecessor) =>
        prepareSelectedDraft('title', state.titleDraftBase, value, undefined, predecessor),
      retry: selectedRetry('title', trimmed),
    },
  );
  // A tag save the debounce timer started has no caller to see it fail; show it (HS2-YA6D31).
  const tagsAutosave = createDebouncedAutosave(
    (value: string[]) => updateSelectedTracked({ tags: value }),
    undefined,
    (reason) => {
      error.value = reason instanceof Error ? reason.message : String(reason);
    },
  );
  function linkedReaderFrame(target: Element) {
    const id = target.closest<HTMLElement>('[data-reader-frame-id]')?.dataset.readerFrameId;
    return id && id !== 'workspace-reader' ? linkedReaderStack.value.find((frame) => frame.id === id) : undefined;
  }
  function replaceLinkedReaderFrame(id: string, update: (frame: TicketReaderFrame) => TicketReaderFrame) {
    linkedReaderStack.value = updateTicketReaderFrame(linkedReaderStack.value, id, update);
  }
  /** Load a linked reader's Code Review tab from its own project for its own ticket (HS2-9Q4AFB). */
  async function refreshLinkedCodeReview(frameId: string) {
    const frame = linkedReaderStack.value.find((item) => item.id === frameId);
    if (!frame || frame.codeReview?.loading) return;
    const setReview = (codeReview: TicketReaderFrame['codeReview']) => {
      replaceLinkedReaderFrame(frameId, (current) => ({ ...current, codeReview }));
    };
    setReview({ review: frame.codeReview?.review, loading: true, message: '' });
    try {
      setReview({
        review: await new Api(frame.apiPath).codeReview(frame.projectId, frame.ticket.qualified_id),
        loading: false,
        message: '',
      });
    } catch (reason) {
      setReview({ loading: false, message: reason instanceof Error ? reason.message : String(reason) });
    }
  }
  async function updateLinkedReader(frameId: string, patch: Record<string, unknown>, dismissedRequestId?: string) {
    const frame = linkedReaderStack.value.find((item) => item.id === frameId);
    if (!frame || !linkedReaderPatchAllowed(frame.capabilities, patch)) return false;
    // A new save of a field the user typed into again settles its earlier conflict card (HS2-Y0VRXR,
    // HS2-JT2AFG); a conflict on another field stays.
    const patchedKeys = Object.keys(patch).map((field) =>
      field === 'note' && typeof patch.note_id === 'string' ? `note:${patch.note_id}` : field,
    );
    replaceLinkedReaderFrame(frameId, (current) => settleReaderFieldConflict(current, patchedKeys));
    const client = new Api(frame.apiPath);
    let base = frame.ticket,
      pending: TicketPatch = patch;
    try {
      // Same field-aware rebase-and-retry as the workspace editor: unrelated remote writes and disjoint
      // edits to the same text merge instead of discarding the reader's edit (HS2-A4XCXE).
      for (let attempt = 0; ; attempt += 1) {
        if (
          isStaleFeedbackDismissal(dismissedRequestId, pending, base.notes) ||
          isStaleDescriptionFeedbackDismissal(dismissedRequestId, pending, base)
        ) {
          showToast(FEEDBACK_ALREADY_RESOLVED);
          flushDraftInputs();
          replaceLinkedReaderFrame(frameId, (current) => reconcileTicketReaderFrame(current, base));
          return true;
        }
        let ticket: FullTicket;
        try {
          ticket = (
            await client.updateCheckoutTicket(
              frame.projectId,
              frame.ticket.qualified_id,
              base.concurrency_token ? { ...pending, expected_token: base.concurrency_token } : pending,
            )
          ).ticket;
        } catch (reason) {
          if (!isTicketConcurrencyConflict(reason) || attempt >= MAX_CONCURRENT_EDIT_RETRIES) throw reason;
          const remote = (await client.checkoutTicket(frame.projectId, frame.ticket.qualified_id)).ticket,
            reconciled = reconcileTicketPatch(base, remote, pending);
          if (
            (typeof pending.response_to === 'string' || dismissedRequestId === DETAILS_FEEDBACK_ID) &&
            typeof pending.note === 'string' &&
            !Object.hasOwn(pending, 'note_id')
          ) {
            // Install current authority while the frame's explicit append draft stays local (HS2-VBFSZJ).
            flushDraftInputs();
            replaceLinkedReaderFrame(frameId, (current) => reconcileTicketReaderFrame(current, remote));
          }
          if (reconciled.conflicts.length) {
            // Reconciling compares against the frame's draft, so typed text must be in it (HS2-WV0VPE).
            // The frame takes the remote ticket and shows the inspector's conflict card beneath the field:
            // the user's text stays in the card until Use Theirs or Apply Merged Value (HS2-JT2AFG).
            flushDraftInputs();
            replaceLinkedReaderFrame(frameId, (current) =>
              showReaderFieldConflict(reconcileTicketReaderFrame(current, remote), reconciled.conflicts[0]),
            );
            error.value = '';
            return false;
          }
          if (
            isStaleFeedbackDismissal(dismissedRequestId, pending, remote.notes) ||
            isStaleDescriptionFeedbackDismissal(dismissedRequestId, pending, remote)
          )
            showToast(FEEDBACK_ALREADY_RESOLVED);
          else if (Object.keys(reconciled.retry).length) {
            base = remote;
            pending = reconciled.retry;
            continue;
          }
          ticket = remote;
        }
        flushDraftInputs();
        replaceLinkedReaderFrame(frameId, (current) =>
          adoptCommittedReaderDrafts(reconcileTicketReaderFrame(current, ticket), patch, ticket),
        );
        const noteId = typeof patch.note_id === 'string' ? patch.note_id : undefined;
        for (const field of DRAFT_FIELDS)
          if (Object.hasOwn(patch, field)) {
            const sent = patchText(patch[field]);
            syncFocusedDraftControl(document.activeElement, sent, ticketFieldText(ticket, field, noteId), (live) =>
              draftMatches(field, live, sent),
            );
          }
        return true;
      }
    } catch (reason) {
      error.value = reason instanceof Error ? reason.message : String(reason);
      return false;
    }
  }

  /**
   * A conflict card's choice supersedes any text still queued for that field's focus-loss save: the
   * inspector's card (and the workspace reader's) for the selected ticket, or linked reader `frameId`'s
   * own card. Both the inspector and linked readers resolve through here (HS2-JT2AFG, HS2-A51MNY).
   */
  function cancelConflictedFieldSave(conflict: TicketFieldConflict, frameId?: string) {
    const queue = conflictedEditorQueue(conflict.field);
    if (!queue) return;
    if (frameId !== undefined) {
      const saves = linkedReaderAutosaves.get(frameId);
      if (!saves || queue === 'title') return;
      (queue === 'details' ? saves.details : queue === 'note' ? saves.note : saves.blocked).cancel();
      return;
    }
    // The inspector and the workspace reader both edit the selected ticket, so either one's queued
    // text for that field predates the choice.
    const selected = {
      details: [detailsAutosave, readerDetailsAutosave],
      blocked_reason: [blockedReasonAutosave, readerBlockedReasonAutosave],
      note: [noteAutosave, readerNoteAutosave],
      title: [titleAutosave],
    }[queue];
    for (const save of selected) save.cancel();
  }
  /**
   * Resolve a linked reader frame's conflict card (HS2-JT2AFG), as the inspector's card resolves its own:
   * Use Theirs keeps the remote text and settles the recovery copy; Apply Merged Value saves the merged
   * text to the frame's ticket and settles the copy once that save lands, so a failed save stays recoverable.
   */
  async function resolveLinkedReaderConflict(frameId: string, choice: 'theirs' | 'merged'): Promise<boolean> {
    const frame = linkedReaderStack.value.find((item) => item.id === frameId),
      owned = frame?.fieldConflict;
    if (!frame || !owned) return false;
    const { conflict, resolution } = owned;
    cancelConflictedFieldSave(conflict, frameId);
    if (choice === 'theirs') {
      clearLinkedConflictRecoveryCopy(frame);
      replaceLinkedReaderFrame(frameId, (current) =>
        resolveReaderFieldConflict(current, conflict.theirs, conflict.theirs),
      );
      return true;
    }
    if (conflict.field === 'title' && !resolution.trim()) {
      error.value = 'Ticket title cannot be empty.';
      return false;
    }
    replaceLinkedReaderFrame(frameId, (current) => resolveReaderFieldConflict(current, resolution, conflict.theirs));
    const saved = await updateLinkedReader(frameId, resolvedConflictPatch(conflict, resolution, frame.ticket));
    if (saved) clearLinkedConflictRecoveryCopy(frame);
    return saved;
  }
  const linkedReaderAutosaves = new Map<
    string,
    {
      details: FocusLossAutosave<string>;
      note: FocusLossAutosave<{ id: string; value: string }>;
      blocked: FocusLossAutosave<string>;
      respond: (value: string, requestId?: string) => Promise<boolean>;
    }
  >();
  function linkedReaderSaves(id: string) {
    let saves = linkedReaderAutosaves.get(id);
    if (!saves) {
      const capture = (
        field: TicketDraftField,
        draft: string,
        noteId?: string,
        previous?: InFlightRecovery<string>,
      ) => {
        const frame = linkedReaderStack.value.find((item) => item.id === id);
        if (!frame)
          return {
            destination: undefined,
            capture: () => {
              throw new Error('Local recovery destination is unavailable.');
            },
          };
        const base =
          field === 'details'
            ? frame.edit.detailsBase
            : field === 'blocked_reason'
              ? frame.edit.blockedReasonBase
              : frame.edit.noteBase;
        const key = ticketDraftKey(frame.projectId, frame.ticket.qualified_id, field, noteId),
          predecessor =
            previous?.destination === key
              ? field === 'blocked_reason'
                ? previous.value.trim()
                : previous.value
              : undefined;
        return {
          destination: key,
          capture: () => {
            captureTicketDraft(draftStorage(), key, {
              base,
              draft,
              ...(predecessor !== undefined && predecessor !== draft ? { pendingIntent: { predecessor } } : {}),
            });
          },
        };
      };
      const retry = <T>(
        field: TicketDraftField,
        text: (value: T) => string,
        noteId?: (value: T) => string,
      ): FailedSaveRetryOptions<T> => ({
        text,
        committed: (value) =>
          committedFieldText(linkedReaderStack.value.find((frame) => frame.id === id)?.ticket, field, noteId?.(value)),
        conflictOwns,
      });
      saves = {
        details: createFocusLossAutosave((value) => updateLinkedReader(id, { details: value }), {
          prepareRecovery: (value, predecessor) => capture('details', value, undefined, predecessor),
          retry: retry('details', plain),
        }),
        note: createFocusLossAutosave(
          ({ id: noteId, value }) =>
            updateLinkedReader(
              id,
              noteEditPatch(linkedReaderStack.value.find((frame) => frame.id === id)?.ticket, noteId, value),
            ),
          {
            prepareRecovery: ({ id: noteId, value }, predecessor) =>
              capture(
                'note',
                value,
                noteId,
                predecessor?.value.id === noteId
                  ? { destination: predecessor.destination, value: predecessor.value.value }
                  : undefined,
              ),
            retry: retry('note', noteText, noteValueId),
          },
        ),
        blocked: createFocusLossAutosave((value) => updateLinkedReader(id, { blocked_reason: value.trim() || null }), {
          prepareRecovery: (value, predecessor) => capture('blocked_reason', value, undefined, predecessor),
          retry: retry('blocked_reason', trimmed),
        }),
        respond: (value, requestId) =>
          updateLinkedReader(id, {
            note: value,
            note_kind: 'regular',
            ...(requestId ? { response_to: requestId } : {}),
          }),
      };
      linkedReaderAutosaves.set(id, saves);
    }
    return saves;
  }
  async function flushLinkedReader(frameId: string) {
    const saves = linkedReaderAutosaves.get(frameId),
      // Title and tag edits in the top linked reader share the reader's autosaves (HS2-KRVWKF); they
      // must land on this frame's ticket before it closes, and a title editor must not leak below.
      top = linkedReaderStack.value.at(-1)?.id === frameId,
      fieldSaves = top ? [titleAutosave.flush(), tagsAutosave.flush()] : [];
    const results = await Promise.all([
      ...fieldSaves,
      ...(saves ? [saves.details.flush(), saves.note.flush(), saves.blocked.flush()] : []),
    ]);
    const saved = results.every(Boolean);
    if (saved && top && titleEditingSurface.value === 'reader') titleEditingSurface.value = undefined;
    return saved;
  }
  function selectedRows() {
    const selected = new Set(selectedTicketSlugs.value);
    const selectedQualified = selectedTicket.value?.qualified_id;
    if (selected.size === 1 && selectedQualified && workerDetails.value[selectedQualified])
      return [workerDetails.value[selectedQualified]];
    return tickets.value.filter((ticket) => selected.has(ticket.slug));
  }
  // An open edit of a ticket the bulk change moved out of view is saved, not cancelled, before the
  // ticket is deselected (HS2-41KEQD), matching the refresh path (HS2-QW3PGE).
  function pruneSelectionForCurrentView() {
    const projectId = project()?.id;
    pruneSelectionSavingEdits({
      visibleSelection: () => selectionVisibleInView(tickets.value, selectedTicketSlugs.value, selectedView.value),
      selectedSlugs: () => selectedTicketSlugs.value,
      selectedTicketSlug: () => selectedTicket.value?.slug,
      flushSelectedTicketEdits,
      stillCurrent: () => project()?.id === projectId,
      apply: (next) => {
        batch(() => {
          selectedTicketSlugs.value = [...next];
          if (state.ticketSelectionAnchor && !next.includes(state.ticketSelectionAnchor))
            state.ticketSelectionAnchor = next[0];
          if (selectedTicket.value && !next.includes(selectedTicket.value.slug)) {
            cancelTicketDrafts();
            selectedTicket.value = null;
          }
          if (next.length !== 1) selectedTicket.value = null;
        });
      },
    });
  }
  interface BulkOperation {
    slug: string;
    id: string;
    patch: TicketPatch;
  }
  interface BulkApplyResult {
    complete: boolean;
    succeeded: Set<string>;
  }
  function setProjectTicketRows(projectId: string, rows: WireTicketRow[], counts?: CheckoutTicketCounts) {
    if (!projects.value.some((item) => item.id === projectId)) return;
    if (project()?.id === projectId) tickets.value = rows;
    ticketRowsByProject.value = { ...ticketRowsByProject.value, [projectId]: rows };
    ticketCountsByProject.value = counts
      ? { ...ticketCountsByProject.value, [projectId]: counts }
      : Object.fromEntries(Object.entries(ticketCountsByProject.value).filter(([id]) => id !== projectId));
  }
  function reportBulkFailure(current: Project, message: string) {
    if (project()?.id === current.id) error.value = message;
    else showToast(`${current.name}: ${message}`);
  }
  async function applyBulkOperations(current: Project, operations: BulkOperation[]): Promise<BulkApplyResult> {
    if (operations.length === 0) return { complete: false, succeeded: new Set<string>() };
    const projectionStarted = performance.now();
    const finishTiming = beginInteractionTiming('bulk-ticket-change', {
        count: operations.length,
        project: current.id,
      }),
      releaseRefresh = beginLocalTicketMutation(),
      slugs = new Set(operations.map((item) => item.slug)),
      before = projectTabTicketRows(current.id).filter((ticket) => slugs.has(ticket.slug)),
      beforeCounts = Object.hasOwn(ticketCountsByProject.value, current.id)
        ? ticketCountsByProject.value[current.id]
        : undefined,
      selectedBefore = project()?.id === current.id ? selectedTicket.value : null;
    const operationsBySlug = new Map(operations.map((operation) => [operation.slug, operation]));
    const optimistic = projectTabTicketRows(current.id).map((ticket) => {
        const operation = operationsBySlug.get(ticket.slug);
        return operation ? projectTicketPatch(ticket, operation.patch) : ticket;
      }),
      optimisticBySlug = new Map(optimistic.map((ticket) => [ticket.slug, ticket])),
      optimisticChanges = before.map((ticket) => ({
        before: ticket,
        after: optimisticBySlug.get(ticket.slug) ?? ticket,
      })),
      optimisticCounts = beforeCounts && projectBulkTicketCounts(beforeCounts, optimisticChanges);
    suppressBulkTicketMotion(operations.length);
    batch(() => {
      setProjectTicketRows(current.id, optimistic, optimisticCounts);
      beginBulkBoardRefill(current.id, before, optimistic, optimisticCounts);
      if (project()?.id === current.id && selectedTicket.value) {
        const operation = operationsBySlug.get(selectedTicket.value.slug);
        if (operation) selectedTicket.value = projectTicketPatch(selectedTicket.value, operation.patch);
      }
    });
    finishTiming();
    const projectionMs = performance.now() - projectionStarted,
      requestStarted = performance.now();
    let updateProgress: BulkUpdateHandle | undefined;
    try {
      const client = new Api(current.apiPath),
        requestOperations = operations.map((operation) => {
          const ticket = before.find((item) => item.slug === operation.slug),
            token = selectedBefore?.slug === operation.slug ? selectedBefore.concurrency_token : ticket?.updated_at;
          if (!ticket) throw new Error(`Ticket ${operation.slug} is no longer available for update.`);
          return {
            ...operation,
            patch: token ? { ...operation.patch, expected_token: token } : operation.patch,
          };
        }),
        atomic = canAtomicallyBulkUpdate(before, capabilitiesFor),
        updated: FullTicket[] = [],
        failures: Array<{ slug: string; message: string }> = [];
      const queuedFields = new Set(['title', 'details', 'category', 'priority', 'tags', 'expected_token']);
      const queueConnection = before[0]?.connection_id;
      const canQueueBatch = Boolean(
        queueConnection &&
        before.every((ticket) => ticket.connection_id === queueConnection) &&
        capabilitiesFor(queueConnection)?.write_behind &&
        requestOperations.every((operation) => Object.keys(operation.patch).every((field) => queuedFields.has(field))),
      );
      let queuedBatch = false;
      if (canQueueBatch && queueConnection) {
        const queuedOperations = requestOperations.map((operation) => {
          const ticket = before.find((item) => item.slug === operation.slug)!;
          const patch = Object.fromEntries(
            Object.entries(operation.patch).filter(([field]) => field !== 'expected_token'),
          );
          return { operation_id: browserOutboxId(), native_id: ticket.native_id, patch };
        });
        try {
          const result = await client.queueProviderUpdates(queueConnection, queuedOperations);
          updated.push(...result.map((item) => item.ticket));
          queuedBatch = true;
        } catch (reason) {
          if (isWriteBehindUnavailable(reason)) {
            // A server without the opt-in flag keeps its synchronous path.
          } else if (!(reason instanceof ApiHttpError)) {
            const admitted = (await client.providerOutbox(queueConnection).catch(() => [])).map(
              (operation) => operation.operation_id,
            );
            if (!queuedOperations.every((operation) => admitted.includes(operation.operation_id))) throw reason;
            updated.push(
              ...(await Promise.all(
                queuedOperations.map(
                  async (operation) =>
                    (await client.checkoutTicket(current.id, `${queueConnection}:${operation.native_id}`)).ticket,
                ),
              )),
            );
            queuedBatch = true;
          } else throw reason;
        }
      }
      if (!queuedBatch && atomic)
        updated.push(
          ...(await client.batchUpdateCheckoutTickets(
            current.id,
            requestOperations.map(({ id, patch }) => ({ id, patch })),
          )),
        );
      else if (!queuedBatch) {
        updateProgress = beginBulkUpdateProgress(requestOperations.length);
        for (const operation of requestOperations) {
          try {
            const ticket = before.find((item) => item.slug === operation.slug)!;
            updated.push((await client.updateCheckoutTicket(current.id, ticket.qualified_id, operation.patch)).ticket);
          } catch (reason) {
            failures.push({ slug: operation.slug, message: reason instanceof Error ? reason.message : String(reason) });
          }
          updateProgress.advance(updated.length + failures.length);
        }
      }
      for (const ticket of updated)
        localTicketChangeAcknowledgements.acknowledge(current.id, {
          store: ticket.connection_id,
          id: ticket.id,
          kind: 'updated',
        });
      const succeeded = new Set(updated.map((item) => item.slug));
      let next = projectTabTicketRows(current.id).map((ticket) => {
        const match = updated.find((item) => item.id === ticket.id);
        return match ? ticketRowFromFull(ticket, match) : ticket;
      });
      if (failures.length)
        next = next.map((ticket) =>
          failures.some((item) => item.slug === ticket.slug)
            ? (before.find((item) => item.slug === ticket.slug) ?? ticket)
            : ticket,
        );
      const actualCounts =
        beforeCounts &&
        projectBulkTicketCounts(
          beforeCounts,
          before.map((ticket) => ({ before: ticket, after: next.find((item) => item.slug === ticket.slug) ?? ticket })),
        );
      suppressBulkTicketMotion(operations.length);
      batch(() => {
        setProjectTicketRows(current.id, next, actualCounts);
        if (project()?.id === current.id) {
          const selectedUpdate = selectedTicket.value && updated.find((item) => item.id === selectedTicket.value?.id);
          if (selectedUpdate) selectedTicket.value = selectedUpdate;
          else if (selectedBefore && failures.some((item) => item.slug === selectedBefore.slug))
            selectedTicket.value = selectedBefore;
          pruneSelectionForCurrentView();
        }
      });
      const failure = failures.length
        ? `Updated ${updated.length} of ${operations.length} tickets. ${failures.length} failed; ${failures[0].slug}: ${failures[0].message}`
        : '';
      if (failure) reportBulkFailure(current, failure);
      else if (project()?.id === current.id) error.value = '';
      reportMutationTiming({
        count: operations.length,
        optimistic_ms: projectionMs,
        request_ms: performance.now() - requestStarted,
        outcome: failures.length ? 'partial' : 'committed',
      });
      return { complete: failures.length === 0, succeeded };
    } catch (reason) {
      suppressBulkTicketMotion(operations.length);
      batch(() => {
        setProjectTicketRows(
          current.id,
          projectTabTicketRows(current.id).map((ticket) => before.find((item) => item.slug === ticket.slug) ?? ticket),
          beforeCounts,
        );
        if (project()?.id === current.id && selectedBefore && slugs.has(selectedBefore.slug))
          selectedTicket.value = selectedBefore;
      });
      reportBulkFailure(current, reason instanceof Error ? reason.message : String(reason));
      reportMutationTiming({
        count: operations.length,
        optimistic_ms: projectionMs,
        request_ms: performance.now() - requestStarted,
        outcome: 'rolled_back',
      });
      return { complete: false, succeeded: new Set<string>() };
    } finally {
      updateProgress?.finish();
      releaseRefresh();
      await finishBulkBoardRefill(current.id);
    }
  }
  /** Restore Trash tickets through the server so each returns to its pre-deletion status. */
  async function restoreTrashedTickets(targetSlugs: readonly string[]) {
    const current = project();
    if (!current) return;
    const api = new Api(current.apiPath),
      rows = projectTabTicketRows(current.id).filter(
        (ticket) => targetSlugs.includes(ticket.slug) && isTrashedTicket(ticket),
      );
    try {
      for (const row of rows) await api.restoreCheckoutTicket(current.id, row.qualified_id);
    } catch (reason) {
      error.value = reason instanceof Error ? reason.message : String(reason);
    }
    await refreshProject({ showLoading: false });
  }
  function executeBulkTicketAction(action: BulkTicketAction, targetSlugs = selectedTicketSlugs.value) {
    const requestedProject = project(),
      requestedSlugs = [...targetSlugs];
    bulkTicketDialog.value = undefined;
    state.bulkTicketSlugs = [];
    if (!requestedProject) return Promise.resolve(false);
    return bulkTicketMutationSequencer.enqueue(requestedProject.id, async () => {
      const requestedSet = new Set(requestedSlugs);
      const selected = projectTabTicketRows(requestedProject.id).filter((ticket) => requestedSet.has(ticket.slug));
      if (!canBulkUpdate(selected, capabilitiesFor)) {
        reportBulkFailure(requestedProject, 'One or more selected ticket providers do not support ticket updates.');
        return false;
      }
      // The Up Next controls are disabled for such providers; this covers any other route (HS2-Y32T8A).
      const upNextRefusal =
        action.kind === 'up-next' ? ticketUpNextDisabledReason(selected, capabilitiesFor) : undefined;
      if (upNextRefusal) {
        reportBulkFailure(requestedProject, upNextRefusal);
        return false;
      }
      const operations: BulkOperation[] = selected.flatMap((ticket) => {
        const patch = bulkTicketPatch(ticket, action);
        return patch ? [{ slug: ticket.slug, id: ticket.id, patch }] : [];
      });
      if (operations.length === 0) return true;
      const result = await applyBulkOperations(requestedProject, operations);
      const selectedBySlug = new Map(selected.map((ticket) => [ticket.slug, ticket]));
      const inverse = operations.map((operation) => {
          const ticket = selectedBySlug.get(operation.slug)!;
          const keys = new Set(Object.keys(operation.patch));
          if ('status' in operation.patch) keys.add('up_next');
          return {
            slug: operation.slug,
            id: operation.id,
            patch: Object.fromEntries(
              // An ungrouped ticket has no `group` value; undo must send null to clear it again.
              [...keys].map((key) => [
                key,
                key === 'group' ? (ticket.group ?? null) : ticket[key as keyof typeof ticket],
              ]),
            ),
          };
        }),
        succeededOperations = operations.filter((operation) => result.succeeded.has(operation.slug)),
        succeededInverse = inverse.filter((operation) => result.succeeded.has(operation.slug));
      if (succeededOperations.length)
        history(requestedProject.id).recordExternal(
          async () => (await applyBulkOperations(requestedProject, succeededInverse)).complete,
          async () => (await applyBulkOperations(requestedProject, succeededOperations)).complete,
        );
      return result.complete;
    });
  }
  function openEmptyTrash() {
    const current = project();
    if (!current) return;
    const count = projectTicketCounts(current.id).trash ?? 0;
    if (count > 0) bulkTicketDialog.value = { kind: 'empty-trash', count };
  }
  async function emptyTrash() {
    const current = project(),
      dialog = bulkTicketDialog.value;
    if (!current || dialog?.kind !== 'empty-trash' || dialog.busy) return;
    const beforeRows = projectTabTicketRows(current.id),
      beforeCounts = projectTicketCounts(current.id),
      remaining = beforeRows.filter((ticket) => !isTrashedTicket(ticket));
    bulkTicketDialog.value = undefined;
    setProjectTicketRows(current.id, remaining);
    ticketCountsByProject.value = {
      ...ticketCountsByProject.value,
      [current.id]: { ...beforeCounts, total: Math.max(0, beforeCounts.total - dialog.count), trash: 0 },
    };
    selectedTicketSlugs.value = [];
    selectedTicket.value = null;
    state.ticketSelectionAnchor = undefined;
    selectTicketView('all', { refresh: false });
    ticketCollectionState.value = { projectId: current.id, view: 'all', status: 'loading' };
    scheduleClaimLeaseExpiry();
    try {
      const result = await new Api(current.apiPath).emptyCheckoutTrash(current.id);
      if (trashEmptyLeftTickets(result)) {
        // A provider kept some Trash (GH-5): show what is still there instead of an empty Trash.
        ticketCountsByProject.value = {
          ...ticketCountsByProject.value,
          [current.id]: {
            ...beforeCounts,
            total: Math.max(0, beforeCounts.total - result.purged),
            trash: Math.max(0, (beforeCounts.trash ?? 0) - result.purged),
          },
        };
        if (project()?.id === current.id) {
          selectTicketView('trash', { refresh: false });
          await refreshTicketCollection('trash');
        }
      } else if (project()?.id === current.id) await refreshTicketCollection('all');
      showToast(trashEmptySummary(result));
    } catch (reason) {
      if (!projects.value.some((item) => item.id === current.id)) return;
      setProjectTicketRows(current.id, beforeRows);
      ticketCountsByProject.value = { ...ticketCountsByProject.value, [current.id]: beforeCounts };
      const message = reason instanceof Error ? reason.message : String(reason);
      if (project()?.id === current.id) {
        selectTicketView('trash', { refresh: false });
        await refreshTicketCollection('trash');
        if (project()?.id === current.id) error.value = `Empty Trash failed: ${message}`;
      } else showToast(`${current.name}: Empty Trash failed — ${message}`);
    }
  }
  function openBulkTicketDialog(kind: 'add-tag' | 'remove-tag' | 'delete', targetSlugs = selectedTicketSlugs.value) {
    const selected = tickets.value.filter((ticket) => targetSlugs.includes(ticket.slug));
    if (!canBulkUpdate(selected, capabilitiesFor)) return;
    state.bulkTicketSlugs = selected.map((ticket) => ticket.slug);
    bulkTicketDialog.value =
      kind === 'delete'
        ? { kind: 'delete', count: selected.length }
        : {
            kind: 'tag',
            mode: kind === 'add-tag' ? 'add' : 'remove',
            count: selected.length,
            choices: bulkTagChoices(selected),
          };
    if (kind !== 'delete')
      queueMicrotask(() => document.querySelector<HTMLElement>('[name="bulk-ticket-tag"]')?.focus());
  }
  function clipboardTicket(ticket: FullTicket | WireTicketRow, loaded?: FullTicket): ClipboardTicket {
    const full = 'attachments' in ticket ? ticket : loaded;
    return {
      id: ticket.id,
      slug: ticket.slug,
      connection_id: ticket.connection_id,
      native_id: ticket.native_id,
      title: ticket.title,
      details: full?.details ?? '',
      category: ticket.category ?? 'issue',
      priority: ticket.priority,
      status: ticket.status,
      up_next: ticket.up_next,
      tags: ticket.tags,
      notes:
        full?.notes.map((note) => ({
          kind: note.kind,
          text: note.text,
          summary: note.summary,
          confidence: note.confidence,
          actor: note.actor,
        })) ?? [],
      attachments: full?.attachments.map((item) => ({ id: item.id, filename: item.filename })) ?? [],
    };
  }
  function copySelection(cut: boolean) {
    const current = project(),
      rows = selectedRows();
    if (!current || !rows.length) return;
    const snapshot = {
      tickets: rows.map((ticket) =>
        clipboardTicket(
          ticket,
          selectedTicket.value?.qualified_id === ticket.qualified_id ? selectedTicket.value : undefined,
        ),
      ),
      cut,
      source: current,
    };
    state.clipboard = snapshot;
    selectedTicketSlugs.value = [...selectedTicketSlugs.value];
    // Plain-HTTP LAN origins (a remote device) have no async Clipboard API, and the full-text write
    // below runs after a fetch, outside the shortcut's user gesture. Copy what is already known
    // synchronously so the local pasteboard still receives the ticket ids (HS2-B68KPQ).
    // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Absent on insecure origins despite its static type. (HS2-B68KPQ)
    if (!navigator.clipboard) copyWithSelection(ticketClipboardText(snapshot.tickets));
    void Promise.all(
      rows.map((ticket) =>
        api()
          .checkoutTicket(current.id, ticket.qualified_id)
          .then((value) => value.ticket),
      ),
    )
      .then((full) => {
        if (state.clipboard !== snapshot) return;
        snapshot.tickets = full.map((ticket) => clipboardTicket(ticket));
        const text = ticketClipboardText(full);
        // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type. (HS2-W3RDCB)
        void navigator.clipboard?.writeText(text).catch(() => undefined);
      })
      .catch((reason: unknown) => {
        error.value = reason instanceof Error ? reason.message : String(reason);
      });
  }
  async function patchTransferTickets(client: Api, checkout: string, changes: Array<{ id: string; status: string }>) {
    for (const change of changes) await client.updateCheckoutTicket(checkout, change.id, { status: change.status });
    return true;
  }
  async function pasteSelection() {
    const current = project(),
      snapshot = state.clipboard;
    if (!current || !snapshot?.tickets.length) return;
    const destinationApi = api(),
      sourceApi = new Api(snapshot.source.apiPath),
      titles = tickets.value.map((ticket) => ticket.title),
      created: Array<{ id: string; slug: string; status: string }> = [];
    try {
      for (const source of snapshot.tickets) {
        const title = deduplicateTitle(source.title, titles);
        titles.push(title);
        let ticket = await destinationApi.createCheckoutTicket(current.id, {
          title,
          details: source.details,
          category: source.category,
          priority: source.priority,
          ...copiedTicketPlacement(source),
          tags: source.tags,
        });
        created.push({ id: ticket.qualified_id, slug: ticket.slug, status: ticket.status ?? 'not_started' });
        for (const note of source.notes)
          ticket = (
            await destinationApi.updateCheckoutTicket(current.id, ticket.qualified_id, {
              note: note.text,
              note_kind: note.kind,
              note_summary: note.summary,
              note_confidence: note.confidence,
              // A pasted note keeps its author; `null` keeps unknown authorship unattributed
              // rather than claiming the paster wrote it (HS2-XF81CJ).
              actor: note.actor ?? null,
            })
          ).ticket;
        for (const attachment of source.attachments)
          ticket = await destinationApi.copyAttachment(
            { connection_id: source.connection_id, native_id: source.native_id, attachment_id: attachment.id },
            { connection_id: ticket.connection_id, native_id: ticket.native_id },
          );
      }
      if (snapshot.cut) {
        for (const source of snapshot.tickets)
          await sourceApi.updateCheckoutTicket(snapshot.source.id, `${source.connection_id}:${source.native_id}`, {
            status: 'deleted',
          });
        state.clipboard = undefined;
      }
      const originals = snapshot.tickets.map((ticket) => ({
        id: `${ticket.connection_id}:${ticket.native_id}`,
        status: ticket.status ?? 'not_started',
      }));
      history().recordExternal(
        async () => {
          await patchTransferTickets(
            destinationApi,
            current.id,
            created.map((ticket) => ({ id: ticket.id, status: 'deleted' })),
          );
          if (snapshot.cut) await patchTransferTickets(sourceApi, snapshot.source.id, originals);
          await refreshProject();
          return true;
        },
        async () => {
          await patchTransferTickets(
            destinationApi,
            current.id,
            created.map((ticket) => ({ id: ticket.id, status: ticket.status })),
          );
          if (snapshot.cut)
            await patchTransferTickets(
              sourceApi,
              snapshot.source.id,
              originals.map((ticket) => ({ id: ticket.id, status: 'deleted' })),
            );
          await refreshProject();
          return true;
        },
      );
      await refreshProject();
      selectedTicketSlugs.value = created.map((ticket) => ticket.slug);
      state.ticketSelectionAnchor = created[0]?.slug;
    } catch (reason) {
      for (const ticket of created)
        await destinationApi.updateCheckoutTicket(current.id, ticket.id, { status: 'deleted' }).catch(() => undefined);
      error.value = reason instanceof Error ? reason.message : String(reason);
      await refreshProject();
    }
  }
  async function copyDraggedTickets(destination: Project, drag: { slugs: string[]; source: Project }) {
    const sourceRows = tickets.value.filter((ticket) => drag.slugs.includes(ticket.slug));
    if (project()?.id !== drag.source.id || sourceRows.length === 0) return;
    const sourceApi = new Api(drag.source.apiPath),
      destinationApi = new Api(destination.apiPath),
      created: Array<{ id: string; slug: string; status: string }> = [];
    try {
      const [sources, destinationRows] = await Promise.all([
          Promise.all(
            sourceRows.map((ticket) =>
              sourceApi.checkoutTicket(drag.source.id, ticket.qualified_id).then((result) => result.ticket),
            ),
          ),
          // De-duplicate against every destination title, not just loaded rows, via
          // bounded pages instead of one whole-checkout response (HS2-CYXS0N).
          destinationApi.checkoutTicketRowsPaged(destination.id, { fields: 'title' }),
        ]),
        titles = destinationRows.items.map((ticket) => ticket.title);
      if (destinationRows.sourceErrors.length)
        throw new Error(
          `Cannot check destination titles while a ticket source is unavailable: ${destinationRows.sourceErrors.join(' · ')}`,
        );
      for (const source of sources) {
        const title = deduplicateTitle(source.title, titles);
        titles.push(title);
        let ticket = await destinationApi.createCheckoutTicket(destination.id, {
          title,
          details: source.details,
          category: source.category ?? 'issue',
          priority: source.priority,
          ...copiedTicketPlacement(source),
          tags: source.tags,
        });
        created.push({ id: ticket.qualified_id, slug: ticket.slug, status: ticket.status ?? 'not_started' });
        for (const note of source.notes.filter((note) => note.kind !== 'activity'))
          ticket = (
            await destinationApi.updateCheckoutTicket(destination.id, ticket.qualified_id, {
              note: note.text,
              note_kind: note.kind,
            })
          ).ticket;
        for (const attachment of source.attachments)
          ticket = await destinationApi.copyAttachment(
            { connection_id: source.connection_id, native_id: source.native_id, attachment_id: attachment.id },
            { connection_id: ticket.connection_id, native_id: ticket.native_id },
          );
      }
      const refreshDestination = async () => {
        if (project()?.id === destination.id) await refreshProject();
        return true;
      };
      history().recordExternal(
        async () => {
          await patchTransferTickets(
            destinationApi,
            destination.id,
            created.map((ticket) => ({ id: ticket.id, status: 'deleted' })),
          );
          return refreshDestination();
        },
        async () => {
          await patchTransferTickets(
            destinationApi,
            destination.id,
            created.map((ticket) => ({ id: ticket.id, status: ticket.status })),
          );
          return refreshDestination();
        },
      );
      if (project()?.id === destination.id) {
        await refreshProject();
        selectedTicketSlugs.value = created.map((ticket) => ticket.slug);
        state.ticketSelectionAnchor = created[0]?.slug;
      }
      showToast(`${created.length} ticket${created.length === 1 ? '' : 's'} copied to ${destination.name}.`);
    } catch (reason) {
      for (const ticket of created)
        await destinationApi
          .updateCheckoutTicket(destination.id, ticket.id, { status: 'deleted' })
          .catch(() => undefined);
      error.value = reason instanceof Error ? reason.message : String(reason);
      if (project()?.id === destination.id) await refreshProject();
    }
  }
  function isEditableEvent(event: Event) {
    return event
      .composedPath()
      .some(
        (target) =>
          target instanceof HTMLElement &&
          (target.matches(
            'input, textarea, select, kui-select, [role="textbox"], [contenteditable]:not([contenteditable="false"])',
          ) ||
            Boolean(target.closest('wa-dialog[open]'))),
      );
  }
  function ticketWorkAreaFocused() {
    const area = document.querySelector<HTMLElement>('.app-shell__work-area'),
      active = document.activeElement;
    return Boolean(
      area &&
      active &&
      (active === area || area.contains(active)) &&
      area.querySelector('[data-component="ticket-list"], [data-component="ticket-board"]'),
    );
  }
  function ordinaryTextSelected() {
    const selection = document.getSelection();
    return Boolean(selection && !selection.isCollapsed && selection.toString().length);
  }
  function ticketUsageFor(ticket: FullTicket) {
    const current = project(),
      state = ticketUsageState.value;
    return current && state.key === `${current.id}:${ticket.qualified_id}` ? state.usage : undefined;
  }
  function timeline(ticket: FullTicket) {
    const entries = ticketTimelineEntries(ticket),
      usage = timelineUsageLabels(entries, ticketUsageFor(ticket)),
      current = project(),
      // The last entry is the stage under way while someone holds a live claim (HS2-ST6RN1).
      inProgress = timelineStageEstimate(ticket, current ? typicalPhasesByProject.value[current.id] : undefined),
      // Each step's slice of the actual, or for an open ticket the estimated, total time (HS2-TE86WX).
      shares = timelineShares(
        entries.map((entry) => entry.timestamp),
        {
          open: ticket.status === 'not_started' || ticket.status === 'started',
          remainingMs: inProgress?.remainingMs,
        },
      );
    return entries.map((entry, index) => ({
      ...entry,
      time: ago(entry.timestamp),
      ...(usage.perEntry[index] ? { usageToNext: usage.perEntry[index] } : {}),
      ...(inProgress && index === entries.length - 1 ? { inProgress } : {}),
      ...(shares[index] ? { share: shares[index] } : {}),
    }));
  }
  /** Load the project's phase medians once, the first time an actively worked ticket is shown (HS2-ST6RN1). */
  async function loadTypicalPhases(ticket: FullTicket) {
    const current = project();
    if (!current || typicalPhaseLoads.has(current.id) || !timelineStageEstimate(ticket, undefined)) return;
    typicalPhaseLoads.add(current.id);
    try {
      const report = await new Api(current.apiPath).checkoutPhaseReport(current.id);
      typicalPhasesByProject.value = { ...typicalPhasesByProject.value, [current.id]: typicalPhaseDurations(report) };
    } catch {
      // The estimate is decoration: without history the stage falls back to the claim ETA, and the
      // next time a ticket is shown may load it again.
      typicalPhaseLoads.delete(current.id);
    }
  }
  function timelineUsage(ticket: FullTicket) {
    return timelineUsageLabels(ticketTimelineEntries(ticket), ticketUsageFor(ticket)).breakdown;
  }
  /** HS2-J4G9GB: the note menu's response link stays visible with its reason when unavailable. */
  function linkState(ticket: FullTicket, noteId: string) {
    const reason = responseLinkUnavailable(ticket.notes, noteId, capabilitiesFor(ticket.connection_id));
    return reason ? { responseLinkable: false, responseLinkReason: reason } : { responseLinkable: true };
  }
  function notes(ticket: FullTicket) {
    return ticket.notes.map((note) => {
      // Authorship comes from the note's recorded actor (HS2-32QDZ3).
      const { author, aiAuthored, aiTool } = noteAuthorship(note);
      return {
        id: note.id,
        kind: presentedNoteKind(note, ticket.notes),
        feedbackPrompt: note.kind === 'feedback_needed',
        author,
        time: ago(note.created_at),
        body: note.text,
        repairable: true,
        storedKind: note.kind,
        metadataEditable: Boolean(
          capabilitiesFor(ticket.connection_id)?.note_edit && capabilitiesFor(ticket.connection_id)?.note_kind_edit,
        ),
        confidenceEditable: Boolean(
          capabilitiesFor(ticket.connection_id)?.note_edit && capabilitiesFor(ticket.connection_id)?.note_confidence,
        ),
        hasLinkedResponses: ticket.notes.some((item) => item.response_to === note.id),
        ...linkState(ticket, note.id),
        aiAuthored,
        humanEdited: note.human_edited,
        aiTool,
        confidence: note.confidence,
        feedbackFor: note.feedback_for,
        ratingMetadata: note.ai_feedback,
      } as const;
    });
  }
  function attachmentContext(ticket: FullTicket, current = project()): AttachmentReferenceContext | undefined {
    return current
      ? {
          baseUrl: current.apiPath,
          checkout: current.id,
          ticket: ticket.slug,
          attachments: ticket.attachments.map((item) => ({ id: item.id, filename: item.filename })),
        }
      : undefined;
  }
  function duplicateTarget(project: Project, ticket: WireTicketRow): DuplicateTarget {
    return {
      id: ticket.id,
      slug: ticket.slug,
      title: ticket.title,
      projectId: project.id,
      projectName: project.name,
      connectionId: ticket.connection_id,
      nativeId: ticket.native_id,
      qualifiedId: ticket.qualified_id,
    };
  }
  function duplicateTargetFor(ticket: FullTicket) {
    if (!ticket.duplicate_of) return undefined;
    const reference = parseDuplicateReference(ticket.duplicate_of),
      projectRows = reference ? projects.value.filter((item) => item.id === reference.project_id) : projects.value,
      match = projectRows
        .flatMap((item) =>
          (item.id === selectedProjectId.value ? tickets.value : (ticketRowsByProject.value[item.id] ?? [])).map(
            (row) => ({ project: item, row }),
          ),
        )
        .find(({ row }) =>
          reference
            ? row.connection_id === reference.connection_id && row.native_id === reference.native_id
            : row.id === ticket.duplicate_of || row.native_id === ticket.duplicate_of,
        ),
      target = match ? duplicateTarget(match.project, match.row) : resolvedDuplicateTargets.value[ticket.duplicate_of];
    // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type. (HS2-W3RDCB)
    return target
      ? { id: ticket.duplicate_of, projectName: target.projectName, slug: target.slug, title: target.title }
      : undefined;
  }
  async function resolveDuplicateTargetFor(ticket: FullTicket) {
    const value = ticket.duplicate_of;
    // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type. (HS2-W3RDCB)
    if (!value || resolvedDuplicateTargets.value[value]) return;
    const target = await resolveDuplicateReferenceTarget(value, projects.value, (item, id) =>
      new Api((item as Project).apiPath).checkoutTicket(item.id, id).then((result) => result.ticket),
    ).catch(() => undefined);
    if (target) resolvedDuplicateTargets.value = { ...resolvedDuplicateTargets.value, [value]: target };
  }
  async function uploadAttachmentWithPoster(
    client: Api,
    current: Project,
    ticket: FullTicket,
    file: File,
    metadata: AttachmentMetadata,
  ) {
    const previousIds = new Set(ticket.attachments.map((item) => item.id)),
      result = await client.addCheckoutAttachment(current.id, ticket.qualified_id, file, metadata),
      added = result.ticket.attachments.find((item) => !previousIds.has(item.id));
    // Posters are stored beside editable (git) attachments only (HS2-HSA64D).
    if (added && isVideoAttachment(file.name) && (capabilitiesFor(ticket.connection_id)?.attachment_edit ?? true)) {
      const posterUrl = client.checkoutAttachmentThumbnailUrl(current.id, ticket.qualified_id, added.id);
      void ensureVideoPoster(posterUrl, file);
    }
    return result.ticket;
  }
  async function addAttachments(slug: string, files: FileList | File[], frameId?: string) {
    // A linked reader frame attaches to its own ticket through its owning project (HS2-W8VEMP).
    const frame = frameId ? linkedReaderStack.value.find((item) => item.id === frameId) : undefined;
    if (frame) {
      await addLinkedReaderAttachments(frame, files);
      return;
    }
    const current = project(),
      ticket = selectedTicket.value?.slug === slug ? selectedTicket.value : undefined;
    if (!current || !ticket || files.length === 0 || !canUseAttachments()) return;
    loading.value = true;
    attachmentMessage.value = `Adding ${files.length} attachment${files.length === 1 ? '' : 's'}…`;
    try {
      const screened = await screenAttachmentFiles(Array.from(files)),
        batch_id = attachmentUploadBatchId(ticket.attachments, ticket.notes, () => browserRandomId());
      let updated: FullTicket = ticket;
      for (const file of screened.readable)
        updated = await uploadAttachmentWithPoster(api(), current, updated, file, {
          batch_id,
          actor: { role: 'human' },
        });
      if (updated !== ticket && selectedTicket.value?.id === ticket.id) selectedTicket.value = updated;
      if (screened.readable.length) await refreshProject();
      const warning = describeUnreadableAttachments(screened.unreadable);
      attachmentMessage.value = warning;
      error.value = warning;
      if (screened.readable.length)
        showToast(`${screened.readable.length} attachment${screened.readable.length === 1 ? '' : 's'} added.`);
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : String(reason);
      attachmentMessage.value = `Attachment failed: ${message}`;
      error.value = message;
    } finally {
      loading.value = false;
    }
  }
  async function addLinkedReaderAttachments(frame: TicketReaderFrame, files: FileList | File[]) {
    const owner = projects.value.find((item) => item.id === frame.projectId);
    if (!owner || files.length === 0 || !frame.capabilities.attachments) return;
    attachmentMessage.value = `Adding ${files.length} attachment${files.length === 1 ? '' : 's'}…`;
    try {
      const client = new Api(frame.apiPath),
        screened = await screenAttachmentFiles(Array.from(files)),
        batch_id = attachmentUploadBatchId(frame.ticket.attachments, frame.ticket.notes, () => browserRandomId());
      let updated: FullTicket = frame.ticket;
      for (const file of screened.readable)
        updated = await uploadAttachmentWithPoster(client, owner, updated, file, {
          batch_id,
          actor: { role: 'human' },
        });
      const result = updated;
      if (result !== frame.ticket)
        replaceLinkedReaderFrame(frame.id, (current) => reconcileTicketReaderFrame(current, result));
      const warning = describeUnreadableAttachments(screened.unreadable);
      attachmentMessage.value = warning;
      error.value = warning;
      if (screened.readable.length)
        showToast(`${screened.readable.length} attachment${screened.readable.length === 1 ? '' : 's'} added.`);
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : String(reason);
      attachmentMessage.value = `Attachment failed: ${message}`;
      error.value = message;
    }
  }
  function selectionOrder(target: Element) {
    const column = target.closest('[data-component="ticket-board-column"]');
    return column
      ? [...column.querySelectorAll<HTMLElement>('[data-action="select-ticket-row"]')].map(
          (item) => data(item).ticketSlug!,
        )
      : navigableTickets().map((ticket) => ticket.slug);
  }
  /** Visible tickets a person can reach: a collapsed group hides its members (HS2-WZ3JGZ). */
  function navigableTickets() {
    const current = project();
    // With Ticket groups and saved views off nothing is grouped, so nothing is collapsed (HS2-3GJZGG).
    return withoutCollapsedGroupMembers(visibleTickets(), (group) =>
      Boolean(current && featureOn('ticket-groups') && ticketGroupCollapsed(current.id, selectedView.value, group)),
    );
  }
  function presentTicket(ticket: FullTicket) {
    flushDraftInputs();
    const changed = selectedTicket.value?.id !== ticket.id,
      current = project(),
      backlinkKey = current ? `${current.id}:${ticket.qualified_id}` : '';
    batch(() => {
      selectedCorruptKey.value = undefined;
      selectedTicket.value = ticket;
      committedTickets.clear();
      committedTickets.set(ticket.slug, ticket);
      duplicateBacklinkState.value = { key: backlinkKey, backlinks: [], inaccessibleProjects: [] };
      if (ticketUsageState.value.key !== backlinkKey) ticketUsageState.value = { key: backlinkKey };
      if (changed) {
        codeReview.value = undefined;
        codeReviewMessage.value = '';
        codeReviewLoading.value = false;
        expandedCodeReviewCommits.value = [];
        readerInlineFeedbackReplies.value = {};
        readerFeedbackChoiceSelections.value = {};
        readerFeedbackChoiceAnchors.clear();
      }
      state.detailsEditGeneration += 1;
      state.readerDetailsEditGeneration += 1;
      detailsMode.value = 'preview';
      readerDetailsMode.value = 'preview';
      detailsDraft.value = ticket.details;
      readerDetailsDraft.value = ticket.details;
      state.detailsDraftBase = ticket.details;
      state.readerDetailsDraftBase = ticket.details;
      titleEditingSurface.value = undefined;
      titleDraft.value = ticket.title;
      state.titleDraftBase = ticket.title;
      blockedReasonEditing.value = false;
      readerBlockedReasonEditing.value = false;
      blockedReasonDraft.value = ticket.blocked_reason ?? '';
      readerBlockedReasonDraft.value = ticket.blocked_reason ?? '';
      state.blockedReasonDraftBase = ticket.blocked_reason ?? '';
      state.readerBlockedReasonDraftBase = ticket.blocked_reason ?? '';
      state.noteDraftBase = '';
      state.readerNoteDraftBase = '';
      editingNoteId.value = undefined;
      readerEditingNoteId.value = undefined;
      fieldConflict.value = undefined;
      fieldConflictResolution.value = '';
      inspectorVisible.value = true;
      error.value = '';
    });
    persistWorkspacePreferences();
    void resolveDuplicateTargetFor(ticket);
    if (current)
      void new Api(current.apiPath)
        .checkoutTicketDuplicateBacklinks(current.id, ticket.qualified_id)
        .then((result) => {
          if (project()?.id === current.id && selectedTicket.value?.qualified_id === ticket.qualified_id)
            duplicateBacklinkState.value = {
              key: backlinkKey,
              backlinks: result.backlinks,
              inaccessibleProjects: [...new Set(result.inaccessible_projects.map((item) => item.project_name))],
            };
        })
        .catch(() => undefined);
    if (inspectorTab.value === 'code-review') void refreshCodeReview();
    void refreshTicketUsage();
    void loadTypicalPhases(ticket);
  }
  /** Read the selected ticket's AI usage for the timeline; a provider without local metrics yields none. */
  async function refreshTicketUsage() {
    const current = project(),
      ticket = selectedTicket.value;
    if (!current || !ticket) return;
    const key = `${current.id}:${ticket.qualified_id}`;
    try {
      const usage = await new Api(current.apiPath).checkoutTicketUsage(current.id, ticket.qualified_id);
      if (project()?.id === current.id && selectedTicket.value?.qualified_id === ticket.qualified_id)
        ticketUsageState.value = { key, usage };
    } catch {
      /* usage is decoration on the timeline; a failed read keeps the last known state */
    }
  }
  /** Send every unsaved text edit now (page hide, explicit save); each keeps its local copy until it commits. */
  function flushTicketDrafts(): Promise<boolean> {
    flushDraftInputs();
    return Promise.all([
      detailsAutosave.flush(),
      readerDetailsAutosave.flush(),
      noteAutosave.flush(),
      readerNoteAutosave.flush(),
      blockedReasonAutosave.flush(),
      readerBlockedReasonAutosave.flush(),
      titleAutosave.flush(),
      ...[...linkedReaderAutosaves.keys()].map((frameId) => flushLinkedReader(frameId)),
    ]).then((results) => results.every(Boolean));
  }
  /**
   * Save the selected ticket's unsaved edits now, before a refresh deselects it; `undefined` when no
   * edit is queued or in flight, so the refresh can prune straight away (HS2-QW3PGE).
   */
  function flushSelectedTicketEdits(): Promise<boolean> | undefined {
    flushDraftInputs();
    const saves = [
      detailsAutosave,
      readerDetailsAutosave,
      noteAutosave,
      readerNoteAutosave,
      blockedReasonAutosave,
      readerBlockedReasonAutosave,
      titleAutosave,
      tagsAutosave,
    ];
    if (!saves.some((save) => save.pending())) return undefined;
    return Promise.all(saves.map((save) => save.flush())).then((results) => results.every(Boolean));
  }
  function captureTicketDraftRecovery() {
    flushDraftInputs();
    for (const save of [
      detailsAutosave,
      readerDetailsAutosave,
      noteAutosave,
      readerNoteAutosave,
      blockedReasonAutosave,
      readerBlockedReasonAutosave,
      titleAutosave,
    ])
      save.captureRecovery();
    for (const saves of linkedReaderAutosaves.values()) {
      saves.details.captureRecovery();
      saves.note.captureRecovery();
      saves.blocked.captureRecovery();
    }
  }
  function cancelTicketDrafts() {
    state.detailsEditGeneration += 1;
    state.readerDetailsEditGeneration += 1;
    detailsAutosave.cancel();
    readerDetailsAutosave.cancel();
    noteAutosave.cancel();
    readerNoteAutosave.cancel();
    blockedReasonAutosave.cancel();
    readerBlockedReasonAutosave.cancel();
    titleAutosave.cancel();
    tagsAutosave.cancel();
    fieldConflict.value = undefined;
    fieldConflictResolution.value = '';
  }
  let latestQualifiedSelection: string | undefined;
  function selectTickets(
    slug: string,
    intent: { range?: boolean; toggle?: boolean } = {},
    ordered = navigableTickets().map((ticket) => ticket.slug),
    qualifiedId?: string,
  ) {
    const selectionIntent = qualifiedId ? {} : intent;
    const loaded = selectedTicket.value;
    if (
      (!qualifiedId || loaded?.qualified_id === qualifiedId) &&
      isPlainTicketReselection(selectedTicketSlugs.value, loaded?.slug, slug, selectionIntent)
    )
      return Promise.resolve(loaded);
    const finishTiming = beginInteractionTiming('ticket-selection', { slug }),
      next = updateTicketSelection(
        ordered,
        { anchor: state.ticketSelectionAnchor, selected: new Set(selectedTicketSlugs.value) },
        slug,
        selectionIntent,
      ),
      selected = [...next.selected];
    state.ticketSelectionAnchor = next.anchor;
    batch(() => {
      selectedCorruptKey.value = undefined;
      selectedTicketSlugs.value = selected;
      cancelTicketDrafts();
      if (selected.length !== 1 || (qualifiedId && loaded?.qualified_id !== qualifiedId)) selectedTicket.value = null;
    });
    scheduleProjectSessionPersistence();
    finishTiming();
    if (selected.length !== 1) return Promise.resolve(undefined);
    const ticket = qualifiedId
        ? workerDetails.value[qualifiedId]
        : tickets.value.find((item) => item.slug === selected[0]),
      current = project();
    if (!ticket || !current) return Promise.resolve(undefined);
    latestQualifiedSelection = ticket.qualified_id;
    // Remember the outgoing detail (it carries any edits made while it was open), then render a recently
    // viewed ticket immediately and refresh it in the background without a loading view (HS2-4Q04Z9).
    if (loaded && loaded.qualified_id !== ticket.qualified_id) detailCache.put(current.id, loaded);
    const cached = detailCache.get(current.id, ticket);
    if (cached) presentTicket(cached.ticket);
    const stillSelected = (slug: string) =>
      project()?.id === current.id &&
      selectedTicketSlugs.value.length === 1 &&
      selectedTicketSlugs.value[0] === slug &&
      latestQualifiedSelection === ticket.qualified_id;
    const refresh = api()
      .checkoutTicket(current.id, ticket.qualified_id)
      .then((result) => {
        detailCache.put(current.id, result.ticket);
        if (!stillSelected(result.ticket.slug)) return result.ticket;
        const shown = selectedTicket.value;
        if (!cached || shown?.qualified_id !== result.ticket.qualified_id) presentTicket(result.ticket);
        else if (!sameTicketRevision(shown, result.ticket)) {
          // Replace stale cached content; keep an edit the user already began against the shown copy.
          // Reconciling (not a bare swap) also moves the committed base, so the blur save sends the
          // refreshed token instead of the cached one and an untouched open editor adopts the
          // refreshed text (HS2-7D3MPP).
          if (ticketEditInProgress()) reconcileRefreshedSelected(shown, result.ticket);
          else presentTicket(result.ticket);
        }
        return result.ticket;
      })
      .catch((reason: unknown) => {
        if (!cached) error.value = reason instanceof Error ? reason.message : String(reason);
        return undefined;
      });
    return cached ? Promise.resolve(cached.ticket) : refresh;
  }
  function ticketEditInProgress() {
    return Boolean(
      detailsMode.value === 'write' ||
      readerDetailsMode.value === 'write' ||
      titleEditingSurface.value ||
      editingNoteId.value ||
      readerEditingNoteId.value ||
      blockedReasonEditing.value ||
      readerBlockedReasonEditing.value,
    );
  }
  async function openTicketReader(slug: string, ordered?: string[], trigger?: HTMLElement) {
    const ticket = await selectTickets(slug, {}, ordered, trigger?.dataset.ticketQualifiedId);
    if (!ticket) return;
    presentTicketReaderDialog('workspace-reader', trigger, () => {
      readerOpen.value = true;
      if (readerTab.value === 'code-review' && !codeReviewLoading.value) void refreshCodeReview();
    });
  }
  function closeNotWorking(clearStored = true) {
    notWorkingAttachmentEpoch += 1;
    const ids = [...notWorkingFiles.value, ...notWorkingUnrestoredFiles.value].map((item) => item.id),
      scope = draftScope('not-working', notWorkingTarget.value.projectId);
    activeNotWorkingScreenings.clear();
    notWorkingScreening.value = false;
    notWorkingTarget.value = CLOSED_NOT_WORKING_TARGET;
    notWorkingNote.value = '';
    notWorkingFiles.value = [];
    notWorkingUnrestoredFiles.value = [];
    notWorkingSubmitting.value = false;
    notWorkingError.value = '';
    if (clearStored) void draftFiles.remove(scope, ids).catch(() => undefined);
    scheduleProjectSessionPersistence();
  }
  function presentNotWorkingDialog() {
    const target = notWorkingTarget.value,
      epoch = notWorkingAttachmentEpoch;
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        if (
          epoch !== notWorkingAttachmentEpoch ||
          notWorkingTarget.value !== target ||
          project()?.id !== target.projectId
        )
          return;
        const dialog = document.querySelector<Control>('[data-component="not-working-dialog"]'),
          // The host is the one exposed dialog; Web Awesome 3.14 would also label the shadow dialog.
          native = dialog ? quietWebAwesomeNativeDialog(dialog) : null;
        if (!native?.open) dialog?.show?.();
        dialog?.querySelector<HTMLTextAreaElement>('[name="not-working-note"]')?.focus();
      }),
    );
  }
  function openNotWorking(ticket: WireTicketRow, mode: NotWorkingTarget['mode'] = 'not-working') {
    const current = project();
    if (!current) return;
    notWorkingAttachmentEpoch += 1;
    notWorkingTarget.value = {
      projectId: current.id,
      apiPath: current.apiPath,
      ticketId: ticket.id,
      slug: ticket.slug,
      connectionId: ticket.connection_id,
      mode,
    };
    activeNotWorkingScreenings.clear();
    notWorkingScreening.value = false;
    notWorkingNote.value = '';
    notWorkingFiles.value = [];
    notWorkingUnrestoredFiles.value = [];
    notWorkingSubmitting.value = false;
    notWorkingError.value = '';
    scheduleProjectSessionPersistence();
    presentNotWorkingDialog();
  }
  function closeTicketCloseDialog() {
    ticketCloseSearchGeneration += 1;
    if (ticketCloseSearchTimer !== undefined) window.clearTimeout(ticketCloseSearchTimer);
    ticketCloseSearchTimer = undefined;
    ticketCloseDialog.value = undefined;
  }
  function openTicketClose(ticket: WireTicketRow) {
    const current = project();
    if (!current) return;
    ticketCloseDialog.value = {
      source: duplicateTarget(current, ticket),
      reason: 'completed',
      query: '',
      candidates: [],
    };
    queueMicrotask(() => document.querySelector<Control>('[name="ticket-close-reason"]')?.focus());
  }
  function setTicketCloseReason(reason: TicketCloseReason) {
    const current = ticketCloseDialog.value;
    if (!current) return;
    ticketCloseDialog.value = {
      ...current,
      reason,
      selected: reason === 'duplicate' ? current.selected : undefined,
      error: '',
    };
    if (reason === 'duplicate')
      queueMicrotask(() => document.querySelector<Control>('[name="ticket-close-target-search"]')?.focus());
  }
  function searchTicketCloseTargets(query: string) {
    const state = ticketCloseDialog.value;
    if (!state) return;
    const generation = ++ticketCloseSearchGeneration;
    if (ticketCloseSearchTimer !== undefined) window.clearTimeout(ticketCloseSearchTimer);
    ticketCloseDialog.value = {
      ...state,
      query,
      selected: undefined,
      candidates: [],
      searching: Boolean(query.trim()),
      error: '',
    };
    if (!query.trim()) return;
    ticketCloseSearchTimer = window.setTimeout(() => {
      void searchTicketReferences(projects.value, query, (project) =>
        new Api(project.apiPath).checkoutTickets(project.id, { text: query, compact: true, limit: 500 }),
      ).then(({ candidates, errors }) => {
        const active = ticketCloseDialog.value;
        if (
          generation !== ticketCloseSearchGeneration ||
          !active ||
          duplicateTargetKey(active.source) !== duplicateTargetKey(state.source)
        )
          return;
        ticketCloseDialog.value = { ...active, candidates, searching: false, error: errors.join(' · ') };
      });
    }, 150);
  }
  async function submitTicketClose() {
    const state = ticketCloseDialog.value,
      current = projects.value.find((item) => item.id === state?.source.projectId);
    if (!state || !current || state.submitting) return;
    const validation = validateTicketClose(state.reason, state.source, state.selected);
    if (validation) {
      ticketCloseDialog.value = { ...state, error: validation };
      return;
    }
    ticketCloseDialog.value = { ...state, submitting: true, error: '' };
    try {
      const result = await new Api(current.apiPath).closeCheckoutTicket(
        current.id,
        state.source.qualifiedId,
        state.reason,
        state.selected ? duplicateReference(state.selected) : undefined,
      );
      if (project()?.id !== current.id) return;
      await refreshProject();
      presentTicket(result.ticket);
      closeTicketCloseDialog();
      showToast(
        state.reason === 'duplicate'
          ? `${state.source.slug} marked as a duplicate of ${state.selected!.slug}.`
          : `${state.source.slug} closed as ${ticketCloseReasonLabel(state.reason)?.toLowerCase() ?? state.reason}.`,
      );
    } catch (reason) {
      const active = ticketCloseDialog.value;
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type. (HS2-W3RDCB)
      if (active)
        ticketCloseDialog.value = {
          ...active,
          submitting: false,
          error: reason instanceof Error ? reason.message : String(reason),
        };
    }
  }
  async function openDuplicateTarget(id: string) {
    const reference = parseDuplicateReference(id);
    if (reference) {
      // The reference names a checkout on the source ticket's own server (HS2-4SPTA4).
      const source = project(),
        sourceRemote = source ? parseRemoteProjectId(source.id) : undefined;
      let target = relationTargetProject(projects.value, reference.project_id, source?.id);
      if (!target && sourceRemote && source?.remoteHost) {
        // HS2-4JTETR: the project is on the same paired host but not open here; offer to open it.
        const host = source.remoteHost,
          name = await dependencies.remoteHostCheckoutName?.(sourceRemote.bindingId, reference.project_id);
        if (name === null) {
          showToast(`That project is not available on ${host}.`);
          return;
        }
        if (
          dependencies.confirm &&
          dependencies.openRemoteHostCheckout &&
          (await dependencies.confirm({
            title: `Open ${name ?? reference.project_id} from ${host}?`,
            message: `This ticket is in a project on ${host} that is not open here. It opens in a new tab.`,
            confirmLabel: 'Open Project',
          }))
        ) {
          try {
            target = await dependencies.openRemoteHostCheckout(sourceRemote.bindingId, reference.project_id);
          } catch (reason) {
            showToast(
              `Could not open that project on ${host}. ${reason instanceof Error ? reason.message : String(reason)}`,
            );
            return;
          }
        } else {
          if (!dependencies.confirm || !dependencies.openRemoteHostCheckout)
            showToast(`Open that project on ${host} to view this ticket.`);
          return;
        }
      }
      if (!target) {
        showToast(`Open project ${reference.project_id} to view this duplicate target.`);
        return;
      }
      try {
        const ticket = (
          await new Api(target.apiPath).checkoutTicket(target.id, `${reference.connection_id}:${reference.native_id}`)
        ).ticket;
        await openTicketLinkMatch({
          projectId: target.id,
          projectName: target.name,
          ticketId: ticket.qualified_id,
          qualifiedId: ticket.qualified_id,
          connectionId: ticket.connection_id,
          slug: ticket.slug,
          title: ticket.title,
          status: ticket.status,
        });
      } catch (reason) {
        error.value = reason instanceof Error ? reason.message : String(reason);
      }
      return;
    }
    const matches = (
      await Promise.allSettled(
        projects.value.map(async (target) => ({
          target,
          ticket: (await new Api(target.apiPath).checkoutTicket(target.id, id)).ticket,
        })),
      )
    ).flatMap((result) =>
      result.status === 'fulfilled'
        ? [
            {
              projectId: result.value.target.id,
              projectName: result.value.target.name,
              ticketId: result.value.ticket.qualified_id,
              qualifiedId: result.value.ticket.qualified_id,
              connectionId: result.value.ticket.connection_id,
              slug: result.value.ticket.slug,
              title: result.value.ticket.title,
              status: result.value.ticket.status,
              nativeId: result.value.ticket.native_id,
              updatedAt: result.value.ticket.updated_at,
            },
          ]
        : [],
    );
    // Copies of one ticket in several tabs open the best copy without asking (HS2-MBFGRV).
    const unique = new Map(bestTicketCopies(matches).map((match) => [ticketLinkMatchKey(match), match]));
    if (unique.size === 0) {
      showToast(`No exact match for ${id}.`);
      return;
    }
    if (unique.size > 1) {
      ticketLinkChoice.value = { kind: 'choose', reference: { raw: id, slug: id }, matches: [...unique.values()] };
      return;
    }
    await openTicketLinkMatch([...unique.values()][0]);
  }
  async function addNotWorkingFiles(files: FileList | File[]) {
    const target = notWorkingTarget.value,
      epoch = notWorkingAttachmentEpoch,
      screening = Symbol();
    if (
      !target.slug ||
      project()?.id !== target.projectId ||
      !(capabilitiesFor(target.connectionId)?.attachments ?? true)
    )
      return;
    const active = () =>
      epoch === notWorkingAttachmentEpoch && notWorkingTarget.value === target && project()?.id === target.projectId;
    activeNotWorkingScreenings.add(screening);
    notWorkingScreening.value = true;
    try {
      const screened = await screenAttachmentFiles(Array.from(files));
      if (!active()) return;
      const pending = screened.readable.map((file) => ({ id: browserRandomId(), name: file.name, file }));
      notWorkingFiles.value = [...notWorkingFiles.value, ...pending];
      await draftFiles.stage(draftScope('not-working', target.projectId), pending);
      if (!active()) return;
      notWorkingError.value = describeUnreadableAttachments(screened.unreadable);
      scheduleProjectSessionPersistence();
      presentNotWorkingDialog();
    } finally {
      activeNotWorkingScreenings.delete(screening);
      if (active()) notWorkingScreening.value = activeNotWorkingScreenings.size > 0;
    }
  }
  function openTicketComposer(trigger?: HTMLElement) {
    if (composerExpanded.value) return;
    if (trigger && document.activeElement !== trigger) trigger.focus({ preventScroll: true });
    const epoch = ++composerAttachmentEpoch,
      projectId = project()?.id;
    // A fresh draft from the Backlog view starts with Add to Backlog checked; a restored draft keeps
    // its own choice (HS2-V72HCC).
    if (!composerTitle.value.trim() && !composerDetails.value.trim() && !composerUpNext.value)
      composerBacklog.value = newTicketStatusForView(selectedView.value) === 'backlog';
    composerExpanded.value = true;
    composerOpened?.(projectId);
    scheduleProjectSessionPersistence();
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        if (epoch !== composerAttachmentEpoch || project()?.id !== projectId) return;
        showQuickTicketComposer(document);
        focusQuickTicketComposerTitle(document);
      }),
    );
  }
  function openFollowUpComposer(ticket: WireTicketRow) {
    const current = project();
    if (!current || composerExpanded.value) return;
    composerFollowUpTo = {
      qualified: `@${current.id}/${ticket.qualified_id}`,
      slug: ticket.slug,
    };
    openTicketComposer();
  }
  /**
   * Run `focus` once the closing composer dialog has handed focus back to its launcher. Web Awesome
   * restores that focus in a task after its hide animation ends, so a fixed delay raced it on a busy
   * main thread: the editor took focus first, the restore moved it to the launcher, and the blur
   * closed the new ticket's details editor (HS2-8YHY6V).
   */
  function afterComposerFocusRestore(focus: () => void) {
    const dialog = document.querySelector<HTMLElement>(SEARCH_AND_COMPOSER_TARGETS.quickTicketComposer.selector),
      closing = dialog?.shadowRoot?.querySelector('dialog')?.open;
    if (!dialog || !closing) {
      window.setTimeout(focus, 0);
      return;
    }
    dialog.addEventListener(
      'wa-after-hide',
      () => {
        // The restore is queued just before this event; focus in the task after it.
        window.setTimeout(focus, 0);
      },
      { once: true },
    );
  }
  function resetTicketComposer(clearStored = true) {
    const ids = [...composerAttachments.value, ...composerUnrestoredAttachments.value].map((item) => item.id),
      scope = draftScope('composer');
    composerAttachmentEpoch += 1;
    activeComposerScreenings.clear();
    composerScreening.value = false;
    composerSubmitting.value = false;
    composerExpanded.value = false;
    composerTitle.value = '';
    composerDetails.value = '';
    composerUpNext.value = false;
    composerPriority.value = 'default';
    composerBacklog.value = false;
    composerSource.value = undefined;
    composerFollowUpTo = undefined;
    composerAttachments.value = [];
    composerUnrestoredAttachments.value = [];
    composerAttachmentMessage.value = '';
    composerAttachmentError.value = false;
    if (clearStored) void draftFiles.remove(scope, ids).catch(() => undefined);
    scheduleProjectSessionPersistence();
  }
  async function addNewTicketFiles(files: FileList | File[]) {
    const origin = project(),
      screening = Symbol();
    openTicketComposer();
    const epoch = composerAttachmentEpoch;
    if (!origin || !canStageNewTicketAttachments()) {
      composerAttachmentMessage.value = 'The selected ticket source cannot create tickets with attachments.';
      composerAttachmentError.value = true;
      return;
    }
    activeComposerScreenings.add(screening);
    composerScreening.value = true;
    composerAttachmentMessage.value = 'Checking attachment files…';
    composerAttachmentError.value = false;
    try {
      const screened = await screenAttachmentFiles(Array.from(files));
      if (composerAttachmentEpoch !== epoch || project()?.id !== origin.id) return;
      const pending = screened.readable.map((file) => ({ id: browserRandomId(), name: file.name, file }));
      composerAttachments.value = [...composerAttachments.value, ...pending];
      await draftFiles.stage(draftScope('composer', origin.id), pending);
      if (composerAttachmentEpoch !== epoch || project()?.id !== origin.id) return;
      const warning = describeUnreadableAttachments(screened.unreadable);
      composerAttachmentMessage.value = warning;
      composerAttachmentError.value = Boolean(warning);
      scheduleProjectSessionPersistence();
      requestAnimationFrame(() =>
        requestAnimationFrame(() => {
          if (composerAttachmentEpoch === epoch && project()?.id === origin.id) focusQuickTicketComposerTitle(document);
        }),
      );
    } finally {
      activeComposerScreenings.delete(screening);
      if (composerAttachmentEpoch === epoch && project()?.id === origin.id)
        composerScreening.value = activeComposerScreenings.size > 0;
    }
  }
  async function submitNewTicket() {
    const origin = project(),
      target = newTicketSource(),
      // A non-default target must be named even when it is the only writable source.
      source = newTicketCreationSourceId(defaultProvider(), target),
      title = composerTitle.value.trim();
    if (
      !origin ||
      !title ||
      composerScreening.value ||
      composerSubmitting.value ||
      !(target?.capabilities.create ?? true)
    )
      return;
    // Staged files are kept, not silently dropped, when the chosen source cannot take them; the
    // composer disables Create and this guard covers Enter/programmatic submits (HS2-8HHHK3).
    if (strandedNewTicketAttachments(composerAttachments.value.length, canStageNewTicketAttachments())) {
      composerAttachmentMessage.value = STRANDED_ATTACHMENTS_MESSAGE;
      composerAttachmentError.value = true;
      return;
    }
    if (
      draftFiles.state(
        draftScope('composer', origin.id),
        composerAttachments.value,
        composerUnrestoredAttachments.value,
      ).blocked
    )
      return;
    const epoch = composerAttachmentEpoch,
      submitted = [...composerAttachments.value],
      scope = draftScope('composer', origin.id),
      snapshot = {
        open: composerExpanded.value,
        title: composerTitle.value,
        details: composerDetails.value,
        category: composerCategory.value,
        upNext: composerUpNext.value,
        priority: composerPriority.value,
        backlog: composerBacklog.value,
        attachments: submitted.map(({ id, name }) => ({ id, name })),
      },
      active = () => composerAttachmentEpoch === epoch && project()?.id === origin.id;
    const client = new Api(origin.apiPath),
      files = submitted.map((item) => item.file),
      batch_id = browserRandomId(),
      // A source without Up Next creates the ticket outside it, as its disabled star shows (HS2-Y32T8A).
      placement = newTicketCreationPlacement(
        composerUpNext.value && !newTicketUpNextDisabledReason(target),
        composerBacklog.value,
      ),
      finishLocalCreation = beginLocalTicketCreation();
    let completedEpoch: number | undefined;
    composerSubmitting.value = true;
    composerAttachmentMessage.value = files.length
      ? `Creating ticket and adding ${files.length} attachment${files.length === 1 ? '' : 's'}…`
      : 'Creating ticket…';
    composerAttachmentError.value = false;
    try {
      const result = await createTicketWithAttachments(
        files,
        async () => {
          const created = await client.createCheckoutTicket(
            origin.id,
            {
              title,
              details: snapshot.details,
              category: snapshot.category,
              ...(snapshot.priority !== 'default' ? { priority: snapshot.priority } : {}),
              ...placement,
              follow_up_to: composerFollowUpTo?.qualified,
            },
            source,
          );
          if (created.follow_up_link_error && composerFollowUpTo) {
            try {
              await client.linkCheckoutFollowUp(
                origin.id,
                composerFollowUpTo.qualified,
                `@${origin.id}/${created.qualified_id}`,
              );
            } catch (reason) {
              showToast(
                `${created.slug} was created, but its source link still needs repair: ${reason instanceof Error ? reason.message : String(reason)}`,
              );
            }
          }
          if (source)
            lastTicketSourceByProject.value = rememberNewTicketSource(
              lastTicketSourceByProject.value,
              origin.id,
              source,
            );
          localTicketChangeAcknowledgements.acknowledge(origin.id, {
            store: created.connection_id,
            id: created.id,
            kind: 'created',
          });
          pendingCreatedTickets.register(origin.id, created);
          return created;
        },
        async (created, file) => {
          const updated = await uploadAttachmentWithPoster(client, origin, created, file, {
            batch_id,
            actor: { role: 'human' },
          });
          localTicketChangeAcknowledgements.acknowledge(origin.id, {
            store: updated.connection_id,
            id: updated.id,
            kind: 'attachment_added',
          });
          return updated;
        },
        (created) => {
          finishDraftSubmission(localStorage, origin.id, { kind: 'composer', draft: snapshot });
          if (!active()) {
            void draftFiles
              .remove(
                scope,
                submitted.map((item) => item.id),
              )
              .catch(() => undefined);
            if (project()?.id === origin.id) {
              const ids = new Set(submitted.map((item) => item.id));
              composerAttachments.value = composerAttachments.value.filter((item) => !ids.has(item.id));
              composerUnrestoredAttachments.value = composerUnrestoredAttachments.value.filter(
                (item) => !ids.has(item.id),
              );
            }
            return;
          }
          resetTicketComposer();
          cancelTicketDrafts();
          tickets.value = prependCreatedTicketRow(tickets.value, created);
          publishOptimisticTicketRows(origin.id);
          if (!createdTicketVisibleInView(created, selectedView.value)) selectTicketView('all');
          selectedTicket.value = null;
          selectedTicketSlugs.value = [created.slug];
          state.ticketSelectionAnchor = created.slug;
          // On a phone the inspector is a closed overlay; open it so the new ticket's details
          // editor is reachable rather than inert offscreen (HS2-QFW2A7).
          revealTicketInspector();
          presentTicket(created);
          beginDetailsEdit();
          completedEpoch = composerAttachmentEpoch;
          afterComposerFocusRestore(() => {
            if (completedEpoch !== composerAttachmentEpoch || project()?.id !== origin.id) return;
            const source = activeTicketSurface().querySelector<HTMLElement>('[name="markdown-source"]'),
              active = document.activeElement; // Only claim focus for the details editor if the user has not already focused another control (e.g. opened the priority select) — otherwise this delayed focus steals it and closes their popup (HS2-43F14D).
            if (
              source &&
              (!active ||
                active === document.body ||
                active === document.documentElement ||
                active.closest('[data-component="quick-ticket-composer-launcher"]'))
            )
              source.focus();
          });
        },
      );
      const failure = describeNewTicketAttachmentFailures(result.failed);
      if (completedEpoch !== composerAttachmentEpoch || project()?.id !== origin.id || composerExpanded.value) return;
      if (selectedTicket.value?.id === result.ticket.id)
        selectedTicket.value = { ...selectedTicket.value, attachments: result.ticket.attachments };
      if (failure) {
        inspectorTab.value = 'attachments';
        attachmentMessage.value = failure;
        error.value = failure;
      }
    } catch (reason) {
      if (!active()) return;
      const message = reason instanceof Error ? reason.message : String(reason);
      composerAttachmentMessage.value = `Ticket creation failed: ${message}`;
      composerAttachmentError.value = true;
      composerSubmitting.value = false;
    } finally {
      await finishLocalCreation();
    }
  }

  async function submitNotWorking() {
    const target = notWorkingTarget.value;
    if (
      !target.slug ||
      notWorkingSubmitting.value ||
      notWorkingScreening.value ||
      draftFiles.state(
        draftScope('not-working', target.projectId),
        notWorkingFiles.value,
        notWorkingUnrestoredFiles.value,
      ).blocked
    )
      return;
    const owning = projects.value.find((item) => item.id === target.projectId);
    if (!owning) {
      notWorkingError.value = 'The project is no longer open.';
      return;
    }
    const client = new Api(target.apiPath),
      capabilities = capabilitiesFor(target.connectionId),
      note = (capabilities?.notes ?? true) ? notWorkingNote.value : '',
      files = [...notWorkingFiles.value],
      epoch = ++notWorkingAttachmentEpoch,
      scope = draftScope('not-working', target.projectId),
      snapshot = {
        ticketId: target.ticketId,
        slug: target.slug,
        connectionId: target.connectionId,
        mode: target.mode,
        note: notWorkingNote.value,
        attachments: files.map(({ id, name }) => ({ id, name })),
      },
      active = () =>
        notWorkingAttachmentEpoch === epoch && project()?.id === target.projectId && notWorkingTarget.value === target;
    notWorkingSubmitting.value = true;
    notWorkingError.value = '';
    try {
      const current = (await client.checkoutTicket(target.projectId, `${target.connectionId}:${target.ticketId}`))
        .ticket;
      let full: FullTicket = current;
      await submitNotWorkingReport(
        { note, files: files.map((item) => item.file) },
        {
          report: async (text, evidence) => {
            full = await client.reportNotWorking(
              target.connectionId,
              target.ticketId,
              text,
              evidence,
              current.concurrency_token,
            );
          },
        },
      );
      if (active()) {
        setProjectTicketRows(
          owning.id,
          projectTabTicketRows(owning.id).map((row) => (row.id === full.id ? ticketRowFromFull(row, full) : row)),
        );
        selectedTicketSlugs.value = [target.slug];
        state.ticketSelectionAnchor = target.slug;
        presentTicket(full);
      }
      finishDraftSubmission(localStorage, target.projectId, { kind: 'not-working', draft: snapshot });
      void draftFiles
        .remove(
          scope,
          files.map((item) => item.id),
        )
        .catch(() => undefined);
      if (active()) closeNotWorking(false);
      else if (project()?.id === target.projectId && notWorkingTarget.value.ticketId === target.ticketId) {
        const ids = new Set(files.map((item) => item.id));
        notWorkingFiles.value = notWorkingFiles.value.filter((item) => !ids.has(item.id));
        notWorkingUnrestoredFiles.value = notWorkingUnrestoredFiles.value.filter((item) => !ids.has(item.id));
      }
    } catch (reason) {
      if (!active()) return;
      notWorkingTarget.value = target;
      notWorkingNote.value = note;
      notWorkingFiles.value = files;
      notWorkingError.value = reason instanceof Error ? reason.message : String(reason);
      notWorkingSubmitting.value = false;
      scheduleProjectSessionPersistence();
      presentNotWorkingDialog();
    }
  }

  return {
    feedbackDelivery,
    chooseFeedbackSession,
    dismissFeedbackDelivery,
    attachmentEpochs: () => ({ composer: composerAttachmentEpoch, notWorking: notWorkingAttachmentEpoch }),
    applyTicketPatch,
    restoreTicketDraft,
    clearConflictRecoveryCopy,
    cancelConflictedFieldSave,
    resolveLinkedReaderConflict,
    flushTicketDrafts,
    flushSelectedTicketEdits,
    captureTicketDraftRecovery,
    updateSelected,
    dismissFeedbackRequest,
    history,
    updateSelectedTracked,
    detailsAutosave,
    readerDetailsAutosave,
    noteAutosave,
    readerNoteAutosave,
    blockedReasonAutosave,
    readerBlockedReasonAutosave,
    titleAutosave,
    tagsAutosave,
    linkedReaderFrame,
    linkedReaderAutosaves,
    replaceLinkedReaderFrame,
    refreshLinkedCodeReview,
    linkedReaderSaves,
    flushLinkedReader,
    selectedRows,
    pruneSelectionForCurrentView,
    setProjectTicketRows,
    applyBulkOperations,
    restoreTrashedTickets,
    executeBulkTicketAction,
    openEmptyTrash,
    emptyTrash,
    openBulkTicketDialog,
    copySelection,
    pasteSelection,
    copyDraggedTickets,
    isEditableEvent,
    ticketWorkAreaFocused,
    ordinaryTextSelected,
    timeline,
    timelineUsage,
    refreshTicketUsage,
    notes,
    attachmentContext,
    duplicateTargetFor,
    resolveDuplicateTargetFor,
    uploadAttachmentWithPoster,
    addAttachments,
    selectionOrder,
    presentTicket,
    reconcileWorkerSelection,
    cancelTicketDrafts,
    selectTickets,
    openTicketReader,
    closeNotWorking,
    presentNotWorkingDialog,
    openNotWorking,
    closeTicketCloseDialog,
    openTicketClose,
    setTicketCloseReason,
    searchTicketCloseTargets,
    submitTicketClose,
    openDuplicateTarget,
    addNotWorkingFiles,
    openTicketComposer,
    openFollowUpComposer,
    followUpSource: () => composerFollowUpTo?.slug,
    resetTicketComposer,
    addNewTicketFiles,
    submitNewTicket,
    submitNotWorking,
  };
}
