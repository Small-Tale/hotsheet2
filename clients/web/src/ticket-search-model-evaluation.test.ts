import { createTokenSearchModel, type TokenSearchRule } from '@kerfjs/ui/token-search-model';
import { describe, expect, it } from 'vitest';

import { parseSearchDate, tokenFromRaw } from './inline-search';

// HS2-HHRYP9: does Kerf 5.0.0-beta.59's managed TokenSearchModel express the ticket search
// grammar that TicketSearchField parses itself today? These tests are the evaluation record:
// the grammar fits, and the remaining blockers are model API gaps recorded in docs/ux-components.md.
const lifecycle = [
  'up-next',
  'active',
  'open',
  'closed',
  'duplicate',
  'not-started',
  'started',
  'completed',
  'verified',
  'backlog',
  'backlogged',
  'archived',
];
const dateFields = ['created', 'completed', 'started', 'verified', 'archived', 'updated'];
const tags = ['client', 'needs design', 'server'];
const rules: TokenSearchRule[] = [
  {
    name: 'tag',
    parse: (input) => tags.find((tag) => tag.toLowerCase() === input.toLowerCase()) ?? input,
    suggest: (input) => tags.filter((tag) => tag.toLowerCase().startsWith(input.toLowerCase())),
  },
  { name: 'is', parse: (input) => (lifecycle.includes(input.toLowerCase()) ? input.toLowerCase() : undefined) },
  {
    name: 'has',
    parse: (input) =>
      ['attachment', 'media-annotation', 'commit'].includes(input.toLowerCase()) ? input.toLowerCase() : undefined,
    label: (value) => `has ${value.replace('-', ' ')}`,
  },
  { name: 'attachment' },
  ...dateFields.flatMap((field) =>
    (['before', 'after'] as const).map((direction): TokenSearchRule => ({
      name: `${field}-${direction}`,
      parse: (input) => parseSearchDate(input),
      label: (_value, input) => `${field} ${direction} ${input}`,
    })),
  ),
];

describe('Kerf TokenSearchModel against the ticket search grammar (HS2-HHRYP9)', () => {
  it('commits every ticket token kind, keeps boolean text and parentheses as text, and matches the app parser', () => {
    const model = createTokenSearchModel({ rules });
    model.edit({
      query: '(client OR server) AND NOT is:archived tag:"needs design" has:commit attachment:*.png ',
      tokens: [],
    });
    // One separator survives per consumed token, exactly like the app's own `consumeSearchTokens`.
    expect(model.state.value.query).toBe('(client OR server) AND NOT    ');
    expect(model.state.value.tokens.map((token) => [token.kind, token.parsedValue, token.value])).toEqual([
      ['is', 'archived', 'is:archived'],
      ['tag', 'needs design', 'tag:"needs design"'],
      ['has', 'commit', 'has:commit'],
      ['attachment', '*.png', 'attachment:*.png'],
    ]);
    expect(model.state.value.tokens.map((token) => token.label)).toEqual([
      'is:archived',
      'tag:needs design',
      'has commit',
      'attachment:*.png',
    ]);
    for (const token of model.state.value.tokens) expect(tokenFromRaw(token.value)?.kind).toBe(token.kind);
  });

  it('expresses the compound lifecycle date prefixes as hyphenated rule names', () => {
    const model = createTokenSearchModel({ rules });
    model.edit({ query: 'updated-after:2026-09-01T11:05 ', tokens: [] });
    const [token] = model.state.value.tokens;
    expect(token.kind).toBe('updated-after');
    expect(token.parsedValue).toBe(parseSearchDate('2026-09-01T11:05'));
    expect(token.label).toBe('updated after 2026-09-01T11:05');
    // An unparseable date stays text, like the app parser.
    model.edit({ query: 'created-before:someday ', tokens: [] });
    expect(model.state.value.tokens).toEqual([]);
    expect(model.state.value.query).toBe('created-before:someday ');
  });

  it('serves tag suggestions for the trailing tag: prefix and commits the chosen chip', () => {
    const model = createTokenSearchModel({ rules });
    model.edit({ query: 'parser tag:ne', tokens: [] });
    expect(model.suggestions.value).toEqual([{ value: 'tag:"needs design"', label: 'tag:needs design' }]);
    model.choose('tag:"needs design"');
    expect(model.state.value).toMatchObject({ query: 'parser ', tokens: [{ value: 'tag:"needs design"' }] });
    expect(model.suggestions.value).toEqual([]);
  });

  it('records the API gaps that block adoption today', () => {
    const model = createTokenSearchModel({ rules });
    // 1. `suggest` sees only the typed input, not the committed tokens, so it cannot exclude a tag
    //    that is already a chip (TicketSearchField's suggestions do).
    model.edit({ query: 'tag:client ', tokens: [] });
    model.edit({ query: 'tag:cl', tokens: model.state.value.tokens });
    expect(model.suggestions.value.map((item) => item.value)).toEqual(['tag:client']);
    // 2. `choose` accepts only a value among the current suggestions, so an external helper (the
    //    lifecycle date picker) cannot commit a computed token for the active prefix.
    model.edit({ query: 'created-after:', tokens: [] });
    model.choose('created-after:2026-09-01');
    expect(model.state.value.tokens).toEqual([]);
    // 3. Only `clear()` bumps `editorRevision`; a programmatic replacement (applying a saved view,
    //    restoring a session) has no model action that rebuilds the DOM-owned editor text.
    const revision = model.editorRevision.value;
    model.edit({ query: 'restored text', tokens: [] }, true);
    expect(model.editorRevision.value).toBe(revision);
    model.clear();
    expect(model.editorRevision.value).toBe(revision + 1);
    expect(typeof (model as { replace?: unknown }).replace).toBe('undefined');
  });
});
