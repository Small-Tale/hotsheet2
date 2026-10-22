import { describe, expect, it } from 'vitest';

import { GuidanceSettings } from './guidance-settings';

describe('GuidanceSettings rule summaries', () => {
  it('leads with the human description and keeps the slug with the available version', () => {
    const markup = String(
      GuidanceSettings({
        loading: false,
        message: '',
        query: '',
        repository: '',
        sourceRef: '',
        sourceId: '',
        ownerFilter: '',
        tagFilter: '',
        overview: {
          owner: 'builtin',
          explicit_selection: true,
          default_repository: '',
          sources: [
            {
              id: 'rules',
              repository: 'https://example.test/rules',
              requested_ref: 'v1',
              commit: '1234567890abcdef',
              rules: [
                {
                  name: 'rust-lint-and-format',
                  description: 'Rust lint and formatting defaults',
                  version: 4,
                  installed_version: 3,
                  owner: 'rust',
                  tags: ['rust'],
                  selected: true,
                },
              ],
              skills: [],
            },
          ],
        },
      }),
    );
    expect(markup).toMatch(/class="kui-checkbox__label"[^>]*>Rust lint and formatting defaults/);
    expect(markup).toContain('rust-lint-and-format · available v4 · installed v3');
  });
});
