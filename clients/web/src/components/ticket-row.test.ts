import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { getPriorityPresentation, normalizeTicketRowProps, TicketRow, ticketRowIndicator } from './ticket-row';

describe('TicketRow', () => {
  it('normalizes fallbacks, tags, and boolean defaults', () => {
    expect(normalizeTicketRowProps({
      slug: ' ', title: ' ', status: 'not_started', priority: 'default', category: ' ', tags: [' client ', '', ' ux '],
    })).toEqual({
      slug: 'HS2-UNKNOWN', title: 'Untitled ticket', status: 'not_started', priority: 'default',
      category: 'issue', categoryIcon: 'circle-alert', categoryColor: '#6b7280', tags: ['client', 'ux'], upNext: false, upNextEligible: true, selected: false, busy: false,
      blocked: false, needsReview: false, agentName: 'AI', updatedLabel: 'Recently',
      presentation: 'list',
    });
  });

  it('uses the picker defaults for API rows while preserving explicit no-icon categories', () => {
    const feature = String(TicketRow({ slug: 'HS2-FEA', title: 'Feature row', status: 'started', priority: 'default', category: 'feature', tags: [] }));
    expect(feature).toContain('data-lucide="sparkles"');
    expect(feature).not.toContain('>FEA<');
    const textOnly = String(TicketRow({ slug: 'HS2-TEXT', title: 'Text row', status: 'started', priority: 'default', category: 'feature', categoryIcon: '', tags: [] }));
    expect(textOnly).toContain('>FEA<');
  });

  it('reserves the indicator rail for special states in HS1 precedence order', () => {
    expect(ticketRowIndicator({})).toBeUndefined();
    expect(ticketRowIndicator({ upNext: true })).toBe('up-next');
    expect(ticketRowIndicator({ upNext: true, blocked: true })).toBe('blocked');
    expect(ticketRowIndicator({ upNext: true, blocked: true, needsReview: true })).toBe('needs-review');
  });

  it('uses one semantic color token for every Up Next presentation', () => {
    const tokenCss = readFileSync(resolve(import.meta.dirname, 'ticket-state-colors.css'), 'utf8');
    const rowCss = readFileSync(resolve(import.meta.dirname, 'ticket-row.css'), 'utf8');
    const inspectorCss = readFileSync(resolve(import.meta.dirname, 'ticket-inspector.css'), 'utf8');
    expect(tokenCss).toContain('--ticket-state-up-next: #eab308');
    expect(rowCss.match(/var\(--ticket-state-up-next\)/g)).toHaveLength(3);
    expect(inspectorCss).toContain('color: var(--ticket-state-up-next)');
  });

  it('maps HS2 priorities onto the HS1 icon and color semantics', () => {
    expect(Object.fromEntries((['urgent', 'high', 'default', 'low'] as const).map(priority => {
      const presentation = getPriorityPresentation(priority);
      return [priority, [presentation.name, presentation.color]];
    }))).toEqual({
      urgent: ['chevrons-up', '#ef4444'], high: ['chevron-up', '#f97316'],
      default: ['minus', '#6b7280'], low: ['chevron-down', '#3b82f6'],
    });
  });

  it('establishes an outer width container so its descendant can enter narrow presentation', () => {
    const markup = String(TicketRow({ slug: 'HS2-WIDTH', title: 'Responsive row', status: 'started', priority: 'default', category: 'task', tags: [] }));
    expect(markup).toContain('data-component="ticket-list-row-container"');
    expect(markup.indexOf('ticket-list-row-container')).toBeLessThan(markup.indexOf('data-component="ticket-list-row"'));
  });

  it('uses an explicit presentation variant for the title line limit', () => {
    const list = String(TicketRow({ slug: 'HS2-LIST', title: 'List row', status: 'started', priority: 'default', category: 'task', tags: [] }));
    const column = String(TicketRow({ slug: 'HS2-COLUMN', title: 'Column row', status: 'started', priority: 'default', category: 'task', tags: [], presentation: 'column' }));
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-row.css'), 'utf8');
    expect(list).toContain('ticket-list-row--list');
    expect(list).toContain('data-presentation="list"');
    expect(column).toContain('ticket-list-row--column');
    expect(column).toContain('data-presentation="column"');
    expect(css).toMatch(/ticket-list-row__identity[^}]*max-height: 2\.6em/);
    expect(css).toMatch(/ticket-list-row--column \.ticket-list-row__identity[^}]*max-height: 3\.9em/);
    // Rows carry no drop shadow in any presentation (HS2-VX9E4Z); only selection/focus insets/outlines remain.
    expect(css).not.toContain('box-shadow: 0 .3rem .9rem');
  });

  it('floats the updated time first in the identity flow so long titles can wrap beneath it', () => {
    const markup = String(TicketRow({ slug: 'HS2-FLOW', title: 'A long title that flows beneath its timestamp', updatedLabel: 'Now', status: 'started', priority: 'default', category: 'task', tags: [] }));
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-row.css'), 'utf8');
    const identityStart = markup.indexOf('class="ticket-list-row__identity"');
    const updated = markup.indexOf('class="ticket-list-row__updated"');
    const slug = markup.indexOf('class="ticket-list-row__slug"');
    expect(identityStart).toBeGreaterThanOrEqual(0);
    expect(identityStart).toBeLessThan(updated);
    expect(updated).toBeLessThan(slug);
    expect(css).toMatch(/ticket-list-row__updated[^}]*float: right/);
  });

  it('shows a blocked pill immediately after status metadata', () => {
    const markup = String(TicketRow({ slug: 'HS2-BLOCK', title: 'Blocked row', status: 'started', priority: 'high', category: 'bug', tags: [], blocked: true }));
    expect(markup).toContain('data-component="blocked-badge"');
    expect(markup.indexOf('data-component="status-badge"')).toBeLessThan(markup.indexOf('data-component="blocked-badge"'));
  });

  it('marks completed and verified rows for readable title-only completion styling', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-row.css'), 'utf8');
    for (const status of ['completed', 'verified'] as const) expect(String(TicketRow({ slug: 'HS2-DONE', title: 'Finished row', status, priority: 'default', category: 'task', tags: [] }))).toContain(`data-status="${status}"`);
    expect(css).toContain('[data-status="completed"], [data-status="verified"]');
    expect(css).toContain('.ticket-list-row__identity strong { color: #6b7280; text-decoration: line-through; }');
  });

  it('offers Up Next only for not-started and started lifecycle states', () => {
    for (const status of ['not_started', 'started'] as const) expect(String(TicketRow({ slug: 'HS2-ACTIVE', title: 'Active', status, priority: 'default', category: 'task', tags: [] }))).toContain('data-action="toggle-row-up-next"');
    for (const status of ['completed', 'verified', 'backlog'] as const) expect(String(TicketRow({ slug: 'HS2-INACTIVE', title: 'Inactive', status, priority: 'default', category: 'task', tags: [] }))).not.toContain('data-action="toggle-row-up-next"');
    expect(String(TicketRow({ slug: 'HS2-ARCHIVE', title: 'Archive projection', status: 'not_started', upNextEligible: false, priority: 'default', category: 'task', tags: [] }))).not.toContain('data-action="toggle-row-up-next"');
  });
});
