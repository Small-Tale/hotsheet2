import { describe, expect, it } from 'vitest';

import {
  acceptsProjectTabFileDrag,
  clearProjectTabDropTarget,
  markProjectTabDropTarget,
  projectTabDropKind,
  projectTabDroppedFiles,
  projectTabDropTarget,
} from './project-tab-file-drop';

describe('project tab file drops (HS2-Z2KY20)', () => {
  it('accepts OS file drags on an open destination and leaves ticket and text drags alone', () => {
    const files = { types: ['Files'] };
    expect(acceptsProjectTabFileDrag(false, true, files)).toBe(true);
    expect(acceptsProjectTabFileDrag(true, true, files)).toBe(false);
    expect(acceptsProjectTabFileDrag(false, false, files)).toBe(false);
    expect(acceptsProjectTabFileDrag(false, true, { types: ['text/plain'] })).toBe(false);
    expect(acceptsProjectTabFileDrag(false, true, null)).toBe(false);
  });

  it('stages every delivered file only for a valid destination and external drag', () => {
    const first = { name: 'first.txt' } as File,
      second = { name: 'second.png' } as File,
      transfer = { files: [first, second] as unknown as FileList };
    expect(projectTabDroppedFiles(false, true, transfer)).toEqual([first, second]);
    expect(projectTabDroppedFiles(true, true, transfer)).toEqual([]);
    expect(projectTabDroppedFiles(false, false, transfer)).toEqual([]);
    expect(projectTabDroppedFiles(false, true, null)).toEqual([]);
  });

  it('keeps one project tab drop target and clears only the matching tab and kind (HS2-6KNWYT)', () => {
    clearProjectTabDropTarget();
    expect(projectTabDropKind('other')).toBeUndefined();
    markProjectTabDropTarget('other', 'file');
    const marked = projectTabDropTarget.value;
    expect(projectTabDropKind('other')).toBe('file');
    // Repeated dragover events keep the same value, so the shell does not re-render per event.
    markProjectTabDropTarget('other', 'file');
    expect(projectTabDropTarget.value).toBe(marked);
    // A stale leave from another tab or another drag kind leaves the current target alone.
    clearProjectTabDropTarget('demo', 'file');
    clearProjectTabDropTarget('other', 'ticket');
    expect(projectTabDropKind('other')).toBe('file');
    markProjectTabDropTarget('third', 'ticket');
    expect(projectTabDropKind('other')).toBeUndefined();
    expect(projectTabDropKind('third')).toBe('ticket');
    clearProjectTabDropTarget(undefined, 'ticket');
    expect(projectTabDropTarget.value).toBeUndefined();
    markProjectTabDropTarget('other', 'file');
    clearProjectTabDropTarget('other', 'file');
    expect(projectTabDropTarget.value).toBeUndefined();
    markProjectTabDropTarget('other', 'ticket');
    clearProjectTabDropTarget();
    expect(projectTabDropTarget.value).toBeUndefined();
  });
});
