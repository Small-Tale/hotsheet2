import { describe, expect, it } from 'vitest';

import { acceptsProjectTabFileDrag, projectTabDroppedFiles } from './project-tab-file-drop';

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
});
