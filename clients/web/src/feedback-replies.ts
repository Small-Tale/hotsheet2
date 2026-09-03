import { marked, type Tokens } from 'marked';

export interface FeedbackBlock { markdown: string }
export interface InlineFeedbackReply { blockIndex: number; text: string }

export function parseFeedbackBlocks(prompt: string): FeedbackBlock[] {
  if (!prompt.trim()) return [];
  let tokens: Tokens.Generic[];
  try {
    tokens = marked.lexer(prompt);
  } catch {
    return [{ markdown: prompt.trim() }];
  }
  const blocks: FeedbackBlock[] = [];
  for (const token of tokens) {
    if (token.type === 'space' || !token.raw.trim()) continue;
    if (token.type === 'list') {
      for (const item of (token as Tokens.List).items) {
        const markdown = item.raw.trim();
        if (markdown) blocks.push({ markdown });
      }
    } else {
      blocks.push({ markdown: token.raw.trim() });
    }
  }
  return blocks;
}

function quoteMarkdown(markdown: string): string {
  return markdown.split('\n').map(line => line ? `> ${line}` : '>').join('\n');
}

export function combineFeedbackReply(prompt: string, replies: readonly InlineFeedbackReply[], generalReply: string): string {
  const blocks = parseFeedbackBlocks(prompt);
  const liveReplies = replies.filter(reply => reply.text.trim());
  const general = generalReply.trim();
  if (!liveReplies.length) return general;
  const pieces: string[] = [];
  for (let index = 0; index < blocks.length; index += 1) {
    pieces.push(quoteMarkdown(blocks[index].markdown));
    for (const reply of liveReplies) if (reply.blockIndex === index) pieces.push(reply.text.trim());
  }
  for (const reply of liveReplies) if (reply.blockIndex >= blocks.length) pieces.push(reply.text.trim());
  if (general) pieces.push(general);
  return pieces.join('\n\n');
}
