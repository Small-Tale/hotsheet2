import { describe, expect, it } from 'vitest';

import { addTicketTag, removeTicketTag, TicketTagEditor } from './ticket-tag-editor';

describe('TicketTagEditor', () => {
  it('normalizes additions, rejects duplicates, and removes exact tags', () => {
    expect(addTicketTag(['client'], ' Needs Review ')).toEqual(['client', 'Needs-Review']);
    expect(addTicketTag(['client'], 'client')).toEqual(['client']);
    expect(removeTicketTag(['client', 'server'], 'client')).toEqual(['server']);
  });

  it('projects removable chips and only unused autocomplete suggestions when editable', () => {
    const editable = String(TicketTagEditor({ tags: ['client'], suggestions: ['server', 'client'], editable: true }));
    expect(editable).toContain('with-remove');
    expect(editable).toContain('aria-label="Add tag"');
    expect(editable).toContain('<option value="server"');
    expect(editable).not.toContain('<option value="client"');
    const readOnly = String(TicketTagEditor({ tags: ['client'], suggestions: ['server'], editable: false }));
    expect(readOnly).not.toContain('with-remove');
    expect(readOnly).not.toContain('aria-label="Add tag"');
  });
});
