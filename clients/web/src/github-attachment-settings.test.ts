import { describe, expect, it } from 'vitest';

import { githubAttachmentSettings } from './github-attachment-settings';

describe('GitHub attachment settings (HS2-8BAHRJ, HS2-DH55NG)', () => {
  it('enables, retains, and clears attachment support from form values', () => {
    const enabled = githubAttachmentSettings(' acme/assets ', '', '');
    expect(enabled).toEqual({
      settings: {
        attachment_repo: 'acme/assets',
        attachment_folder: 'hotsheet-attachments',
        attachment_branch: 'main',
      },
    });
    expect(githubAttachmentSettings('acme/assets', ' /evidence/2026/ ', ' media ')).toEqual({
      settings: { attachment_repo: 'acme/assets', attachment_folder: 'evidence/2026', attachment_branch: 'media' },
    });
    expect(githubAttachmentSettings('', 'evidence', 'media')).toEqual({ settings: {} });
  });

  it('rejects the same unsafe repository, folder, and branch shapes as the server', () => {
    for (const repository of ['acme', 'acme/assets/extra', '../assets', 'acme/assets name'])
      expect(githubAttachmentSettings(repository, '', '')).toHaveProperty('error');
    for (const folder of ['../assets', 'foo//bar', './assets'])
      expect(githubAttachmentSettings('acme/assets', folder, '')).toHaveProperty('error');
    for (const branch of ['feature..new', 'feature new'])
      expect(githubAttachmentSettings('acme/assets', '', branch)).toHaveProperty('error');
  });
});
