import { describe, expect, it } from 'vitest';

import { GuidanceSettings } from './guidance-settings';

describe('GuidanceSettings rule summaries', () => {
  it('shows only blocked newer bundled sections as a review notice', () => {
    const props = {
      loading: false,
      message: '',
      query: '',
      repository: '',
      sourceRef: '',
      sourceId: '',
      ownerFilter: '',
      tagFilter: '',
    };
    expect(String(GuidanceSettings(props))).not.toContain('Bundled Guidance Needs Review');
    const markup = String(
      GuidanceSettings({
        ...props,
        bundledGuidanceBlocked: [
          { path: 'AGENTS.md', section: 'ticket-driven-work', installedVersion: 1, bundledVersion: 6 },
        ],
      }),
    );
    expect(markup).toContain('Bundled Guidance Needs Review');
    expect(markup).toContain('AGENTS.md: ticket-driven-work (local v1, bundled v6)');
    expect(markup).toContain('Hot Sheet preserved the local text');
  });
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
                {
                  name: 'hotsheet-visual-qa',
                  description: 'Visual review of rendered changes',
                  version: 2,
                  installed_version: null,
                  owner: 'hotsheet',
                  tags: ['testing'],
                  selected: false,
                },
              ],
              skills: [],
            },
          ],
        },
      }),
    );
    expect(markup).toMatch(/class="kui-checkbox__label"[^>]*>Rust lint and formatting defaults/);
    expect(markup).toContain('rust-lint-and-format · available v4 · v3 installed');
    expect(markup).toContain('hotsheet-visual-qa · available v2 · not installed');
  });
});
