import type { TokenSearchFieldValue } from '@kerfjs/ui/token-search-field';
import {
  createTokenSearchModel,
  type TokenSearchModel,
  type TokenSearchRule,
  type TokenSearchState,
} from '@kerfjs/ui/token-search-model';

import {
  consumeSearchTokens,
  type InlineSearchToken,
  parseSearchDate,
  type SearchDateDirection,
  type SearchDateField,
  searchDateLabel,
  tokenFromRaw,
  toTokenSearchToken,
} from './inline-search';

/**
 * The ticket search grammar as Kerf `TokenSearchModel` rules (HS2-5JXBQY, evaluated in
 * HS2-HHRYP9 / HS2-06Q4MG): `tag`, `is`, `has`, and `attachment`, plus the twelve lifecycle date
 * filters as hyphenated rule names. Every rule parses through the app's own `tokenFromRaw`, so a
 * chip Kerf commits is exactly a chip the app parser would have produced, and the `tag` rule's
 * suggestions exclude tags already committed as chips.
 */
export const TICKET_SEARCH_DATE_FIELDS: readonly SearchDateField[] = [
  'created',
  'completed',
  'started',
  'verified',
  'archived',
  'updated',
];
const DATE_DIRECTIONS: readonly SearchDateDirection[] = ['before', 'after'];
/** The most suggestions a `tag:` prefix offers at once. */
export const TICKET_SEARCH_TAG_SUGGESTION_LIMIT = 8;

const quoteTag = (tag: string) => (/\s/.test(tag) ? `"${tag}"` : tag);
const parseAs = (kind: InlineSearchToken['kind'], name: string) => (input: string) => {
  const token = tokenFromRaw(`${name}:${quoteTag(input)}`);
  return token?.kind === kind ? token.value : undefined;
};

export interface TicketSearchModelOptions {
  /** Every tag the project offers, canonical casing, read lazily when a `tag:` prefix is typed. */
  readonly tags: () => readonly string[];
  readonly initial?: TokenSearchFieldValue;
  /** Runs after Kerf's managed clear, for app state that belongs to the field (open help, errors). */
  readonly onClear?: () => void;
}

/** The grammar rules for one ticket search model. */
export function ticketSearchRules(tags: () => readonly string[]): TokenSearchRule[] {
  return [
    {
      name: 'tag',
      parse: (input) => tags().find((tag) => tag.toLowerCase() === input.toLowerCase()) ?? input,
      label: (value) => `tag:${value}`,
      suggest: (input, state) => {
        // Kerf hands an unfinished quoted value through with its opening quote.
        const prefix = input.replace(/^"/, '').toLowerCase(),
          committed = new Set(
            state.tokens.filter((token) => token.kind === 'tag').map((token) => token.parsedValue.toLowerCase()),
          );
        return tags()
          .filter((tag) => !committed.has(tag.toLowerCase()) && tag.toLowerCase().startsWith(prefix))
          .slice(0, TICKET_SEARCH_TAG_SUGGESTION_LIMIT)
          .map((tag) => ({ value: tag, label: `tag:${quoteTag(tag)}` }));
      },
    },
    { name: 'is', parse: parseAs('is', 'is') },
    {
      name: 'has',
      parse: parseAs('has', 'has'),
      label: (value) => `has ${value.replace('-', ' ')}`,
    },
    { name: 'attachment', parse: parseAs('attachment', 'attachment') },
    ...TICKET_SEARCH_DATE_FIELDS.flatMap((field) =>
      DATE_DIRECTIONS.map((direction): TokenSearchRule => ({
        name: `${field}-${direction}`,
        parse: (input) => parseSearchDate(input),
        label: (_value, input) => searchDateLabel(field, direction, input),
      })),
    ),
  ];
}

/**
 * A Kerf-managed ticket search model: parsing, chips, suggestions, edit/remove/clear/commit/replace
 * all belong to Kerf; the app projects `state` into its own signals and reacts to clears.
 */
export function createTicketSearchModel({ tags, initial, onClear }: TicketSearchModelOptions): TokenSearchModel {
  const model = createTokenSearchModel({ rules: ticketSearchRules(tags), initial });
  const normalized = (value: TokenSearchFieldValue): TokenSearchFieldValue => ({
    ...value,
    query: quoteRelativeDates(value.query),
  });
  return {
    ...model,
    edit(value, commit) {
      model.edit(normalized(value), commit);
    },
    submit(value) {
      model.submit(normalized(value));
    },
    replace(value) {
      model.replace(normalized(value));
    },
    clear() {
      model.clear();
      onClear?.();
    },
  };
}

const RELATIVE_DATE_FILTER =
  /((?:created|completed|started|verified|archived|updated)-(?:before|after):)(\d+(?:\.\d+)?\s*[mhdw])\s+ago(?=$|[\s)])/gi;

/**
 * Kerf's grammar takes an unquoted value up to the next whitespace, so the documented relative
 * syntax `updated-after:4h ago` is quoted (`updated-after:"4h ago"`) before Kerf parses it; the
 * quoted form is the chip's canonical value and the app parser reads both.
 */
export function quoteRelativeDates(query: string): string {
  return query.replace(RELATIVE_DATE_FILTER, (_match, prefix: string, amount: string) => `${prefix}"${amount} ago"`);
}

/** The model's committed chips as the app's structured tokens (parsed values, raw, label, offset). */
export function inlineSearchTokens(state: TokenSearchState): InlineSearchToken[] {
  return state.tokens.flatMap((token) => {
    const parsed = tokenFromRaw(token.value);
    return parsed ? [{ ...parsed, offset: token.offset }] : [];
  });
}

/**
 * Replace a model's query from plain text (a restored session, a seeded saved-view query): complete
 * filters in the text become chips, through the app parser so a trailing filter commits too.
 */
export function replaceTicketSearch(model: TokenSearchModel, text: string): void {
  const parsed = consumeSearchTokens(text, true);
  model.replace({ query: parsed.text, tokens: parsed.tokens.map(toTokenSearchToken) });
}
