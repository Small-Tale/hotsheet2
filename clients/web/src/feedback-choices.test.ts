import { describe, expect, it } from 'vitest';

import {
  feedbackChoiceGroupFor,
  parseFeedbackChoices,
  selectedFeedbackChoicesMarkdown,
  updateFeedbackChoiceDocumentSelection,
  updateFeedbackChoiceSelection,
} from './feedback-choices';
import { combineFeedbackReply } from './feedback-replies';

const MULTI = [
  'Intro prose.',
  '',
  '**1. Which store?**',
  '',
  'CHOICE:',
  '- Ticket store',
  '- Document store',
  '',
  '## 2. Does the "Notes" field stay?',
  '',
  'CHOICE',
  '',
  '- Keep it',
  '',
  '- Remove it',
  '',
  '3. When should it run?',
  '',
  'CHOICE:',
  '',
  '1. On launch',
  '2) On command',
  '',
  'Trailing prose.',
].join('\n');

describe('feedback choices', () => {
  it('parses an uppercase header with an optional colon and Markdown list', () => {
    const source = 'Which direction?\n\nCHOICE:\n- **Keep** this\n- `attachment:proof.png`\n\nExplain anything else.';
    const parsed = parseFeedbackChoices(source);
    expect(parsed?.regions.map((region) => region.source)).toEqual(['Which direction?', 'Explain anything else.']);
    expect(parsed?.regions[1].sourceStart).toBe(source.indexOf('Explain'));
    expect(parsed?.groups).toMatchObject([
      {
        index: 1,
        question: 'Which direction?',
        choices: [
          { id: 'choice-1', markdown: '**Keep** this' },
          { id: 'choice-2', markdown: '`attachment:proof.png`' },
        ],
      },
    ]);
    expect(parseFeedbackChoices('CHOICE\n1. First\n2. Second')?.groups[0].choices).toHaveLength(2);
  });

  it('reads a question written on the CHOICE: header line, plain or bold (HS2-3MHSBS)', () => {
    const source = [
      'FEEDBACK NEEDED: the eviction is real. Choose how to treat it.',
      '',
      '| Arm | Paints |',
      '|---|---|',
      '| a | b |',
      '',
      'CHOICE: How should Domotion handle GC eviction?',
      '- **Leave it unmodeled (recommended).** Keep the strong model.',
      '- **Force a known GC state.** Call collectGarbage before each capture.',
      "- **Record Chrome's answer.** Read getPlatformFontsForNode.",
    ].join('\n');
    const parsed = parseFeedbackChoices(source)!;
    expect(parsed.groups).toHaveLength(1);
    expect(parsed.groups[0]).toMatchObject({
      question: 'How should Domotion handle GC eviction?',
      inlineQuestion: true,
    });
    expect(parsed.groups[0].choices.map((choice) => choice.markdown)).toEqual([
      '**Leave it unmodeled (recommended).** Keep the strong model.',
      '**Force a known GC state.** Call collectGarbage before each capture.',
      "**Record Chrome's answer.** Read getPlatformFontsForNode.",
    ]);
    // The header line is not repeated in the preceding prose.
    expect(parsed.regions[0].source).not.toContain('CHOICE');
    expect(parseFeedbackChoices('**CHOICE:** Which store?\n- Git\n- Jira')!.groups[0]).toMatchObject({
      question: 'Which store?',
      inlineQuestion: true,
    });
    expect(parseFeedbackChoices('CHOICE: **Which store?**\n- Git')!.groups[0].question).toBe('Which store?');
    // A bare header keeps taking its question from the preceding prose.
    const prose = parseFeedbackChoices('Which store?\nCHOICE:\n- Git')!.groups[0];
    expect(prose.question).toBe('Which store?');
    expect(prose.inlineQuestion).toBeUndefined();
    // Text after a colon-less CHOICE is prose, not a header.
    expect(parseFeedbackChoices('CHOICE the best one\n- A')).toBeUndefined();
    // Several inline groups label themselves.
    expect(
      parseFeedbackChoices('CHOICE: First?\n- A\n\nCHOICE: Second?\n- B')!.groups.map((group) => group.question),
    ).toEqual(['First?', 'Second?']);
  });

  it('does not interpret casual, empty, or fenced choice text as a choice block', () => {
    expect(parseFeedbackChoices('Choice:\n- Not uppercase')).toBeUndefined();
    expect(parseFeedbackChoices('CHOICE:\nNo list follows')).toBeUndefined();
    expect(parseFeedbackChoices('CHOICE:\n\n')).toBeUndefined();
    expect(parseFeedbackChoices('```\nCHOICE:\n- In code\n```')).toBeUndefined();
    expect(parseFeedbackChoices('~~~~md\nCHOICE\n- In code\n```\nCHOICE\n- Still code\n~~~~')).toBeUndefined();
    const afterFence = parseFeedbackChoices('```\nCHOICE:\n- In code\n```\n\nCHOICE:\n- Real');
    expect(afterFence?.groups).toHaveLength(1);
    expect(afterFence?.groups[0].choices).toEqual([{ id: 'choice-1', markdown: 'Real' }]);
  });

  it('parses every group, blank-line separated lists, and preserves prose in order', () => {
    const parsed = parseFeedbackChoices(MULTI)!;
    expect(parsed.groups.map((group) => group.choices.map((choice) => choice.id))).toEqual([
      ['choice-1', 'choice-2'],
      ['group-2-choice-1', 'group-2-choice-2'],
      ['group-3-choice-1', 'group-3-choice-2'],
    ]);
    expect(parsed.groups.map((group) => group.question)).toEqual([
      '1. Which store?',
      '2. Does the "Notes" field stay?',
      '3. When should it run?',
    ]);
    expect(parsed.groups[1].choices.map((choice) => choice.markdown)).toEqual(['Keep it', 'Remove it']);
    expect(parsed.regions.map((region) => region.source)).toEqual([
      'Intro prose.\n\n**1. Which store?**',
      '## 2. Does the "Notes" field stay?',
      '3. When should it run?',
      'Trailing prose.',
    ]);
    for (const region of parsed.regions) expect(MULTI.slice(region.sourceStart).startsWith(region.source)).toBe(true);
    expect(feedbackChoiceGroupFor(parsed, 'group-3-choice-2')?.index).toBe(3);
    expect(feedbackChoiceGroupFor(parsed, 'missing')).toBeUndefined();
  });

  it('skips a header without a list and keeps parsing later groups', () => {
    const parsed = parseFeedbackChoices('CHOICE\nprose\n\nQ?\nCHOICE\n- A')!;
    expect(parsed.groups).toHaveLength(1);
    expect(parsed.groups[0].question).toBe('Q?');
    expect(parsed.regions[0].source).toBe('CHOICE\nprose\n\nQ?');
    expect(parseFeedbackChoices('CHOICE\n- A\nCHOICE\n- B')!.groups.map((group) => group.question)).toEqual([
      undefined,
      undefined,
    ]);
  });

  it('supports exclusive, zero, additive, and range selection', () => {
    const ids = ['choice-1', 'choice-2', 'choice-3'];
    expect(updateFeedbackChoiceSelection(ids, [], 'choice-2', undefined, {})).toEqual({
      selected: ['choice-2'],
      anchor: 'choice-2',
    });
    expect(updateFeedbackChoiceSelection(ids, ['choice-2'], 'choice-2', 'choice-2', {})).toEqual({
      selected: [],
      anchor: 'choice-2',
    });
    expect(updateFeedbackChoiceSelection(ids, ['choice-1'], 'choice-3', 'choice-1', { additive: true })).toEqual({
      selected: ['choice-1', 'choice-3'],
      anchor: 'choice-3',
    });
    expect(updateFeedbackChoiceSelection(ids, ['choice-1'], 'choice-3', 'choice-1', { range: true })).toEqual({
      selected: ids,
      anchor: 'choice-1',
    });
  });

  it('keeps selection semantics within each group across a multi-step sequence', () => {
    const parsed = parseFeedbackChoices(MULTI)!;
    let state = updateFeedbackChoiceDocumentSelection(parsed, [], 'choice-1', undefined, {});
    state = updateFeedbackChoiceDocumentSelection(parsed, state.selected, 'group-3-choice-2', state.anchor, {});
    expect(state.selected).toEqual(['choice-1', 'group-3-choice-2']);
    // A plain click replaces only its own group's selection.
    state = updateFeedbackChoiceDocumentSelection(parsed, state.selected, 'choice-2', state.anchor, {});
    expect(state.selected).toEqual(['choice-2', 'group-3-choice-2']);
    // A range click whose anchor lives in another group acts as a plain click in the clicked group.
    state = updateFeedbackChoiceDocumentSelection(parsed, state.selected, 'group-2-choice-2', 'choice-2', {
      range: true,
    });
    expect(state).toEqual({
      selected: ['choice-2', 'group-2-choice-2', 'group-3-choice-2'],
      anchor: 'group-2-choice-2',
    });
    state = updateFeedbackChoiceDocumentSelection(parsed, state.selected, 'group-2-choice-1', state.anchor, {
      range: true,
    });
    expect(state.selected).toEqual(['choice-2', 'group-2-choice-1', 'group-2-choice-2', 'group-3-choice-2']);
    state = updateFeedbackChoiceDocumentSelection(parsed, state.selected, 'group-3-choice-1', state.anchor, {
      additive: true,
    });
    expect(state.selected).toContain('group-3-choice-1');
    state = updateFeedbackChoiceDocumentSelection(parsed, state.selected, 'group-3-choice-1', state.anchor, {
      additive: true,
    });
    expect(state.selected).not.toContain('group-3-choice-1');
    expect(updateFeedbackChoiceDocumentSelection(parsed, ['choice-1'], 'nope', 'choice-1', {})).toEqual({
      selected: ['choice-1'],
      anchor: 'choice-1',
    });
  });

  it('serializes selected Markdown options into a durable response', () => {
    const source = 'CHOICE:\n- Keep it\n- Use **new** behavior';
    expect(selectedFeedbackChoicesMarkdown(source, ['choice-2'])).toBe('Selected choice:\n- Use **new** behavior');
    expect(selectedFeedbackChoicesMarkdown(source, ['choice-1', 'choice-2'])).toBe(
      'Selected choices:\n- Keep it\n- Use **new** behavior',
    );
    expect(selectedFeedbackChoicesMarkdown(source, [])).toBe('');
    expect(selectedFeedbackChoicesMarkdown('No choices', ['choice-1'])).toBe('');
  });

  it('lists multi-group selections under each group question', () => {
    expect(selectedFeedbackChoicesMarkdown(MULTI, ['choice-1', 'group-3-choice-1', 'group-3-choice-2'])).toBe(
      '**1. Which store?**\n- Ticket store\n\n**3. When should it run?**\n- On launch\n- On command',
    );
    expect(selectedFeedbackChoicesMarkdown('CHOICE\n- A\nCHOICE\n- B', ['group-2-choice-1'])).toBe('**Choice 2**\n- B');
    expect(combineFeedbackReply(MULTI, [], 'Also this.', ['group-2-choice-2'])).toBe(
      '**2. Does the "Notes" field stay?**\n- Remove it\n\nAlso this.',
    );
  });

  it('places each group selection after its quoted prose when inline replies exist', () => {
    const offset = MULTI.indexOf('Intro prose.') + 'Intro prose.'.length;
    expect(combineFeedbackReply(MULTI, [{ offset, text: 'Inline.' }], '', ['choice-2', 'group-3-choice-1'])).toBe(
      [
        '> Intro prose.',
        'Inline.',
        '> **1. Which store?**',
        'Selected choice:\n- Document store',
        '> ## 2. Does the "Notes" field stay?',
        '> 3. When should it run?',
        'Selected choice:\n- On launch',
        '> Trailing prose.',
      ].join('\n\n'),
    );
  });
});
