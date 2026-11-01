import { isTicketActivelyWorkedOn } from './active-ticket-work';
import type { TicketRow } from './api';
import { type InlineSearchToken, tokenFromRaw } from './inline-search';

const normalized = (value: string) => value.trim().toLowerCase();
type SearchExpression =
  | { kind: 'term'; value: string }
  | { kind: 'not'; value: SearchExpression }
  | { kind: 'and' | 'or'; left: SearchExpression; right: SearchExpression };
type Lexeme = { kind: 'term' | 'and' | 'or' | 'not' | 'left' | 'right'; value: string };
const lifecycleAliases: Partial<Record<string, string[]>> = {
  'not-started': ['not_started'],
  started: ['started'],
  completed: ['completed'],
  verified: ['verified'],
  backlog: ['backlog'],
  backlogged: ['backlog'],
  archived: ['archive'],
  open: ['not_started', 'started'],
  closed: ['completed', 'verified', 'archive'],
};

function lexSearchExpression(query: string): Lexeme[] {
  const result: Lexeme[] = [];
  let index = 0;
  while (index < query.length) {
    if (/\s/.test(query[index])) {
      index += 1;
      continue;
    }
    if (query[index] === '(') {
      result.push({ kind: 'left', value: '(' });
      index += 1;
      continue;
    }
    if (query[index] === ')') {
      result.push({ kind: 'right', value: ')' });
      index += 1;
      continue;
    }
    if (query[index] === '"') {
      let value = '',
        closed = false;
      index += 1;
      while (index < query.length) {
        const character = query[index++];
        if (character === '\\' && index < query.length) {
          value += query[index++];
          continue;
        }
        if (character === '"') {
          closed = true;
          break;
        }
        value += character;
      }
      result.push({ kind: 'term', value: closed ? value : `"${value}` });
      continue;
    }
    const start = index;
    let quoted = false;
    while (index < query.length) {
      const character = query[index];
      if (character === '\\' && quoted && index + 1 < query.length) {
        index += 2;
        continue;
      }
      if (character === '"') quoted = !quoted;
      if (!quoted && /[\s()]/.test(character)) break;
      index += 1;
    }
    const value = query.slice(start, index),
      keyword = value.toLowerCase();
    result.push({ kind: keyword === 'and' || keyword === 'or' || keyword === 'not' ? keyword : 'term', value });
  }
  return result;
}

function parseSearchExpression(query: string): SearchExpression | undefined {
  const lexemes = lexSearchExpression(query);
  if (!lexemes.length) return undefined;
  let cursor = 0;
  const primary = (): SearchExpression | undefined => {
    const token = lexemes.at(cursor);
    if (!token) return undefined;
    if (token.kind === 'term') {
      cursor += 1;
      return { kind: 'term', value: token.value };
    }
    if (token.kind === 'left') {
      cursor += 1;
      const value = or();
      if (!value || lexemes.at(cursor)?.kind !== 'right') return undefined;
      cursor += 1;
      return value;
    }
    return undefined;
  };
  const not = (): SearchExpression | undefined => {
    if (lexemes[cursor]?.kind === 'not') {
      cursor += 1;
      const value = not();
      return value ? { kind: 'not', value } : undefined;
    }
    return primary();
  };
  const and = (): SearchExpression | undefined => {
    let left = not();
    if (!left) return undefined;
    while (cursor < lexemes.length && lexemes[cursor].kind !== 'right' && lexemes[cursor].kind !== 'or') {
      if (lexemes[cursor].kind === 'and') cursor += 1;
      const right = not();
      if (!right) return undefined;
      left = { kind: 'and', left, right };
    }
    return left;
  };
  const or = (): SearchExpression | undefined => {
    let left = and();
    if (!left) return undefined;
    while (lexemes[cursor]?.kind === 'or') {
      cursor += 1;
      const right = and();
      if (!right) return undefined;
      left = { kind: 'or', left, right };
    }
    return left;
  };
  const expression = or();
  return expression && cursor === lexemes.length ? expression : undefined;
}

