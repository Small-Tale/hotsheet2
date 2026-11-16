import { describe, expect, it } from 'vitest';

import type { CheckoutTicketRows, TicketRow } from './api';
import { searchTicketReferences, TICKET_REFERENCE_RESULT_LIMIT } from './ticket-reference-search';

const row = (connection: string, native: string, slug: string, title = `Synthetic ${slug}`) =>
  ({
    id: native,
    slug,
    title,
    connection_id: connection,
    native_id: native,
    qualified_id: `${connection}:${native}`,
  }) as TicketRow;
const rows = (items: TicketRow[], extra: Partial<CheckoutTicketRows> = {}): CheckoutTicketRows => ({
  items,
  partial: false,
  truncated: false,
  ...extra,
});

describe('searchTicketReferences (HS2-MCGCPQ)', () => {
  it('merges every project, qualifies each row by project, and lists an exact number first', async () => {
    const result = await searchTicketReferences(
      [
        { id: 'alpha', name: 'Alpha' },
        { id: 'beta', name: 'Beta' },
      ],
      'hs2-exact',
      async (project) =>
        project.id === 'alpha'
          ? rows([row('git-a', '1', 'HS2-OTHER', 'Mentions HS2-EXACT'), row('git-a', '2', 'HS2-EXACT')])
          : rows([row('git-b', '3', 'HS2-EXACT')]),
    );
    expect(result.errors).toEqual([]);
    expect(result.candidates.map((item) => `${item.projectId}/${item.qualifiedId}`)).toEqual([
      'alpha/git-a:2',
      'beta/git-b:3',
      'alpha/git-a:1',
    ]);
    expect(result.candidates[1]).toMatchObject({ projectName: 'Beta', slug: 'HS2-EXACT', nativeId: '3' });
  });

  it('keeps answering projects when another fails or is partial', async () => {
    const result = await searchTicketReferences(
      [
        { id: 'alpha', name: 'Alpha' },
        { id: 'beta', name: 'Beta' },
        { id: 'gamma', name: 'Gamma' },
      ],
      'x',
      async (project) => {
        if (project.id === 'alpha') throw new Error('Alpha is offline.');
        return project.id === 'beta' ? rows([row('git-b', '1', 'HS2-B')], { partial: true }) : rows([]);
      },
    );
    expect(result.candidates.map((item) => item.slug)).toEqual(['HS2-B']);
    expect(result.errors).toEqual(['Alpha is offline.', 'Search results may be incomplete for Beta.']);
  });

  it('bounds the list and returns nothing for no projects', async () => {
    const many = Array.from({ length: TICKET_REFERENCE_RESULT_LIMIT + 5 }, (_, index) =>
      row('git', String(index), `HS2-${index}`),
    );
    const result = await searchTicketReferences([{ id: 'alpha', name: 'Alpha' }], 'x', async () => rows(many));
    expect(result.candidates).toHaveLength(TICKET_REFERENCE_RESULT_LIMIT);
    expect(await searchTicketReferences([], 'x', async () => rows(many))).toEqual({ candidates: [], errors: [] });
  });
});
