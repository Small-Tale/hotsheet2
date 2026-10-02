import { describe, expect, it } from 'vitest';

import componentCatalogExtension from '../../ai/component-catalog-extension.json';
import { demosUsing, findDemo } from './catalog';
import { ProviderIconDemo } from './provider-icon-demo';

describe('ProviderIcon UX demo (HS2-PK8THJ)', () => {
  it('renders every provider kind at both public size variants', () => {
    const markup = String(ProviderIconDemo());
    for (const kind of ['github', 'gitlab', 'jira']) {
      expect(markup.match(new RegExp(`data-provider-icon="${kind}"`, 'g'))).toHaveLength(2);
    }
    expect(markup.match(/data-size="l"/g)).toHaveLength(3);
    expect(markup).toContain('data-size-variant="m"');
    expect(markup).toContain('data-size-variant="l"');
    expect(markup).toContain('aria-label="GitHub"');
  });

  it('is catalogued as an implemented component used by the provider surfaces', () => {
    expect(findDemo('provider-icon')).toMatchObject({ name: 'ProviderIcon', implemented: true });
    expect(demosUsing('provider-icon').map((entry) => entry.id)).toEqual([
      'ticket-source-setup-dialog',
      'provider-setup-form',
      'ticket-sources-settings',
    ]);
    const entry = componentCatalogExtension.entries.find((item) => item.id === 'provider-icon');
    expect(entry).toMatchObject({
      kind: 'component',
      publicClasses: ['provider-icon'],
      geometry: { margin: 'none', border: 'none', padding: 'none' },
    });
  });
});
