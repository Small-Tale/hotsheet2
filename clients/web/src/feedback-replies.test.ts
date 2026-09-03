import { describe, expect, it } from 'vitest';

import { combineFeedbackReply, parseFeedbackBlocks } from './feedback-replies';

describe('inline feedback replies', () => {
  it('splits top-level Markdown blocks and each top-level list item', () => {
    const blocks = parseFeedbackBlocks('FEEDBACK NEEDED\n\nHello there.\n\n1. Something\n2. Another thing');
    expect(blocks.map(block => block.markdown)).toEqual([
      'FEEDBACK NEEDED',
      'Hello there.',
      '1. Something',
      '2. Another thing',
    ]);
  });

  it('keeps nested list detail with its top-level parent', () => {
    const blocks = parseFeedbackBlocks('1. First\n   - detail a\n   - detail b\n2. Second');
    expect(blocks).toHaveLength(2);
    expect(blocks[0].markdown).toContain('detail a');
    expect(blocks[1].markdown).toBe('2. Second');
  });

  it('returns an ordinary response unchanged when no inline points are used', () => {
    expect(combineFeedbackReply('Question?', [], '  General answer.  ')).toBe('General answer.');
  });

  it('quotes the prompt and interleaves responses after clicked blocks', () => {
    const result = combineFeedbackReply(
      'FEEDBACK NEEDED\n\nHello there.\n\n1. Something\n2. Another thing',
      [{ blockIndex: 2, text: 'My first response' }, { blockIndex: 3, text: 'My second response' }],
      '',
    );
    expect(result).toBe('> FEEDBACK NEEDED\n\n> Hello there.\n\n> 1. Something\n\nMy first response\n\n> 2. Another thing\n\nMy second response');
  });

  it('appends general feedback after inline responses', () => {
    expect(combineFeedbackReply('First?\n\nSecond?', [{ blockIndex: 0, text: 'Yes.' }], 'Overall note.'))
      .toBe('> First?\n\nYes.\n\n> Second?\n\nOverall note.');
  });
});
