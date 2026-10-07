import { describe, expect, it } from 'vitest';

import { TicketInfoPanel } from './ticket-info-panel';
import { TicketInspectorSkeleton } from './ticket-inspector';
import { TicketNotes } from './ticket-notes';

describe('TicketInspectorSkeleton', () => {
  it('renders the real inspector chrome in a busy, value-free placeholder state', () => {
    const markup = String(TicketInspectorSkeleton({ collapseControl: true }));
    expect(markup).toContain('data-component="ticket-inspector-skeleton"');
    expect(markup).toContain('aria-busy="true"');
    // It reuses the real inspector chrome so it still looks like the inspector.
    expect(markup).toContain('class="ticket-inspector ticket-inspector--placeholder"');
    expect(markup).toContain('class="kui-tab-bar ticket-inspector__tabs"');
    expect(markup).toContain('data-component="toolbar-text" data-size="small" data-placeholder="true"');
    expect(markup).toContain('class="ticket-info-panel__section ticket-info-panel__details-section"');
    // Real Kerf tab bar with four disabled placeholder tabs and Info selected.
    expect(markup.match(/data-component="app-tab"/g)).toHaveLength(4);
    expect(markup).toMatch(/data-tab-id="info"[^>]*data-selected="true"[^>]*data-placeholder="true"/);
    expect(markup).toContain('role="tab" aria-selected="true"');
    // Real section headers/controls are drawn (labels are chrome, not per-ticket values).
    for (const label of ['Category', 'Priority', 'Status', 'Block ticket', 'Details', 'Tags', 'Notes']) {
      expect(markup).toContain(label);
    }
    // Category and priority keep their Select placeholders; Status uses a value-free Skeleton.
    expect(markup.match(/kui-select--placeholder/g)).toHaveLength(2);
    expect(markup).toContain('class="kui-text" data-component="text"');
    expect(markup).toContain('data-font="default" data-border="none">Status</h2>');
    expect(markup).toContain('class="kui-list-inset-control"');
    expect(markup).toContain('<div class="ticket-info-panel__status-line">');
    // Unknown value slots (title, details, note bodies, provenance) use the native Skeleton block.
    expect(markup).toContain('kui-skeleton');
    // The collapse control still works while loading; nothing else is interactive.
    expect(markup).toContain('data-action="toggle-ticket-inspector"');
    expect(markup).toContain('aria-label="Hide ticket inspector"');
    expect(markup).toContain('data-component="ticket-inspector-skeleton-body"');
    expect(markup).not.toMatch(/HS2-/);
  });

  it('composes the info panel and notes placeholder variants instead of borrowing their classes', () => {
    const markup = String(TicketInspectorSkeleton());
    // HS2-XBHADT: the body is the TicketInfoPanel placeholder variant, which composes TicketNotes'.
    expect(markup).toMatch(/data-component="ticket-info-panel"[^>]*data-placeholder="true"/);
    expect(markup).toMatch(/<section class="ticket-notes" data-component="ticket-notes" data-placeholder="true">/);
    expect(markup).not.toContain('ticket-inspector-skeleton__');
    expect(markup).not.toContain('ticket-inspector-skeleton-panel');
    // HS2-MYS1MR: the loading tabs use the loaded sidebar inspector's icon-only presentation.
    expect(markup.match(/data-component="app-tab"[^>]*data-presentation="icon-only"/g)).toHaveLength(4);
  });

  it('shows the known slug while its ticket loads, and a skeleton slug otherwise', () => {
    expect(String(TicketInspectorSkeleton({ slug: 'HS2-4J50K3' }))).toContain('HS2-4J50K3');
    // Without a known slug the header uses ToolbarText's own placeholder, not ticket text.
    expect(String(TicketInspectorSkeleton())).not.toMatch(/HS2-/);
  });
});

describe('placeholder variants', () => {
  it('renders TicketInfoPanel value-free, inert, and hidden in either presentation', () => {
    const sidebar = String(TicketInfoPanel({ placeholder: true }));
    expect(sidebar).not.toContain('ticket-inspector-panel--reader');
    expect(sidebar).toContain('data-placeholder="true"');
    expect(sidebar).toContain('aria-hidden="true"');
    expect(sidebar).toMatch(/\binert\b/);
    expect(sidebar).toContain('class="ticket-info-panel__details-placeholder"');
    expect(sidebar.match(/kui-select--placeholder/g)).toHaveLength(2);
    expect(sidebar).not.toContain('Not started');
    // The Tags and Notes add actions are drawn but disabled while loading.
    expect(sidebar.match(/aria-label="Add (tag|note)"[^>]*disabled/g)).toHaveLength(2);
    // Value-free: no tags, notes, provider, or updated time.
    expect(sidebar).not.toContain('Hot Sheet git');
    expect(sidebar).not.toContain('No notes added.');
    expect(String(TicketInfoPanel({ placeholder: true, readerPresentation: true }))).toMatch(
      /ticket-inspector-panel--reader/,
    );
  });

  it('renders TicketNotes as two activity entries and one regular note card', () => {
    const markup = String(TicketNotes({ placeholder: true }));
    expect(markup.match(/data-kind="activity"/g)).toHaveLength(2);
    expect(markup.match(/data-kind="regular"/g)).toHaveLength(1);
    expect(markup).toContain('Activity');
    expect(markup).toContain('kui-skeleton');
    // The loaded notes variant is unchanged.
    const loaded = String(TicketNotes({ notes: [] }));
    expect(loaded).toContain('No notes added.');
    expect(loaded).not.toContain('data-placeholder');
  });
});
