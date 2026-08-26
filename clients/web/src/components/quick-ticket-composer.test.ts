import { describe, expect, it } from 'vitest';
import { QuickTicketComposer } from './quick-ticket-composer';

describe('QuickTicketComposer', () => {
  it('has distinct collapsed, editable, and provider-disabled presentations', () => {
    expect(String(QuickTicketComposer({}))).toContain('data-action="expand-ticket-composer"');
    const expanded = String(QuickTicketComposer({ expanded: true, title: 'New work', category: 'bug' }));
    expect(expanded).toContain('data-action="create-ticket-form"');
    expect(expanded).toContain('value="New work"');
    const disabled = String(QuickTicketComposer({ expanded: true, canCreate: false, providerName: 'Read-only Jira' }));
    expect(disabled).toContain('does not support creating tickets');
    expect(disabled).toContain('disabled');
  });
});
