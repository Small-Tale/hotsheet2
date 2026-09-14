import { describe, expect, it } from 'vitest';

import { TrashSettings } from './trash-settings';

describe('TrashSettings', () => {
  it('renders a positive whole-day shared retention control with recovery context', () => {
    const markup = String(TrashSettings({ days: 30, message: 'Saved for this project.' }));
    expect(markup).toContain('data-action="save-trash-settings"');
    expect(markup).toContain('name="trash-cleanup-days" type="number"');
    expect(markup).toContain('value="30" required');
    expect(markup).toContain('label="Keep deleted tickets for (days)"');
    expect(markup).toContain('The default is 30 days');
    expect(markup).toContain('Git history keeps every purged ticket file');
    expect(markup).toContain('Saved for this project.');
  });
});
