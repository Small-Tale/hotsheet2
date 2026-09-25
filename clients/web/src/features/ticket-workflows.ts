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
import type { BulkTicketDialogState } from '../components/bulk-ticket-dialog';
import type { MarkdownEditorMode } from '../components/markdown-editor';
import { focusQuickTicketComposerTitle, showQuickTicketComposer } from '../components/quick-ticket-composer';
import type { TicketCloseDialogState } from '../components/ticket-close-dialog';
import type { InspectorTab } from '../components/ticket-inspector';
import type { TicketLinkChoice } from '../components/ticket-link-choice-dialog';
import { isPlainTicketReselection, updateTicketSelection } from '../components/ticket-selection';
import { createDebouncedAutosave, type DebouncedAutosave } from '../debounced-autosave';
import { presentedNoteKind } from '../feedback-needed';
import type { InlineFeedbackReply } from '../feedback-replies';
import { beginInteractionTiming } from '../interaction-performance';
import { data } from '../interactions/dom';
import type { Control, NotWorkingTarget, PendingEvidence, Project } from '../interactions/types';
import type { LocalTicketChangeAcknowledgements } from '../local-ticket-changes';
import { createTicketWithAttachments, describeNewTicketAttachmentFailures } from '../new-ticket-attachments';
import { submitNotWorkingReport } from '../not-working-workflow';
import { type PendingCreatedTickets, prependCreatedTicketRow } from '../pending-created-tickets';
import {
  bulkTagChoices,
  type BulkTicketAction,
  type BulkTicketMutationSequencer,
  bulkTicketPatch,
  canAtomicallyBulkUpdate,
  canBulkUpdate,
} from '../ticket-bulk-operations';
import {
  duplicateReference,
  type DuplicateTarget,
  duplicateTargetKey,
  parseDuplicateReference,
  resolveDuplicateReferenceTarget,
  validateTicketClose,
} from '../ticket-close';
import {
  isTicketConcurrencyConflict,
  reconcileTicketPatch,
  type TicketFieldConflict,
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
import { reconcileTicketReaderFrame, type TicketReaderFrame, updateTicketReaderFrame } from '../ticket-reader-stack';
import { ticketTimelineEntries } from '../ticket-timeline-data';
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
  titleEditing: Signal<boolean>;
  titleDraft: Signal<string>;
  blockedReasonEditing: Signal<boolean>;
  blockedReasonDraft: Signal<string>;
  readerBlockedReasonEditing: Signal<boolean>;
  readerBlockedReasonDraft: Signal<string>;
  editingNoteId: Signal<string | undefined>;
  readerEditingNoteId: Signal<string | undefined>;
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
  defaultProvider: () => { name: string; capabilities: Capabilities } | undefined;
  capabilitiesFor: (connectionId: string) => Capabilities | undefined;
  canUseAttachments: () => boolean;
  canStageNewTicketAttachments: () => boolean;
  ticketSnapshot: (slug: string) => TicketSnapshot | undefined;
  visibleTickets: () => WireTicketRow[];
  projectTabTicketRows: (projectId: string) => WireTicketRow[];
  projectTicketCounts: (projectId: string) => CheckoutTicketCounts;
  publishOptimisticTicketRows: (projectId: string) => void;
  beginLocalTicketMutation: () => () => void;
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
    titleEditing,
    titleDraft,
    blockedReasonEditing,
    blockedReasonDraft,
    readerBlockedReasonEditing,
    readerBlockedReasonDraft,
    editingNoteId,
    readerEditingNoteId,
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
    capabilitiesFor,
    canUseAttachments,
    canStageNewTicketAttachments,
    ticketSnapshot,
    visibleTickets,
    projectTabTicketRows,
    projectTicketCounts,
    publishOptimisticTicketRows,
    beginLocalTicketMutation,
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
    activeTicketSurface,
    draftScope,
    ago,
  } = dependencies;
  let ticketCloseSearchGeneration = 0,
    ticketCloseSearchTimer: number | undefined,
    composerAttachmentEpoch = 0;
  const activeComposerScreenings = new Set<symbol>();
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
        const base =
          committedTickets.get(slug) ?? selectedBefore ?? (await api().checkoutTicket(current.id, ticket.id)).ticket;
        let updated: FullTicket;
        try {
          updated = (
            await api().updateCheckoutTicket(
              current.id,
              ticket.id,
              base.concurrency_token ? { ...patch, expected_token: base.concurrency_token } : patch,
            )
          ).ticket;
          localTicketChangeAcknowledgements.acknowledge(current.id, {
            store: updated.connection_id,
            id: updated.id,
            kind: 'updated',
          });
        } catch (reason) {
          if (!isTicketConcurrencyConflict(reason)) throw reason;
          const remote = (await api().checkoutTicket(current.id, ticket.id)).ticket,
            reconciled = reconcileTicketPatch(base, remote, patch);
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
          if (Object.keys(reconciled.retry).length === 0) updated = remote;
          else {
            updated = (
              await api().updateCheckoutTicket(
                current.id,
                ticket.id,
                remote.concurrency_token
                  ? { ...reconciled.retry, expected_token: remote.concurrency_token }
                  : reconciled.retry,
              )
            ).ticket;
            localTicketChangeAcknowledgements.acknowledge(current.id, {
              store: updated.connection_id,
              id: updated.id,
              kind: 'updated',
            });
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
        recordCommittedDraftBases(patch);
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
    if (typeof patch.title === 'string' && titleEditing.value) state.titleDraftBase = patch.title;
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
  const detailsAutosave = createDebouncedAutosave((value: string) => updateSelectedTracked({ details: value }));
  const readerDetailsAutosave = createDebouncedAutosave((value: string) => updateSelectedTracked({ details: value }));
  const noteAutosave = createDebouncedAutosave(({ id, value }: { id: string; value: string }) =>
    updateSelected({ note_id: id, note: value }),
  );
  const readerNoteAutosave = createDebouncedAutosave(({ id, value }: { id: string; value: string }) =>
    updateSelected({ note_id: id, note: value }),
  );
  const blockedReasonAutosave = createDebouncedAutosave((value: string) =>
    updateSelected({ blocked_reason: value.trim() || null }),
  );
  const readerBlockedReasonAutosave = createDebouncedAutosave((value: string) =>
    updateSelected({ blocked_reason: value.trim() || null }),
  );
  const titleAutosave = createDebouncedAutosave((value: string) => updateSelectedTracked({ title: value.trim() }));
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
    try {
      const ticket = (
        await client.updateCheckoutTicket(
          frame.projectId,
          frame.ticket.id,
          frame.ticket.concurrency_token ? { ...patch, expected_token: frame.ticket.concurrency_token } : patch,
        )
      ).ticket;
      replaceLinkedReaderFrame(frameId, (current) => reconcileTicketReaderFrame(current, ticket));
      return true;
    } catch (reason) {
      if (isTicketConcurrencyConflict(reason)) {
        const ticket = (await client.checkoutTicket(frame.projectId, frame.ticket.id)).ticket;
        replaceLinkedReaderFrame(frameId, (current) => reconcileTicketReaderFrame(current, ticket));
        error.value = 'This linked ticket changed remotely. Your draft was preserved; review it and try again.';
      } else error.value = reason instanceof Error ? reason.message : String(reason);
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
        details: createDebouncedAutosave((value) => updateLinkedReader(id, { details: value })),
        note: createDebouncedAutosave(({ id: noteId, value }) =>
          updateLinkedReader(id, { note_id: noteId, note: value }),
        ),
        blocked: createDebouncedAutosave((value) => updateLinkedReader(id, { blocked_reason: value.trim() || null })),
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
  function setProjectTicketRows(projectId: string, rows: WireTicketRow[]) {
    if (!projects.value.some((item) => item.id === projectId)) return;
    if (project()?.id === projectId) tickets.value = rows;
    ticketRowsByProject.value = { ...ticketRowsByProject.value, [projectId]: rows };
    ticketCountsByProject.value = Object.fromEntries(
      Object.entries(ticketCountsByProject.value).filter(([id]) => id !== projectId),
    );
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
      selectedBefore = project()?.id === current.id ? selectedTicket.value : null;
    setProjectTicketRows(
      current.id,
      projectTabTicketRows(current.id).map((ticket) => {
        const operation = operations.find((item) => item.slug === ticket.slug);
        return operation ? projectTicketPatch(ticket, operation.patch) : ticket;
      }),
    );
    if (project()?.id === current.id && selectedTicket.value) {
      const operation = operations.find((item) => item.slug === selectedTicket.value?.slug);
      if (operation) selectedTicket.value = projectTicketPatch(selectedTicket.value, operation.patch);
    }
    finishTiming();
    try {
      const client = new Api(current.apiPath),
        requestOperations = operations.map((operation) => {
          const ticket = before.find((item) => item.slug === operation.slug),
            token = selectedBefore?.slug === operation.slug ? selectedBefore.concurrency_token : ticket?.updated_at;
          return { ...operation, patch: token ? { ...operation.patch, expected_token: token } : operation.patch };
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
        showToast(`Updating tickets… 0 of ${requestOperations.length}`);
        for (const operation of requestOperations) {
          try {
            updated.push((await client.updateCheckoutTicket(current.id, operation.id, operation.patch)).ticket);
          } catch (reason) {
            failures.push({ slug: operation.slug, message: reason instanceof Error ? reason.message : String(reason) });
          }
          showToast(`Updating tickets… ${updated.length + failures.length} of ${requestOperations.length}`);
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
      setProjectTicketRows(current.id, next);
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
      );
      if (project()?.id === current.id && selectedBefore && slugs.has(selectedBefore.slug))
        selectedTicket.value = selectedBefore;
      reportBulkFailure(current, reason instanceof Error ? reason.message : String(reason));
      return { complete: false, succeeded: new Set<string>() };
    } finally {
      releaseRefresh();
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
      for (const row of rows) await api.restoreCheckoutTicket(current.id, row.id);
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
      notes: full?.notes.map((note) => ({ kind: note.kind, text: note.text, summary: note.summary })) ?? [],
      attachments: full?.attachments.map((item) => ({ id: item.id, filename: item.filename })) ?? [],
    };
  }
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  function copySelection(cut:boolean){const current=project(),rows=selectedRows();if(!current||!rows.length)return;const snapshot={tickets:rows.map(ticket=>clipboardTicket(ticket,selectedTicket.value?.id===ticket.id?selectedTicket.value:undefined)),cut,source:current};state.clipboard=snapshot;selectedTicketSlugs.value=[...selectedTicketSlugs.value];void Promise.all(rows.map(ticket=>api().checkoutTicket(current.id,ticket.id).then(value=>value.ticket))).then(full=>{if(state.clipboard!==snapshot)return;snapshot.tickets=full.map(ticket=>clipboardTicket(ticket));const text=full.map(ticket=>[`${ticket.slug}: ${ticket.title}`,ticket.details,...ticket.notes.map(note=>`- ${note.text}`)].filter(Boolean).join('\n\n')).join('\n\n');void navigator.clipboard?.writeText(text).catch(()=>undefined)}).catch((reason:unknown)=>{error.value=reason instanceof Error?reason.message:String(reason)})}
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
        created.push({ id: ticket.id, slug: ticket.slug, status: ticket.status ?? 'not_started' });
        for (const note of source.notes)
          ticket = (
            await destinationApi.updateCheckoutTicket(current.id, ticket.id, {
              note: note.text,
              note_kind: note.kind,
              note_summary: note.summary,
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
          await sourceApi.updateCheckoutTicket(snapshot.source.id, source.id, { status: 'deleted' });
        state.clipboard = undefined;
      }
      const originals = snapshot.tickets.map((ticket) => ({ id: ticket.id, status: ticket.status ?? 'not_started' }));
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
              sourceApi.checkoutTicket(drag.source.id, ticket.id).then((result) => result.ticket),
            ),
          ),
          // De-duplicate against every destination title, not just loaded rows, via
          // bounded pages instead of one whole-checkout response (HS2-CYXS0N).
          destinationApi.checkoutTicketRowsPaged(destination.id, { fields: 'title' }),
        ]),
        titles = destinationRows.map((ticket) => ticket.title);
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
        created.push({ id: ticket.id, slug: ticket.slug, status: ticket.status ?? 'not_started' });
        for (const note of source.notes.filter((note) => note.kind !== 'activity'))
          ticket = (
            await destinationApi.updateCheckoutTicket(destination.id, ticket.id, {
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
      const aiAuthored = note.text.includes('hotsheet:activity-distillation:v1:');
      return {
        id: note.id,
        kind: presentedNoteKind(note, ticket.notes),
        author: aiAuthored ? 'Hot Sheet AI' : 'Hot Sheet',
        time: ago(note.created_at),
        body: note.text,
        aiAuthored,
        aiTool: aiAuthored ? 'Hot Sheet AI' : undefined,
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
      result = await client.addCheckoutAttachment(current.id, ticket.id, file, metadata),
      added = result.ticket.attachments.find((item) => !previousIds.has(item.id));
    if (added && isVideoAttachment(file.name)) {
      const posterUrl = client.checkoutAttachmentThumbnailUrl(current.id, ticket.id, added.id);
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
      titleEditing.value = false;
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
        .checkoutTicket(current.id, ticket.id)
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
          rows: await new Api(project.apiPath).checkoutTickets(project.id, { text: query, compact: true, limit: 500 }),
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
          for (const row of result.value.rows) {
            const candidate = duplicateTarget(result.value.project, row);
            candidates.set(duplicateTargetKey(candidate), candidate);
          }
        }
        ticketCloseDialog.value = {
          ...active,
          candidates: [...candidates.values()].slice(0, 20),
          searching: false,
          error: candidates.size || !errors.length ? '' : errors[0],
        };
      });
    }, 150);
  }
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  async function submitTicketClose(){const state=ticketCloseDialog.value,current=projects.value.find(item=>item.id===state?.source.projectId);if(!state||!current||state.submitting)return;const validation=validateTicketClose(state.reason,state.source,state.selected);if(validation){ticketCloseDialog.value={...state,error:validation};return}ticketCloseDialog.value={...state,submitting:true,error:''};try{const result=await new Api(current.apiPath).closeCheckoutTicket(current.id,state.source.qualifiedId,state.reason,state.selected?duplicateReference(state.selected):undefined);if(project()?.id!==current.id)return;await refreshProject();presentTicket(result.ticket);closeTicketCloseDialog();showToast(state.reason==='duplicate'?`${state.source.slug} marked as a duplicate of ${state.selected!.slug}.`:`${state.source.slug} closed as ${state.reason.replace('_',' ')}.`)}catch(reason){const active=ticketCloseDialog.value;if(active)ticketCloseDialog.value={...active,submitting:false,error:reason instanceof Error?reason.message:String(reason)}}}
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
      composerAttachmentMessage.value = 'The default ticket provider cannot create tickets with attachments.';
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
      provider = defaultProvider(),
      title = composerTitle.value.trim();
    if (
      !origin ||
      !title ||
      composerScreening.value ||
      composerSubmitting.value ||
      !(provider?.capabilities.create ?? true)
    )
      return;
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
          const created = await client.createCheckoutTicket(origin.id, {
            title,
            details: composerDetails.value,
            category: composerCategory.value,
            ...placement,
          });
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

  async function submitNotWorking(){const target=notWorkingTarget.value;if(!target.slug||notWorkingSubmitting.value)return;const owning=projects.value.find(item=>item.id===target.projectId);if(!owning){notWorkingError.value='The project is no longer open.';return}const client=new Api(target.apiPath),capabilities=capabilitiesFor(target.connectionId),note=(capabilities?.notes??true)?notWorkingNote.value:'',files=[...notWorkingFiles.value],scope=draftScope('not-working',target.projectId);notWorkingSubmitting.value=true;notWorkingError.value='';notWorkingTarget.value=CLOSED_NOT_WORKING_TARGET;try{const current=(await client.checkoutTicket(target.projectId,target.ticketId)).ticket;let full:FullTicket=current;await submitNotWorkingReport({note,files:files.map(item=>item.file)},{report:async(text,evidence)=>{full=await client.reportNotWorking(target.connectionId,target.ticketId,text,evidence,current.concurrency_token)}});if(project()?.id===owning.id){setProjectTicketRows(owning.id,projectTabTicketRows(owning.id).map(row=>row.id===full.id?ticketRowFromFull(row,full):row));selectedTicketSlugs.value=[target.slug];state.ticketSelectionAnchor=target.slug;presentTicket(full)}notWorkingNote.value='';notWorkingFiles.value=[];notWorkingSubmitting.value=false;void deleteDraftFiles(scope,files.map(item=>item.id));scheduleProjectSessionPersistence()}catch(reason){notWorkingTarget.value=target;notWorkingNote.value=note;notWorkingFiles.value=files;notWorkingError.value=reason instanceof Error?reason.message:String(reason);notWorkingSubmitting.value=false;scheduleProjectSessionPersistence();presentNotWorkingDialog()}}

  return {
    applyTicketPatch,
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
