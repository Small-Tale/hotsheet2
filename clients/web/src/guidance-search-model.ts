import { createSearchFieldModel, type SearchFieldState } from '@kerfjs/ui/search-field-model';

import type { GuidanceOverview } from './api';

/** Rule-catalog filters share Kerf's toolbar editor but have their own grammar (HS2-6ZA2RN). */
export function createGuidanceSearchModel(overview: () => GuidanceOverview | undefined) {
  const values = (kind: 'owner' | 'tag') =>
    [
      ...new Set(
        overview()?.sources.flatMap((source) =>
          source.rules.flatMap((rule) => (kind === 'owner' ? (rule.owner ? [rule.owner] : []) : rule.tags)),
        ) ?? [],
      ),
    ].sort((left, right) => left.localeCompare(right));
  return createSearchFieldModel({
    rules: (['owner', 'tag'] as const).map((kind) => ({
      name: kind,
      parse: (input: string) => values(kind).find((value) => value.toLowerCase() === input.toLowerCase()) ?? input,
      label: (value: string) => `${kind}:${value}`,
      suggest: (input: string, state: SearchFieldState) => {
        const prefix = input.replace(/^"/, '').toLowerCase();
        const committed = new Set(
          state.tokens.filter((token) => token.kind === kind).map((token) => token.parsedValue.toLowerCase()),
        );
        return values(kind)
          .filter((value) => !committed.has(value.toLowerCase()) && value.toLowerCase().startsWith(prefix))
          .slice(0, 8)
          .map((value) => ({ value, label: `${kind}:${/\s/.test(value) ? `"${value}"` : value}` }));
      },
    })),
  });
}

/** A rule must satisfy every committed owner/tag chip and every free-text word. */
export function guidanceRuleMatches(
  rule: GuidanceOverview['sources'][number]['rules'][number],
  state: SearchFieldState,
): boolean {
  const words = state.query.toLowerCase().trim().split(/\s+/).filter(Boolean);
  const content = `${rule.name} ${rule.description}`.toLowerCase();
  return (
    words.every((word) => content.includes(word)) &&
    state.tokens.every((token) =>
      token.kind === 'owner'
        ? rule.owner?.toLowerCase() === token.parsedValue.toLowerCase()
        : token.kind === 'tag'
          ? rule.tags.some((tag) => tag.toLowerCase() === token.parsedValue.toLowerCase())
          : true,
    )
  );
}
