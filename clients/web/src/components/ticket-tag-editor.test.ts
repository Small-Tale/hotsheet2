import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { addTicketTag, removeTicketTag, TicketTagEditor } from './ticket-tag-editor';

describe('TicketTagEditor', () => {
  it('normalizes additions, rejects duplicates, and removes exact tags', () => {
    expect(addTicketTag(['client'], ' Needs Review ')).toEqual(['client', 'Needs-Review']);
    expect(addTicketTag(['client'], 'client')).toEqual(['client']);
    expect(removeTicketTag(['client', 'server'], 'client')).toEqual(['server']);
  });

  it('renders the header-targeted add-tag popover with only unused autocomplete suggestions when editable', () => {
    const editable = String(TicketTagEditor({ tags: ['client'], suggestions: ['server', 'client'], editable: true, popoverId: 'ticket-tag-sidebar-test' }));
    expect(editable).toContain('with-remove');
    expect(editable).toContain('data-component="ticket-tag-popover" popover="auto" role="dialog" aria-labelledby="ticket-tag-sidebar-test-title"');
    expect(editable).toContain('name="ticket-tag-input" list="ticket-tag-sidebar-test-suggestions"');
    expect(editable).not.toContain('aria-haspopup="dialog"');
    expect(editable).toContain('<option value="server"');
    expect(editable).not.toContain('<option value="client"');
    const readOnly = String(TicketTagEditor({ tags: ['client'], suggestions: ['server'], editable: false }));
    expect(readOnly).not.toContain('with-remove');
    expect(readOnly).not.toContain('ticket-tag-popover');
  });

  it('styles the editor as an anchored popup surface', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-tag-editor.css'), 'utf8');
    expect(css).not.toContain('.ticket-tag-editor__add');
    expect(css).toMatch(/\.ticket-tag-editor__popover \{[^}]*position: fixed;[^}]*position-area: block-end span-inline-end;[^}]*box-shadow: var\(--wa-shadow-l\)/);
  });
});
