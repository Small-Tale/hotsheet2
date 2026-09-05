import { describe, expect, it } from 'vitest';

import { parseFeedbackChoices, selectedFeedbackChoicesMarkdown, updateFeedbackChoiceSelection } from './feedback-choices';

describe('feedback choices', () => {
  it('parses an uppercase header with an optional colon and Markdown list', () => {
    const source = 'Which direction?\n\nCHOICE:\n- **Keep** this\n- `attachment:proof.png`\n\nExplain anything else.';
    expect(parseFeedbackChoices(source)).toMatchObject({
      before: 'Which direction?',
      after: 'Explain anything else.',
      choices: [
        { id: 'choice-1', markdown: '**Keep** this' },
        { id: 'choice-2', markdown: '`attachment:proof.png`' },
      ],
    });
    expect(parseFeedbackChoices('CHOICE\n1. First\n2. Second')?.choices).toHaveLength(2);
  });

  it('does not interpret casual or empty choice text as a choice block', () => {
    expect(parseFeedbackChoices('Choice:\n- Not uppercase')).toBeUndefined();
    expect(parseFeedbackChoices('CHOICE:\nNo list follows')).toBeUndefined();
  });

  it('supports exclusive, zero, additive, and range selection', () => {
    const ids = ['choice-1', 'choice-2', 'choice-3'];
    expect(updateFeedbackChoiceSelection(ids, [], 'choice-2', undefined, {})).toEqual({ selected: ['choice-2'], anchor: 'choice-2' });
    expect(updateFeedbackChoiceSelection(ids, ['choice-2'], 'choice-2', 'choice-2', {})).toEqual({ selected: [], anchor: 'choice-2' });
    expect(updateFeedbackChoiceSelection(ids, ['choice-1'], 'choice-3', 'choice-1', { additive: true })).toEqual({ selected: ['choice-1', 'choice-3'], anchor: 'choice-3' });
    expect(updateFeedbackChoiceSelection(ids, ['choice-1'], 'choice-3', 'choice-1', { range: true })).toEqual({ selected: ids, anchor: 'choice-1' });
  });

  it('serializes selected Markdown options into a durable response', () => {
    const source = 'CHOICE:\n- Keep it\n- Use **new** behavior';
    expect(selectedFeedbackChoicesMarkdown(source, ['choice-2'])).toBe('Selected choice:\n- Use **new** behavior');
    expect(selectedFeedbackChoicesMarkdown(source, [])).toBe('');
  });
});
