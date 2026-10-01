import { createTokenSearchModel, type TokenSearchRule, type TokenSearchState } from '@kerfjs/ui/token-search-model';
import { describe, expect, it } from 'vitest';

import { parseSearchDate, tokenFromRaw } from './inline-search';

// HS2-HHRYP9: does Kerf's managed TokenSearchModel express the ticket search grammar that
// TicketSearchField parses itself today? These tests are the evaluation record: the grammar fits,
// and the three beta.59 API gaps recorded in docs/ux-components.md are closed in beta.62
// (HS2-06Q4MG), so the migration (HS2-5JXBQY) has every model action it needs.
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

  it('confirms beta.62 closed the three API gaps recorded against beta.59 (HS2-06Q4MG)', () => {
    const model = createTokenSearchModel({
      rules: rules.map((rule) =>
        rule.name === 'tag'
          ? {
              ...rule,
              // KF-YBJ27D: `suggest` now receives the committed tokens, so a tag that is already a
              // chip is excluded exactly as the app's former suggestion helper excludes it.
              suggest: (input: string, state: TokenSearchState) =>
                tags
                  .filter((tag) => tag.toLowerCase().startsWith(input.toLowerCase()))
                  .filter((tag) => !state.tokens.some((token) => token.kind === 'tag' && token.parsedValue === tag)),
            }
          : rule,
      ),
    });
    // 1. Committed-token-aware suggestions.
    model.edit({ query: 'tag:client ', tokens: [] });
    model.edit({ query: 'tag:cl', tokens: model.state.value.tokens });
    expect(model.suggestions.value).toEqual([]);
    model.edit({ query: 'tag:', tokens: model.state.value.tokens });
    expect(model.suggestions.value.map((item) => item.value)).toEqual(['tag:"needs design"', 'tag:server']);
    // 2. KF-K3EJM5: `commit` accepts a computed value for the active prefix, so the lifecycle date
    //    helper can commit its token; an invalid value or a missing prefix leaves the query alone.
    model.edit({ query: 'parser created-after:', tokens: [] });
    model.commit('2026-09-01');
    expect(model.state.value.query).toBe('parser ');
    expect(model.state.value.tokens.map((token) => [token.kind, token.parsedValue])).toEqual([
      ['created-after', parseSearchDate('2026-09-01')],
    ]);
    model.edit({ query: 'created-before:', tokens: model.state.value.tokens });
    model.commit('someday');
    expect(model.state.value.query).toBe('created-before:');
    expect(model.state.value.tokens).toHaveLength(1);
    model.edit({ query: 'plain text', tokens: model.state.value.tokens });
    model.commit('2026-09-01');
    expect(model.state.value.query).toBe('plain text');
    // 3. KF-ER975X: `replace` rebuilds the DOM-owned editor text, so applying a saved view or
    //    restoring a session has a model action; `edit` still leaves the revision alone.
    const revision = model.editorRevision.value;
    model.edit({ query: 'typed text', tokens: [] }, true);
    expect(model.editorRevision.value).toBe(revision);
    model.replace({ query: 'restored text', tokens: [{ value: 'is:open', label: 'is:open' }] });
    expect(model.editorRevision.value).toBe(revision + 1);
    expect(model.state.value).toMatchObject({
      query: 'restored text',
      tokens: [{ kind: 'is', parsedValue: 'open', value: 'is:open' }],
    });
    model.clear();
    expect(model.editorRevision.value).toBe(revision + 2);
  });
});
