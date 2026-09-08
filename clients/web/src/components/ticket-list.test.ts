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
    expect(markup).toContain('data-key="ticket-list"');
    expect(markup).toContain('data-key="ticket:HS2-LIST"');
    expect(markup).toContain('aria-label="Up Next tickets"');
    expect(markup).toContain('data-ticket-selection-root="true"');
    expect(markup).toContain('aria-multiselectable="true"');
    expect(markup.match(/data-component="ticket-list-row"/g)).toHaveLength(2);
    expect(markup).not.toContain('ticket-card');
  });

  it('reports progressive rendering without losing the authoritative total',()=>{
    const markup=String(TicketList({tickets:[ticket],totalCount:250,label:'Archive tickets'}));
    expect(markup).toContain('data-rendered-count="1"');
    expect(markup).toContain('data-total-count="250"');
    expect(markup).toContain('data-ticket-progressive-loading="true"');
    expect(markup).toContain('Loading more tickets…');
  });

  it('keeps actionable corrupt diagnostics outside the healthy-ticket listbox', () => {
    const markup = String(TicketList({
      tickets: [ticket],
      corruptTickets: [{ store: 'local', store_path: '/tickets', path: '/tickets/bad.md', error: 'could not parse' }],
    }));
    expect(markup.match(/data-component="ticket-list-row"/g)).toHaveLength(1);
    expect(markup.match(/data-component="corrupt-ticket-row"/g)).toHaveLength(1);
    expect(markup).toContain('role="group"');
    expect(markup).toContain('aria-label="Unreadable tickets"');
    expect(markup).toContain('data-action="select-corrupt-ticket"');
    expect(markup).not.toContain('data-action="reveal-corrupt-ticket"');
  });

  it('can reserve an unresolved empty collection without projecting empty-state copy', () => {
    const markup = String(TicketList({ tickets: [] }));
    expect(markup).toContain('data-empty="true"');
    expect(markup).not.toContain('data-component="ticket-empty-state"');
    expect(markup).not.toContain('No tickets');
  });

  it('overlaps only adjacent selected list-row borders into one seam', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-list.css'), 'utf8');
    expect(css).toContain(':has(> .ticket-list-row--selected) + .ticket-list-row-container:has(> .ticket-list-row--selected)');
    expect(css).toContain('margin-top: -1px');
  });

  it('keeps narrow-list rounding on the outer edges rather than every row', () => {
    const listCss = readFileSync(resolve(import.meta.dirname, 'ticket-list.css'), 'utf8');
    const rowCss = readFileSync(resolve(import.meta.dirname, 'ticket-row.css'), 'utf8');
    const narrowListRule = rowCss.match(/\.ticket-list-row--list \{([^}]*)\}/)?.[1] ?? '';
    expect(narrowListRule).not.toContain('border-radius');
    expect(listCss).toContain('.ticket-list__tickets > .ticket-list-row-container:first-child .ticket-list-row { border-radius: .65rem .65rem 0 0; }');
    expect(listCss).toContain('.ticket-list__tickets > .ticket-list-row-container:last-child .ticket-list-row { border-radius: 0 0 .65rem .65rem; }');
  });

  it('fills the width supplied by its host instead of imposing an internal cap', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-list.css'), 'utf8');
    const listRule = css.match(/\.ticket-list \{([^}]*)\}/)?.[1] ?? '';
    expect(listRule).toContain('width: 100%');
    expect(listRule).not.toContain('max-width');
  });
});
