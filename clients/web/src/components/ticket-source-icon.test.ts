import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { TicketSourceIcon } from './ticket-source-icon';
import { GIT_SOURCE_MARK, GITHUB_SOURCE_MARK } from './ticket-source-marks';

describe('TicketSourceIcon', () => {
  it('renders the supplied filled Git and GitHub marks with the selected source color', () => {
    for (const [provider, mark] of [
      ['git', GIT_SOURCE_MARK],
      ['github', GITHUB_SOURCE_MARK],
    ] as const) {
      const markup = String(TicketSourceIcon({ source: { provider, name: provider, color: '#3b82f6' } }));
      expect(markup).toContain('style="color: #3b82f6"');
      expect(markup).toContain('fill="currentColor"');
      expect(markup).toContain(`d="${mark}"`);
      expect(markup).not.toContain('background-color');
    }
  });

  it('uses the sunken surface fill for a transparent source', () => {
    const markup = String(TicketSourceIcon({ source: { provider: 'git', name: 'Local', color: 'transparent' } }));
    const css = readFileSync(new URL('./ticket-source-icon.css', import.meta.url), 'utf8');
    expect(markup).not.toContain('style="color:');
    expect(css).toContain('color: var(--wa-color-surface-lowered)');
    expect(css).not.toContain('border-radius');
  });
});
