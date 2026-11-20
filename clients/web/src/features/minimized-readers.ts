import { batch, type Signal } from 'kerfjs';

import type { Capabilities, FullTicket } from '../api';
import type { MinimizedTicketSummary } from '../components/minimized-tickets-control';
import type { InspectorTab } from '../components/ticket-inspector';
import type { Confirm } from '../confirmation';
import type { FeedbackResponseDrafts, FeedbackResponseScope } from '../feedback-response-drafts';
import type { Project } from '../interactions/types';
import {
  addMinimizedReader,
  disposeMinimizedReaders,
  minimizedDraftLabel,
  type MinimizedReader,
  minimizedReaderDrafts,
  type MinimizedReaderField,
  type MinimizedReaderResponse,
  minimizedReadersForTab,
  minimizedReadersHaveDrafts,
  restoredReaderEditState,
  saveMinimizedReaders,
  takeMinimizedReader,
  takeMinimizedReadersForTab,
} from '../minimized-readers';
import {
  animateReaderMinimize,
  animateReaderRestore,
  MINIMIZED_TICKETS_CONTROL_SELECTOR,
  type MotionRect,
  readerDialogRect,
} from '../reader-minimize-motion';
import { clearTicketDraft, saveTicketDraft, ticketDraftKey } from '../ticket-draft-store';
import { reconcileTicketReaderFrame, type TicketReaderEditState, type TicketReaderFrame } from '../ticket-reader-stack';

interface Cancelable {
  cancel(): void;
}

export const WORKSPACE_READER_ID = 'workspace-reader';

/** Live application bindings the minimized-reader workflow reads and writes (HS2-Q8WF64). */
export interface MinimizedReadersDependencies {
  minimizedReaders: Signal<MinimizedReader[]>;
  linkedReaderStack: Signal<TicketReaderFrame[]>;
  linkedReaderAutosaves: Map<string, { details: Cancelable; note: Cancelable; blocked: Cancelable }>;
  selectedProjectId: Signal<string>;
  projects: Signal<Project[]>;
  project: () => Project | undefined;
  selectedTicket: Signal<FullTicket | null>;
  readerOpen: Signal<boolean>;
  readerTab: Signal<InspectorTab>;
  /** The workspace reader's own drafts and edit modes, as a frame would hold them. */
  workspaceReaderEdit: () => TicketReaderEditState;
  /** Return the workspace reader's drafts to their resting state once a frame owns them. */
  resetWorkspaceReader: () => void;
  /** Publish typed text still waiting for a typing pause (HS2-WV0VPE). */
  flushDraftInputs: () => void;
  /** Commit the workspace reader's pending focus-loss saves without leaving its edit modes. */
  flushWorkspaceReader: () => Promise<boolean>;
  flushLinkedReader: (frameId: string) => Promise<boolean>;
  replaceLinkedReaderFrame: (id: string, update: (frame: TicketReaderFrame) => TicketReaderFrame) => void;
  feedbackResponseDrafts: FeedbackResponseDrafts;
  capabilitiesFor: (connectionId: string) => Capabilities | undefined;
  fetchTicket: (owner: Project, qualifiedId: string) => Promise<FullTicket>;
  presentTicketReaderDialog: (id: string, trigger: HTMLElement | undefined, onOpen: () => void) => void;
  readerDialog: (id: string) => Element | null;
  confirm: Confirm;
  showToast: (message: string) => void;
  storage: () => Storage | undefined;
  newId: () => string;
  now?: () => number;
  /** Open the several-ticket menu for a keyboard restore; absent where no menu renders. */
  openMinimizedMenu?: () => void;
  animate?: {
    minimize: (from: MotionRect | undefined) => Promise<void>;
    restore: (to: MotionRect | undefined) => Promise<() => void>;
  };
  afterOpen?: (run: () => void) => void;
  /** Move focus to the minimized tickets control once a reader minimizes. */
  focusControl?: () => void;
}

