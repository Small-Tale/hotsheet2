import { describe, expect, it } from 'vitest';

import { combineFeedbackReply, sourceOffsetForVisibleOffset, splitFeedbackPrompt } from './feedback-replies';

describe('inline feedback replies', () => {
  it('splits at exact character offsets in source order', () => {
    expect(splitFeedbackPrompt('Something else', [{ offset: 4, text: 'First' }, { offset: 9, text: 'Second' }])).toEqual([
      { start: 0, end: 4, markdown: 'Some', reply: { offset: 4, text: 'First' } },
      { start: 4, end: 9, markdown: 'thing', reply: { offset: 9, text: 'Second' } },
      { start: 9, end: 14, markdown: ' else' },
    ]);
  });

  it('maps rendered caret boundaries past Markdown syntax', () => {
    expect(sourceOffsetForVisibleOffset('**Something**', 'Something', 4)).toBe(6);
    expect(sourceOffsetForVisibleOffset('[A link](/target)', 'A link', 6)).toBe(7);
  });

  it('returns an ordinary response unchanged when no inline points are used', () => {
    expect(combineFeedbackReply('Question?', [], '  General answer.  ')).toBe('General answer.');
  });

  it('combines selected choices with an optional freeform response', () => {
    expect(combineFeedbackReply('CHOICE:\n- First\n- **Second**', [], 'Because it is clearer.', ['choice-2']))
      .toBe('Selected choice:\n- **Second**\n\nBecause it is clearer.');
  });

  it('omits choice syntax while interleaving inline and selected responses', () => {
    const prompt = 'Question before.\n\nCHOICE:\n- First\n- Second\n\nAnything after?';
    expect(combineFeedbackReply(prompt, [{ offset: 'Question'.length, text: 'Inline answer.' }], '', ['choice-1']))
      .toBe('> Question\n\nInline answer.\n\n>  before.\n\nSelected choice:\n- First\n\n> Anything after?');
  });

  it('quotes the prompt and interleaves responses after clicked characters', () => {
    const prompt = 'FEEDBACK NEEDED\n\nHello there.\n\n1. Something\n2. Another thing';
    const result = combineFeedbackReply(
      prompt,
      [
        { offset: prompt.indexOf('Something') + 'Something'.length, text: 'My first response' },
        { offset: prompt.length, text: 'My second response' },
      ],
      '',
    );
    expect(result).toBe('> FEEDBACK NEEDED\n>\n> Hello there.\n>\n> 1. Something\n\nMy first response\n\n> 2. Another thing\n\nMy second response');
  });

  it('appends general feedback after inline responses', () => {
    expect(combineFeedbackReply('First?\n\nSecond?', [{ offset: 6, text: 'Yes.' }], 'Overall note.'))
      .toBe('> First?\n\nYes.\n\n> Second?\n\nOverall note.');
  });
});
