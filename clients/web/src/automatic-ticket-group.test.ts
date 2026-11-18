import { describe, expect, it } from 'vitest';

import {
  AUTOMATIC_GROUP_PROMPT_PREFIX,
  AUTOMATIC_GROUP_TICKET_LIMIT,
  AutomaticGroupReplyError,
  buildAutomaticGroupPrompt,
  parseAutomaticGroupAssignments,
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
    // HS2-4H6XJ1: one assignment per ticket, never one group for the whole selection.
    expect(prompt).toContain('for each of the tickets below, judging each ticket on its own');
    expect(prompt).toContain('{"assignments": [{"slug": "<a ticket slug>", "group_id": "<an id from groups>"}');
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
        groups: unknown[];
      };
    expect(context.tickets).toHaveLength(AUTOMATIC_GROUP_TICKET_LIMIT);
    expect(context.tickets[0]).not.toHaveProperty('category');
    expect(context.groups).toEqual([]);
  });
});

const TICKETS = [
  { slug: 'HS2-AAAAAA', title: 'Fix drag' },
  { slug: 'HS2-BBBBBB', title: 'Query parser' },
  { slug: 'HS2-CCCCCC', title: 'Changelog' },
];
const parse = (reply: string, groups = GROUPS) => parseAutomaticGroupAssignments(reply, TICKETS, groups);

describe('parseAutomaticGroupAssignments (HS2-4C32A0, HS2-4H6XJ1)', () => {
  it('gives each ticket its own group from a bare, fenced, or embedded reply', () => {
    const reply = JSON.stringify({
      assignments: [
        { slug: 'HS2-AAAAAA', group_id: '7K3M9P2QXA' },
        { slug: 'HS2-BBBBBB', group_id: ' 8B4N0R3SYB ' },
        { slug: 'HS2-CCCCCC', new_group: '  Release   prep ' },
      ],
    });
    const expected = [
      { slug: 'HS2-AAAAAA', proposal: { kind: 'existing', id: '7K3M9P2QXA', name: 'Board polish' } },
      { slug: 'HS2-BBBBBB', proposal: { kind: 'existing', id: '8B4N0R3SYB', name: 'Search syntax' } },
      { slug: 'HS2-CCCCCC', proposal: { kind: 'new', name: 'Release prep' } },
    ];
    expect(parse(reply)).toEqual(expected);
    expect(parse(`\`\`\`json\n${reply}\n\`\`\``)).toEqual(expected);
    expect(parse(`Here you go: ${reply} — done.`)).toEqual(expected);
  });

  it('maps an existing name to its group and lets tickets share a new name', () => {
    expect(
      parse(
        JSON.stringify({
          assignments: [
            { slug: 'HS2-AAAAAA', new_group: 'search SYNTAX' },
            { slug: 'HS2-BBBBBB', new_group: 'Docs' },
            { slug: 'HS2-CCCCCC', new_group: 'docs' },
          ],
        }),
      ),
    ).toEqual([
      { slug: 'HS2-AAAAAA', proposal: { kind: 'existing', id: '8B4N0R3SYB', name: 'Search syntax' } },
      { slug: 'HS2-BBBBBB', proposal: { kind: 'new', name: 'Docs' } },
      { slug: 'HS2-CCCCCC', proposal: { kind: 'new', name: 'docs' } },
    ]);
  });

  it('skips each unusable assignment on its own and keeps the rest', () => {
    const twins = [...GROUPS, { id: '9C5P1S4TZC', name: 'board polish', duplicate: true }];
    expect(
      parse(
        JSON.stringify({
          assignments: [
            { slug: 'HS2-ZZZZZZ', group_id: '7K3M9P2QXA' },
            { slug: 'HS2-AAAAAA', group_id: '0000000000' },
            { slug: 'HS2-AAAAAA', group_id: '7K3M9P2QXA', new_group: 'Other' },
            { slug: 'HS2-AAAAAA', new_group: 'Board Polish' },
            { slug: 'HS2-BBBBBB', new_group: 'Two\nlines' },
            { slug: 'HS2-BBBBBB', new_group: 'x'.repeat(121) },
            { slug: 'HS2-BBBBBB', group_id: 7 },
            { group_id: '7K3M9P2QXA' },
            'HS2-CCCCCC',
            { slug: 'HS2-CCCCCC', group_id: '8B4N0R3SYB' },
            { slug: 'HS2-CCCCCC', group_id: '7K3M9P2QXA' },
          ],
        }),
        twins,
      ),
    ).toEqual([{ slug: 'HS2-CCCCCC', proposal: { kind: 'existing', id: '8B4N0R3SYB', name: 'Search syntax' } }]);
    expect(parse('{"assignments": []}')).toEqual([]);
  });

  it.each([
    ['prose', 'I would put these in Board polish.', 'not the expected JSON'],
    ['empty', '', 'not the expected JSON'],
    ['array', '[{"group_id":"7K3M9P2QXA"}]', 'not the expected JSON'],
    ['null', 'null', 'not the expected JSON'],
    ['single-group shape', '{"group_id":"7K3M9P2QXA"}', 'no assignments list'],
    ['non-list assignments', '{"assignments":{"HS2-AAAAAA":"7K3M9P2QXA"}}', 'no assignments list'],
  ])('refuses a %s reply', (_label, reply, message) => {
    expect(() => parse(reply)).toThrow(AutomaticGroupReplyError);
    expect(() => parse(reply)).toThrow(message);
  });
});
