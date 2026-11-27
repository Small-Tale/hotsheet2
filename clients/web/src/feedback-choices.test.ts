import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import {
  feedbackChoiceGroupFor,
  parseFeedbackChoices,
  selectedFeedbackChoicesMarkdown,
  updateFeedbackChoiceDocumentSelection,
  updateFeedbackChoiceSelection,
} from './feedback-choices';
import { combineFeedbackReply } from './feedback-replies';

interface CorpusCase {
  name: string;
  source: string;
  groups:
    | {
        raw: string;
        question: string | null;
        inlineQuestion: boolean;
        choices: { label: string; details: string | null }[];
      }[]
    | null;
  regions?: string[];
  selections?: { select: string[]; markdown: string }[];
}

// The shared TS/Rust grammar corpus (HS2-EWF0FT); crates/hotsheet-model/tests/feedback_choices.rs reads it too.
const CORPUS = (
  JSON.parse(
    readFileSync(
      new URL('../../../crates/hotsheet-model/tests/fixtures/feedback-choices.json', import.meta.url),
      'utf8',
    ),
  ) as { cases: CorpusCase[] }
).cases;

describe('feedback choice grammar corpus (HS2-EWF0FT)', () => {
  it('has unique case names', () => {
    expect(new Set(CORPUS.map((entry) => entry.name)).size).toBe(CORPUS.length);
  });

  it.each(CORPUS.map((entry) => [entry.name, entry] as const))('%s', (_name, entry) => {
    const parsed = parseFeedbackChoices(entry.source);
    if (!entry.groups) {
      expect(parsed).toBeUndefined();
      return;
    }
    expect(parsed).toBeDefined();
    const document = parsed!;
    expect(
      document.groups.map((group) => ({
        raw: entry.source.slice(group.start, group.end),
        question: group.question ?? null,
        inlineQuestion: group.inlineQuestion ?? false,
        choices: group.choices.map((choice) => ({ label: choice.markdown, details: choice.details ?? null })),
      })),
    ).toEqual(entry.groups);
    expect(document.groups.map((group) => group.index)).toEqual(entry.groups.map((_, index) => index + 1));
    expect(document.regions.map((region) => region.source)).toEqual(entry.regions);
    for (const region of document.regions)
      expect(entry.source.slice(region.sourceStart).startsWith(region.source)).toBe(true);
    for (const selection of entry.selections ?? []) {
      const markdown = selectedFeedbackChoicesMarkdown(entry.source, selection.select);
      expect(markdown).toBe(selection.markdown);
      // A recorded response never reads as a new choice prompt, and the prompt re-parses to the same ids.
      expect(parseFeedbackChoices(markdown)).toBeUndefined();
      expect(parseFeedbackChoices(entry.source)).toEqual(document);
    }
  });
});

// The HS2-8T71YW note shape (synthetic design wording): numbered `CHOICE N: <question>` headers,
// each followed by a lettered bullet list whose first option carries the recommendation.
const NUMBERED = [
  'FEEDBACK NEEDED: milestones design. Please answer each CHOICE; my recommendation is first and marked.',
  '',
  'CHOICE 1: How does a Git store hold a milestone?',
  '- A (recommended): a ticket of a built-in `milestone` category',
  '- B: a registry file per milestone beside the group registry',
  '',
  'CHOICE 2: What is a milestone in Jira?',
  '- A (recommended): a fixVersion (release with releaseDate)',
  '- B: the parent epic',
  '- C: a per-connection setting choosing fixVersion or epic',
  '',
  'CHOICE 3: Nested milestones?',
  '- A (recommended): not in the first version',
  '- B: allow nesting, cycle-checked',
  '',
  'CHOICE 4: When does a milestone orchestration prerequisite hold?',
  '- A (recommended): every linked ticket confirmed finished',
  '- B: when the milestone itself is closed',
  '- C: either, chosen per step',
  '',
  'CHOICE 5: Where do milestones live in the client?',
  '- A (recommended): a built-in Milestones sidebar view',
  '- B: only "group by milestone" in the list and column views',
  '- C: a global dashboard across projects',
  '',
  'CHOICE 6: What instant is a date-only due date for scheduling?',
  '- A (recommended): end of that day in UTC',
  '- B: end of that day in a project time-zone setting',
].join('\n');

