import { beforeEach, describe, expect, it } from 'vitest';

import {
  BULK_TICKET_DIALOG_SCENARIOS,
  BulkTicketDialogDemo,
  BulkTicketDialogSettings,
  bulkTicketDialogSettings,
  bulkTicketDialogState,
  closeBulkTicketDialogDemo,
  openBulkTicketDialogDemo,
  resetBulkTicketDialogDemo,
  setBulkTicketDialogScenario,
} from './bulk-ticket-dialog-demo';

function fakeRoot() {
  const control = { value: 'delete', checked: false };
  const root = {
    querySelector: (selector: string) => (selector.includes('[name="bulk-scenario"]') ? control : null),
  } as unknown as ParentNode;
  return { root, control };
}

describe('BulkTicketDialog demo (HS2-PS9BQV)', () => {
  beforeEach(() => {
    bulkTicketDialogSettings.scenario.value = 'add-tag';
    bulkTicketDialogSettings.open.value = true;
  });

  it('maps every scenario to the production dialog state it previews', () => {
    expect(BULK_TICKET_DIALOG_SCENARIOS.map(({ value }) => value)).toEqual([
      'add-tag',
      'remove-tag',
      'delete',
      'empty-trash',
      'empty-trash-busy',
      'empty-trash-error',
    ]);
    expect(bulkTicketDialogState('add-tag')).toMatchObject({ kind: 'tag', mode: 'add' });
    expect(bulkTicketDialogState('remove-tag')).toMatchObject({ kind: 'tag', mode: 'remove' });
    expect(bulkTicketDialogState('delete')).toEqual({ kind: 'delete', count: 5 });
    expect(bulkTicketDialogState('empty-trash')).toEqual({ kind: 'empty-trash', count: 5 });
    expect(bulkTicketDialogState('empty-trash-busy')).toMatchObject({ busy: true });
    expect(bulkTicketDialogState('empty-trash-error')).toMatchObject({ error: expect.stringContaining('Trash') });
  });

  it('renders remove-mode tag choices, closes on an action, reopens on a scenario change, and resets', () => {
    setBulkTicketDialogScenario('remove-tag');
    const remove = String(BulkTicketDialogDemo());
    expect(remove).toContain('data-tag-mode="remove"');
    expect(remove.match(/data-action="choose-bulk-tag"/g)).toHaveLength(4);

    closeBulkTicketDialogDemo('Delete 5 tickets requested.');
    const closed = String(BulkTicketDialogDemo());
    expect(closed).not.toContain('wa-dialog');
    expect(closed).toContain('data-action="open-bulk-ticket-dialog-demo"');
    expect(closed).toContain('Delete 5 tickets requested.');
    openBulkTicketDialogDemo();
    expect(String(BulkTicketDialogDemo())).toContain('data-component="bulk-tag-dialog"');

    closeBulkTicketDialogDemo('Dismissed');
    setBulkTicketDialogScenario('delete');
    expect(bulkTicketDialogSettings.open.value).toBe(true);
    expect(String(BulkTicketDialogDemo())).toContain('data-component="bulk-delete-dialog"');

    const { root, control } = fakeRoot();
    resetBulkTicketDialogDemo(root);
    expect(bulkTicketDialogSettings.scenario.value).toBe('add-tag');
    expect(control.value).toBe('add-tag');
    expect(String(BulkTicketDialogDemo())).toContain('data-tag-mode="add"');
    expect(String(BulkTicketDialogSettings())).toContain('data-action="reset-settings"');
  });
});
