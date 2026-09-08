import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

describe('dialog lifecycle event contracts', () => {
  const source = readFileSync(new URL('./main.tsx', import.meta.url), 'utf8');

  it('uses cancelable Web Awesome wa-hide for every component dialog', () => {
    for (const selector of ['data-terminal-visibility-name-dialog', 'data-terminal-rename-dialog', 'bulk-tag-dialog', 'bulk-delete-dialog', 'not-working-dialog']) {
      expect(source).toContain(`'wa-hide'`);
      expect(source).not.toMatch(new RegExp(`wa-request-close[^\\n]*${selector}`));
    }
    expect(source).toMatch(/'wa-hide','\[data-component="not-working-dialog"\]'.*notWorkingSubmitting\.value.*event\.preventDefault\(\).*closeNotWorking\(\)/);
  });

  it('uses captured native close for the remaining HTML command dialogs', () => {
    expect(source).toMatch(/delegateCapture\(document\.body,'close','\[data-component="command-run-dialog"\], \[data-component="command-cancellation-dialog"\]'/);
    expect(source).not.toMatch(/wa-request-close[^\n]*command-run-dialog/);
    expect(source).not.toContain("'wa-request-close'");
  });
});
