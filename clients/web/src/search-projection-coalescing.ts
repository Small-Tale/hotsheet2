import type { CoalescedInput } from './debounced-autosave';
import { coalescedDraft } from './draft-input-coalescing';
import type { InlineSearchToken } from './inline-search';

export interface SearchProjectionState {
  readonly query: string;
  readonly tokens: readonly InlineSearchToken[];
}

/**
 * Whether a search edit changes what the rest of the app shows beyond the typed text: a chip was
 * committed, removed, or reordered, or the search became empty or non-empty (HS2-X72PD6). These
 * publish at once; plain typing waits for a pause.
 */
export function isStructuralSearchChange(previous: SearchProjectionState, next: SearchProjectionState): boolean {
  if (Boolean(previous.query.trim()) !== Boolean(next.query.trim())) return true;
  if (previous.tokens.length !== next.tokens.length) return true;
  return previous.tokens.some((token, index) => token.raw !== next.tokens[index].raw);
}

/**
 * Coalesce the workspace search model's per-keystroke state into the app's `searchQuery` and
 * `searchTokens` projections (HS2-X72PD6). Writing them re-renders the whole workspace, so plain
 * typing publishes when typing pauses (the ticket search request was already debounced), while
 * structural edits, and programmatic replacements passed as `immediate`, publish immediately.
 * The pending text joins the editor drafts, so `flushDraftInputs()` (or `flush()`) publishes it
 * before logic reads the projection.
 */
export function createSearchProjection(
  current: () => SearchProjectionState,
  publish: (next: SearchProjectionState, coalesced: boolean) => void,
  delay?: number,
): {
  update(next: SearchProjectionState, options?: { immediate?: boolean }): void;
  flush(): void;
  cancel(): void;
  pending(): boolean;
} {
  const typing: CoalescedInput<SearchProjectionState> = coalescedDraft((next) => {
    publish(next, true);
  }, delay);
  return {
    update(next, { immediate = false } = {}) {
      if (immediate || isStructuralSearchChange(current(), next)) {
        typing.cancel();
        publish(next, false);
      } else typing.schedule(next);
    },
    flush: () => {
      typing.flush();
    },
    cancel: () => {
      typing.cancel();
    },
    pending: () => typing.pending(),
  };
}
