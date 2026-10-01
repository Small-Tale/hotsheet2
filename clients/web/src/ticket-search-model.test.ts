import { describe, expect, it } from 'vitest';

import { tokenFromRaw } from './inline-search';
import {
  createTicketSearchModel,
  inlineSearchTokens,
  quoteRelativeDates,
  replaceTicketSearch,
  TICKET_SEARCH_TAG_SUGGESTION_LIMIT,
} from './ticket-search-model';

const tags = ['client', 'Docs', 'needs design', 'server', 'a1', 'a2', 'a3', 'a4', 'a5', 'a6', 'a7', 'a8', 'a9'];

describe('ticket search model (HS2-5JXBQY)', () => {
  it('parses every ticket token kind through the app parser and projects chips back as inline tokens', () => {
    const model = createTicketSearchModel({ tags: () => tags });
    model.edit({
      query:
        '(client OR server) AND NOT is:archived tag:"needs design" has:commit attachment:*.png created-after:2026-09-01 ',
      tokens: [],
    });
    expect(model.state.value.query).toBe('(client OR server) AND NOT     ');
    expect(inlineSearchTokens(model.state.value)).toEqual([
      { ...tokenFromRaw('is:archived'), offset: 27 },
      { ...tokenFromRaw('tag:"needs design"'), offset: 28 },
      { ...tokenFromRaw('has:commit'), offset: 29 },
      { ...tokenFromRaw('attachment:*.png'), offset: 30 },
      { ...tokenFromRaw('created-after:2026-09-01'), offset: 31 },
    ]);
    // Invalid values stay text, exactly as the app parser leaves them.
    model.replace({ query: 'is:nope has:nothing created-before:someday ', tokens: [] });
    expect(model.state.value.tokens).toEqual([]);
  });

  it('suggests project tags for the trailing tag: prefix, excluding committed chips and capping the list', () => {
    const model = createTicketSearchModel({ tags: () => tags });
    model.edit({ query: 'tag:client ', tokens: [] });
    model.edit({ query: 'tag:', tokens: model.state.value.tokens });
    const values = model.suggestions.value.map((item) => item.value);
    expect(values).not.toContain('tag:client');
    expect(values).toHaveLength(TICKET_SEARCH_TAG_SUGGESTION_LIMIT);
    model.edit({ query: 'tag:"nee', tokens: model.state.value.tokens });
    expect(model.suggestions.value).toEqual([{ value: 'tag:"needs design"', label: 'tag:"needs design"' }]);
    model.edit({ query: 'tag:d', tokens: model.state.value.tokens });
    expect(model.suggestions.value).toEqual([{ value: 'tag:Docs', label: 'tag:Docs' }]);
    model.choose('tag:Docs');
    expect(inlineSearchTokens(model.state.value).map((token) => token.raw)).toEqual(['tag:client', 'tag:Docs']);
  });

  it('quotes the documented relative date syntax so Kerf commits it as one chip', () => {
    expect(quoteRelativeDates('updated-after:4h ago words')).toBe('updated-after:"4h ago" words');
    expect(quoteRelativeDates('(created-before:2 d ago)')).toBe('(created-before:"2 d ago")');
    expect(quoteRelativeDates('updated-after:4h agoish')).toBe('updated-after:4h agoish');
    expect(quoteRelativeDates('updated-after:4h ag')).toBe('updated-after:4h ag');
    const model = createTicketSearchModel({ tags: () => tags });
    model.submit({ query: 'parser updated-after:4h ago', tokens: [] });
    const [token] = inlineSearchTokens(model.state.value);
    expect(model.state.value.query).toBe('parser ');
    expect(token.kind).toBe('date');
    expect(token.raw).toBe('updated-after:"4h ago"');
    expect(model.state.value.tokens[0].label).toBe('updated after 4h ago');
  });

  it('labels a date-helper commit in the locale date format, like every other path to that chip (HS2-074E0P)', () => {
    const model = createTicketSearchModel({ tags: () => tags }),
      localized = new Intl.DateTimeFormat(undefined, { dateStyle: 'short', timeStyle: 'short' }).format(
        new Date(2026, 8, 1, 11, 5),
      );
    model.edit({ query: 'parser created-after:', tokens: [] });
    model.commit('2026-09-01T11:05');
    const [chip] = model.state.value.tokens;
    expect(chip.value).toBe('created-after:2026-09-01T11:05');
    expect(chip.label).toBe(`created after ${localized}`);
    // A typed filter, a restored query, and the app projection all agree on the label.
    expect(inlineSearchTokens(model.state.value)[0].label).toBe(chip.label);
    model.clear();
    model.edit({ query: 'created-after:2026-09-01T11:05 ', tokens: [] });
    expect(model.state.value.tokens[0].label).toBe(chip.label);
    replaceTicketSearch(model, 'created-after:2026-09-01T11:05 parser');
    expect(model.state.value.tokens[0].label).toBe(chip.label);
    // A relative value keeps the text the person wrote.
    model.clear();
    model.edit({ query: 'created-after:', tokens: [] });
    model.commit('4h ago');
    expect(model.state.value.tokens[0].label).toBe('created after 4h ago');
  });

  it('replaces from plain text through the app parser and runs the clear hook', () => {
    let cleared = 0;
    const model = createTicketSearchModel({
      tags: () => tags,
      onClear: () => {
        cleared += 1;
      },
    });
    replaceTicketSearch(model, 'before is:active after tag:client');
    expect(model.state.value.query).toBe('before  after ');
    expect(inlineSearchTokens(model.state.value).map((token) => token.raw)).toEqual(['is:active', 'tag:client']);
    expect(model.editorRevision.value).toBe(1);
    model.clear();
    expect(cleared).toBe(1);
    expect(model.state.value).toEqual({ query: '', tokens: [] });
    expect(model.editorRevision.value).toBe(2);
  });
});
