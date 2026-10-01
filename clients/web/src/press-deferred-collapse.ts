/**
 * Defers a focus-loss collapse until the pointer press that caused it has finished (HS2-YVBGW3).
 *
 * A collapsible field that closes when it loses focus closes at `pointerdown`/`mousedown`, before the
 * press completes. When closing it removes a layout row (the workspace search sits above the project
 * tab strip), everything below shifts under the pointer, `pointerup` lands on a different element,
 * and the browser dispatches the click to their common ancestor: the control the user pressed never
 * activates. While a press is in flight the field should stay open; once the press (and its click)
 * is over, collapse it if it lost focus and is still eligible.
 */
export interface PressDeferredCollapseOptions {
  /** The document whose pointer presses are tracked. */
  doc: Pick<Document, 'addEventListener' | 'removeEventListener'>;
  /** Whether the collapsible field currently contains focus. */
  ownsFocus: () => boolean;
  /** Whether the field may collapse now (for example it is open and empty). */
  shouldCollapse: () => boolean;
  /** Collapse the field. */
  collapse: () => void;
  /** Runs after the press's click has been dispatched; defaults to a zero-delay task. */
  schedule?: (callback: () => void) => void;
}

export interface PressDeferredCollapse {
  /** True between a press's pointerdown and its pointerup/pointercancel. */
  pressing: () => boolean;
  dispose: () => void;
}

export function createPressDeferredCollapse({
  doc,
  ownsFocus,
  shouldCollapse,
  collapse,
  schedule = (callback) => {
    setTimeout(callback, 0);
  },
}: PressDeferredCollapseOptions): PressDeferredCollapse {
  let pressing = false,
    focusedAtPress = false;
  const down = () => {
    pressing = true;
    focusedAtPress = ownsFocus();
  };
  const up = () => {
    if (!pressing) return;
    pressing = false;
    // Only a press that took focus away from the field may close it, matching blur-only collapse.
    if (!focusedAtPress) return;
    focusedAtPress = false;
    schedule(() => {
      if (!pressing && !ownsFocus() && shouldCollapse()) collapse();
    });
  };
  doc.addEventListener('pointerdown', down, true);
  doc.addEventListener('pointerup', up, true);
  doc.addEventListener('pointercancel', up, true);
  return {
    pressing: () => pressing,
    dispose: () => {
      doc.removeEventListener('pointerdown', down, true);
      doc.removeEventListener('pointerup', up, true);
      doc.removeEventListener('pointercancel', up, true);
    },
  };
}
