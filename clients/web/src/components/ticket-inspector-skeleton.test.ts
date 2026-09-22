import { describe, expect, it } from 'vitest';

import { TicketInspectorSkeleton } from './ticket-inspector-skeleton';

describe('TicketInspectorSkeleton', () => {
  it('renders the real inspector chrome in a busy, value-free placeholder state', () => {
    const markup = String(TicketInspectorSkeleton());
    expect(markup).toContain('data-component="ticket-inspector-skeleton"');
    expect(markup).toContain('aria-busy="true"');
    // It reuses the real inspector chrome so it still looks like the inspector.
    expect(markup).toContain('class="ticket-inspector ticket-inspector--placeholder"');
    expect(markup).toContain('class="kui-tab-bar ticket-inspector__tabs"');
    expect(markup).toContain('class="ticket-inspector__section ticket-inspector__details-section"');
    // Real Kerf tab bar with four disabled placeholder tabs and Info selected.
    expect(markup.match(/data-component="app-tab"/g)).toHaveLength(4);
    expect(markup).toContain('data-tab-id="info" data-selected="true" data-placeholder="true"');
    expect(markup).toContain('role="tab" aria-selected="true"');
    // Real section headers/controls are drawn (labels are chrome, not per-ticket values).
    for (const label of ['Category', 'Priority', 'Status', 'Block ticket', 'Details', 'Tags', 'Notes']) {
      expect(markup).toContain(label);
    }
    // The metadata controls use the @kerfjs/ui native Select placeholder mode (real chrome, skeleton value).
    expect(markup.match(/kui-select--placeholder/g)).toHaveLength(3);
    // Unknown value slots (title, details, note bodies, provenance) use the native Skeleton block.
    expect(markup).toContain('kui-skeleton');
    // The collapse control still works while loading; nothing else is interactive.
    expect(markup).toContain('data-action="close-ticket-inspector"');
    expect(markup).not.toMatch(/HS2-/);
  });

  it('shows the known slug while its ticket loads, and a skeleton slug otherwise', () => {
    expect(String(TicketInspectorSkeleton({ slug: 'HS2-4J50K3' }))).toContain('HS2-4J50K3');
    // Without a known slug the header slug is a Skeleton block, not ticket text.
    expect(String(TicketInspectorSkeleton())).not.toMatch(/HS2-/);
  });
});
