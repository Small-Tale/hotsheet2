import { describe, expect, it } from 'vitest';

import { TicketGroup } from './ticket-group';
import { TicketGroupContextMenu } from './ticket-group-context-menu';
import { TicketGroupDialog, type TicketGroupDialogState } from './ticket-group-dialog';

describe('TicketGroup (HS2-WZ3JGZ, HS2-HV8S4Y)', () => {
  it('renders a lowered panel with a case-preserving disclosure header and count', () => {
    const markup = String(TicketGroup({ name: 'Board drag & drop', count: 3, scope: 'list', children: [] }));
    expect(markup).toContain('data-component="sunken-panel"');
    expect(markup).toMatch(/data-component="list-header"[^>]*data-size="m"[^>]*data-wrap="true"/);
    expect(markup).toContain('data-action="toggle-ticket-group"');
    expect(markup).toContain('aria-expanded="true"');
    expect(markup).toContain('aria-label="Board drag &amp; drop, 3 tickets"');
    expect(markup).toContain('>Board drag &amp; drop<');
    expect(markup).toContain('data-ticket-drop-group="Board drag &amp; drop"');
    expect(markup).toContain('data-presentation="list"');
    // The toggle names the region it shows and hides.
    const region = /aria-controls="([^"]+)"/.exec(markup)![1];
    expect(markup).toContain(`id="${region}" class="ticket-group__tickets"`);
    expect(markup).not.toContain('data-drop-target');
  });

  it('keeps the header and removes the region when collapsed', () => {
    const markup = String(
      TicketGroup({ name: 'Board', count: 1, scope: 'started', presentation: 'column', collapsed: true }),
    );
    expect(markup).toContain('aria-expanded="false"');
    expect(markup).toContain('aria-label="Board, 1 ticket"');
    expect(markup).not.toContain('ticket-group__tickets');
    expect(markup).toContain('data-collapsed="true"');
    expect(markup).toContain('data-presentation="column"');
  });

  it('keys each column’s copy of a group separately and marks a drop target', () => {
    const list = String(TicketGroup({ name: 'Board', count: 1, scope: 'list' })),
      column = String(TicketGroup({ name: 'board', count: 1, scope: 'started', dropTarget: true }));
    expect(list).toContain('data-key="ticket-group:list:board"');
    expect(column).toContain('data-key="ticket-group:started:board"');
    expect(/aria-controls="([^"]+)"/.exec(list)![1]).not.toBe(/aria-controls="([^"]+)"/.exec(column)![1]);
    expect(column).toContain('data-drop-target="true"');
    expect(column).toMatch(/data-component="sunken-panel"[^>]*data-outlined="true"/);
    expect(list).not.toMatch(/data-outlined="true"/);
  });
});

describe('TicketGroupDialog (HS2-TX17CM, HS2-8CDFKR)', () => {
  const state = (overrides: Partial<TicketGroupDialogState> = {}): TicketGroupDialogState => ({
    mode: 'new',
    name: '',
    groups: ['Board drag & drop', 'Search syntax'],
    session: 1,
    ...overrides,
  });
  const submit = (markup: string) => /<button[^>]*type="submit"[^>]*>/.exec(markup)![0];

  it('renders nothing when closed', () => {
    expect(String(TicketGroupDialog({}))).toBe('');
  });

  it('asks for a required Group Name and disables Create Group until it is valid', () => {
    const empty = String(TicketGroupDialog({ state: state() }));
    expect(empty).toContain('New Group');
    expect(empty).toContain('Group Name');
    expect(empty).toContain('Create Group');
    expect(empty).toContain('data-dialog-component="ticket-group-dialog"');
    expect(submit(empty)).toContain('disabled');
    // An empty field shows no error text; it is simply not submittable.
    expect(empty).not.toContain('already exists');
    const valid = String(TicketGroupDialog({ state: state({ name: 'Polish' }) }));
    expect(submit(valid)).not.toContain('disabled');
  });

  it('shows an inline duplicate error, compared case-insensitively', () => {
    const markup = String(TicketGroupDialog({ state: state({ name: ' board DRAG &  drop ' }) }));
    expect(markup).toContain('A group named “board DRAG &amp; drop” already exists.');
    expect(markup).toContain('aria-invalid="true"');
    expect(submit(markup)).toContain('disabled');
  });

  it('renames with the current name allowed, and reports busy and failed writes', () => {
    const rename = state({ mode: 'rename', group: 'Board drag & drop', name: 'Board drag & drop' }),
      markup = String(TicketGroupDialog({ state: rename }));
    expect(markup).toContain('Rename Group');
    expect(markup).toContain('>Rename<');
    expect(markup).not.toContain('already exists');
    expect(submit(markup)).not.toContain('disabled');
    expect(String(TicketGroupDialog({ state: { ...rename, name: 'search syntax' } }))).toContain('already exists');
    const busy = String(TicketGroupDialog({ state: { ...rename, name: 'Next', busy: true } }));
    expect(submit(busy)).toContain('disabled');
    const failed = String(TicketGroupDialog({ state: { ...rename, name: 'Next', error: 'The store is locked.' } }));
    expect(failed).toMatch(/role="alert"[^>]*>The store is locked\./);
  });

  it('replaces the field per session so it never shows a previous open’s text', () => {
    expect(String(TicketGroupDialog({ state: state({ session: 4 }) }))).toContain('data-key="ticket-group-name-4"');
  });
});

describe('TicketGroupContextMenu (HS2-8CDFKR)', () => {
  it('offers Rename Group and Ungroup for the group it was opened on', () => {
    const markup = String(TicketGroupContextMenu({ x: 10, y: 20, group: 'Board' }));
    expect(markup).toContain('data-context-menu="ticket"');
    expect(markup).toContain('aria-label="Board Group Actions"');
    expect(markup).toMatch(/data-action="rename-ticket-group"/);
    expect(markup).toMatch(/data-action="ungroup-ticket-group"/);
    expect(markup.match(/data-ticket-group="Board"/g)).toHaveLength(2);
    expect(markup).not.toContain('aria-disabled="true"');
  });

  it('disables both actions with the reason when the provider cannot group', () => {
    const reason = 'One or more selected ticket providers do not support groups.',
      markup = String(TicketGroupContextMenu({ x: 0, y: 0, group: 'Board', disabledReason: reason }));
    expect(markup.match(/aria-disabled="true"/g)).toHaveLength(2);
    expect(markup).toContain(`title="${reason}"`);
  });
});
