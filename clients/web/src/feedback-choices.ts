export interface FeedbackChoice {
  id: string;
  markdown: string;
}
export interface FeedbackChoiceGroup {
  /** One-based position of the group within its note. */
  index: number;
  start: number;
  end: number;
  /** The last prose line before the `CHOICE` header, used to label the group and its response. */
  question?: string;
  choices: FeedbackChoice[];
}
/** Prose that surrounds the choice groups, kept with its source offset for inline replies. */
export interface FeedbackChoiceRegion {
  source: string;
  sourceStart: number;
}
/** A FEEDBACK NEEDED prompt split into prose regions and choice groups: `regions[i]` precedes `groups[i]`. */
export interface FeedbackChoiceDocument {
  groups: FeedbackChoiceGroup[];
  regions: FeedbackChoiceRegion[];
}
export interface FeedbackChoiceModifiers {
  additive?: boolean;
  range?: boolean;
}
export interface FeedbackChoiceSelection {
  selected: string[];
  anchor?: string;
}

interface SourceLine {
  start: number;
  end: number;
  text: string;
}

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

const CHOICE_HEADER = /^\s*CHOICE:?\s*$/;
const LIST_ITEM = /^\s*([-+*]|\d+[.)])\s+(.+?)\s*$/;
const FENCE = /^\s*(`{3,}|~{3,})/;

/** Unique, stable choice id; group 1 keeps the original `choice-N` form. */
export function feedbackChoiceId(groupIndex: number, choiceIndex: number): string {
  return groupIndex === 1 ? `choice-${choiceIndex}` : `group-${groupIndex}-choice-${choiceIndex}`;
}

function questionLine(prose: string): string | undefined {
  const line = prose
    .split('\n')
    .map((text) => text.trim())
    .filter(Boolean)
    .at(-1);
  if (!line) return undefined;
  const text = line
    .replace(/^#{1,6}\s+/, '')
    .replace(/^(\*\*|__)(.*)\1$/, '$2')
    .trim();
  return text || undefined;
}

function region(source: string, from: number, to: number): FeedbackChoiceRegion {
  const raw = source.slice(from, to),
    leading = raw.match(/^\n+/)?.[0].length ?? 0;
  return { source: raw.slice(leading).replace(/\n+$/, ''), sourceStart: from + leading };
}

/**
 * Parse every uppercase `CHOICE` (or `CHOICE:`) header followed by a Markdown list. Blank lines may
 * separate the header from its list and the list items from each other; headers inside fenced code
 * are ignored. Returns undefined when the note has no choice group.
 */
export function parseFeedbackChoices(source: string): FeedbackChoiceDocument | undefined {
  const lines = sourceLines(source),
    groups: FeedbackChoiceGroup[] = [],
    regions: FeedbackChoiceRegion[] = [];
  let fence: string | undefined,
    proseStart = 0;
  for (let lineIndex = 0; lineIndex < lines.length; lineIndex += 1) {
    const text = lines[lineIndex].text,
      fenceMatch = text.match(FENCE);
    if (fence) {
      if (fenceMatch && fenceMatch[1][0] === fence[0] && fenceMatch[1].length >= fence.length) fence = undefined;
      continue;
    }
    if (fenceMatch) {
      fence = fenceMatch[1];
      continue;
    }
    if (!CHOICE_HEADER.test(text)) continue;
    const choices: FeedbackChoice[] = [],
      groupIndex = groups.length + 1;
    let end = lines[lineIndex].end,
      lastItem = lineIndex,
      ordered: boolean | undefined;
    for (let next = lineIndex + 1; next < lines.length; next += 1) {
      if (!lines[next].text.trim()) continue;
      const item = lines[next].text.match(LIST_ITEM);
      // One list per group: a change between bullet and numbered markers ends it.
      if (!item || (ordered !== undefined && ordered !== /\d/.test(item[1]))) break;
      ordered = /\d/.test(item[1]);
      choices.push({ id: feedbackChoiceId(groupIndex, choices.length + 1), markdown: item[2] });
      end = lines[next].end;
      lastItem = next;
    }
    if (!choices.length) continue;
    const start = lines[lineIndex].start,
      before = region(source, proseStart, start);
    regions.push(before);
    groups.push({ index: groupIndex, start, end, question: questionLine(before.source), choices });
    proseStart = end;
    lineIndex = lastItem;
  }
  if (!groups.length) return undefined;
  regions.push(region(source, proseStart, source.length));
  return { groups, regions };
}

/** Find the group that owns `choiceId`. */
export function feedbackChoiceGroupFor(
  document: FeedbackChoiceDocument,
  choiceId: string,
): FeedbackChoiceGroup | undefined {
  return document.groups.find((group) => group.choices.some((choice) => choice.id === choiceId));
}

/**
 * Apply a click to a multi-group selection: the click's plain/additive/range semantics stay within
 * the clicked choice's group, and every other group's selection is preserved.
 */
export function updateFeedbackChoiceDocumentSelection(
  document: FeedbackChoiceDocument,
  current: readonly string[],
  clicked: string,
  anchor: string | undefined,
  modifiers: FeedbackChoiceModifiers,
): FeedbackChoiceSelection {
  const group = feedbackChoiceGroupFor(document, clicked);
  if (!group) return { selected: [...current], anchor };
  const ids = group.choices.map((choice) => choice.id),
    inGroup = new Set(ids),
    next = updateFeedbackChoiceSelection(
      ids,
      current.filter((id) => inGroup.has(id)),
      clicked,
      anchor && inGroup.has(anchor) ? anchor : undefined,
      modifiers,
    ),
    selected = new Set([...current.filter((id) => !inGroup.has(id)), ...next.selected]);
  return {
    selected: document.groups
      .flatMap((item) => item.choices.map((choice) => choice.id))
      .filter((id) => selected.has(id)),
    anchor: next.anchor,
  };
}

export function updateFeedbackChoiceSelection(
  choiceIds: readonly string[],
  current: readonly string[],
  clicked: string,
  anchor: string | undefined,
  modifiers: FeedbackChoiceModifiers,
): FeedbackChoiceSelection {
  if (!choiceIds.includes(clicked)) return { selected: [...current], anchor };
  if (modifiers.range && anchor && choiceIds.includes(anchor)) {
    const [start, end] = [choiceIds.indexOf(anchor), choiceIds.indexOf(clicked)].sort((left, right) => left - right);
    const selected = new Set(current);
    for (const id of choiceIds.slice(start, end + 1)) selected.add(id);
    return { selected: choiceIds.filter((id) => selected.has(id)), anchor };
  }
  if (modifiers.additive) {
    const selected = new Set(current);
    if (selected.has(clicked)) selected.delete(clicked);
    else selected.add(clicked);
    return { selected: choiceIds.filter((id) => selected.has(id)), anchor: clicked };
  }
  return { selected: current.length === 1 && current[0] === clicked ? [] : [clicked], anchor: clicked };
}

function choiceBullets(choices: readonly FeedbackChoice[]): string {
  return choices.map((choice) => `- ${choice.markdown.replace(/\n/g, '\n  ')}`).join('\n');
}

/** The selected choices of one group, in the backward-compatible `Selected choice(s):` form. */
export function selectedFeedbackGroupMarkdown(
  group: FeedbackChoiceGroup,
  selectedChoiceIds: readonly string[],
): string {
  const selected = new Set(selectedChoiceIds),
    choices = group.choices.filter((choice) => selected.has(choice.id));
  if (!choices.length) return '';
  return `Selected choice${choices.length === 1 ? '' : 's'}:\n${choiceBullets(choices)}`;
}

/**
 * Markdown listing the selected choices. A single-group prompt keeps the `Selected choice(s):` form;
 * a multi-group prompt lists each answered group under its bold question line.
 */
export function selectedFeedbackChoicesMarkdown(source: string, selectedChoiceIds: readonly string[]): string {
  const document = parseFeedbackChoices(source);
  if (!document) return '';
  if (document.groups.length === 1) return selectedFeedbackGroupMarkdown(document.groups[0], selectedChoiceIds);
  const selected = new Set(selectedChoiceIds);
  return document.groups
    .map((group) => {
      const choices = group.choices.filter((choice) => selected.has(choice.id));
      if (!choices.length) return '';
      return `**${group.question ?? `Choice ${group.index}`}**\n${choiceBullets(choices)}`;
    })
    .filter(Boolean)
    .join('\n\n');
}
