/** What a selection prune reads and writes; the ticket workflows supply the live signals. */
export interface SelectionPruneDependencies {
  /** The current selection with every row the view no longer shows removed. */
  readonly visibleSelection: () => readonly string[];
  readonly selectedSlugs: () => readonly string[];
  /** Slug of the ticket whose detail is open, if any. */
  readonly selectedTicketSlug: () => string | undefined;
  /** Saves the open ticket's unsaved edits; `undefined` when nothing is queued or in flight (HS2-QW3PGE). */
  readonly flushSelectedTicketEdits: () => Promise<boolean> | undefined;
  /** False once the owning project is no longer active, so a deferred rerun stops. */
  readonly stillCurrent: () => boolean;
  /** Commits the pruned selection, deselecting the open ticket when it left the view. */
  readonly apply: (next: readonly string[]) => void;
}

/**
 * Drop selected rows the current view no longer shows, as after a bulk change (HS2-41KEQD). When the
 * open ticket is about to leave while one of its edits is unsaved, that edit is saved first, while the
 * ticket is still selected, so the save resolves its own identity instead of being discarded; the prune
 * then reruns once the save settles. A ticket that stays in view is never written mid-typing
 * (HS2-RE1PS6).
 */
export function pruneSelectionSavingEdits(dependencies: SelectionPruneDependencies): void {
  const next = dependencies.visibleSelection();
  if (next.length === dependencies.selectedSlugs().length) return;
  const open = dependencies.selectedTicketSlug();
  if (open !== undefined && !next.includes(open)) {
    const saving = dependencies.flushSelectedTicketEdits();
    if (saving) {
      void saving
        .catch(() => false)
        .then(() => {
          if (dependencies.stillCurrent()) pruneSelectionSavingEdits(dependencies);
        });
      return;
    }
  }
  dependencies.apply(next);
}
