import { describe, expect, it } from 'vitest';

import { TicketFieldConflict } from './ticket-field-conflict';

describe('TicketFieldConflict', () => {
  it('presents both versions and an editable merge choice', () => {
    const html = String(TicketFieldConflict({ conflict: { key: 'details', field: 'details', label: 'Details', base: 'Base', mine: 'Mine', theirs: 'Theirs' }, resolution: 'Mine plus theirs' }));
    expect(html).toContain('data-component="ticket-field-conflict"');
    expect(html).toContain('Their latest version');
    expect(html).toContain('Your version');
    expect(html).toContain('name="ticket-conflict-resolution"');
    expect(html).toContain('Mine plus theirs');
    expect(html).toContain('data-action="accept-remote-ticket-field"');
    expect(html).toContain('data-action="apply-ticket-field-merge"');
    expect(html).toContain('data-lucide="git-merge"');
  });
});
