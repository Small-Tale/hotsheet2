import { describe, expect, it } from 'vitest';

import {
  AUTOMATIC_GROUP_PROMPT_PREFIX,
  AUTOMATIC_GROUP_TICKET_LIMIT,
  AutomaticGroupReplyError,
  buildAutomaticGroupPrompt,
  parseAutomaticGroupProposal,
} from './automatic-ticket-group';

const GROUPS = [
  { id: '7K3M9P2QXA', name: 'Board polish' },
  { id: '8B4N0R3SYB', name: 'Search syntax' },
];

describe('buildAutomaticGroupPrompt (HS2-4C32A0)', () => {
  it('asks for JSON only and carries the tickets and groups as JSON', () => {
    const prompt = buildAutomaticGroupPrompt([{ slug: 'HS2-AAAAAA', title: 'Fix drag', category: 'bug' }], GROUPS);
    expect(prompt.startsWith(AUTOMATIC_GROUP_PROMPT_PREFIX)).toBe(true);
    expect(prompt).toContain('Do not call tools');
    expect(prompt).toContain('{"group_id": "<an id from groups>"} or {"new_group": "<name>"}');
    const context = JSON.parse(prompt.slice(prompt.indexOf('\n\n') + 2)) as unknown;
    expect(context).toEqual({
      tickets: [{ slug: 'HS2-AAAAAA', title: 'Fix drag', category: 'bug' }],
      groups: [
        { id: '7K3M9P2QXA', name: 'Board polish' },
        { id: '8B4N0R3SYB', name: 'Search syntax' },
      ],
    });
  });

  it('omits a missing category and caps the ticket list', () => {
    const tickets = Array.from({ length: AUTOMATIC_GROUP_TICKET_LIMIT + 3 }, (_, index) => ({
        slug: `HS2-${index}`,
        title: `Ticket ${index}`,
      })),
      prompt = buildAutomaticGroupPrompt(tickets, []),
      context = JSON.parse(prompt.slice(prompt.indexOf('\n\n') + 2)) as {
        tickets: { category?: string }[];
        more_tickets?: number;
        groups: unknown[];
      };
    expect(context.tickets).toHaveLength(AUTOMATIC_GROUP_TICKET_LIMIT);
    expect(context.more_tickets).toBe(3);
    expect(context.tickets[0]).not.toHaveProperty('category');
    expect(context.groups).toEqual([]);
  });
});

describe('parseAutomaticGroupProposal (HS2-4C32A0)', () => {
  it('accepts an existing id in a bare, fenced, or embedded reply', () => {
    const expected = { kind: 'existing', id: '7K3M9P2QXA', name: 'Board polish' };
    expect(parseAutomaticGroupProposal('{"group_id":"7K3M9P2QXA"}', GROUPS)).toEqual(expected);
    expect(parseAutomaticGroupProposal('```json\n{"group_id": " 7K3M9P2QXA "}\n```', GROUPS)).toEqual(expected);
    expect(parseAutomaticGroupProposal('Here you go: {"group_id":"7K3M9P2QXA"} — done.', GROUPS)).toEqual(expected);
  });

  it('accepts a new name, normalized, and maps an existing name to its group', () => {
    expect(parseAutomaticGroupProposal('{"new_group":"  Release   prep "}', GROUPS)).toEqual({
      kind: 'new',
      name: 'Release prep',
    });
    expect(parseAutomaticGroupProposal('{"new_group":"search SYNTAX"}', GROUPS)).toEqual({
      kind: 'existing',
      id: '8B4N0R3SYB',
      name: 'Search syntax',
    });
  });

  it.each([
    ['prose', 'I would put these in Board polish.', 'not the expected JSON'],
    ['empty', '', 'not the expected JSON'],
    ['array', '[{"group_id":"7K3M9P2QXA"}]', 'not the expected JSON'],
    ['null', 'null', 'not the expected JSON'],
    ['both', '{"group_id":"7K3M9P2QXA","new_group":"Other"}', 'neither one group'],
    ['neither', '{"group":"Board polish"}', 'neither one group'],
    ['blank id', '{"group_id":"  "}', 'neither one group'],
    ['unknown id', '{"group_id":"0000000000"}', 'does not exist'],
    ['blank name', '{"new_group":"   "}', 'not allowed'],
    ['multi-line name', '{"new_group":"Two\\nlines"}', 'not allowed'],
    ['long name', JSON.stringify({ new_group: 'x'.repeat(121) }), 'not allowed'],
    ['number id', '{"group_id":7}', 'neither one group'],
  ])('refuses a %s reply', (_label, reply, message) => {
    expect(() => parseAutomaticGroupProposal(reply, GROUPS)).toThrow(AutomaticGroupReplyError);
    expect(() => parseAutomaticGroupProposal(reply, GROUPS)).toThrow(message);
  });

  it('refuses a name that two duplicate groups share', () => {
    const twins = [...GROUPS, { id: '9C5P1S4TZC', name: 'board polish', duplicate: true }];
    expect(() => parseAutomaticGroupProposal('{"new_group":"Board Polish"}', twins)).toThrow('More than one group');
  });
});
