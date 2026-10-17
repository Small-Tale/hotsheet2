import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { addTicketTag, removeTicketTag, TicketTagEditor, TicketTagPopoverContent } from './ticket-tag-editor';

describe('TicketTagEditor', () => {
  it('normalizes additions, rejects duplicates, and removes exact tags', () => {
    expect(addTicketTag(['client'], ' Needs Review ')).toEqual(['client', 'Needs-Review']);
    expect(addTicketTag(['client'], 'client')).toEqual(['client']);
    expect(removeTicketTag(['client', 'server'], 'client')).toEqual(['server']);
  });

  it('renders the header-targeted add-tag popover with only unused autocomplete suggestions when editable', () => {
    const editable = String(
      TicketTagEditor({
        tags: ['client'],
        suggestions: ['server', 'client'],
        editable: true,
        popoverId: 'ticket-tag-sidebar-test',
      }),
    );
    expect(editable).toContain('data-action="remove-tag-chip"');
    // Kerf openAnchoredSurface hosts the Add tag content; the editor keeps only the suggestions (HS2-Q4JE3V).
    expect(editable).not.toContain('popover="auto"');
    expect(editable).toContain('<datalist id="ticket-tag-sidebar-test-suggestions"');
    const popover = String(
      TicketTagPopoverContent({
        suggestionsId: 'ticket-tag-sidebar-test-suggestions',
        suggestions: ['server'],
      }),
    );
    expect(popover).toContain('data-combobox-id="ticket-tag-sidebar-test-input"');
    expect(popover).toContain('name="ticket-tag-input"');
    expect(popover).toContain('role="option"');
    expect(popover).toContain('data-value="server"');
    expect(popover).toContain('Press Enter to add');
    expect(editable).not.toContain('aria-haspopup="dialog"');
    expect(editable).toContain('<option value="server"');
    expect(editable).not.toContain('<option value="client"');
    const readOnly = String(TicketTagEditor({ tags: ['client'], suggestions: ['server'], editable: false }));
    expect(readOnly).not.toContain('data-action="remove-tag-chip"');
    expect(readOnly).not.toContain('<datalist');
  });

  it('styles the editor as an anchored popup surface', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-tag-editor.css'), 'utf8');
    expect(css).not.toContain('.ticket-tag-editor__add');
    expect(css).not.toContain('--wa-space-');
    expect(css).toMatch(/\.ticket-tag-editor \{[^}]*gap: var\(--kui-space-xs\)/);
    // Kerf's AnchoredSurface owns the popup chrome; no app surface rule remains (HS2-Q4JE3V).
    expect(css).not.toContain('__popover');
    expect(css).not.toContain('box-shadow');
    // Kerf Row lays out the chips and Kerf Text owns the title, caption, and hint typography (HS2-DYNGY5).
    expect(css).not.toContain('__chips');
    expect(css).not.toMatch(/__popover (?:strong|small|label) \{/);
    expect(css).not.toContain('__field');
    expect(css).not.toContain('__input');
    expect(css).not.toContain('@media (pointer: coarse)');
  });
});
