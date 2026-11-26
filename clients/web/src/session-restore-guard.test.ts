import { describe, expect, it } from 'vitest';

import { restoreSectionTabs, selectionRestorable, type SessionSectionTabs } from './session-restore-guard';

describe('session restore guard (HS2-CN0CS7)', () => {
  it('restores the parked selection only while the applied array is still selected', () => {
    const applied: string[] = [];
    expect(selectionRestorable(undefined, [])).toBe(true);
    expect(selectionRestorable(applied, applied)).toBe(true);
    // Equal contents are still a new selection: every user selection writes a new array.
    expect(selectionRestorable(applied, [])).toBe(false);
    expect(selectionRestorable(applied, ['HS2-DEMO01'])).toBe(false);
  });

  it('keeps a ticket the user selected between two restores of one activation', () => {
    // Activation applies its selection, the first restore applies the parked one and records it.
    const activation: string[] = [];
    expect(selectionRestorable(activation, activation)).toBe(true);
    // The first restore wrote the parked selection and recorded that array.
    const record: string[] = [];
    // The user selects a ticket, then a second activation's restore runs.
    const current = ['HS2-DEMO01'];
    expect(selectionRestorable(record, current)).toBe(false);
    // A third restore still sees the user's selection.
    expect(selectionRestorable(record, current)).toBe(false);
  });

  it('keeps a selection the user made during the first restore through the second', () => {
    const record: string[] = [],
      current = ['HS2-DEMO01'];
    // The first restore keeps the user's selection and leaves the record, so the second does too.
    expect(selectionRestorable(record, current)).toBe(false);
    expect(selectionRestorable(record, current)).toBe(false);
  });

  it('restores every section part when nothing was applied yet', () => {
    const parked: SessionSectionTabs = { inspector: 'timeline', reader: 'attachments', view: 'backlog' };
    expect(restoreSectionTabs(undefined, { inspector: 'info', reader: 'info', view: 'all' }, parked)).toEqual({
      show: parked,
      applied: parked,
    });
  });

  it('restores unchanged parts and keeps each part the user changed across repeated restores', () => {
    const activation: SessionSectionTabs = { inspector: 'info', reader: 'info', view: 'all' },
      parked: SessionSectionTabs = { inspector: 'timeline', reader: 'attachments', view: 'backlog' };
    const first = restoreSectionTabs(activation, activation, parked);
    expect(first).toEqual({ show: parked, applied: parked });
    // The user picks the Info tab between the restores; the view and reader tab are untouched.
    const userChoice: SessionSectionTabs = { ...first.show, inspector: 'info' };
    const second = restoreSectionTabs(first.applied, userChoice, parked);
    expect(second.show).toEqual({ inspector: 'info', reader: 'attachments', view: 'backlog' });
    // The record keeps the earlier applied tab, so a third restore leaves the user's choice too.
    expect(second.applied).toEqual(parked);
    expect(restoreSectionTabs(second.applied, second.show, parked).show.inspector).toBe('info');
  });

  it('keeps a part the user changed during activation before the first restore', () => {
    const activation: SessionSectionTabs = { inspector: 'info', reader: 'info', view: 'all' },
      parked: SessionSectionTabs = { inspector: 'timeline', reader: 'timeline', view: 'archive' },
      current: SessionSectionTabs = { inspector: 'info', reader: 'info', view: 'backlog' };
    const result = restoreSectionTabs(activation, current, parked);
    expect(result.show).toEqual({ inspector: 'timeline', reader: 'timeline', view: 'backlog' });
    expect(result.applied).toEqual({ inspector: 'timeline', reader: 'timeline', view: 'all' });
  });
});
