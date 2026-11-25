import { batch, type Signal } from 'kerfjs';
import { Plus } from 'lucide';

import {
  addMinimizedComposer,
  type ComposerDraftSnapshot,
  composerHasDraft,
  type MinimizedComposer,
  minimizedComposerDraftLabel,
  minimizedComposerId,
  minimizedComposerLabel,
  saveMinimizedComposers,
  withoutMinimizedComposers,
} from '../minimized-composer';
import type { MinimizedDialogItem, MinimizedDialogKind } from '../minimized-dialogs';
import {
  animateReaderMinimize,
  animateReaderRestore,
  type MotionRect,
  readerDialogRect,
} from '../reader-minimize-motion';
import { focusMinimizedControl, nextFrames } from './minimized-dialogs';

/** The minimized dialog kind id of the new-ticket composer (HS2-3JR0ZH). */
export const MINIMIZED_COMPOSER_KIND = 'new-ticket';

/** Live application bindings the minimized composer reads and writes (HS2-3JR0ZH). */
export interface MinimizedComposerDependencies {
  minimizedComposers: Signal<MinimizedComposer[]>;
  selectedProjectId: Signal<string>;
  /** The composer's open state; minimizing hides it without resetting the draft. */
  composerExpanded: Signal<boolean>;
  /** True while a Create is in flight; the composer cannot minimize then. */
  submitting: () => boolean;
  draft: () => ComposerDraftSnapshot;
  /** Publish typed text still waiting for a typing pause (HS2-WV0VPE). */
  flushDraftInputs: () => void;
  /** Open the composer with its kept draft (the workflow's `openTicketComposer`). */
  openComposer: () => void;
  /** Close the composer and discard its draft and staged files (`resetTicketComposer`). */
  discardComposer: () => void;
  /** Save the project session now, so a reload keeps the hidden draft. */
  persistSession: () => void;
  /** The composer's visible panel, for the minimize and restore motion. */
  composerPanel: () => Element | null;
  /** Settles once a composer that is still closing has hidden, before it reopens. */
  composerHidden?: () => Promise<void>;
  storage: () => Storage | undefined;
  now?: () => number;
  animate?: {
    minimize: (from: MotionRect | undefined) => Promise<void>;
    restore: (to: MotionRect | undefined) => Promise<() => void>;
  };
  afterOpen?: (run: () => void) => void;
  focusControl?: () => void;
}

/**
 * Minimizing the new-ticket composer into the Minimized Tickets control (HS2-3JR0ZH). Minimizing
 * only hides the composer: its draft signals, project session, and staged files stay as they were,
 * so restoring is opening the composer again. Opening the composer any other way (New ticket…, a
 * file drop, a follow-up) also takes it off the list.
 */
export function createMinimizedComposer(dependencies: MinimizedComposerDependencies) {
  const {
    minimizedComposers,
    selectedProjectId,
    composerExpanded,
    submitting,
    draft,
    flushDraftInputs,
    openComposer,
    discardComposer,
    persistSession,
    composerPanel,
    composerHidden = () => Promise.resolve(),
    storage,
    now = () => Date.now(),
    animate = { minimize: animateReaderMinimize, restore: animateReaderRestore },
    afterOpen = nextFrames,
    focusControl = focusMinimizedControl,
  } = dependencies;
  const lastRects = new Map<string, MotionRect>();

  function persist() {
    saveMinimizedComposers(storage(), minimizedComposers.value);
  }
  function remove(predicate: (entry: MinimizedComposer) => boolean) {
    const next = minimizedComposers.value.filter((entry) => !predicate(entry));
    if (next.length === minimizedComposers.value.length) return;
    minimizedComposers.value = next;
    persist();
  }

  /** Hide the open composer into the minimized list, keeping its draft; `false` when it cannot. */
  async function minimize(): Promise<boolean> {
    const tabProjectId = selectedProjectId.value;
    if (!tabProjectId || !composerExpanded.value || submitting()) return false;
    flushDraftInputs();
    const snapshot = draft(),
      rect = readerDialogRect(composerPanel()),
      entry: MinimizedComposer = {
        id: minimizedComposerId(tabProjectId),
        tabProjectId,
        title: snapshot.title,
        hasDraft: composerHasDraft(snapshot),
        minimizedAt: now(),
      };
    // Closing while not expanded is not a dismissal, so the dialog's hide keeps the draft.
    batch(() => {
      minimizedComposers.value = addMinimizedComposer(minimizedComposers.value, entry);
      composerExpanded.value = false;
    });
    if (rect) lastRects.set(entry.id, rect);
    persist();
    persistSession();
    afterOpen(focusControl);
    await animate.minimize(rect);
    return true;
  }

  /** Reopen the current tab's minimized composer with its draft. */
  async function restore(ids: readonly string[]): Promise<boolean> {
    const tabProjectId = selectedProjectId.value,
      entry = minimizedComposers.value.find((item) => ids.includes(item.id) && item.tabProjectId === tabProjectId);
    if (!entry) return false;
    const [release] = await Promise.all([animate.restore(lastRects.get(entry.id)), composerHidden()]);
    lastRects.delete(entry.id);
    remove((item) => item.id === entry.id);
    openComposer();
    afterOpen(release);
    return true;
  }

  /** Close the current tab's minimized composer and discard its draft and staged files. */
  function discard(ids: readonly string[]) {
    const tabProjectId = selectedProjectId.value,
      closing = minimizedComposers.value.filter((item) => ids.includes(item.id));
    if (closing.length === 0) return;
    for (const entry of closing) lastRects.delete(entry.id);
    remove((item) => ids.includes(item.id));
    if (closing.some((item) => item.tabProjectId === tabProjectId) && !composerExpanded.value) discardComposer();
  }

  function dispose(projectIds: readonly string[]) {
    const closing = new Set(projectIds);
    for (const id of projectIds) lastRects.delete(minimizedComposerId(id));
    const next = withoutMinimizedComposers(minimizedComposers.value, closing);
    if (next.length === minimizedComposers.value.length) return;
    minimizedComposers.value = next;
    persist();
  }

  /** The composer opened (from any path), so it is no longer minimized in that tab. */
  function opened(projectId: string | undefined) {
    if (projectId) remove((item) => item.tabProjectId === projectId);
  }

  function items(tabProjectId: string): MinimizedDialogItem[] {
    return minimizedComposers.value
      .filter((entry) => entry.tabProjectId === tabProjectId)
      .map((entry) => ({
        id: entry.id,
        kind: MINIMIZED_COMPOSER_KIND,
        label: minimizedComposerLabel(entry.title),
        shortLabel: 'new ticket',
        icon: { icon: Plus, name: 'plus' },
        draft: minimizedComposerDraftLabel(entry),
        minimizedAt: entry.minimizedAt,
      }));
  }

  /** Whether the current tab has a minimized composer. */
  function isMinimized(): boolean {
    return minimizedComposers.value.some((entry) => entry.tabProjectId === selectedProjectId.value);
  }

  const kind: MinimizedDialogKind = { kind: MINIMIZED_COMPOSER_KIND, items, restore, discard, dispose };

  return { minimize, restore, discard, dispose, opened, items, isMinimized, kind };
}

export type MinimizedComposerController = ReturnType<typeof createMinimizedComposer>;