describe('numbered CHOICE group headers (HS2-WX8BE6)', () => {
  it('reads each CHOICE N: <question> followed by a list as its own group', () => {
    const document = parseFeedbackChoices(NUMBERED)!;
    expect(document.groups.map((group) => group.choices.length)).toEqual([2, 3, 2, 3, 3, 2]);
    expect(document.groups.map((group) => group.question)).toEqual([
      'How does a Git store hold a milestone?',
      'What is a milestone in Jira?',
      'Nested milestones?',
      'When does a milestone orchestration prerequisite hold?',
      'Where do milestones live in the client?',
      'What instant is a date-only due date for scheduling?',
    ]);
    expect(document.groups.every((group) => group.inlineQuestion)).toBe(true);
    for (const group of document.groups) {
      expect(group.choices[0].markdown).toMatch(/^A \(recommended\): /);
      expect(group.choices.slice(1).every((choice) => !choice.markdown.includes('recommended'))).toBe(true);
    }
    expect(document.regions.map((region) => region.source)).toEqual([
      'FEEDBACK NEEDED: milestones design. Please answer each CHOICE; my recommendation is first and marked.',
      '',
      '',
      '',
      '',
      '',
      '',
    ]);
  });

  it('answers one option per group under each question', () => {
    const document = parseFeedbackChoices(NUMBERED)!;
    let selected: string[] = [];
    for (const group of document.groups)
      selected = updateFeedbackChoiceDocumentSelection(
        document,
        selected,
        group.choices.at(-1)!.id,
        undefined,
        {},
      ).selected;
    expect(selected).toEqual(document.groups.map((group) => group.choices.at(-1)!.id));
    const markdown = selectedFeedbackChoicesMarkdown(NUMBERED, selected);
    expect(markdown.split('\n\n')).toHaveLength(6);
    expect(markdown.startsWith('**How does a Git store hold a milestone?**\n- B: a registry file')).toBe(true);
    expect(parseFeedbackChoices(markdown)).toBeUndefined();
  });

  it('keeps the earlier header and lettered-block formats', () => {
    expect(parseFeedbackChoices('CHOICE\n- a\n- b')?.groups).toHaveLength(1);
    expect(parseFeedbackChoices('Which?\n\nCHOICE:\n- a\n- b')?.groups[0].question).toBe('Which?');
    expect(parseFeedbackChoices('CHOICE: Which?\n- a\n- b')?.groups[0]).toMatchObject({
      question: 'Which?',
      inlineQuestion: true,
    });
    const lettered = parseFeedbackChoices('**CHOICE A: Memory.** Fast.\n\n**CHOICE B: Disk.** Durable.')!;
    expect(lettered.groups.map((group) => group.choices.map((choice) => choice.markdown))).toEqual([
      ['CHOICE A: Memory.', 'CHOICE B: Disk.'],
    ]);
    const options = parseFeedbackChoices('Pick one.\n\nOption A — Memory.\nOption B — Disk.')!;
    expect(options.groups[0].choices.map((choice) => choice.markdown)).toEqual([
      'Option A — Memory.',
      'Option B — Disk.',
    ]);
    // Numbered CHOICE blocks without a following list stay one lettered run.
    const numbered = parseFeedbackChoices('Pick one.\n\nCHOICE 1: Memory.\nCHOICE 2: Disk.')!;
    expect(numbered.groups).toHaveLength(1);
    expect(numbered.groups[0]).toMatchObject({
      question: 'Pick one.',
      choices: [{ markdown: 'CHOICE 1: Memory.' }, {}],
    });
    // A lettered block whose details are an indented list stays a lettered block.
    const indented = parseFeedbackChoices('CHOICE A: Script it.\n\n    - pro\n\nCHOICE B: By hand.')!;
    expect(indented.groups[0].choices.map((choice) => choice.markdown)).toEqual([
      'CHOICE A: Script it.',
      'CHOICE B: By hand.',
    ]);
  });
});

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

  it('keeps selection and inline replies working across list and lettered groups (HS2-EWF0FT)', () => {
    const source =
      'CHOICE: Which rollout?\n- **Ship now.**\n  Costs later.\n- Wait\n\nPick a cache.\n\n**CHOICE A: Memory.** Fast.\n\n**CHOICE B: Disk.** Durable.\n\nRecommendation: B.';
    const parsed = parseFeedbackChoices(source)!;
    expect(parsed.groups.map((group) => group.choices.map((choice) => choice.id))).toEqual([
      ['choice-1', 'choice-2'],
      ['group-2-choice-1', 'group-2-choice-2'],
    ]);
    let state = updateFeedbackChoiceDocumentSelection(parsed, [], 'group-2-choice-1', undefined, {});
    state = updateFeedbackChoiceDocumentSelection(parsed, state.selected, 'group-2-choice-2', state.anchor, {
      range: true,
    });
    state = updateFeedbackChoiceDocumentSelection(parsed, state.selected, 'choice-1', state.anchor, {});
    expect(state.selected).toEqual(['choice-1', 'group-2-choice-1', 'group-2-choice-2']);
    const reply = combineFeedbackReply(source, [], 'Note.', state.selected);
    expect(reply).toBe(
      '**Which rollout?**\n- **Ship now.**\n\n**Pick a cache.**\n- CHOICE A: Memory.\n- CHOICE B: Disk.\n\nNote.',
    );
    // The recorded answer never parses as a prompt, and the prompt keeps its ids after re-parsing.
    expect(parseFeedbackChoices(reply)).toBeUndefined();
    expect(parseFeedbackChoices(source)).toEqual(parsed);
    const offset = source.indexOf('Recommendation');
    expect(
      combineFeedbackReply(source, [{ offset: offset + 'Recommendation: B.'.length, text: 'Agreed.' }], '', [
        'group-2-choice-2',
      ]),
    ).toBe(['> Pick a cache.', 'Selected choice:\n- CHOICE B: Disk.', '> Recommendation: B.', 'Agreed.'].join('\n\n'));
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
