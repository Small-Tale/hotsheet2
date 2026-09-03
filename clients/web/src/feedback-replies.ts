export interface InlineFeedbackReply { offset: number; text: string }
export interface FeedbackSegment { start: number; end: number; markdown: string; reply?: InlineFeedbackReply }

export function splitFeedbackPrompt(prompt: string, replies: readonly InlineFeedbackReply[]): FeedbackSegment[] {
  const byOffset = new Map<number, InlineFeedbackReply>();
  for (const reply of replies) byOffset.set(Math.max(0, Math.min(prompt.length, reply.offset)), reply);
  const offsets = [...byOffset.keys()].sort((left, right) => left - right);
  const segments: FeedbackSegment[] = [];
  let start = 0;
  for (const end of offsets) {
    segments.push({ start, end, markdown: prompt.slice(start, end), reply: byOffset.get(end) });
    start = end;
  }
  segments.push({ start, end: prompt.length, markdown: prompt.slice(start) });
  return segments;
}

/** Map a caret offset in rendered text back to the corresponding Markdown source boundary. */
export function sourceOffsetForVisibleOffset(source: string, visibleText: string, visibleOffset: number): number {
  if (visibleOffset <= 0) return 0;
  let sourceOffset = 0;
  for (let index = 0; index < Math.min(visibleOffset, visibleText.length); index += 1) {
    const match = source.indexOf(visibleText[index], sourceOffset);
    if (match >= 0) sourceOffset = match + 1;
  }
  return sourceOffset;
}

function quoteMarkdown(markdown: string): string {
  return markdown.split('\n').map(line => line ? `> ${line}` : '>').join('\n');
}

export function combineFeedbackReply(prompt: string, replies: readonly InlineFeedbackReply[], generalReply: string): string {
  const liveReplies = replies.filter(reply => reply.text.trim());
  const general = generalReply.trim();
  if (!liveReplies.length) return general;
  const pieces: string[] = [];
  for (const segment of splitFeedbackPrompt(prompt, liveReplies)) {
    const markdown = segment.markdown.replace(/^\n+|\n+$/g, '');
    if (markdown) pieces.push(quoteMarkdown(markdown));
    if (segment.reply?.text.trim()) pieces.push(segment.reply.text.trim());
  }
  if (general) pieces.push(general);
  return pieces.join('\n\n');
}
