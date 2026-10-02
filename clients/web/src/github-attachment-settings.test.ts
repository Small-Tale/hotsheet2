import { describe, expect, it } from 'vitest';

import { carryGithubAttachmentSettings } from './github-attachment-settings';

describe('GitHub attachment settings carry-over (HS2-HSA64D)', () => {
  it('keeps the assets repository through a rebuilt connection edit', () => {
    const settings = { credential: { secret: 'github-app-1' } };
    expect(
      carryGithubAttachmentSettings(
        {
          credential: { secret: 'old' },
          attachment_repo: 'acme/assets',
          attachment_folder: 'evidence',
          attachment_branch: 'media',
          api_base: 'https://ghe.example/api/v3',
        },
        settings,
      ),
    ).toEqual({
      credential: { secret: 'github-app-1' },
      attachment_repo: 'acme/assets',
      attachment_folder: 'evidence',
      attachment_branch: 'media',
    });
  });

  it('adds nothing for a new connection or non-string values', () => {
    expect(carryGithubAttachmentSettings(undefined, { a: 1 })).toEqual({ a: 1 });
    expect(carryGithubAttachmentSettings({ attachment_repo: null, attachment_branch: 3 }, {})).toEqual({});
  });
});
