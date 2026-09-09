import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { TicketCloseDialog } from './ticket-close-dialog';

const source = { id: 'source', slug: 'HS2-SOURCE', title: 'Repeated report' };
const target = { id: 'target', slug: 'HS2-TARGET', title: 'Canonical report' };

describe('TicketCloseDialog', () => {
  it('offers every structured close reason and requires a duplicate target', () => {
    const markup = String(TicketCloseDialog({ state: { source, reason: 'duplicate', query: 'canon', candidates: [source, target] } }));
    for (const reason of ['completed', 'not_planned', 'duplicate', 'obsolete']) expect(markup).toContain(`value="${reason}"`);
    expect(markup).toContain('data-action="select-ticket-close-target" data-item-id="target"');
    expect(markup).not.toContain('data-item-id="source"');
    expect(markup).toContain('Select the existing ticket');
    expect(markup).toContain('disabled>Mark as duplicate');
  });

  it('shows the selected canonical ticket and enables duplicate close', () => {
    const markup = String(TicketCloseDialog({ state: { source, reason: 'duplicate', query: '', candidates: [], selected: target } }));
    expect(markup).toContain('HS2-TARGET');
    expect(markup).toContain('Canonical report');
    expect(markup).toContain('data-action="clear-ticket-close-target"');
    expect(markup).toContain('>Mark as duplicate</wa-button>');
    expect(markup).not.toContain('disabled>Mark as duplicate');
  });

  it('contains long result lists within a scrollable dialog surface', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-close-dialog.css'), 'utf8');
    expect(css).toMatch(/ticket-close-dialog__results \{[^}]*min-height: 4\.25rem;[^}]*max-height: 14rem;[^}]*overflow-y: auto/);
    expect(css).toContain('width: min(34rem, calc(100vw - 2 * var(--wa-space-m)))');
  });
});
