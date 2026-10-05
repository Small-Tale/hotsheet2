import { describe, expect, it } from 'vitest';

import { type TicketLinkChoice, TicketLinkChoiceDialog } from './ticket-link-choice-dialog';

const choice: TicketLinkChoice = {
  kind: 'choose',
  reference: { raw: 'HS2-SHARED1', slug: 'HS2-SHARED1' },
  matches: [
    {
      projectId: 'alpha-01',
      projectName: 'Alpha',
      ticketId: 'one',
      qualifiedId: 'git:one',
      connectionId: 'git',
      slug: 'HS2-SHARED1',
      title: 'Local implementation',
      status: 'started',
    },
    {
      projectId: 'alpha-01',
      projectName: 'Alpha',
      ticketId: 'two',
      qualifiedId: 'github:42',
      connectionId: 'github',
      slug: 'HS2-SHARED1',
      title: 'Provider mirror',
      status: 'completed',
    },
  ],
};

describe('TicketLinkChoiceDialog', () => {
  it('renders a focused exact-match choice without advanced-search controls', () => {
    const markup = String(TicketLinkChoiceDialog({ choice }));
    expect(markup).toContain('data-component="ticket-link-choice-dialog"');
    expect(markup).toContain('More than one ticket has this exact reference.');
    expect(markup.match(/data-action="select-ticket-link-match"/g)).toHaveLength(2);
    expect(markup).toContain('data-ticket-project-id="alpha-01"');
    expect(markup).toContain('data-ticket-qualified-id="git:one"');
    expect(markup).toContain('data-match-key="alpha-01::github%3A42"');
    expect(markup).toContain('Local implementation');
    expect(markup).toContain('Provider mirror');
    expect(markup).toContain('data-action="cancel-ticket-link-choice"');
    expect(markup).not.toContain('global-search-query');
    expect(markup).not.toContain('save-search-as-view');
  });

  it('stays absent without an ambiguous resolution and renders wrapping public choice rows', () => {
    expect(String(TicketLinkChoiceDialog({}))).toBe('');
    const markup = String(TicketLinkChoiceDialog({ choice }));
    expect(markup).toContain('data-component="dialog-surface"');
    expect(markup.match(/data-component="list-item"/g)).toHaveLength(2);
    expect(markup).toContain('data-multiline="true"');
    expect(markup).toContain('data-component="text"');
    expect(markup).toContain('data-wrap="anywhere"');
    expect(markup).toContain('data-status="started"');
    expect(markup).toContain('data-status="completed"');
    // The delegated action reads its metadata from the activated row, not a wrapper.
    for (const qualifiedId of ['git:one', 'github:42']) {
      const row = markup.match(new RegExp(`<button[^>]*data-ticket-qualified-id="${qualifiedId}"[^>]*>`))?.[0];
      expect(row).toContain('data-action="select-ticket-link-match"');
      expect(row).toContain('data-match-key="alpha-01::');
    }
  });
});
