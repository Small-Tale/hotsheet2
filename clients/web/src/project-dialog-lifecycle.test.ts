import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

describe('project dialog lifecycle', () => {
  it('synchronizes native Web Awesome dismissal back to the Kerf open signal', () => {
    const source = readFileSync(new URL('./main.tsx', import.meta.url), 'utf8');

    expect(source).toMatch(/delegate\(document\.body,'wa-hide','\[data-project-dialog\]',\(\)=>\{projectDialogOpen\.value=false\}\)/);
    expect(source).not.toMatch(/delegate\(document\.body,'wa-request-close','\[data-project-dialog\]'/);
  });
});
