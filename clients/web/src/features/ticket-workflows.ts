import { batch, type Signal } from 'kerfjs';

import {
  Api,
  type AttachmentMetadata,
  type Capabilities,
  type CheckoutTicketCounts,
  type CodeReview,
  type DuplicateBacklink,
  type FullTicket,
  type TicketCloseReason,
  type TicketRow as WireTicketRow,
} from '../api';
import { describeUnreadableAttachments, screenAttachmentFiles } from '../attachment-files';
import { attachmentUploadBatchId } from '../attachment-grouping';
import { type AttachmentReferenceContext, isVideoAttachment } from '../attachment-references';
import { browserRandomId } from '../browser-id';
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
import { createDebouncedAutosave, createFocusLossAutosave, type DebouncedAutosave } from '../debounced-autosave';
import { presentedNoteKind } from '../feedback-needed';
import type { InlineFeedbackReply } from '../feedback-replies';
import { syncFocusedDraftControl } from '../focused-draft-sync';
import { beginInteractionTiming } from '../interaction-performance';
import { data } from '../interactions/dom';
import type { Control, NotWorkingTarget, PendingEvidence, Project } from '../interactions/types';
import type { LocalTicketChangeAcknowledgements } from '../local-ticket-changes';
import { createTicketWithAttachments, describeNewTicketAttachmentFailures } from '../new-ticket-attachments';
import {
  newTicketCreationSourceId,
  type ProjectTicketSources,
  rememberNewTicketSource,
  type TicketSourceChoice,
} from '../new-ticket-source';
import { submitNotWorkingReport } from '../not-working-workflow';
import { noteAuthorship } from '../note-authorship';
import { type PendingCreatedTickets, prependCreatedTicketRow } from '../pending-created-tickets';
import {
  bulkTagChoices,
  type BulkTicketAction,
  type BulkTicketMutationSequencer,
  bulkTicketPatch,
  canAtomicallyBulkUpdate,
  canBulkUpdate,
  projectBulkTicketCounts,
} from '../ticket-bulk-operations';
import {
  duplicateReference,
  type DuplicateTarget,
  duplicateTargetKey,
  parseDuplicateReference,
  resolveDuplicateReferenceTarget,
  ticketCloseReasonLabel,
  validateTicketClose,
} from '../ticket-close';
import {
  clearTicketDraft,
  loadTicketDraft,
  saveTicketDraft,
  type TicketDraftField,
  ticketDraftKey,
} from '../ticket-draft-store';
import {
  isTicketConcurrencyConflict,
  rebaseDraftValue,
  reconcileActiveDraft,
  reconcileTicketPatch,
  type TicketFieldConflict,
  ticketFieldLabel,
  ticketFieldText,
} from '../ticket-field-reconciliation';
import { type TicketLinkMatch, ticketLinkMatchKey } from '../ticket-link-resolution';
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
  reconcileTicketReaderFrame,
  type TicketReaderFrame,
  updateTicketReaderFrame,
} from '../ticket-reader-stack';
import { ticketTimelineEntries } from '../ticket-timeline-data';
import type { TicketTitleEditSurface } from '../ticket-title-editing';
import { copiedTicketPlacement } from '../ticket-transfer';
import {
  createdTicketVisibleInView,
  isTrashedTicket,
  newTicketCreationPlacement,
  selectionVisibleInView,
  type TicketView,
} from '../ticket-views';
import { ensureVideoPoster } from '../video-posters';
import { deleteDraftFiles, saveDraftFile } from '../workspace-session';

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
  state: TicketWorkflowMutableState;
  CLOSED_NOT_WORKING_TARGET: NotWorkingTarget;
  projects: Signal<Project[]>;
  selectedProjectId: Signal<string>;
  tickets: Signal<WireTicketRow[]>;
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
  resolvedDuplicateTargets: Signal<Record<string, DuplicateTarget>>;
  ticketCloseDialog: Signal<TicketCloseDialogState | undefined>;
  ticketLinkChoice: Signal<TicketLinkChoice | undefined>;
  bulkTicketDialog: Signal<BulkTicketDialogState | undefined>;
  notWorkingTarget: Signal<NotWorkingTarget>;
  notWorkingNote: Signal<string>;
  notWorkingFiles: Signal<PendingEvidence[]>;
  notWorkingSubmitting: Signal<boolean>;
  notWorkingError: Signal<string>;
  composerExpanded: Signal<boolean>;
  composerTitle: Signal<string>;
  composerDetails: Signal<string>;
  composerCategory: Signal<string>;
  composerUpNext: Signal<boolean>;
  /** The source picked in the open composer, if any (HS2-NZMJBJ). */
  composerSource: Signal<string | undefined>;
  /** The source each project last created a ticket in, in memory only (HS2-NZMJBJ). */
  lastTicketSourceByProject: Signal<Record<string, string>>;
  composerAttachments: Signal<PendingEvidence[]>;
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
}

