import { describe, expect, it } from 'vitest';

import { TicketInspectorSkeleton } from './ticket-inspector-skeleton';

describe('TicketInspectorSkeleton', () => {
  it('renders the real inspector chrome in a busy, value-free placeholder state', () => {
    const markup = String(TicketInspectorSkeleton());
    expect(markup).toContain('data-component="ticket-inspector-skeleton"');
    expect(markup).toContain('aria-busy="true"');
    // It reuses the real inspector chrome so it still looks like the inspector.
    expect(markup).toContain('class="ticket-inspector ticket-inspector--placeholder"');
    expect(markup).toContain('class="ticket-inspector__tabs"');
    // Real segmented tab bar with four tabs, Info current.
    expect(markup.match(/ticket-inspector__tab-label/g)).toHaveLength(4);
    expect(markup).toContain('aria-current="page"');
    // Real section headers/controls are drawn (labels are chrome, not per-ticket values).
    for (const label of ['Category', 'Priority', 'Status', 'Block ticket', 'Details', 'Tags', 'Notes']) {
      expect(markup).toContain(label);
    }
    // Placeholder value slots stand in for the unknown values, and there is no ticket text.
    expect(markup).toContain('ticket-inspector__ph-control');
    expect(markup).toContain('ticket-inspector__ph-status');
    expect(markup.match(/ticket-inspector__ph-line\b/g)).toHaveLength(3);
    // The collapse control still works while loading; nothing else is interactive.
    expect(markup).toContain('data-action="close-ticket-inspector"');
    expect(markup).not.toMatch(/HS2-/);
  });

  it('shows the known slug while its ticket loads, and a placeholder slug otherwise', () => {
    expect(String(TicketInspectorSkeleton({ slug: 'HS2-4J50K3' }))).toContain('HS2-4J50K3');
    expect(String(TicketInspectorSkeleton())).toContain('ticket-inspector__ph-slug');
  });
});
