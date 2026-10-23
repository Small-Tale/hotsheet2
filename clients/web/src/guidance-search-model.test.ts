import { describe, expect, it } from 'vitest';

import { createGuidanceSearchModel, guidanceRuleMatches } from './guidance-search-model';

const rule = {
  name: 'rust-cli',
  description: 'Maintain the command line',
  version: 1,
  installed_version: null,
  owner: 'Hot Sheet',
  tags: ['rust', 'cli'],
  selected: false,
};
const overview = {
  owner: 'builtin' as const,
  explicit_selection: true,
  default_repository: '',
  sources: [{ id: 'rules', repository: '', requested_ref: 'main', commit: 'a', rules: [rule], skills: [] }],
};

describe('guidance toolbar search (HS2-6ZA2RN)', () => {
  it('commits owner and tag tokens, filters rules, and clears both tokens and text', () => {
    const model = createGuidanceSearchModel(() => overview);
    model.edit({ query: 'command owner:"Hot Sheet" tag:rust ', tokens: [] });
    expect(model.state.value.query.trim()).toBe('command');
    expect(model.state.value.tokens.map((token) => token.kind)).toEqual(['owner', 'tag']);
    expect(guidanceRuleMatches(rule, model.state.value)).toBe(true);
    expect(guidanceRuleMatches({ ...rule, tags: ['web'] }, model.state.value)).toBe(false);
    model.clear();
    expect(model.state.value.query).toBe('');
    expect(model.state.value.tokens).toEqual([]);
    model.edit({ query: 'missing', tokens: [] });
    expect(guidanceRuleMatches(rule, model.state.value)).toBe(false);
  });

  it('suggests catalog owners and tags without repeating committed chips', () => {
    const model = createGuidanceSearchModel(() => overview);
    model.edit({ query: 'owner:hot', tokens: [] });
    expect(model.suggestions.value[0]?.value).toBe('owner:"Hot Sheet"');
    model.choose('owner:"Hot Sheet"');
    model.edit({ query: 'owner:hot', tokens: model.state.value.tokens });
    expect(model.suggestions.value).toEqual([]);
    model.edit({ query: 'tag:r', tokens: model.state.value.tokens });
    expect(model.suggestions.value[0]?.value).toBe('tag:rust');
  });
});
