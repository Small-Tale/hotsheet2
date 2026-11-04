import {
  choiceHeader,
  dedent,
  FENCE,
  HEADING,
  indentOf,
  interruptsParagraph,
  isBlank,
  letteredHeader,
  listItem,
  type SourceLine,
  sourceLines,
} from './feedback-choice-lines';

/**
 * The FEEDBACK NEEDED choice grammar (HS2-83YCDP, HS2-3MHSBS, HS2-EWF0FT). The Rust port in
 * `crates/hotsheet-model/src/feedback_choices.rs` implements the same grammar; both read the shared
 * corpus `crates/hotsheet-model/tests/fixtures/feedback-choices.json`, so change them together.
 */
export interface FeedbackChoice {
  id: string;
  /** The choice label: a list item's first line or a lettered block's header text. */
  markdown: string;
  /** Continuation lines, indented paragraphs, or a lettered block's following paragraphs (HS2-EWF0FT). */
  details?: string;
}
export interface FeedbackChoiceGroup {
  /** One-based position of the group within its note. */
  index: number;
  start: number;
  end: number;
  /** The question written after `CHOICE:` on the header line, else the last prose line before
   * the group; labels the group and its response. */
  question?: string;
  /** The question was written on the `CHOICE:` line itself, so no prose region shows it. */
  inlineQuestion?: boolean;
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

/** One choice under construction: its label line and the last line its details reach. */
interface PendingChoice {
  line: number;
  label: string;
  /** Leading details already on the label line (a lettered bold lead). */
  firstDetails?: string;
  /** Columns removed from detail lines; undefined removes their common indentation. */
  dedent?: number;
  lastLine: number;
}

/** The details Markdown of one choice: every line after the label line through its last line. */
function choiceDetails(lines: readonly SourceLine[], choice: PendingChoice): string | undefined {
  const raw = lines.slice(choice.line + 1, choice.lastLine + 1).map((line) => line.text),
    columns = choice.dedent ?? Math.min(...raw.filter((text) => !isBlank(text)).map(indentOf), Number.MAX_SAFE_INTEGER),
    body = raw.map((text) => dedent(text, columns).trimEnd());
  if (choice.firstDetails) body.unshift(choice.firstDetails);
  while (body.length && !body[0]) body.shift();
  while (body.length && !body[body.length - 1]) body.pop();
  return body.length ? body.join('\n') : undefined;
}

interface ScannedGroup {
  choices: PendingChoice[];
  lastLine: number;
}

/**
 * A Markdown list: each item's first line is its label; lazy continuation lines and lines
 * indented to the item's content (nested lists included, blank lines allowed) are its details.
 * A switch between bullet and numbered markers, or an unindented non-item line after a blank
 * line, ends the list.
 */
function scanList(lines: readonly SourceLine[], first: number): ScannedGroup {
  const firstItem = listItem(lines[first].text)!,
    choices: PendingChoice[] = [];
  let current: (PendingChoice & { indent: number }) | undefined,
    lastLine = first,
    blankBefore = false;
  for (let index = first; index < lines.length; index += 1) {
    const text = lines[index].text;
    if (isBlank(text)) {
      blankBefore = true;
      continue;
    }
    if (current && indentOf(text) >= current.indent + 2) {
      current.lastLine = index;
      lastLine = index;
      blankBefore = false;
      continue;
    }
    const item = listItem(text);
    if (item) {
      if (item.ordered !== firstItem.ordered) break;
      current = { line: index, label: item.label, dedent: item.contentColumn, lastLine: index, indent: item.indent };
      choices.push(current);
      lastLine = index;
      blankBefore = false;
      continue;
    }
    if (current && !blankBefore && !interruptsParagraph(text)) {
      current.lastLine = index;
      lastLine = index;
      continue;
    }
    break;
  }
  return { choices, lastLine };
}

/** End of the fenced block that opens at `index` (the closing line, or the last line). */
function fenceEnd(lines: readonly SourceLine[], index: number): number {
  const open = FENCE.exec(lines[index].text)![1];
  for (let next = index + 1; next < lines.length; next += 1) {
    const close = FENCE.exec(lines[next].text);
    if (close && close[1][0] === open[0] && close[1].length >= open.length) return next;
  }
  return lines.length - 1;
}

/**
 * A run of lettered blocks, A, B, C (or 1, 2, 3 after a keyword) in one marker style. A block
 * owns its header line's paragraph; the paragraphs between two headers belong to the earlier
 * block, while after the last block only its own paragraph and indented paragraphs do, so
 * trailing prose (a recommendation, say) stays prose. Headings and `CHOICE` headers end the run.
 */
function scanLettered(lines: readonly SourceLine[], first: number): ScannedGroup {
  const header = letteredHeader(lines[first].text)!,
    choices: PendingChoice[] = [];
  const isNext = (index: number) => {
    const next = letteredHeader(lines[index].text);
    return next?.style === header.style && next.ordinal === choices.length + 1;
  };
  let index = first;
  while (index < lines.length && isNext(index)) {
    const block = letteredHeader(lines[index].text)!,
      current: PendingChoice = {
        line: index,
        label: block.label,
        ...(block.details ? { firstDetails: block.details } : {}),
        lastLine: index,
      };
    choices.push(current);
    index += 1;
    // The header's own paragraph.
    while (
      index < lines.length &&
      !isBlank(lines[index].text) &&
      !interruptsParagraph(lines[index].text) &&
      letteredHeader(lines[index].text) === undefined
    ) {
      current.lastLine = index;
      index += 1;
    }
    // Following paragraphs: kept when indented, or when the next header follows them.
    const headerOnly = current.lastLine === current.line && !current.firstDetails;
    let probe = index,
      pendingEnd: number | undefined,
      firstUnitEnd: number | undefined,
      foundNext = false;
    for (;;) {
      while (probe < lines.length && isBlank(lines[probe].text)) probe += 1;
      if (probe >= lines.length) break;
      if (isNext(probe)) {
        if (pendingEnd !== undefined) current.lastLine = pendingEnd;
        foundNext = true;
        break;
      }
      const text = lines[probe].text;
      if (HEADING.test(text) || choiceHeader(text) !== undefined || letteredHeader(text) !== undefined) break;
      let unitEnd = probe;
      if (FENCE.test(text)) unitEnd = fenceEnd(lines, probe);
      else
        while (
          unitEnd + 1 < lines.length &&
          !isBlank(lines[unitEnd + 1].text) &&
          !interruptsParagraph(lines[unitEnd + 1].text) &&
          letteredHeader(lines[unitEnd + 1].text) === undefined
        )
          unitEnd += 1;
      if (pendingEnd === undefined && indentOf(text) >= block.indent + 2) current.lastLine = unitEnd;
      else pendingEnd = unitEnd;
      firstUnitEnd ??= unitEnd;
      probe = unitEnd + 1;
    }
    // A last block written as a bare header takes the next paragraph as its details when an
    // earlier block in the run has details; one-line runs keep trailing prose as prose.
    if (
      !foundNext &&
      headerOnly &&
      current.lastLine === current.line &&
      firstUnitEnd !== undefined &&
      choices.slice(0, -1).some((choice) => choice.lastLine > choice.line || choice.firstDetails)
    )
      current.lastLine = firstUnitEnd;
    index = foundNext ? probe : lines.length;
  }
  return { choices, lastLine: choices.at(-1)!.lastLine };
}

/** The group a header (or a headerless lettered run) opens at `first`, if any. */
function scanGroup(lines: readonly SourceLine[], first: number, explicitHeader: boolean): ScannedGroup | undefined {
  if (first >= lines.length) return undefined;
  if (explicitHeader && listItem(lines[first].text)) return scanList(lines, first);
  if (letteredHeader(lines[first].text)?.ordinal !== 1) return undefined;
  const group = scanLettered(lines, first);
  // Without a CHOICE header, one lettered paragraph is ordinary prose.
  return explicitHeader || group.choices.length >= 2 ? group : undefined;
}

/**
 * Parse every choice group: an uppercase `CHOICE` (or `CHOICE:`, optionally `CHOICE: <question>`)
 * header followed by a Markdown list or lettered blocks, or a headerless run of at least two
 * lettered blocks. Blank lines may separate the header from its choices; headers inside fenced
 * code are ignored. Returns undefined when the note has no choice group.
 */
export function parseFeedbackChoices(source: string): FeedbackChoiceDocument | undefined {
  const lines = sourceLines(source),
    groups: FeedbackChoiceGroup[] = [],
    regions: FeedbackChoiceRegion[] = [];
  let fence: string | undefined,
    proseStart = 0;
  for (let lineIndex = 0; lineIndex < lines.length; lineIndex += 1) {
    const text = lines[lineIndex].text,
      fenceMatch = FENCE.exec(text);
    if (fence) {
      if (fenceMatch && fenceMatch[1][0] === fence[0] && fenceMatch[1].length >= fence.length) fence = undefined;
      continue;
    }
    if (fenceMatch) {
      fence = fenceMatch[1];
      continue;
    }
    const header = choiceHeader(text);
    let scanned: ScannedGroup | undefined;
    if (header) {
      let first = lineIndex + 1;
      while (first < lines.length && isBlank(lines[first].text)) first += 1;
      scanned = scanGroup(lines, first, true);
    } else scanned = scanGroup(lines, lineIndex, false);
    if (!scanned?.choices.length) continue;
    const groupIndex = groups.length + 1,
      start = lines[lineIndex].start,
      end = lines[scanned.lastLine].end,
      before = region(source, proseStart, start);
    regions.push(before);
    groups.push({
      index: groupIndex,
      start,
      end,
      question: header?.question ?? questionLine(before.source),
      ...(header?.question ? { inlineQuestion: true } : {}),
      choices: scanned.choices.map((choice, index) => {
        const details = choiceDetails(lines, choice);
        return {
          id: feedbackChoiceId(groupIndex, index + 1),
          markdown: choice.label,
          ...(details ? { details } : {}),
        };
      }),
    });
    proseStart = end;
    lineIndex = scanned.lastLine;
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
