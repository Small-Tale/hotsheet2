import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { QuickTicketComposer } from './quick-ticket-composer';

describe('QuickTicketComposer', () => {
  it('gives the title the available width while keeping category compact', () => {
    const css=readFileSync(new URL('./quick-ticket-composer.css',import.meta.url),'utf8');
    expect(css).toContain('grid-template-columns: minmax(0, 1fr) minmax(12rem, 15rem)');
    expect(css).toMatch(/@media \(max-width: 38rem\)[^{]*\{[^}]*\.quick-ticket-composer \{ grid-template-columns: 1fr/);
  });
  it('has distinct collapsed, editable, and provider-disabled presentations', () => {
    const collapsed = String(QuickTicketComposer({}));
    expect(collapsed).toContain('data-action="expand-ticket-composer"');
    expect(collapsed).toContain('data-new-ticket-drop-target="true"');
    expect(collapsed).toContain('drop attachment files here');
    const expanded = String(QuickTicketComposer({ expanded: true, title: 'New work', category: 'bug', attachments: [{ id: 'proof', name: 'proof.png' }] }));
    expect(expanded).toContain('data-action="create-ticket-form"');
    expect(expanded).toContain('value="New work"');
    expect(expanded).toContain('data-lucide="bug"');
    expect(expanded).toContain('Browse attachments for new ticket');
    expect(expanded).toContain('Drop attachment files anywhere in this area or browse');
    expect(expanded).toContain('data-pending-attachment-id="proof"');
    expect(expanded).toContain('aria-label="Remove proof.png" title="Remove proof.png"');
    expect(expanded).not.toContain('data-lucide="x"');
    const disabled = String(QuickTicketComposer({ expanded: true, canCreate: false, attachmentsEnabled: false, providerName: 'Read-only Jira' }));
    expect(disabled).toContain('does not support creating tickets');
    expect(disabled).toContain('does not support attachments');
    expect(disabled).not.toContain('name="new-ticket-attachments"');
    expect(disabled).toContain('disabled');
  });

  it('shows creation progress and attachment errors accessibly', () => {
    const markup = String(QuickTicketComposer({ expanded: true, submitting: true, attachmentMessage: 'proof.png could not be read', attachmentError: true }));
    expect(markup).toContain('data-submitting="true"');
    expect(markup).toContain('Creating…');
    expect(markup).toContain('role="alert"');
    expect(markup).toContain('proof.png could not be read');
  });

  it('blocks creation while attachment screening is still busy', () => {
    const markup = String(QuickTicketComposer({ expanded: true, busy: true }));
    expect(markup).toContain('Create ticket');
    expect(markup).not.toContain('Creating…');
    expect(markup.match(/disabled/g)).toHaveLength(1);
  });
});
