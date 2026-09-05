export interface FeedbackChoice { id: string; markdown: string }
export interface FeedbackChoiceGroup { start: number; end: number; afterStart: number; before: string; after: string; choices: FeedbackChoice[] }
export interface FeedbackChoiceModifiers { additive?: boolean; range?: boolean }
export interface FeedbackChoiceSelection { selected: string[]; anchor?: string }

interface SourceLine { start: number; end: number; text: string }

function sourceLines(source: string): SourceLine[] {
  const lines: SourceLine[] = [];
  let start = 0;
  for (const match of source.matchAll(/[^\n]*(?:\n|$)/g)) {
    if (!match[0]) continue;
    lines.push({ start, end: start + match[0].length, text: match[0].replace(/\n$/, '') });
    start += match[0].length;
  }
  return lines;
}

/** Parse the first uppercase CHOICE block followed by a Markdown list. */
export function parseFeedbackChoices(source: string): FeedbackChoiceGroup | undefined {
  const lines = sourceLines(source);
  const headerIndex = lines.findIndex(line => /^\s*CHOICE:?\s*$/.test(line.text));
  if (headerIndex < 0) return undefined;
  const choices: FeedbackChoice[] = [];
  let lineIndex = headerIndex + 1;
  let end = lines[headerIndex].end;
  for (; lineIndex < lines.length; lineIndex += 1) {
    const item = lines[lineIndex].text.match(/^\s*(?:[-+*]|\d+[.)])\s+(.+?)\s*$/);
    if (!item) break;
    choices.push({ id: `choice-${choices.length + 1}`, markdown: item[1] });
    end = lines[lineIndex].end;
  }
  if (!choices.length) return undefined;
  const start = lines[headerIndex].start;
  const remainder = source.slice(end);
  const leadingBreaks = remainder.match(/^\n+/)?.[0].length ?? 0;
  return { start, end, afterStart: end + leadingBreaks, before: source.slice(0, start).replace(/\n+$/, ''), after: remainder.slice(leadingBreaks), choices };
}

export function updateFeedbackChoiceSelection(choiceIds: readonly string[], current: readonly string[], clicked: string, anchor: string | undefined, modifiers: FeedbackChoiceModifiers): FeedbackChoiceSelection {
  if (!choiceIds.includes(clicked)) return { selected: [...current], anchor };
  if (modifiers.range && anchor && choiceIds.includes(anchor)) {
    const [start, end] = [choiceIds.indexOf(anchor), choiceIds.indexOf(clicked)].sort((left, right) => left - right);
    const selected = new Set(current);
    for (const id of choiceIds.slice(start, end + 1)) selected.add(id);
    return { selected: choiceIds.filter(id => selected.has(id)), anchor };
  }
  if (modifiers.additive) {
    const selected = new Set(current);
    if (selected.has(clicked)) selected.delete(clicked); else selected.add(clicked);
    return { selected: choiceIds.filter(id => selected.has(id)), anchor: clicked };
  }
  return { selected: current.length === 1 && current[0] === clicked ? [] : [clicked], anchor: clicked };
}

export function selectedFeedbackChoicesMarkdown(source: string, selectedChoiceIds: readonly string[]): string {
  const group = parseFeedbackChoices(source);
  if (!group) return '';
  const selected = new Set(selectedChoiceIds);
  const choices = group.choices.filter(choice => selected.has(choice.id));
  if (!choices.length) return '';
  return `Selected choice${choices.length === 1 ? '' : 's'}:\n${choices.map(choice => `- ${choice.markdown.replace(/\n/g, '\n  ')}`).join('\n')}`;
}
