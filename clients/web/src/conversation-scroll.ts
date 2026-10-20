const BOTTOM_SLOP_PX = 32;

interface TranscriptScrollState {
  pinned: boolean;
  top: number;
  viewportTop: number;
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

function trackedState(transcript: HTMLElement): TranscriptScrollState {
  let state = transcriptStates.get(transcript);
  if (state) return state;
  const created: TranscriptScrollState = {
    pinned: true,
    top: transcript.scrollTop,
    viewportTop: transcript.getBoundingClientRect().top,
  };
  transcriptStates.set(transcript, created);
  transcript.addEventListener(
    'scroll',
    () => {
      // Browser scroll anchoring may adjust scrollTop as the selection row changes
      // height. Wait for sync to reconcile that layout change against the last
      // reader position before treating the event as an intentional scroll.
      if (Math.abs(transcript.getBoundingClientRect().top - created.viewportTop) > 0.5) return;
      const top = transcript.scrollTop;
      if (atLatestEdge(transcript)) created.pinned = true;
      else if (top < created.top) created.pinned = false;
      created.top = top;
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
    const viewportTop = transcript.getBoundingClientRect().top;
    if (!force && !state.pinned) {
      // The selection bar is a row above the scroller. Offset its movement so the
      // message under the reader's eye stays at the same screen position.
      transcript.scrollTop = state.top + viewportTop - state.viewportTop;
      state.viewportTop = viewportTop;
      state.top = transcript.scrollTop;
      continue;
    }
    transcript.scrollTop = transcript.scrollHeight;
    state.pinned = true;
    state.top = transcript.scrollTop;
    state.viewportTop = viewportTop;
  }
}
