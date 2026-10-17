import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { TicketCloseDialog } from './ticket-close-dialog';

const source = {
  id: 'source',
  slug: 'HS2-SOURCE',
  title: 'Repeated report',
  projectId: 'alpha',
  projectName: 'Alpha',
  connectionId: 'git-alpha',
  nativeId: 'source',
  qualifiedId: 'git-alpha:source',
};
const target = {
  id: 'target',
  slug: 'HS2-TARGET',
  title: 'Canonical report',
  projectId: 'beta',
  projectName: 'Beta',
  connectionId: 'git-beta',
  nativeId: 'target',
  qualifiedId: 'git-beta:target',
};
const sameSlug = {
  ...target,
  id: 'collision',
  projectId: 'gamma',
  projectName: 'Gamma',
  connectionId: 'git-gamma',
  nativeId: 'collision',
  qualifiedId: 'git-gamma:collision',
};

describe('TicketCloseDialog', () => {
  it('offers every structured close reason and requires a duplicate target', () => {
    const markup = String(
      TicketCloseDialog({
        state: { source, reason: 'duplicate', query: 'canon', candidates: [source, target, sameSlug] },
      }),
    );
    for (const reason of ['completed', 'not_planned', 'duplicate', 'obsolete', 'works_as_designed'])
      expect(markup).toContain(`value="${reason}"`);
    expect(markup).toContain('data-action="select-ticket-close-target" data-item-id="beta::git-beta%3Atarget"');
    expect(markup).not.toContain('data-item-id="alpha::git-alpha%3Asource"');
    expect(markup).toContain('data-item-id="gamma::git-gamma%3Acollision"');
    expect(markup).toMatch(/HS2-TARGET <span class="kui-text"[^>]*data-tone="quiet"[^>]*>Beta<\/span>/);
    expect(markup).toMatch(/<span class="kui-text"[^>]*data-tone="quiet"[^>]*>Gamma<\/span>/);
    expect(markup).toContain('Select the existing ticket');
    expect(markup).toMatch(/<button(?=[^>]*disabled)[^>]*>[^]*Mark as duplicate<\/span><\/button>/);
  });

  it('shows the selected canonical ticket and enables duplicate close', () => {
    const markup = String(
      TicketCloseDialog({ state: { source, reason: 'duplicate', query: '', candidates: [], selected: target } }),
    );
    expect(markup).toContain('HS2-TARGET');
    expect(markup).toContain('Canonical report');
    expect(markup).toContain('data-action="clear-ticket-close-target"');
    expect(markup).toContain('data-component="state-banner"');
    expect(markup).toContain('data-tone="success"');
    expect(markup).toContain('>Mark as duplicate</span></button>');
    expect(markup).not.toMatch(/<button(?=[^>]*disabled)[^>]*>[^]*Mark as duplicate<\/span><\/button>/);
  });

  it('contains long result lists within a scrollable dialog surface', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-close-dialog.css'), 'utf8');
    // Kerf's framed, bounded List owns the frame and scroll bounds (KF-SHW2J9, HS2-Z46VA7).
    expect(css).not.toMatch(
      /ticket-close-dialog__results \{[^}]*\n\s*(min-height|max-height|overflow|border|padding)[\w-]*:/,
    );
    // DialogSurface owns the width; Kerf List, Row, Text, and StateBanner own the rest (HS2-F8HT1E).
    expect(css).not.toMatch(/::part|__form|__selected|__candidate|__actions|__hint/);
    const markup = String(TicketCloseDialog({ state: { source, reason: 'completed', query: '', candidates: [] } }));
    expect(markup).toContain('data-component="dialog-surface"');
    expect(markup).toContain('data-preferred-width');
  });
});
