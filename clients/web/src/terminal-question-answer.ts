import type { TerminalQuestion } from './api';

/** Extract the complete answer map Claude's AskUserQuestion tool expects. */
export function terminalQuestionAnswers(
  questions: NonNullable<TerminalQuestion['questions']>,
  data: FormData,
): Record<string, string> | undefined {
  if (questions.length === 0 || questions.length > 4) return;
  const answers: Record<string, string> = {};
  for (const [index, question] of questions.entries()) {
    if (!question.question || Object.hasOwn(answers, question.question)) return;
    const options = question.options ?? [];
    const selected = data.getAll(`choice-${index}`).map(String);
    const other = data.get(`other-${index}`);
    const free = typeof other === 'string' ? other.trim() : '';
    const labels = selected.flatMap((choice) => {
      if (choice === 'other') return free ? [free] : [];
      const optionIndex = Number(choice);
      return Number.isInteger(optionIndex) && options[optionIndex] ? [options[optionIndex].label] : [];
    });
    if (options.length === 0 && free) labels.push(free);
    if ((!question.multiSelect && labels.length !== 1) || labels.length === 0) return;
    answers[question.question] = labels.join(', ');
  }
  return answers;
}
