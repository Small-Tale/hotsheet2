import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { TicketList } from './ticket-list';
import type { TicketRowProps } from './ticket-row';

const ticket: TicketRowProps = { slug: 'HS2-LIST', title: 'Shared list row', status: 'started', priority: 'default', category: 'task', tags: [] };

describe('TicketList', () => {
  it('renders every item through TicketRow with listbox semantics', () => {
    const markup = String(TicketList({ tickets: [ticket, { ...ticket, slug: 'HS2-NEXT' }], label: 'Up Next tickets' }));
    expect(markup).toContain('class="ticket-list"');
    expect(markup).toContain('aria-label="Up Next tickets"');
    expect(markup).toContain('data-ticket-selection-root="true"');
    expect(markup).toContain('aria-multiselectable="true"');
    expect(markup.match(/data-component="ticket-list-row"/g)).toHaveLength(2);
    expect(markup).not.toContain('ticket-card');
  });

  it('overlaps only adjacent selected list-row borders into one seam', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-list.css'), 'utf8');
    expect(css).toContain(':has(> .ticket-list-row--selected) + .ticket-list-row-container:has(> .ticket-list-row--selected)');
    expect(css).toContain('margin-top: -1px');
  });

  it('fills the width supplied by its host instead of imposing an internal cap', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-list.css'), 'utf8');
    const listRule = css.match(/\.ticket-list \{([^}]*)\}/)?.[1] ?? '';
    expect(listRule).toContain('width: 100%');
    expect(listRule).not.toContain('max-width');
  });
});
