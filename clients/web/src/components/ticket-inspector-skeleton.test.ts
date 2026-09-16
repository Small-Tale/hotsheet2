import { describe, expect, it } from 'vitest';

import { TicketInspectorSkeleton } from './ticket-inspector-skeleton';

describe('TicketInspectorSkeleton', () => {
  it('renders a shape-preserving, busy placeholder without ticket values', () => {
    const markup = String(TicketInspectorSkeleton());
    expect(markup).toContain('data-component="ticket-inspector-skeleton"');
    expect(markup).toContain('aria-busy="true"');
    // Keeps the inspector chrome (a title bar, a four-tab bar, metadata fields, and body lines).
    expect(markup).toContain('ticket-inspector-skeleton__title');
    expect(markup.match(/ticket-inspector-skeleton__tab\b/g)).toHaveLength(4);
    expect(markup.match(/ticket-inspector-skeleton__field/g)).toHaveLength(3);
    expect(markup.match(/ticket-inspector-skeleton__line\b/g)).toHaveLength(4);
    // The collapse control still works while loading, but no ticket text is shown.
    expect(markup).toContain('data-action="close-ticket-inspector"');
    expect(markup).not.toMatch(/HS2-/);
  });
});