function ticketContains(ticket: TicketRow, value: string) {
  const needle = normalized(value);
  return (
    Boolean(needle) &&
    [
      ticket.slug,
      ticket.native_id,
      ticket.qualified_id,
      ticket.title,
      ticket.details ?? '',
      ...ticket.tags,
      ...(ticket.notes ?? []).map((note) => note.text),
    ].some((field) => normalized(field).includes(needle))
  );
}
function matchesTerm(ticket: TicketRow, value: string, now: number) {
  const tag = value.match(/^tag:("(?:\\.|[^"])*"|\S+)$/i);
  if (tag) {
    const raw = tag[1],
      name = raw.startsWith('"') ? raw.slice(1, -1).replaceAll('\\"', '"') : raw;
    return ticket.tags.some((item) => normalized(item) === normalized(name));
  }
  const match = value.match(/^is:(.+)$/i),
    state = match?.[1].toLowerCase();
  if (!state) return ticketContains(ticket, value);
  if (state === 'up-next') return ticket.up_next;
  if (state === 'active') return isTicketActivelyWorkedOn(ticket, now);
  if (state === 'needs-review') return ticket.needs_review ?? ticket.feedback_needed;
  if (state === 'duplicate') return ticket.close_reason === 'duplicate';
  const statuses = lifecycleAliases[state];
  return statuses ? statuses.includes(ticket.status ?? '') : false;
}
type StructuredTerm = Extract<InlineSearchToken, { kind: 'has' | 'attachment' | 'date' }>;
function structuredTerm(value: string): StructuredTerm | undefined {
  const token = tokenFromRaw(value);
  return token?.kind === 'has' || token?.kind === 'attachment' || token?.kind === 'date' ? token : undefined;
}
export function structuredSearchTerms(query: string): StructuredTerm[] {
  const expression = parseSearchExpression(query),
    terms = new Map<string, StructuredTerm>();
  const visit = (node: SearchExpression): void => {
    if (node.kind === 'term') {
      const term = structuredTerm(node.value);
      if (term) terms.set(term.raw, term);
    } else if (node.kind === 'not') visit(node.value);
    else {
      visit(node.left);
      visit(node.right);
    }
  };
  if (expression) visit(expression);
  return [...terms.values()];
}
export type StructuredTermResolver = (ticket: TicketRow, term: StructuredTerm) => boolean;
function evaluateSearchExpression(
  ticket: TicketRow,
  expression: SearchExpression,
  now: number,
  resolve?: StructuredTermResolver,
): boolean {
  switch (expression.kind) {
    case 'term':
      if (resolve) {
        const term = structuredTerm(expression.value);
        if (term) return resolve(ticket, term);
      }
      return matchesTerm(ticket, expression.value, now);
    case 'not':
      return !evaluateSearchExpression(ticket, expression.value, now, resolve);
    case 'and':
      return (
        evaluateSearchExpression(ticket, expression.left, now, resolve) &&
        evaluateSearchExpression(ticket, expression.right, now, resolve)
      );
    case 'or':
      return (
        evaluateSearchExpression(ticket, expression.left, now, resolve) ||
        evaluateSearchExpression(ticket, expression.right, now, resolve)
      );
  }
}
export function usesBooleanSearchExpression(query: string) {
  return /(?:^|[\s(])(?:AND|OR|NOT)(?=$|[\s)])|[()]/i.test(query);
}
export function usesAdvancedSearchExpression(query: string) {
  return (
    usesBooleanSearchExpression(query) ||
    /(?:^|\s)is:(?:up-next|active|needs-review|open|closed|duplicate|not-started|started|completed|verified|backlog|backlogged|archived)(?=$|\s|\))/i.test(
      query,
    )
  );
}
export function matchesSearchExpression(
  ticket: TicketRow,
  query: string,
  now = Date.now(),
  resolve?: StructuredTermResolver,
) {
  const expression = parseSearchExpression(query);
  return expression ? evaluateSearchExpression(ticket, expression, now, resolve) : ticketContains(ticket, query);
}
export async function collectMatchingSearchPages<T>(
  first: { items: T[]; next_cursor?: string },
  load: (cursor: string) => Promise<{ items: T[]; next_cursor?: string }>,
  matches: (item: T) => boolean,
  current: () => boolean = () => true,
): Promise<T[] | undefined> {
  const result: T[] = [],
    seen = new Set<string>();
  let page = first;
  while (current()) {
    result.push(...page.items.filter(matches));
    const cursor = page.next_cursor;
    if (!cursor) return result;
    if (seen.has(cursor)) throw new Error('Search pagination returned a repeated cursor.');
    seen.add(cursor);
    page = await load(cursor);
  }
  return undefined;
}
/** Inline search filters only expressions the provider cannot represent directly. */
export function filterAdvancedSearchResults(rows: TicketRow[], query: string, resolve?: StructuredTermResolver) {
  const advanced = usesAdvancedSearchExpression(query);
  const now = Date.now();
  return rows.filter((ticket) => !advanced || matchesSearchExpression(ticket, query, now, resolve));
}
