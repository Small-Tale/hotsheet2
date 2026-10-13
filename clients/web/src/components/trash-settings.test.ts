import { describe, expect, it } from 'vitest';

import { trashDraftIsDirty, trashPurgeHint, TrashSettings } from './trash-settings';

describe('TrashSettings', () => {
  it('renders a positive whole-day shared retention control with recovery context', () => {
    const markup = String(TrashSettings({ days: 30, message: 'Enter a positive whole number of days.' }));
    expect(markup).toContain('data-action="save-trash-settings"');
    expect(markup).toContain('name="trash-cleanup-days" type="number"');
    expect(markup).toContain('value="30" required');
    expect(markup).toContain('label="Keep deleted tickets for (days)"');
    expect(markup).toContain('The default is 30 days');
    expect(markup).toContain('Git history keeps every purged ticket file');
    expect(markup).toContain('Enter a positive whole number of days.');
    expect(markup).toContain('data-component="list"');
    expect(markup).toContain('data-component="row"');
    expect(markup).toContain('data-component="text"');
  });

  it('puts "Apply now" beside the field and enables it only for an unapplied value (HS2-KPD942)', () => {
    const clean = String(TrashSettings({ days: 30 }));
    expect(clean).toMatch(/<wa-button type="submit" variant="brand" disabled[^>]*>\s*Apply now/);
    expect(clean).not.toContain('Save retention');
    expect(clean).not.toContain('data-role="trash-purge-hint"');
    const same = String(TrashSettings({ days: 30, draft: { value: ' 30 ', purgeCount: 4 } }));
    expect(same).not.toContain('data-role="trash-purge-hint"');
    const dirty = String(TrashSettings({ days: 30, draft: { value: '7', purgeCount: 3 } }));
    expect(dirty).toContain('value="7"');
    expect(dirty).not.toMatch(/variant="brand" disabled/);
    expect(dirty).toContain('data-dirty="true"');
    expect(dirty).toContain('3 tickets in Trash will be purged');
    expect(dirty).not.toContain('trash-retention-leave-dialog');
  });

  it('asks to apply or revert an unapplied value when navigation is held (HS2-F55956)', () => {
    const markup = String(TrashSettings({ days: 30, draft: { value: '1', purgeCount: 1, leaving: true } }));
    expect(markup).toContain('data-component="trash-retention-leave-dialog"');
    expect(markup).toContain('You changed Trash retention to 1 day but');
    expect(markup).toContain('1 ticket in Trash will be purged');
    expect(markup).toContain('data-action="revert-trash-settings"');
    expect(markup).toContain('Revert to 30 days');
    expect(markup).toContain('data-action="apply-leaving-trash-settings"');
    // A held navigation whose draft matches the applied value needs no question.
    expect(String(TrashSettings({ days: 30, draft: { value: '30', leaving: true } }))).not.toContain(
      'trash-retention-leave-dialog',
    );
  });

  it('formats the purge hint and dirty check', () => {
    expect(trashPurgeHint(undefined)).toBe('');
    expect(trashPurgeHint(0)).toBe('');
    expect(trashPurgeHint(1)).toBe('1 ticket in Trash will be purged');
    expect(trashPurgeHint(12)).toBe('12 tickets in Trash will be purged');
    expect(trashDraftIsDirty(30)).toBe(false);
    expect(trashDraftIsDirty(30, { value: '30' })).toBe(false);
    expect(trashDraftIsDirty(30, { value: '' })).toBe(true);
    expect(trashDraftIsDirty(30, { value: '14' })).toBe(true);
  });
});
