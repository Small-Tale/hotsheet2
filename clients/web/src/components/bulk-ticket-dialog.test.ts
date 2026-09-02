import { describe, expect, it } from 'vitest';

import { BulkTicketDialog } from './bulk-ticket-dialog';

describe('BulkTicketDialog', () => {
  it('renders add and remove tag workflows with stable action contracts', () => {
    const add = String(BulkTicketDialog({ state: { kind: 'tag', mode: 'add', count: 3, choices: [] } }));
    expect(add).toContain('label="Add tag — 3 selected"');
    expect(add).toContain('data-action="submit-bulk-tag" data-tag-mode="add"');
    const remove = String(BulkTicketDialog({ state: { kind: 'tag', mode: 'remove', count: 2, choices: ['bug', 'client'] } }));
    expect(remove).toContain('label="Remove tag — 2 selected"');
    expect(remove).toContain('data-action="choose-bulk-tag" data-tag="bug"');
  });

  it('requires explicit confirmation for deletion', () => {
    const markup = String(BulkTicketDialog({ state: { kind: 'delete', count: 2 } }));
    expect(markup).toContain('label="Delete 2 tickets?"');
    expect(markup).toContain('data-action="confirm-bulk-delete"');
    expect(markup).toContain('Delete 2 tickets');
  });
});
