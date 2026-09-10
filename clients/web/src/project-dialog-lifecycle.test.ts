import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

describe('project dialog lifecycle', () => {
  it('synchronizes native Web Awesome dismissal back to the Kerf open signal', () => {
    const source = readFileSync(new URL('./main.tsx', import.meta.url), 'utf8');

    expect(source).toMatch(/delegate\(document\.body,'wa-hide','\[data-project-dialog\]',\(\)=>\{projectDialogOpen\.value=false\}\)/);
    expect(source).not.toMatch(/delegate\(document\.body,'wa-request-close','\[data-project-dialog\]'/);
  });

  it('clears setup state on the actual dialog hide event so later renders cannot reopen it', () => {
    const source = readFileSync(new URL('./main.tsx', import.meta.url), 'utf8');

    expect(source).toMatch(/delegate\(document\.body,'wa-hide','\[data-ticket-source-setup-dialog\]'/);
    expect(source).not.toMatch(/delegate\(document\.body,'wa-request-close','\[data-ticket-source-setup-dialog\]'/);
  });

  it('routes the project-tab plus directly through the native chooser', () => {
    const source = readFileSync(new URL('./main.tsx', import.meta.url), 'utf8');

    expect(source).toMatch(/delegate\(document\.body,'click','\[data-action="choose-project"\]',\(\)=>\{void chooseAndOpenProject\(\)\}\)/);
    expect(source).toMatch(/if\(result\.path\)await openProject\(result\.path\)/);
  });
});
