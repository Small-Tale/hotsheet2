import { describe, expect, it } from 'vitest';

import { createTicketSearchModel } from '../ticket-search-model';
import { insertSearchSyntax } from './ticket-search-field';

const raws = (model: ReturnType<typeof createTicketSearchModel>) =>
  model.state.value.tokens.map((token) => token.value);

describe('insertSearchSyntax (HS2-FR1Y70)', () => {
  it('adds a help example as a committed chip and keeps earlier tokens and words', () => {
    const model = createTicketSearchModel({ tags: () => ['client'] });
    insertSearchSyntax(model, 'tag:client');
    expect(raws(model)).toEqual(['tag:client']);
    model.edit({ query: `${model.state.value.query}parser`, tokens: model.state.value.tokens });
    insertSearchSyntax(model, 'is:up-next');
    expect(raws(model)).toEqual(['tag:client', 'is:up-next']);
    expect(model.state.value.query).toContain('parser');
    // A date example with a space and a combined expression both land without losing what is there.
    insertSearchSyntax(model, 'updated-after:4h ago');
    expect(raws(model).length).toBeGreaterThanOrEqual(3);
    expect(raws(model).slice(0, 2)).toEqual(['tag:client', 'is:up-next']);
  });

  it('starts from an empty field without a leading space', () => {
    const model = createTicketSearchModel({ tags: () => [] });
    insertSearchSyntax(model, '(client OR server) AND NOT is:archived');
    expect(model.state.value.query.startsWith(' ')).toBe(false);
    expect(`${model.state.value.query} ${raws(model).join(' ')}`).toContain('client');
  });
});
