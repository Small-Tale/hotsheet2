import { describe, expect, it, vi } from 'vitest';

import { loadTicketEditorSizes, saveTicketEditorSize, ticketEditorKind, ticketEditorSizeStorageKey, ticketEditorSizeVariable } from './ticket-editor-size';

describe('ticket editor size preferences', () => {
  it('keeps separate global keys and variables for every field and presentation', () => {
    expect(ticketEditorSizeStorageKey('details', 'sidebar')).not.toBe(ticketEditorSizeStorageKey('details', 'reader'));
    expect(ticketEditorSizeVariable('note', 'reader')).toBe('--hs-note-reader-height');
    expect(ticketEditorKind('markdown-source')).toBe('details');
    expect(ticketEditorKind('blocked-reason')).toBe('blocked-reason');
    expect(ticketEditorKind('note-body')).toBe('note');
  });

  it('rounds, persists, and restores valid heights while ignoring invalid storage', () => {
    const values = new Map<string, string>([[ticketEditorSizeStorageKey('details', 'reader'), '237.6'], [ticketEditorSizeStorageKey('note', 'sidebar'), 'bad']]);
    const storage = { getItem: (key: string) => values.get(key) ?? null, setItem: (key: string, value: string) => values.set(key, value) };
    const setProperty = vi.fn();
    loadTicketEditorSizes(storage, { setProperty });
    expect(setProperty).toHaveBeenCalledWith('--hs-details-reader-height', '238px');
    expect(setProperty).not.toHaveBeenCalledWith('--hs-note-sidebar-height', expect.anything());
    saveTicketEditorSize(storage, { setProperty }, 'blocked-reason', 'sidebar', 111.4);
    expect(values.get(ticketEditorSizeStorageKey('blocked-reason', 'sidebar'))).toBe('111');
    expect(setProperty).toHaveBeenCalledWith('--hs-blocked-reason-sidebar-height', '111px');
  });
});
