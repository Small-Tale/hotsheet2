import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { TicketCloseDialog } from './ticket-close-dialog';

const source = { id: 'source', slug: 'HS2-SOURCE', title: 'Repeated report', projectId: 'alpha', projectName: 'Alpha', connectionId: 'git-alpha', nativeId: 'source', qualifiedId: 'git-alpha:source' };
const target = { id: 'target', slug: 'HS2-TARGET', title: 'Canonical report', projectId: 'beta', projectName: 'Beta', connectionId: 'git-beta', nativeId: 'target', qualifiedId: 'git-beta:target' };
const sameSlug = { ...target, id: 'collision', projectId: 'gamma', projectName: 'Gamma', connectionId: 'git-gamma', nativeId: 'collision', qualifiedId: 'git-gamma:collision' };

describe('TicketCloseDialog', () => {
  it('offers every structured close reason and requires a duplicate target', () => {
    const markup = String(TicketCloseDialog({ state: { source, reason: 'duplicate', query: 'canon', candidates: [source, target, sameSlug] } }));
    for (const reason of ['completed', 'not_planned', 'duplicate', 'obsolete']) expect(markup).toContain(`value="${reason}"`);
    expect(markup).toContain('data-action="select-ticket-close-target" data-item-id="beta::git-beta%3Atarget"');
    expect(markup).not.toContain('data-item-id="alpha::git-alpha%3Asource"');
    expect(markup).toContain('data-item-id="gamma::git-gamma%3Acollision"');
    expect(markup).toContain('<small>Beta</small>');
    expect(markup).toContain('<small>Gamma</small>');
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
