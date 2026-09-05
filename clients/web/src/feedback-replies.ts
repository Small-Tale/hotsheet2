import { parseFeedbackChoices, selectedFeedbackChoicesMarkdown } from './feedback-choices';

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

export function combineFeedbackReply(prompt: string, replies: readonly InlineFeedbackReply[], generalReply: string, selectedChoiceIds: readonly string[] = []): string {
  const liveReplies = replies.filter(reply => reply.text.trim());
  const general = generalReply.trim();
  const selectedChoices = selectedFeedbackChoicesMarkdown(prompt, selectedChoiceIds);
  if (!liveReplies.length) return [selectedChoices, general].filter(Boolean).join('\n\n');
  const pieces: string[] = [];
  const appendRegion = (source: string, sourceStart: number) => {
    const localReplies = liveReplies.filter(reply => reply.offset >= sourceStart && reply.offset <= sourceStart + source.length).map(reply => ({ ...reply, offset: reply.offset - sourceStart }));
    for (const segment of splitFeedbackPrompt(source, localReplies)) {
      const markdown = segment.markdown.replace(/^\n+|\n+$/g, '');
      if (markdown) pieces.push(quoteMarkdown(markdown));
      if (segment.reply?.text.trim()) pieces.push(segment.reply.text.trim());
    }
  };
  const choiceGroup = parseFeedbackChoices(prompt);
  if (choiceGroup) {
    appendRegion(choiceGroup.before, 0);
    if (selectedChoices) pieces.push(selectedChoices);
    appendRegion(choiceGroup.after, choiceGroup.afterStart);
  } else appendRegion(prompt, 0);
  if (!choiceGroup && selectedChoices) pieces.push(selectedChoices);
  if (general) pieces.push(general);
  return pieces.join('\n\n');
}