/** Ticket mutation, selection, transfer, attachment, close, and composer workflows. */
export function createTicketWorkflows(dependencies: TicketWorkflowDependencies) {
  const {
    state,
    CLOSED_NOT_WORKING_TARGET,
    projects,
    selectedProjectId,
    tickets,
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
    resolvedDuplicateTargets,
    ticketCloseDialog,
    ticketLinkChoice,
    bulkTicketDialog,
    notWorkingTarget,
    notWorkingNote,
    notWorkingFiles,
    notWorkingSubmitting,
    notWorkingError,
    composerExpanded,
    composerTitle,
    composerDetails,
    composerCategory,
    composerUpNext,
    composerSource,
    lastTicketSourceByProject,
    composerAttachments,
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
  } = dependencies;
  /** Rebase-and-retry attempts after a stale-token rejection before an edit is reported as failed. */
  const MAX_CONCURRENT_EDIT_RETRIES = 4;
  let ticketCloseSearchGeneration = 0,
    ticketCloseSearchTimer: number | undefined,
    composerAttachmentEpoch = 0;
  const activeComposerScreenings = new Set<symbol>();
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
  ): { patch: TicketPatch; recorded: TicketPatch; conflict?: TicketFieldConflict } {
    const next: TicketPatch = { ...patch },
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
          conflict: {
            key: field === 'note' ? `note:${noteId}` : field,
            field,
            label: ticketFieldLabel(field),
            base: draftBase,
            mine,
            theirs: current,
          },
        };
      if (result.kind === 'merged')
        next[field] = field === 'blocked_reason' && !result.value.trim() ? null : result.value;
    }
    return { patch: next, recorded: { ...patch, ...adoptMergedDrafts(patch, next) } };
  }
  async function applyTicketPatch(slug: string, patch: TicketPatch) {
    const current = project(),
      ticket = tickets.value.find((item) => item.slug === slug);
    if (!current || !ticket) return false;
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
    return singleTicketMutationSequencer.enqueue(slug, async () => {
      try {
        // Base off the last edit this client committed for the ticket (its up-to-date token), falling back to
        // the pre-edit selection or a fresh fetch. A real external write still fails the token check below.
        let base =
          committedTickets.get(slug) ??
          selectedBefore ??
          (await api().checkoutTicket(current.id, ticket.qualified_id)).ticket;
        // A draft typed on top of an older value must merge with what the ticket holds now, or the save would
        // silently overwrite a concurrent edit to the same field (HS2-A4XCXE).
        const rebased = rebaseDraftPatch(patch, base);
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
              outcome: 'rolled_back',
            });
          }
          return false;
        }
        let pending = rebased.patch,
          recorded = rebased.recorded,
          updated: FullTicket | undefined;
        // Token drift from unrelated writes (an AI's notes, lease renewals) can land between a refetch and the
        // retry; rebase and retry a bounded number of times instead of failing the edit (HS2-A4XCXE).
        for (let attempt = 0; !updated; attempt += 1) {
          try {
            updated = (
              await api().updateCheckoutTicket(
                current.id,
                ticket.qualified_id,
                base.concurrency_token ? { ...pending, expected_token: base.concurrency_token } : pending,
              )
            ).ticket;
            localTicketChangeAcknowledgements.acknowledge(current.id, {
              store: updated.connection_id,
              id: updated.id,
              kind: 'updated',
            });
          } catch (reason) {
            if (!isTicketConcurrencyConflict(reason) || attempt >= MAX_CONCURRENT_EDIT_RETRIES) throw reason;
            const remote = (await api().checkoutTicket(current.id, ticket.qualified_id)).ticket,
              reconciled = reconcileTicketPatch(base, remote, pending);
            committedTickets.set(slug, remote);
            rollbackRow = ticketRowFromFull(ticket, remote);
            rollbackSelected = remote;
            if (mutationGenerations.get(slug) !== generation) return true;
            tickets.value = tickets.value.map((item) => (item.slug === slug ? ticketRowFromFull(item, remote) : item));
            publishOptimisticTicketRows(current.id);
            if (selectedTicket.value?.slug === slug) reconcileRefreshedSelected(base, remote);
            const conflict = reconciled.conflicts[0];
            // prettier-ignore
            // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
            if(conflict){showFieldConflict(conflict);reportMutationTiming({slug,optimistic_ms:optimistic,request_ms:performance.now()-started,outcome:'rolled_back'});return false}
            recorded = { ...recorded, ...adoptMergedDrafts(pending, reconciled.retry) };
            if (Object.keys(reconciled.retry).length === 0) updated = remote;
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
            outcome: 'stale',
          });
          return true;
        }
        tickets.value = tickets.value.map((item) => (item.slug === slug ? ticketRowFromFull(item, updated) : item));
        publishOptimisticTicketRows(current.id);
        if (selectedTicket.value?.slug === slug) selectedTicket.value = updated;
        recordCommittedDraftBases(recorded);
        error.value = updated.warnings?.join('\n') ?? '';
        reportMutationTiming({
          slug,
          optimistic_ms: optimistic,
          request_ms: performance.now() - started,
          outcome: 'committed',
        });
        return true;
      } catch (reason) {
        if (mutationGenerations.get(slug) === generation) {
          tickets.value = tickets.value.map((item) => (item.slug === slug ? rollbackRow : item));
          publishOptimisticTicketRows(current.id);
          if (rollbackSelected && selectedTicket.value?.slug === slug) selectedTicket.value = rollbackSelected;
          error.value = reason instanceof Error ? reason.message : String(reason);
          reportMutationTiming({
            slug,
            optimistic_ms: optimistic,
            request_ms: performance.now() - started,
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
    const current = project(),
      ticket = selectedTicket.value;
    return current && ticket ? ticketDraftKey(current.id, ticket.qualified_id, field, noteId) : undefined;
  }
  function persistSelectedDraft(field: TicketDraftField, base: string, draft: string, noteId?: string) {
    const storage = draftStorage(),
      key = selectedDraftKey(field, noteId);
    if (storage && key) saveTicketDraft(storage, key, { base, draft });
  }
  /** Drop the recovery copy once the value it holds is committed; a newer copy typed meanwhile stays. */
  function clearCommittedDraft(field: TicketDraftField, committed: string, noteId?: string) {
    const storage = draftStorage(),
      key = selectedDraftKey(field, noteId);
    if (!storage || !key) return;
    const stored = loadTicketDraft(storage, key);
    if (!stored || stored.draft.trim() === committed.trim()) clearTicketDraft(storage, key);
  }
  /**
   * The recovery copy for a field the user is about to edit, rebased onto the field's current value:
   * the stored draft continues on the same base when nothing changed, merges when the remote change
   * is disjoint, and is dropped (the current value wins) when it cannot be merged. `undefined` when
   * there is nothing to restore.
   */
  function restoreTicketDraft(
    field: TicketDraftField,
    current: string,
    noteId?: string,
  ): { draft: string; base: string } | undefined {
    const storage = draftStorage(),
      key = selectedDraftKey(field, noteId);
    if (!storage || !key) return undefined;
    const stored = loadTicketDraft(storage, key);
    if (!stored || stored.draft === stored.base || stored.draft === current) {
      if (stored) clearTicketDraft(storage, key);
      return undefined;
    }
    const next = reconcileActiveDraft(stored.base, stored.draft, current);
    if (next.kind === 'conflict') {
      clearTicketDraft(storage, key);
      showToast('An unsaved edit could not be merged with a newer version of this ticket and was discarded.');
      return undefined;
    }
    showToast('Restored an unsaved edit.');
    return next.kind === 'unchanged'
      ? { draft: stored.draft, base: stored.base }
      : { draft: next.draft, base: next.base };
  }
  const savingDraft =
    <T>(
      field: TicketDraftField,
      save: (value: T) => Promise<boolean>,
      text: (value: T) => string,
      noteId?: (value: T) => string | undefined,
    ) =>
    async (value: T) => {
      const saved = await save(value);
      if (saved) clearCommittedDraft(field, text(value), noteId?.(value));
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
    },
  );
  const readerDetailsAutosave = createFocusLossAutosave(
    savingDraft('details', (value: string) => updateSelectedTracked({ details: value }), plain),
    {
      persist: (value) => {
        persistSelectedDraft('details', state.readerDetailsDraftBase, value);
      },
    },
  );
  const noteAutosave = createFocusLossAutosave(
    savingDraft(
      'note',
      ({ id, value }: { id: string; value: string }) => updateSelected({ note_id: id, note: value }),
      noteText,
      ({ id }) => id,
    ),
    {
      persist: ({ id, value }) => {
        persistSelectedDraft('note', state.noteDraftBase, value, id);
      },
    },
  );
  const readerNoteAutosave = createFocusLossAutosave(
    savingDraft(
      'note',
      ({ id, value }: { id: string; value: string }) => updateSelected({ note_id: id, note: value }),
      noteText,
      ({ id }) => id,
    ),
    {
      persist: ({ id, value }) => {
        persistSelectedDraft('note', state.readerNoteDraftBase, value, id);
      },
    },
  );
  const blockedReasonAutosave = createFocusLossAutosave(
    savingDraft('blocked_reason', (value: string) => updateSelected({ blocked_reason: value.trim() || null }), plain),
    {
      persist: (value) => {
        persistSelectedDraft('blocked_reason', state.blockedReasonDraftBase, value);
      },
    },
  );
  const readerBlockedReasonAutosave = createFocusLossAutosave(
    savingDraft('blocked_reason', (value: string) => updateSelected({ blocked_reason: value.trim() || null }), plain),
    {
      persist: (value) => {
        persistSelectedDraft('blocked_reason', state.readerBlockedReasonDraftBase, value);
      },
    },
  );
  const titleAutosave = createFocusLossAutosave(
    savingDraft('title', (value: string) => updateSelectedTracked({ title: value.trim() }), plain),
    {
      persist: (value) => {
        persistSelectedDraft('title', state.titleDraftBase, value);
      },
    },
  );
  const tagsAutosave = createDebouncedAutosave((value: string[]) => updateSelectedTracked({ tags: value }));
  function linkedReaderFrame(target: Element) {
    const id = target.closest<HTMLElement>('[data-reader-frame-id]')?.dataset.readerFrameId;
    return id && id !== 'workspace-reader' ? linkedReaderStack.value.find((frame) => frame.id === id) : undefined;
  }
  function replaceLinkedReaderFrame(id: string, update: (frame: TicketReaderFrame) => TicketReaderFrame) {
    linkedReaderStack.value = updateTicketReaderFrame(linkedReaderStack.value, id, update);
  }
  async function updateLinkedReader(frameId: string, patch: Record<string, unknown>) {
    const frame = linkedReaderStack.value.find((item) => item.id === frameId);
    if (!frame || !frame.capabilities.update) return false;
    const client = new Api(frame.apiPath);
    let base = frame.ticket,
      pending: TicketPatch = patch;
    try {
      // Same field-aware rebase-and-retry as the workspace editor: unrelated remote writes and disjoint
      // edits to the same text merge instead of discarding the reader's edit (HS2-A4XCXE).
      for (let attempt = 0; ; attempt += 1) {
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
          if (reconciled.conflicts.length) {
            replaceLinkedReaderFrame(frameId, (current) => reconcileTicketReaderFrame(current, remote));
            error.value = `This linked ticket's ${ticketFieldLabel(reconciled.conflicts[0].field).toLocaleLowerCase()} changed remotely in the same place you edited. Your draft was preserved; review it and try again.`;
            return false;
          }
          if (Object.keys(reconciled.retry).length) {
            base = remote;
            pending = reconciled.retry;
            continue;
          }
          ticket = remote;
        }
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

  const linkedReaderAutosaves = new Map<
    string,
    {
      details: DebouncedAutosave<string>;
      note: DebouncedAutosave<{ id: string; value: string }>;
      blocked: DebouncedAutosave<string>;
    }
  >();
  function linkedReaderSaves(id: string) {
    let saves = linkedReaderAutosaves.get(id);
    if (!saves) {
      saves = {
        details: createFocusLossAutosave((value) => updateLinkedReader(id, { details: value })),
        note: createFocusLossAutosave(({ id: noteId, value }) =>
          updateLinkedReader(id, { note_id: noteId, note: value }),
        ),
        blocked: createFocusLossAutosave((value) => updateLinkedReader(id, { blocked_reason: value.trim() || null })),
      };
      linkedReaderAutosaves.set(id, saves);
    }
    return saves;
  }
  async function flushLinkedReader(frameId: string) {
    const saves = linkedReaderAutosaves.get(frameId);
    if (!saves) return true;
    const results = await Promise.all([saves.details.flush(), saves.note.flush(), saves.blocked.flush()]);
    return results.every(Boolean);
  }
  function selectedRows() {
    const selected = new Set(selectedTicketSlugs.value);
    return tickets.value.filter((ticket) => selected.has(ticket.slug));
  }
  function pruneSelectionForCurrentView() {
    const next = selectionVisibleInView(tickets.value, selectedTicketSlugs.value, selectedView.value);
    if (next.length === selectedTicketSlugs.value.length) return;
    selectedTicketSlugs.value = next;
    if (state.ticketSelectionAnchor && !next.includes(state.ticketSelectionAnchor))
      state.ticketSelectionAnchor = next[0];
    if (selectedTicket.value && !next.includes(selectedTicket.value.slug)) {
      cancelTicketDrafts();
      selectedTicket.value = null;
    }
    if (next.length !== 1) selectedTicket.value = null;
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
    const optimistic = projectTabTicketRows(current.id).map((ticket) => {
        const operation = operations.find((item) => item.slug === ticket.slug);
        return operation ? projectTicketPatch(ticket, operation.patch) : ticket;
      }),
      optimisticChanges = before.map((ticket) => ({
        before: ticket,
        after: optimistic.find((item) => item.slug === ticket.slug) ?? ticket,
      })),
      optimisticCounts = beforeCounts && projectBulkTicketCounts(beforeCounts, optimisticChanges);
    setProjectTicketRows(current.id, optimistic, optimisticCounts);
    beginBulkBoardRefill(current.id, before, optimistic, optimisticCounts);
    if (project()?.id === current.id && selectedTicket.value) {
      const operation = operations.find((item) => item.slug === selectedTicket.value?.slug);
      if (operation) selectedTicket.value = projectTicketPatch(selectedTicket.value, operation.patch);
    }
    finishTiming();
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
      if (atomic)
        updated.push(
          ...(await client.batchUpdateCheckoutTickets(
            current.id,
            requestOperations.map(({ id, patch }) => ({ id, patch })),
          )),
        );
      else {
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
      setProjectTicketRows(current.id, next, actualCounts);
      if (project()?.id === current.id) {
        const selectedUpdate = selectedTicket.value && updated.find((item) => item.id === selectedTicket.value?.id);
        if (selectedUpdate) selectedTicket.value = selectedUpdate;
        else if (selectedBefore && failures.some((item) => item.slug === selectedBefore.slug))
          selectedTicket.value = selectedBefore;
        pruneSelectionForCurrentView();
      }
      const failure = failures.length
        ? `Updated ${updated.length} of ${operations.length} tickets. ${failures.length} failed; ${failures[0].slug}: ${failures[0].message}`
        : '';
      if (failure) reportBulkFailure(current, failure);
      else if (project()?.id === current.id) error.value = '';
      return { complete: failures.length === 0, succeeded };
    } catch (reason) {
      setProjectTicketRows(
        current.id,
        projectTabTicketRows(current.id).map((ticket) => before.find((item) => item.slug === ticket.slug) ?? ticket),
        beforeCounts,
      );
      if (project()?.id === current.id && selectedBefore && slugs.has(selectedBefore.slug))
        selectedTicket.value = selectedBefore;
      reportBulkFailure(current, reason instanceof Error ? reason.message : String(reason));
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
      const selected = projectTabTicketRows(requestedProject.id).filter((ticket) =>
        requestedSlugs.includes(ticket.slug),
      );
      if (!canBulkUpdate(selected, capabilitiesFor)) {
        reportBulkFailure(requestedProject, 'One or more selected ticket providers do not support ticket updates.');
        return false;
      }
      const operations: BulkOperation[] = selected.flatMap((ticket) => {
        const patch = bulkTicketPatch(ticket, action);
        return patch ? [{ slug: ticket.slug, id: ticket.id, patch }] : [];
      });
      if (operations.length === 0) return true;
      const inverse = operations.map((operation) => {
          const ticket = selected.find((item) => item.slug === operation.slug)!;
          const keys = new Set(Object.keys(operation.patch));
          if ('status' in operation.patch) keys.add('up_next');
          return {
            slug: operation.slug,
            id: operation.id,
            patch: Object.fromEntries([...keys].map((key) => [key, ticket[key as keyof typeof ticket]])),
          };
        }),
        result = await applyBulkOperations(requestedProject, operations),
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
      if (project()?.id === current.id) await refreshTicketCollection('all');
      showToast(`Emptied Trash — ${result.purged} ticket${result.purged === 1 ? '' : 's'} permanently removed.`);
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
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  function copySelection(cut:boolean){const current=project(),rows=selectedRows();if(!current||!rows.length)return;const snapshot={tickets:rows.map(ticket=>clipboardTicket(ticket,selectedTicket.value?.qualified_id===ticket.qualified_id?selectedTicket.value:undefined)),cut,source:current};state.clipboard=snapshot;selectedTicketSlugs.value=[...selectedTicketSlugs.value];void Promise.all(rows.map(ticket=>api().checkoutTicket(current.id,ticket.qualified_id).then(value=>value.ticket))).then(full=>{if(state.clipboard!==snapshot)return;snapshot.tickets=full.map(ticket=>clipboardTicket(ticket));const text=full.map(ticket=>[`${ticket.slug}: ${ticket.title}`,ticket.details,...ticket.notes.map(note=>`- ${note.text}`)].filter(Boolean).join('\n\n')).join('\n\n');void navigator.clipboard?.writeText(text).catch(()=>undefined)}).catch((reason:unknown)=>{error.value=reason instanceof Error?reason.message:String(reason)})}
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
            'input, textarea, select, wa-input, wa-textarea, wa-select, [role="textbox"], [contenteditable]:not([contenteditable="false"])',
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
  function timeline(ticket: FullTicket) {
    return ticketTimelineEntries(ticket).map((entry) => ({ ...entry, time: ago(entry.timestamp) }));
  }
  function notes(ticket: FullTicket) {
    return ticket.notes.map((note) => {
      // Authorship comes from the note's recorded actor (HS2-32QDZ3).
      const { author, aiAuthored, aiTool } = noteAuthorship(note);
      return {
        id: note.id,
        kind: presentedNoteKind(note, ticket.notes),
        author,
        time: ago(note.created_at),
        body: note.text,
        aiAuthored,
        aiTool,
        confidence: note.confidence,
        feedbackFor: note.feedback_for,
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
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  function duplicateTargetFor(ticket:FullTicket){if(!ticket.duplicate_of)return undefined;const reference=parseDuplicateReference(ticket.duplicate_of),projectRows=reference?projects.value.filter(item=>item.id===reference.project_id):projects.value,match=projectRows.flatMap(item=>(item.id===selectedProjectId.value?tickets.value:ticketRowsByProject.value[item.id]??[]).map(row=>({project:item,row}))).find(({row})=>reference?row.connection_id===reference.connection_id&&row.native_id===reference.native_id:row.id===ticket.duplicate_of||row.native_id===ticket.duplicate_of),target=match?duplicateTarget(match.project,match.row):resolvedDuplicateTargets.value[ticket.duplicate_of];return target?{id:ticket.duplicate_of,projectName:target.projectName,slug:target.slug,title:target.title}:undefined}
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  async function resolveDuplicateTargetFor(ticket:FullTicket){const value=ticket.duplicate_of;if(!value||resolvedDuplicateTargets.value[value])return;const target=await resolveDuplicateReferenceTarget(value,projects.value,(item,id)=>new Api((item as Project).apiPath).checkoutTicket(item.id,id).then(result=>result.ticket)).catch(()=>undefined);if(target)resolvedDuplicateTargets.value={...resolvedDuplicateTargets.value,[value]:target}}
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
  async function addAttachments(slug: string, files: FileList | File[]) {
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
  function selectionOrder(target: Element) {
    const column = target.closest('[data-component="ticket-board-column"]');
    return column
      ? [...column.querySelectorAll<HTMLElement>('[data-action="select-ticket-row"]')].map(
          (item) => data(item).ticketSlug!,
        )
      : visibleTickets().map((ticket) => ticket.slug);
  }
  function presentTicket(ticket: FullTicket) {
    const changed = selectedTicket.value?.id !== ticket.id,
      current = project(),
      backlinkKey = current ? `${current.id}:${ticket.qualified_id}` : '';
    batch(() => {
      selectedCorruptKey.value = undefined;
      selectedTicket.value = ticket;
      committedTickets.clear();
      committedTickets.set(ticket.slug, ticket);
      duplicateBacklinkState.value = { key: backlinkKey, backlinks: [], inaccessibleProjects: [] };
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
  }
  /** Send every unsaved text edit now (page hide, explicit save); each keeps its local copy until it commits. */
  function flushTicketDrafts(): Promise<boolean> {
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
  function selectTickets(
    slug: string,
    intent: { range?: boolean; toggle?: boolean } = {},
    ordered = visibleTickets().map((ticket) => ticket.slug),
  ) {
    const loaded = selectedTicket.value;
    if (isPlainTicketReselection(selectedTicketSlugs.value, loaded?.slug, slug, intent)) return Promise.resolve(loaded);
    const finishTiming = beginInteractionTiming('ticket-selection', { slug }),
      next = updateTicketSelection(
        ordered,
        { anchor: state.ticketSelectionAnchor, selected: new Set(selectedTicketSlugs.value) },
        slug,
        intent,
      ),
      selected = [...next.selected];
    state.ticketSelectionAnchor = next.anchor;
    batch(() => {
      selectedCorruptKey.value = undefined;
      selectedTicketSlugs.value = selected;
      cancelTicketDrafts();
      if (selected.length !== 1) selectedTicket.value = null;
    });
    scheduleProjectSessionPersistence();
    finishTiming();
    if (selected.length !== 1) return Promise.resolve(undefined);
    const ticket = tickets.value.find((item) => item.slug === selected[0]),
      current = project();
    if (ticket && current)
      return api()
        .checkoutTicket(current.id, ticket.qualified_id)
        .then((result) => {
          if (
            project()?.id === current.id &&
            selectedTicketSlugs.value.length === 1 &&
            selectedTicketSlugs.value[0] === result.ticket.slug
          )
            presentTicket(result.ticket);
          return result.ticket;
        })
        .catch((reason: unknown) => {
          error.value = reason instanceof Error ? reason.message : String(reason);
          return undefined;
        });
    return Promise.resolve(undefined);
  }
  async function openTicketReader(slug: string, ordered?: string[], trigger?: HTMLElement) {
    const ticket = await selectTickets(slug, {}, ordered);
    if (!ticket) return;
    presentTicketReaderDialog('workspace-reader', trigger, () => {
      readerOpen.value = true;
      if (readerTab.value === 'code-review' && !codeReviewLoading.value) void refreshCodeReview();
    });
  }
  function closeNotWorking(clearStored = true) {
    const ids = notWorkingFiles.value.map((item) => item.id),
      scope = draftScope('not-working', notWorkingTarget.value.projectId);
    notWorkingTarget.value = CLOSED_NOT_WORKING_TARGET;
    notWorkingNote.value = '';
    notWorkingFiles.value = [];
    notWorkingSubmitting.value = false;
    notWorkingError.value = '';
    if (clearStored) void deleteDraftFiles(scope, ids);
    scheduleProjectSessionPersistence();
  }
  function presentNotWorkingDialog() {
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        const dialog = document.querySelector<Control>('[data-component="not-working-dialog"]'),
          native = dialog?.shadowRoot?.querySelector<HTMLDialogElement>('dialog');
        if (!native?.open) dialog?.show?.();
        dialog?.querySelector<HTMLTextAreaElement>('[name="not-working-note"]')?.focus();
      }),
    );
  }
  function openNotWorking(ticket: WireTicketRow, mode: NotWorkingTarget['mode'] = 'not-working') {
    const current = project();
    if (!current) return;
    notWorkingTarget.value = {
      projectId: current.id,
      apiPath: current.apiPath,
      ticketId: ticket.id,
      slug: ticket.slug,
      connectionId: ticket.connection_id,
      mode,
    };
    notWorkingNote.value = '';
    notWorkingFiles.value = [];
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
      void Promise.allSettled(
        projects.value.map(async (project) => ({
          project,
          result: await new Api(project.apiPath).checkoutTickets(project.id, {
            text: query,
            compact: true,
            limit: 500,
          }),
        })),
      ).then((results) => {
        const active = ticketCloseDialog.value;
        if (
          generation !== ticketCloseSearchGeneration ||
          !active ||
          duplicateTargetKey(active.source) !== duplicateTargetKey(state.source)
        )
          return;
        const candidates = new Map<string, DuplicateTarget>(),
          errors: string[] = [];
        for (const result of results) {
          if (result.status === 'rejected') {
            errors.push(result.reason instanceof Error ? result.reason.message : String(result.reason));
            continue;
          }
          if (result.value.result.partial || result.value.result.truncated)
            errors.push(`Search results may be incomplete for ${result.value.project.name}.`);
          for (const row of result.value.result.items) {
            const candidate = duplicateTarget(result.value.project, row);
            candidates.set(duplicateTargetKey(candidate), candidate);
          }
        }
        ticketCloseDialog.value = {
          ...active,
          candidates: [...candidates.values()].slice(0, 20),
          searching: false,
          error: errors.join(' · '),
        };
      });
    }, 150);
  }
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  async function submitTicketClose(){const state=ticketCloseDialog.value,current=projects.value.find(item=>item.id===state?.source.projectId);if(!state||!current||state.submitting)return;const validation=validateTicketClose(state.reason,state.source,state.selected);if(validation){ticketCloseDialog.value={...state,error:validation};return}ticketCloseDialog.value={...state,submitting:true,error:''};try{const result=await new Api(current.apiPath).closeCheckoutTicket(current.id,state.source.qualifiedId,state.reason,state.selected?duplicateReference(state.selected):undefined);if(project()?.id!==current.id)return;await refreshProject();presentTicket(result.ticket);closeTicketCloseDialog();showToast(state.reason==='duplicate'?`${state.source.slug} marked as a duplicate of ${state.selected!.slug}.`:`${state.source.slug} closed as ${ticketCloseReasonLabel(state.reason)?.toLowerCase()??state.reason}.`)}catch(reason){const active=ticketCloseDialog.value;if(active)ticketCloseDialog.value={...active,submitting:false,error:reason instanceof Error?reason.message:String(reason)}}}
  async function openDuplicateTarget(id: string) {
    const reference = parseDuplicateReference(id);
    if (reference) {
      const target = projects.value.find((item) => item.id === reference.project_id);
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
            },
          ]
        : [],
    );
    const unique = new Map(matches.map((match) => [ticketLinkMatchKey(match), match]));
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
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  async function addNotWorkingFiles(files:FileList|File[]){const target=notWorkingTarget.value;if(!target||!(capabilitiesFor(target.connectionId)?.attachments??true))return;const screened=await screenAttachmentFiles(Array.from(files)),pending=screened.readable.map(file=>({id:browserRandomId(),name:file.name,file}));notWorkingFiles.value=[...notWorkingFiles.value,...pending];await Promise.all(pending.map(item=>saveDraftFile(draftScope('not-working',target.projectId),item.id,item.file))).catch(()=>undefined);notWorkingError.value=describeUnreadableAttachments(screened.unreadable);scheduleProjectSessionPersistence();presentNotWorkingDialog()}
  function openTicketComposer(trigger?: HTMLElement) {
    if (composerExpanded.value) return;
    if (trigger && document.activeElement !== trigger) trigger.focus({ preventScroll: true });
    composerExpanded.value = true;
    scheduleProjectSessionPersistence();
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        showQuickTicketComposer(document);
        focusQuickTicketComposerTitle(document);
      }),
    );
  }
  function resetTicketComposer(clearStored = true) {
    const ids = composerAttachments.value.map((item) => item.id),
      scope = draftScope('composer');
    composerAttachmentEpoch += 1;
    activeComposerScreenings.clear();
    composerScreening.value = false;
    composerSubmitting.value = false;
    composerExpanded.value = false;
    composerTitle.value = '';
    composerDetails.value = '';
    composerUpNext.value = false;
    composerSource.value = undefined;
    composerAttachments.value = [];
    composerAttachmentMessage.value = '';
    composerAttachmentError.value = false;
    if (clearStored) void deleteDraftFiles(scope, ids);
    scheduleProjectSessionPersistence();
  }
  async function addNewTicketFiles(files: FileList | File[]) {
    const origin = project(),
      epoch = composerAttachmentEpoch,
      screening = Symbol();
    openTicketComposer();
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
      await Promise.all(
        pending.map((item) => saveDraftFile(draftScope('composer', origin.id), item.id, item.file)),
      ).catch(() => undefined);
      const warning = describeUnreadableAttachments(screened.unreadable);
      composerAttachmentMessage.value = warning;
      composerAttachmentError.value = Boolean(warning);
      scheduleProjectSessionPersistence();
      requestAnimationFrame(() => requestAnimationFrame(() => focusQuickTicketComposerTitle(document)));
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
    const client = new Api(origin.apiPath),
      files = composerAttachments.value.map((item) => item.file),
      batch_id = browserRandomId(),
      placement = newTicketCreationPlacement(selectedView.value, composerUpNext.value),
      finishLocalCreation = beginLocalTicketCreation();
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
              details: composerDetails.value,
              category: composerCategory.value,
              ...placement,
            },
            source,
          );
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
          resetTicketComposer();
          if (project()?.id !== origin.id) return;
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
          window.setTimeout(() => {
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
          }, 300);
        },
      );
      const failure = describeNewTicketAttachmentFailures(result.failed);
      if (project()?.id !== origin.id) return;
      if (selectedTicket.value?.id === result.ticket.id)
        selectedTicket.value = { ...selectedTicket.value, attachments: result.ticket.attachments };
      if (failure) {
        inspectorTab.value = 'attachments';
        attachmentMessage.value = failure;
        error.value = failure;
      }
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : String(reason);
      composerAttachmentMessage.value = `Ticket creation failed: ${message}`;
      composerAttachmentError.value = true;
      composerSubmitting.value = false;
    } finally {
      await finishLocalCreation();
    }
  }
  // prettier-ignore

  async function submitNotWorking(){const target=notWorkingTarget.value;if(!target.slug||notWorkingSubmitting.value)return;const owning=projects.value.find(item=>item.id===target.projectId);if(!owning){notWorkingError.value='The project is no longer open.';return}const client=new Api(target.apiPath),capabilities=capabilitiesFor(target.connectionId),note=(capabilities?.notes??true)?notWorkingNote.value:'',files=[...notWorkingFiles.value],scope=draftScope('not-working',target.projectId);notWorkingSubmitting.value=true;notWorkingError.value='';notWorkingTarget.value=CLOSED_NOT_WORKING_TARGET;try{const current=(await client.checkoutTicket(target.projectId,`${target.connectionId}:${target.ticketId}`)).ticket;let full:FullTicket=current;await submitNotWorkingReport({note,files:files.map(item=>item.file)},{report:async(text,evidence)=>{full=await client.reportNotWorking(target.connectionId,target.ticketId,text,evidence,current.concurrency_token)}});if(project()?.id===owning.id){setProjectTicketRows(owning.id,projectTabTicketRows(owning.id).map(row=>row.id===full.id?ticketRowFromFull(row,full):row));selectedTicketSlugs.value=[target.slug];state.ticketSelectionAnchor=target.slug;presentTicket(full)}notWorkingNote.value='';notWorkingFiles.value=[];notWorkingSubmitting.value=false;void deleteDraftFiles(scope,files.map(item=>item.id));scheduleProjectSessionPersistence()}catch(reason){notWorkingTarget.value=target;notWorkingNote.value=note;notWorkingFiles.value=files;notWorkingError.value=reason instanceof Error?reason.message:String(reason);notWorkingSubmitting.value=false;scheduleProjectSessionPersistence();presentNotWorkingDialog()}}

  return {
    applyTicketPatch,
    restoreTicketDraft,
    flushTicketDrafts,
    updateSelected,
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
    notes,
    attachmentContext,
    duplicateTargetFor,
    resolveDuplicateTargetFor,
    uploadAttachmentWithPoster,
    addAttachments,
    selectionOrder,
    presentTicket,
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
    resetTicketComposer,
    addNewTicketFiles,
    submitNewTicket,
    submitNotWorking,
  };
}
