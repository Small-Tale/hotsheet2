const BOTTOM_SLOP_PX = 32;

interface TranscriptScrollState {
  pinned: boolean;
  top: number;
  /** The scroller's top relative to its parent, which the selection bar row shifts. */
  offset: number;
  /** The scroller's width. A change reflows every message, so `top` no longer names the reading position. */
  width: number;
  /**
   * The position a sync wanted for a scrolled-back reader when the browser clamped it short. A re-render
   * drops the composer clearance for a moment, so the end is briefly shorter than the reading position;
   * the restored clearance makes room again, and this is where the reader returns (HS2-FAP9B8).
   */
  wanted?: number;
}

/**
 * Reader intent per mounted transcript, derived from its scroll events. Kept outside the
 * DOM because Kerf's morph removes attributes a render did not declare, which previously
 * made every rerender (for example selecting a message) look like a first render.
 */
const transcriptStates = new WeakMap<HTMLElement, TranscriptScrollState>();

function atLatestEdge(transcript: HTMLElement): boolean {
  return transcript.scrollTop + transcript.clientHeight >= transcript.scrollHeight - BOTTOM_SLOP_PX;
}

/**
 * Where the scroller sits within its parent. The selection bar is a sibling row above it, so it
 * moves this offset; moving the whole conversation (a window resize recentering the dialog) does
 * not, and is never compensated as if it were selection chrome (HS2-TG2ZV3).
 */
function parentOffset(transcript: HTMLElement): number {
  const parentTop = transcript.parentElement?.getBoundingClientRect().top ?? 0;
  return transcript.getBoundingClientRect().top - parentTop;
}

/** A position the browser clamped below the last synced one because the content got shorter. */
function clampedBelow(transcript: HTMLElement, state: TranscriptScrollState): boolean {
  return (
    transcript.scrollTop < state.top && transcript.scrollTop >= transcript.scrollHeight - transcript.clientHeight - 1
  );
}

/**
 * Judge a scroll position the tracker has not classified yet. The listener and the sync share this,
 * because the browser dispatches `scroll` only at its next frame: on a slow host a render can sync
 * before the reader's scroll event arrives, and must not act on the stale intent (HS2-TG2ZV3).
 */
function observeReaderScroll(transcript: HTMLElement, state: TranscriptScrollState): void {
  const top = transcript.scrollTop;
  // The position a sync just set is not the reader's choice. Judging it would pin a reader whom
  // a selection-bar offset placed near an end that a re-render had briefly shortened by
  // dropping the composer clearance (HS2-4J9QFX).
  if (top === state.top) return;
  state.wanted = undefined;
  if (atLatestEdge(transcript)) state.pinned = true;
  else if (top < state.top) state.pinned = false;
  state.top = top;
}

function trackedState(transcript: HTMLElement): TranscriptScrollState {
  let state = transcriptStates.get(transcript);
  if (state) return state;
  const created: TranscriptScrollState = {
    pinned: true,
    top: transcript.scrollTop,
    offset: parentOffset(transcript),
    width: transcript.clientWidth,
  };
  transcriptStates.set(transcript, created);
  transcript.addEventListener(
    'scroll',
    () => {
      // Browser scroll anchoring may adjust scrollTop as the selection row changes
      // height, and a resize reflows the messages. Wait for sync to reconcile that layout
      // change against the last reader position before treating the event as intentional.
      if (Math.abs(parentOffset(transcript) - created.offset) > 0.5 || transcript.clientWidth !== created.width) return;
      observeReaderScroll(transcript, created);
    },
    { passive: true },
  );
  state = created;
  return state;
}

/**
 * Keep a growing conversation pinned only while the reader remains at its latest edge.
 * Scrolling back unpins; rerenders that merely change selection or chrome never move a
 * scrolled-back reader, and returning to the bottom pins again.
 */
export function syncConversationScroll(root: ParentNode = document, force = false): void {
  for (const transcript of root.querySelectorAll<HTMLElement>('.ai-conversation__transcript')) {
    const state = trackedState(transcript);
    const offset = parentOffset(transcript),
      width = transcript.clientWidth,
      reflowed = width !== state.width,
      chromeMoved = !reflowed && Math.abs(offset - state.offset) > 0.5;
    // Adopt a reader scroll whose event is still pending, and after a reflow the position the browser
    // anchored. Only a selection-bar move leaves the old position as the one to compensate from.
    if (!force && !chromeMoved && !clampedBelow(transcript, state)) observeReaderScroll(transcript, state);
    if (!force && !state.pinned) {
      // The selection bar is a row above the scroller. Offset its movement so the
      // message under the reader's eye stays at the same screen position.
      const target = (state.wanted ?? state.top) + (chromeMoved ? offset - state.offset : 0);
      transcript.scrollTop = target;
      state.offset = offset;
      state.width = width;
      state.top = transcript.scrollTop;
      state.wanted = state.top < target - 0.5 ? target : undefined;
      continue;
    }
    transcript.scrollTop = transcript.scrollHeight;
    state.pinned = true;
    state.wanted = undefined;
    state.top = transcript.scrollTop;
    state.offset = offset;
    state.width = width;
  }
}

/**
 * Keep transcripts that follow their latest message at the end, after chrome over the end (the drawer
 * composer's published clearance) changed size. A scrolled-back reader is never pulled to the end or
 * pinned (HS2-4J9QFX); it only returns to the position a sync wanted and the shorter end clamped
 * (HS2-FAP9B8). A transcript no render has synced yet is never moved.
 */
export function followConversationEnd(root: ParentNode = document): void {
  for (const transcript of root.querySelectorAll<HTMLElement>('.ai-conversation__transcript')) {
    const state = transcriptStates.get(transcript);
    if (!state) continue;
    if (state.pinned) {
      transcript.scrollTop = transcript.scrollHeight;
      state.top = transcript.scrollTop;
      continue;
    }
    if (state.wanted === undefined) continue;
    const wanted = state.wanted;
    transcript.scrollTop = wanted;
    state.top = transcript.scrollTop;
    state.wanted = state.top < wanted - 0.5 ? wanted : undefined;
  }
}
