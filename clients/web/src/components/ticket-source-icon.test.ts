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

  it('uses visible Gray for an unset or legacy transparent source (HS2-H1FZNV)', () => {
    for (const color of [undefined, 'transparent']) {
      const markup = String(TicketSourceIcon({ source: { provider: 'git', name: 'Local', color } }));
      expect(markup).toContain('style="color: #6b7280"');
    }
    // Kerf LucideIcon owns the mark geometry; the wrapper keeps one layout rule and no color rule.
    const css = readFileSync(new URL('./ticket-source-icon.css', import.meta.url), 'utf8');
    expect(css.match(/^\.[^{]*\{/gm)).toEqual(['.ticket-source-icon {']);
    expect(css).not.toContain('color:');
    for (const provider of ['git', 'github', 'gitlab', 'jira', 'linear'])
      expect(String(TicketSourceIcon({ source: { provider, name: provider } }))).toContain(`data-lucide="${provider}"`);
  });
});
