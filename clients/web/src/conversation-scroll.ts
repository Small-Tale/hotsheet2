const BOTTOM_SLOP_PX = 32;

/** Keep a growing conversation pinned only while the reader remains at its latest edge. */
export function syncConversationScroll(root: ParentNode = document, force = false): void {
  const transcript = root.querySelector<HTMLElement>('.ai-conversation__transcript');
  if (!transcript) return;
  const previousHeight = Number(transcript.dataset.conversationScrollHeight);
  const hasPreviousHeight = Number.isFinite(previousHeight) && previousHeight > 0;
  const wasAtBottom = force
    || !hasPreviousHeight
    || transcript.scrollTop + transcript.clientHeight >= previousHeight - BOTTOM_SLOP_PX;
  transcript.dataset.conversationScrollHeight = String(transcript.scrollHeight);
  if (wasAtBottom) transcript.scrollTop = transcript.scrollHeight;
}
