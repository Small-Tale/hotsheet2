import { describe, expect, it } from 'vitest';

import { ProviderIcon } from './provider-icon';

describe('ProviderIcon', () => {
  it.each(['github', 'gitlab', 'jira'] as const)('renders the %s provider identity', (kind) => {
    const markup = String(ProviderIcon({ kind }));
    expect(markup).toContain(`data-provider-icon="${kind}"`);
    expect(markup).toContain(`aria-label="${kind === 'github' ? 'GitHub' : kind === 'gitlab' ? 'GitLab' : 'Jira'}"`);
    expect(markup).not.toContain('data-size');
  });

  it.each(['github', 'gitlab', 'jira'] as const)('renders the large %s variant as a size the icon owns', (kind) => {
    expect(String(ProviderIcon({ kind, size: 'l' }))).toContain('data-size="l"');
    expect(String(ProviderIcon({ kind, size: 'm' }))).not.toContain('data-size');
  });

  it('sizes itself through its own SVG dimensions instead of consumer or size CSS (HS2-ETZEFC)', async () => {
    expect(String(ProviderIcon({ kind: 'jira' }))).toContain('width="1em" height="1em"');
    expect(String(ProviderIcon({ kind: 'gitlab', size: 'l' }))).toContain('width="1.5rem" height="1.5rem"');
    const { readFileSync } = await import('node:fs');
    const css = readFileSync(new URL('./provider-icon.css', import.meta.url), 'utf8');
    expect(css).not.toMatch(/width|height|data-size/);
    const consumer = readFileSync(new URL('./ticket-sources-settings.css', import.meta.url), 'utf8');
    expect(consumer).not.toMatch(/__account-header > svg|provider-icon/);
  });
});
