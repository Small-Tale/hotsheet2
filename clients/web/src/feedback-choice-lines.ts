/**
 * Line classification for the FEEDBACK NEEDED choice grammar (HS2-EWF0FT): headers, list items,
 * lettered block leads, fences, and the lines that interrupt a paragraph. `feedback-choices.ts`
 * scans groups from these; the Rust port mirrors both in `crates/hotsheet-model`.
 */
export interface SourceLine {
  start: number;
  end: number;
  text: string;
}

export function sourceLines(source: string): SourceLine[] {
  const lines: SourceLine[] = [];
  let start = 0;
  for (const match of source.matchAll(/[^\n]*(?:\n|$)/g)) {
    if (!match[0]) continue;
    lines.push({ start, end: start + match[0].length, text: match[0].replace(/\n$/, '') });
    start += match[0].length;
  }
  return lines;
}

/**
 * A choice group header: an uppercase `CHOICE` or `CHOICE:` line, optionally bold, optionally
 * carrying the group's question after the colon (`CHOICE: Which approach?`, HS2-3MHSBS). Text
 * after a bare `CHOICE` without a colon is ordinary prose, not a header.
 */
const CHOICE_HEADER = /^\s*(?:\*\*|__)?CHOICE(:)?(?:\*\*|__)?(?:\s+(.*?))?\s*$/;

export function choiceHeader(text: string): { question?: string } | undefined {
  const match = text.match(CHOICE_HEADER);
  if (!match) return undefined;
  // Optional capture groups are undefined when they do not participate in the match.
  const colon = match.at(1),
    inline = match.at(2)?.trim();
  if (!inline) return {};
  if (!colon) return undefined;
  const question = inline.replace(/^(\*\*|__)(.*)\1$/, '$2').trim();
  return question ? { question } : {};
}

export const FENCE = /^\s*(`{3,}|~{3,})/;
export const HEADING = /^ {0,3}#{1,6}(?:\s|$)/;
const BLOCKQUOTE = /^ {0,3}>/;
const THEMATIC_BREAK = /^ {0,3}([-*_])(?:[ \t]*\1){2,}\s*$/;

/** Leading indentation in columns; a tab counts as four. */
export function indentOf(text: string): number {
  let columns = 0;
  for (const char of text) {
    if (char === ' ') columns += 1;
    else if (char === '\t') columns += 4;
    else break;
  }
  return columns;
}

/** Remove up to `columns` columns of leading indentation. */
export function dedent(text: string, columns: number): string {
  let removed = 0,
    index = 0;
  while (index < text.length && removed < columns) {
    const char = text[index];
    if (char === ' ') removed += 1;
    else if (char === '\t') removed += 4;
    else break;
    index += 1;
  }
  return text.slice(index);
}

export function isBlank(text: string): boolean {
  return !text.trim();
}

export interface ListItemLine {
  indent: number;
  ordered: boolean;
  /** Column where the item's content starts; continuation lines are dedented by it. */
  contentColumn: number;
  label: string;
}

/** A Markdown bullet (`-`, `+`, `*`) or numbered (`1.`, `1)`) list item line. */
export function listItem(text: string): ListItemLine | undefined {
  const match = /^([ \t]*)([-+*]|\d+[.)])([ \t]+)([\s\S]*)$/.exec(text);
  if (!match) return undefined;
  const label = match[4].trim();
  if (!label) return undefined;
  const indent = indentOf(match[1]);
  return {
    indent,
    ordered: /\d/.test(match[2]),
    contentColumn: indent + match[2].length + indentOf(match[3]),
    label,
  };
}

export interface LetteredHeader {
  /** Marker family and ordinal kind; one group never mixes styles. */
  style: string;
  ordinal: number;
  indent: number;
  label: string;
  /** Text after a bold lead that carries its own label, e.g. `**CHOICE A: Label.** details`. */
  details?: string;
}

const LETTERED_KEYWORD = /^(CHOICE|Option|OPTION)[ \t]+([A-Z]|[1-9]\d?)(?![A-Za-z0-9])[ \t]*([:.)—–-])?/;
const LETTERED_BARE = /^([A-Z])([.):])/;

/**
 * A lettered choice block header (HS2-EWF0FT): `CHOICE A: ...`, `Option B — ...`, or a bare
 * `C) ...` / `C. ...` lead, optionally bold. Keyword forms also accept numbers (`Option 2:`).
 */
export function letteredHeader(text: string): LetteredHeader | undefined {
  const indentMatch = /^ {0,3}/.exec(text)?.[0] ?? '',
    afterIndent = text.slice(indentMatch.length),
    bold = afterIndent.startsWith('**') ? '**' : afterIndent.startsWith('__') ? '__' : '',
    body = afterIndent.slice(bold.length);
  let style: string, ordinal: number, marker: string, separated: boolean;
  const keyword = LETTERED_KEYWORD.exec(body);
  if (keyword) {
    const value = keyword[2];
    style = `${keyword[1].toLowerCase()}-${/\d/.test(value) ? 'number' : 'letter'}`;
    ordinal = /\d/.test(value) ? Number(value) : value.charCodeAt(0) - 64;
    marker = keyword[0];
    separated = keyword.at(3) !== undefined;
  } else {
    const bare = LETTERED_BARE.exec(body);
    if (!bare) return undefined;
    style = 'bare-letter';
    ordinal = bare[1].charCodeAt(0) - 64;
    marker = bare[0];
    separated = true;
  }
  const rest = body.slice(marker.length);
  if (bold) {
    const close = rest.indexOf(bold);
    if (close < 0) return undefined;
    const inner = rest.slice(0, close),
      after = rest.slice(close + bold.length).trim(),
      lead = body.slice(0, marker.length + close).trim();
    if (!separated && inner.trim()) return undefined;
    if (inner && !/^\s/.test(inner)) return undefined;
    if (inner.trim())
      return { style, ordinal, indent: indentMatch.length, label: lead, ...(after ? { details: after } : {}) };
    return { style, ordinal, indent: indentMatch.length, label: after ? `${lead} ${after}` : lead };
  }
  if (!separated || (rest && !/^\s/.test(rest))) return undefined;
  return { style, ordinal, indent: indentMatch.length, label: afterIndent.trim() };
}

/** A line that cannot lazily continue the paragraph before it. */
export function interruptsParagraph(text: string): boolean {
  return (
    FENCE.test(text) ||
    HEADING.test(text) ||
    BLOCKQUOTE.test(text) ||
    THEMATIC_BREAK.test(text) ||
    choiceHeader(text) !== undefined
  );
}
