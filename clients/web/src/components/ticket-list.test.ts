import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { listEdge, TicketList } from './ticket-list';
import type { TicketRowProps } from './ticket-row';

const ticket: TicketRowProps = {
  slug: 'HS2-LIST',
  title: 'Shared list row',
  status: 'started',
  priority: 'default',
  category: 'task',
  tags: [],
};

describe('TicketList', () => {
  it('renders every item through TicketRow with listbox semantics', () => {
    const markup = String(TicketList({ tickets: [ticket, { ...ticket, slug: 'HS2-NEXT' }], label: 'Up Next tickets' }));
    expect(markup).toContain('class="ticket-list"');
    expect(markup).toContain('data-key="ticket-list"');
    expect(markup).toContain('data-key="ticket:HS2-LIST"');
    expect(markup).toContain('aria-label="Up Next tickets"');
    expect(markup).toContain('data-ticket-selection-root="true"');
    expect(markup).toContain('aria-multiselectable="true"');
    expect(markup).toContain('data-component="list"');
    expect(markup).toContain('data-gap="true"');
    expect(markup).toContain('kui-space-none');
    expect(markup.match(/data-component="ticket-list-row"/g)).toHaveLength(2);
    expect(markup).not.toContain('ticket-card');
  });

  it('reports progressive rendering without losing the authoritative total', () => {
    const markup = String(TicketList({ tickets: [ticket], totalCount: 250, label: 'Archive tickets' }));
    expect(markup).toContain('data-rendered-count="1"');
    expect(markup).toContain('data-total-count="250"');
    expect(markup).toContain('data-ticket-progressive-loading="true"');
    expect(markup).toContain('Loading more tickets…');
  });

  it('keeps actionable corrupt diagnostics outside the healthy-ticket listbox', () => {
    const markup = String(
      TicketList({
        tickets: [ticket],
        corruptTickets: [{ store: 'local', store_path: '/tickets', path: '/tickets/bad.md', error: 'could not parse' }],
      }),
    );
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
    expect(markup).not.toContain('data-component="empty-state"');
    expect(markup).not.toContain('No tickets');
  });

  it('overlaps only adjacent selected list-row borders into one seam', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-row.css'), 'utf8');
    expect(css).toContainSource(
      '.ticket-list-row-container:has(> .ticket-list-row--list.ticket-list-row--selected) + .ticket-list-row-container:has(> .ticket-list-row--list.ticket-list-row--selected)',
    );
    expect(css).toContain('margin-top: var(--hs-selected-row-overlap)');
  });

  it('rounds only the rows at each section edge through the row listEdge prop (HS2-4APEJP)', () => {
    const listCss = readFileSync(resolve(import.meta.dirname, 'ticket-list.css'), 'utf8');
    const rowCss = readFileSync(resolve(import.meta.dirname, 'ticket-row.css'), 'utf8'),
      shellCss = readFileSync(resolve(import.meta.dirname, 'ticket-row-shell.css'), 'utf8');
    const narrowListRule = rowCss.match(/\.ticket-list-row--list \{([^}]*)\}/)?.[1] ?? '';
    expect(narrowListRule).not.toContain('border-radius');
    // The list styles none of its rows; each row owns its edge corners.
    expect(listCss).not.toContain('ticket-list-row');
    expect(listCss).not.toContain('corrupt-ticket-row');
    // The shared row shell owns the edge corners for TicketRow and CorruptTicketRow (HS2-1FH8BG).
    expect(shellCss).toContainSource(
      ".ticket-row-shell[data-list-edge='start'] { border-radius: remify(10.4px) remify(10.4px) 0 0; }",
    );
    expect(shellCss).toContainSource(
      ".ticket-row-shell[data-list-edge='end'] { border-radius: 0 0 remify(10.4px) remify(10.4px); }",
    );
    expect(shellCss).toContainSource(".ticket-row-shell[data-list-edge='only'] { border-radius: remify(10.4px); }");
    expect([0, 1, 2].map((index) => listEdge(index, 3))).toEqual(['start', undefined, 'end']);
    expect(listEdge(0, 1)).toBe('only');
    const markup = String(
      TicketList({
        tickets: [ticket, { ...ticket, slug: 'HS2-MID' }, { ...ticket, slug: 'HS2-LAST' }],
        corruptTickets: [
          { store: 'git-local', store_path: '/s', path: '/s/a.md', slug: 'HS2-BAD', error: 'unreadable' },
        ],
      }),
    );
    expect(markup.match(/data-list-edge="[a-z]+"/g)).toEqual([
      'data-list-edge="only"',
      'data-list-edge="start"',
      'data-list-edge="end"',
    ]);
  });

  it('fills the width supplied by its host instead of imposing an internal cap', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-list.css'), 'utf8');
    // The list owns only its fill width, backing surface, and edge-row radius: no grid, clipping, or
    // cap (HS2-5G16WK, HS2-3239MS).
    const listRule = css.match(/\.ticket-list \{([^}]*)\}/u)?.[1] ?? '';
    expect(listRule).toContain('width: 100%');
    expect(listRule).not.toContain('display');
    expect(listRule).not.toContain('overflow');
    expect(css).not.toContain('max-width');
  });

  it('projects each row confidence through the shared summary (HS2-A0Q6G6)', () => {
    const markup = String(
      TicketList({
        tickets: [
          { ...ticket, status: 'verified', latestConfidence: 91 },
          { ...ticket, slug: 'HS2-OPEN', latestConfidence: 91 },
        ],
        label: 'Tickets',
      }),
    );
    expect(markup.match(/data-confidence-badge=/gu)).toHaveLength(1);
    expect(markup).toContain('data-band="verified"');
  });
});