/** Focus follows the reader into the minimized tickets control, unless something else took it. */
function focusMinimizedControl() {
  if (typeof document === 'undefined') return;
  const active = document.activeElement;
  if (active && active !== document.body && active.isConnected) return;
  const control = document.querySelector<HTMLElement>(MINIMIZED_TICKETS_CONTROL_SELECTOR),
    button = control?.matches('button') ? control : control?.querySelector<HTMLElement>('button');
  button?.focus({ preventScroll: true });
}

function nextFrames(run: () => void) {
  if (typeof requestAnimationFrame === 'function')
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        run();
      }),
    );
  else queueMicrotask(run);
}

/**
 * Minimizing, restoring, and closing ticket readers without losing their drafts (HS2-Q8WF64). A
 * minimized reader leaves the modal reader stack, so nothing is modal while it waits; restoring
 * pushes its frame back on top and opens it modal again. The workspace reader follows the
 * selection, so minimizing it detaches it into a frame of its own first.
 */
export function createMinimizedReaders(dependencies: MinimizedReadersDependencies) {
  const {
    minimizedReaders,
    linkedReaderStack,
    linkedReaderAutosaves,
    selectedProjectId,
    projects,
    project,
    selectedTicket,
    readerOpen,
    readerTab,
    workspaceReaderEdit,
    resetWorkspaceReader,
    flushDraftInputs,
    flushWorkspaceReader,
    flushLinkedReader,
    replaceLinkedReaderFrame,
    feedbackResponseDrafts,
    capabilitiesFor,
    fetchTicket,
    presentTicketReaderDialog,
    readerDialog,
    confirm,
    showToast,
    storage,
    newId,
    now = () => Date.now(),
    openMinimizedMenu,
    animate = { minimize: animateReaderMinimize, restore: animateReaderRestore },
    afterOpen = nextFrames,
    focusControl = focusMinimizedControl,
  } = dependencies;
  const busy = new Set<string>();
  /** Where each frame's reader last showed, so it restores to the same place. */
  const lastRects = new Map<string, MotionRect>();

  function persist() {
    saveMinimizedReaders(storage(), minimizedReaders.value);
  }
  function responseScope(frame: TicketReaderFrame): FeedbackResponseScope {
    return { projectId: frame.projectId, qualifiedId: frame.ticket.qualified_id, surface: frame.id };
  }
  function responseOf(frame: TicketReaderFrame): MinimizedReaderResponse | undefined {
    const owner = feedbackResponseDrafts.get(responseScope(frame));
    if (!owner) return undefined;
    const { id, kind, text, created_at, edited_at } = owner.request;
    return {
      request: { id, kind, text, created_at, edited_at },
      draft: owner.draft ?? frame.edit.noteDraft,
    };
  }
  /** Keep field edits a save could not commit in the ticket draft store, where a reload finds them. */
  function keepUnsavedFields(frame: TicketReaderFrame, fields: readonly MinimizedReaderField[]) {
    const store = storage();
    if (!store) return;
    const { edit } = frame,
      key = (field: MinimizedReaderField, noteId?: string) =>
        ticketDraftKey(frame.projectId, frame.ticket.qualified_id, field, noteId);
    for (const field of fields) {
      if (field === 'details') saveTicketDraft(store, key(field), { base: edit.detailsBase, draft: edit.detailsDraft });
      if (field === 'blocked_reason')
        saveTicketDraft(store, key(field), { base: edit.blockedReasonBase, draft: edit.blockedReasonDraft });
      if (field === 'note' && edit.editingNoteId)
        saveTicketDraft(store, key(field, edit.editingNoteId), { base: edit.noteBase, draft: edit.noteDraft });
    }
  }
  function discardUnsavedFields(entry: MinimizedReader) {
    const store = storage();
    if (!store) return;
    for (const field of entry.drafts.unsavedFields)
      clearTicketDraft(
        store,
        ticketDraftKey(
          entry.projectId,
          entry.qualifiedId,
          field,
          field === 'note' ? entry.frame?.edit.editingNoteId : undefined,
        ),
      );
  }
  function entryFor(frame: TicketReaderFrame, tabProjectId: string): MinimizedReader {
    const drafts = minimizedReaderDrafts(frame.edit, responseOf(frame));
    return {
      id: frame.id,
      tabProjectId,
      projectId: frame.projectId,
      qualifiedId: frame.ticket.qualified_id,
      slug: frame.ticket.slug,
      title: frame.ticket.title,
      category: frame.ticket.category ?? 'issue',
      activeTab: frame.activeTab,
      minimizedAt: now(),
      drafts,
      frame: { ...frame, open: false },
    };
  }
  function detachWorkspaceReader(): TicketReaderFrame | undefined {
    const current = project(),
      ticket = selectedTicket.value,
      capabilities = ticket ? capabilitiesFor(ticket.connection_id) : undefined;
    if (!current || !ticket || !capabilities) return undefined;
    const frame: TicketReaderFrame = {
        id: newId(),
        open: false,
        projectId: current.id,
        projectName: current.name,
        apiPath: current.apiPath,
        ticket,
        activeTab: readerTab.value,
        capabilities,
        edit: workspaceReaderEdit(),
      },
      owner = feedbackResponseDrafts.get({
        projectId: current.id,
        qualifiedId: ticket.qualified_id,
        surface: WORKSPACE_READER_ID,
      });
    // A response in progress moves with the reader: the frame owns it under its own surface (HS2-VBFSZJ).
    if (owner) {
      feedbackResponseDrafts.begin(responseScope(frame), owner.request, owner.draft);
      frame.edit = {
        ...frame.edit,
        editingNoteId: owner.request.id,
        responseRequestId: owner.request.id,
        noteDraft: owner.draft ?? frame.edit.noteDraft,
      };
    }
    return frame;
  }

  /** Take a reader off the modal stack into the minimized list; `false` when there was none to take. */
  async function takeReader(frameId: string, tabProjectId: string): Promise<{ rect?: MotionRect } | false> {
    flushDraftInputs();
    const workspace = frameId === WORKSPACE_READER_ID;
    if (workspace ? !readerOpen.value : !linkedReaderStack.value.some((frame) => frame.id === frameId)) return false;
    // Focus already left the editor for the Minimize control, so its focus-loss save commits now; a
    // failed save keeps its text in the frame and in the ticket draft store (HS2-RE1PS6).
    await (workspace ? flushWorkspaceReader() : flushLinkedReader(frameId));
    const rect = readerDialogRect(readerDialog(frameId));
    let entry: MinimizedReader | undefined;
    batch(() => {
      if (workspace) {
        if (!readerOpen.value) return;
        const frame = detachWorkspaceReader();
        if (!frame) return;
        entry = entryFor(frame, tabProjectId);
        // The list owns the moved response before the reader closes and the registry is pruned.
        minimizedReaders.value = addMinimizedReader(minimizedReaders.value, entry);
        const previous = feedbackResponseDrafts.get({
          projectId: frame.projectId,
          qualifiedId: frame.ticket.qualified_id,
          surface: WORKSPACE_READER_ID,
        });
        feedbackResponseDrafts.clear(previous);
        resetWorkspaceReader();
        readerOpen.value = false;
        return;
      }
      const frame = linkedReaderStack.value.find((item) => item.id === frameId);
      if (!frame) return;
      entry = entryFor(frame, tabProjectId);
      minimizedReaders.value = addMinimizedReader(minimizedReaders.value, entry);
      const saves = linkedReaderAutosaves.get(frameId);
      saves?.details.cancel();
      saves?.note.cancel();
      saves?.blocked.cancel();
      linkedReaderAutosaves.delete(frameId);
      linkedReaderStack.value = linkedReaderStack.value.filter((item) => item.id !== frameId);
    });
    const minimized: MinimizedReader | undefined = entry;
    if (!minimized?.frame) return false;
    if (rect) lastRects.set(minimized.id, rect);
    keepUnsavedFields(minimized.frame, minimized.drafts.unsavedFields);
    persist();
    return { rect };
  }

  /** Minimize the reader `frameId` (the workspace reader or a linked frame), keeping all its drafts. */
  async function minimize(frameId: string): Promise<boolean> {
    const tabProjectId = selectedProjectId.value;
    if (!tabProjectId || busy.has(frameId)) return false;
    busy.add(frameId);
    const taken = await takeReader(frameId, tabProjectId).finally(() => {
      busy.delete(frameId);
    });
    // The reader is already minimized; it can be restored while the stand-in still flies.
    if (!taken) return false;
    afterOpen(focusControl);
    await animate.minimize(taken.rect);
    return true;
  }

  async function rebuildFrame(entry: MinimizedReader): Promise<TicketReaderFrame | undefined> {
    const owner = projects.value.find((item) => item.id === entry.projectId);
    if (!owner) {
      showToast(`Open the project that owns ${entry.slug} to restore it.`);
      return undefined;
    }
    let ticket: FullTicket;
    try {
      ticket = await fetchTicket(owner, entry.qualifiedId);
    } catch (reason) {
      showToast(`Could not restore ${entry.slug}: ${reason instanceof Error ? reason.message : String(reason)}`);
      return undefined;
    }
    const capabilities = capabilitiesFor(ticket.connection_id);
    if (!capabilities) {
      showToast(`Could not restore ${entry.slug}: its source is unavailable.`);
      return undefined;
    }
    const frame: TicketReaderFrame = {
      id: entry.id,
      open: false,
      projectId: owner.id,
      projectName: owner.name,
      apiPath: owner.apiPath,
      ticket,
      activeTab: entry.activeTab,
      capabilities,
      edit: restoredReaderEditState(ticket, entry.drafts),
    };
    const response = entry.drafts.response;
    if (response) feedbackResponseDrafts.begin(responseScope(frame), { ...response.request }, response.draft);
    return frame;
  }
  /** Bring a live frame's ticket up to date after it waited minimized, keeping its drafts (HS2-A4XCXE). */
  function refreshRestored(frame: TicketReaderFrame) {
    const owner = projects.value.find((item) => item.id === frame.projectId);
    if (!owner) return;
    void fetchTicket(owner, frame.ticket.qualified_id)
      .then((ticket) => {
        flushDraftInputs();
        replaceLinkedReaderFrame(frame.id, (current) => reconcileTicketReaderFrame(current, ticket));
      })
      .catch(() => undefined);
  }

  /** Restore minimized readers of the current tab, oldest first, so the last one is on top. */
  async function restoreEntries(ids: readonly string[]): Promise<boolean> {
    const pending = ids.filter((id) => !busy.has(id));
    if (pending.length === 0) return false;
    for (const id of pending) busy.add(id);
    try {
      const restored: { entry: MinimizedReader; frame: TicketReaderFrame; live: boolean }[] = [];
      for (const id of pending) {
        const entry = minimizedReaders.value.find((item) => item.id === id);
        if (!entry) continue;
        const frame = entry.frame ?? (await rebuildFrame(entry));
        if (frame) restored.push({ entry, frame, live: Boolean(entry.frame) });
      }
      const top = restored.at(-1);
      if (!top) return false;
      const release = await animate.restore(lastRects.get(top.entry.id));
      batch(() => {
        for (const { entry, frame } of restored) {
          minimizedReaders.value = takeMinimizedReader(minimizedReaders.value, entry.id).list;
          linkedReaderStack.value = [
            ...linkedReaderStack.value.filter((item) => item.id !== frame.id),
            { ...frame, open: false },
          ];
        }
      });
      persist();
      presentTicketReaderDialog(top.frame.id, undefined, () => {
        batch(() => {
          for (const { frame } of restored)
            replaceLinkedReaderFrame(frame.id, (current) => ({ ...current, open: true }));
        });
      });
      afterOpen(release);
      for (const { frame, live } of restored) if (live) refreshRestored(frame);
      return true;
    } finally {
      for (const id of pending) busy.delete(id);
    }
  }

  function restore(id: string) {
    return restoreEntries([id]);
  }
  function restoreAll() {
    return restoreEntries(
      minimizedReadersForTab(minimizedReaders.value, selectedProjectId.value).map((item) => item.id),
    );
  }

  /** Close every minimized reader of the current tab, asking first only when a draft would be lost. */
  async function closeAll(): Promise<boolean> {
    const tabProjectId = selectedProjectId.value,
      entries = minimizedReadersForTab(minimizedReaders.value, tabProjectId);
    if (entries.length === 0) return false;
    if (minimizedReadersHaveDrafts(entries)) {
      const drafted = entries.filter((entry) => minimizedDraftLabel(entry.drafts) !== undefined).length;
      const confirmed = await confirm({
        title: `Close ${entries.length} minimized tickets?`,
        message: `${drafted === 1 ? 'One of them has' : `${drafted} of them have`} unsent text that will be discarded.`,
        confirmLabel: 'Close All',
        tone: 'danger',
      });
      if (!confirmed) return false;
    }
    const taken = takeMinimizedReadersForTab(minimizedReaders.value, tabProjectId),
      closing = new Set(entries.map((entry) => entry.id));
    for (const entry of taken.entries) {
      if (!closing.has(entry.id)) continue;
      discardUnsavedFields(entry);
      feedbackResponseDrafts.clear(
        feedbackResponseDrafts.get({ projectId: entry.projectId, qualifiedId: entry.qualifiedId, surface: entry.id }),
      );
      lastRects.delete(entry.id);
    }
    // Readers minimized while the question was open stay minimized.
    minimizedReaders.value = minimizedReaders.value.filter((entry) => !closing.has(entry.id));
    persist();
    return true;
  }

  /** The keyboard command: minimize the active reader, or restore this tab's minimized ticket(s). */
  function toggle(activeReaderId: string | undefined) {
    if (activeReaderId) {
      void minimize(activeReaderId);
      return;
    }
    const entries = minimizedReadersForTab(minimizedReaders.value, selectedProjectId.value);
    if (entries.length === 1) void restore(entries[0].id);
    else if (entries.length > 1) openMinimizedMenu?.();
  }

  /** Closing project tabs drops their minimized readers (HS2-BSSWGV teardown). */
  function disposeProjects(ids: readonly string[]) {
    const result = disposeMinimizedReaders(minimizedReaders.value, new Set(ids));
    if (result.disposed.length === 0) return;
    for (const entry of result.disposed) {
      lastRects.delete(entry.id);
      feedbackResponseDrafts.clear(
        feedbackResponseDrafts.get({ projectId: entry.projectId, qualifiedId: entry.qualifiedId, surface: entry.id }),
      );
    }
    minimizedReaders.value = result.retained;
    persist();
  }

  /** Response scopes minimized readers keep alive while the reader registry prunes closed surfaces. */
  function retainedResponseScopes(): FeedbackResponseScope[] {
    return minimizedReaders.value.map((entry) => ({
      projectId: entry.projectId,
      qualifiedId: entry.qualifiedId,
      surface: entry.id,
    }));
  }

  /** The current tab's minimized tickets as the floating control lists them, oldest first. */
  function summaries(): MinimizedTicketSummary[] {
    return minimizedReadersForTab(minimizedReaders.value, selectedProjectId.value).map((entry) => ({
      id: entry.id,
      slug: entry.slug,
      title: entry.title,
      category: entry.category,
      draft: minimizedDraftLabel(entry.drafts),
    }));
  }

  return { minimize, restore, restoreAll, closeAll, toggle, disposeProjects, retainedResponseScopes, summaries };
}

export type MinimizedReadersController = ReturnType<typeof createMinimizedReaders>;
