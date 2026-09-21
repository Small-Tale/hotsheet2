import { existsSync, readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

/** Regression boundary for the retired freeform ModelInput: model entry now belongs to the
 * shared catalog submenu plus ManualModelDialog, so the obsolete production component must
 * not quietly return while older coverage references migrate. */
describe('retired ModelInput boundary', () => {
  it('keeps model selection on the shared submenu and exact-id dialog', () => {
    expect(existsSync(new URL('./model-input.tsx', import.meta.url))).toBe(false);
    const commandEditor = readFileSync(new URL('./command-settings-editor.tsx', import.meta.url), 'utf8');
    expect(commandEditor).toContain('ProviderModelEffortSubmenus');
    expect(commandEditor).toContain('open-command-manual-model');
  });
});
