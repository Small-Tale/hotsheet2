import { expect, test } from 'vitest';

import { terminalQuestionAnswers } from './terminal-question-answer';

test('collects choices, multiple selections and free text across questions', () => {
  const data = new FormData();
  data.append('choice-0', '1');
  data.append('choice-1', '0');
  data.append('choice-1', 'other');
  data.append('other-1', '  Custom  ');
  data.append('other-2', ' typed answer ');
  expect(
    terminalQuestionAnswers(
      [
        { question: 'One?', options: [{ label: 'A' }, { label: 'B' }] },
        { question: 'Two?', multiSelect: true, options: [{ label: 'C' }] },
        { question: 'Three?' },
      ],
      data,
    ),
  ).toEqual({ 'One?': 'B', 'Two?': 'C, Custom', 'Three?': 'typed answer' });
});

test('rejects partial, ambiguous, and empty answers', () => {
  const questions = [{ question: 'One?', options: [{ label: 'A' }] }, { question: 'Two?' }];
  const data = new FormData();
  data.append('choice-0', '0');
  expect(terminalQuestionAnswers(questions, data)).toBeUndefined();
  data.append('other-1', 'text');
  expect(terminalQuestionAnswers(questions, data)).toEqual({ 'One?': 'A', 'Two?': 'text' });
  expect(terminalQuestionAnswers([{ question: 'Same?' }, { question: 'Same?' }], data)).toBeUndefined();
});
