import { describe, expect, it } from 'vitest';

import type { CheckoutGroupSummary } from '../server-response-parsers';
import { TicketGroupMergeDialog } from './ticket-group-merge-dialog';
import { TicketGroupOperationBanner } from './ticket-group-operation-banner';
import { TicketGroupSettings, ticketGroupShortId } from './ticket-group-settings';

const group = (id: string, name: string, count: number, extra: Partial<CheckoutGroupSummary> = {}) => ({
  id,
  name,
  count,
  named: true,
  legacy: false,
  duplicate: false,
  ...extra,
});
const groups = [
  group('01M7P6A000', 'Launch readiness', 18),
  group('01M7P6B000', 'Design system', 7),
  group('01M7P6C000', 'Future work', 0),
];

describe('TicketGroupSettings (HS2-GXKX1Q)', () => {
  it('lists every group with its icon, a count chip after the name, and a row menu', () => {
    const markup = String(TicketGroupSettings({ groups }));
    expect(markup).toContain('data-component="ticket-group-settings"');
    expect(markup).toContain('All project groups appear here, including empty ones.');
    expect(markup).toContain('Deleting a group keeps its tickets and removes their group assignment.');
    // Rows keep the list's order; an empty group shows a 0 chip.
    const rows = [...markup.matchAll(/data-group-row="" data-group-id="([^"]+)"/g)].map((match) => match[1]);
    expect(rows).toEqual(['01M7P6A000', '01M7P6B000', '01M7P6C000']);
    expect(markup).toMatch(
      /data-lucide="folder-kanban"[\s\S]*>Launch readiness<[\s\S]*data-group-count="18"[^>]*>[\s\S]*18/,
    );
    expect(markup).toMatch(/>Future work<[\s\S]*data-group-count="0"/);
    // The row's primary action views the group's tickets; its menu offers the four commands.
    expect(markup).toContain('data-action="view-group-tickets"');
    expect(markup).toContain('aria-label="View tickets in Launch readiness"');
    expect(markup).toContain('Launch readiness Group Actions');
    for (const action of [
      'view-group-tickets',
      'rename-settings-group',
      'merge-settings-group',
      'delete-settings-group',
    ])
      expect(markup).toContain(action);
    // No duplicate: no short id.
    expect(markup).not.toContain('01M7P6A<');
  });

  it('tells same-named groups apart by short id', () => {
    const markup = String(
      TicketGroupSettings({
        groups: [
          group('01M7P6A000', 'Launch readiness', 18, { duplicate: true }),
          group('01M7P6B000', 'Launch readiness', 3, { duplicate: true }),
        ],
      }),
    );
    expect(ticketGroupShortId('01M7P6A000')).toBe('01M7P6A');
    expect(markup).toContain('>01M7P6A<');
    expect(markup).toContain('>01M7P6B<');
  });

  it('shows loading, empty, error, and busy states', () => {
    expect(String(TicketGroupSettings({}))).toContain('Loading Ticket Groups…');
    const empty = String(TicketGroupSettings({ groups: [] }));
    expect(empty).toContain('No Ticket Groups Yet');
    expect(empty).not.toContain('data-group-row');
    const failed = String(TicketGroupSettings({ groups, error: 'The server is unreachable.' }));
    expect(failed).toMatch(/role="alert"[^>]*>[\s\S]*The server is unreachable\./);
    expect(failed).toContain('data-group-row');
    // A running merge or delete disables its own row only.
    const busy = String(TicketGroupSettings({ groups, busyGroup: '01M7P6B000' }));
    const row = (id: string) => new RegExp(`data-group-id="${id}"[^>]*>`).exec(busy)?.[0] ?? '';
    expect(busy.match(/aria-busy="true"/g) ?? []).toHaveLength(1);
    expect(row('01M7P6A000')).not.toContain('aria-busy');
  });

  it('disables Merge into… when there is no other group', () => {
    const markup = String(TicketGroupSettings({ groups: [group('01M7P6A000', 'Only', 2)] }));
    expect(markup).toContain('There is no other group to merge into.');
  });
});

describe('TicketGroupMergeDialog (HS2-GXKX1Q)', () => {
  const state = {
    group: { id: '01M7P6A000', name: 'Launch readiness', count: 18 },
    destinations: [
      { id: '01M7P6B000', name: 'Design system' },
      { id: '01M7P6D000', name: 'Design system', duplicate: true },
    ],
    into: '01M7P6B000',
    session: 1,
  };
  it('says what moves and that the source is removed, then offers the destinations', () => {
    expect(String(TicketGroupMergeDialog({}))).toBe('');
    const markup = String(TicketGroupMergeDialog({ state }));
    expect(markup).toContain('data-dialog-component="ticket-group-merge-dialog"');
    expect(markup).toContain('Merge group');
    expect(markup).toMatch(/Move 18 tickets from <strong>Launch readiness<\/strong> into another group\./);
    expect(markup).toContain('The source group will be removed.');
    expect(markup).toContain('name="ticket-group-merge-into"');
    expect(markup).toContain('Destination Group');
    expect(markup).toContain('Design system (01M7P6D)');
    expect(markup).toContain('data-action="submit-group-merge"');
    expect(markup).toContain('data-action="cancel-group-merge"');
    expect(markup).toContain('Merge Groups');
    expect(String(TicketGroupMergeDialog({ state: { ...state, group: { ...state.group, count: 1 } } }))).toContain(
      'Move 1 ticket from',
    );
  });
  it('disables its controls while busy and shows a failure', () => {
    const busy = String(TicketGroupMergeDialog({ state: { ...state, busy: true, error: 'Merge failed.' } }));
    expect(busy).toMatch(/role="alert"[^>]*>[\s\S]*Merge failed\./);
    expect(busy).toMatch(/type="submit"[^>]*disabled/);
  });
});

describe('TicketGroupOperationBanner (HS2-GXKX1Q)', () => {
  it('shows a running merge or delete as a non-blocking info banner with a progress ring', () => {
    expect(TicketGroupOperationBanner({})).toBeUndefined();
    const markup = String(
      TicketGroupOperationBanner({
        operation: { projectId: 'p', title: 'Merging Launch readiness into Design system' },
      }),
    );
    expect(markup).toContain('data-component="ticket-group-operation-banner"');
    expect(markup).toContain('data-tone="info"');
    expect(markup).toContain('Merging Launch readiness into Design system…');
    expect(markup).toContain('You can keep working');
    expect(markup).toContain('progress-ring');
  });
});
