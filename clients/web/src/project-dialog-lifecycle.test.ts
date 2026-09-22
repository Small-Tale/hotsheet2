import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

describe('project dialog lifecycle', () => {
  it('synchronizes native Web Awesome dismissal back to the Kerf open signal', () => {
    const source = ['./main.tsx', './interactions/project-lifecycle.ts']
      .map((file) => readFileSync(new URL(file, import.meta.url), 'utf8'))
      .join('\n');

    expect(source).toMatchSource(
      /delegate\(document\.body,'wa-hide','\[data-project-dialog\]',\(\)=>\{unhealthyServerRecovery\.value=undefined;projectDialogOpen\.value=false\}\)/,
    );
    expect(source).not.toMatch(/delegate\(document\.body,'wa-request-close','\[data-project-dialog\]'/);
  });

  it('clears setup state on the actual dialog hide event so later renders cannot reopen it', () => {
    const source = ['./main.tsx', './interactions/project-lifecycle.ts']
      .map((file) => readFileSync(new URL(file, import.meta.url), 'utf8'))
      .join('\n');

    expect(source).toMatchSource(/delegate\(document\.body,'wa-hide','\[data-ticket-source-setup-dialog\]'/);
    expect(source).not.toMatch(/delegate\(document\.body,'wa-request-close','\[data-ticket-source-setup-dialog\]'/);
  });

  it('routes the project-tab plus through the native chooser on same-device clients and the open-projects list on remote clients (HS2-VFNCXG)', () => {
    const source = ['./main.tsx', './interactions/project-lifecycle.ts']
      .map((file) => readFileSync(new URL(file, import.meta.url), 'utf8'))
      .join('\n');

    expect(source).toMatchSource(
      /delegate\(document\.body,'click','\[data-action="choose-project"\]',\(\)=>\{if\(isRemoteClient\(\)\)void openRemoteProjectDialog\(\);else void chooseAndOpenProject\(\)\}\)/,
    );
    expect(source).toMatchSource(/if\(result\.path\)await openProject\(result\.path\)/);
  });
});
