import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { RepositorySetup } from './repository-setup';

describe('RepositorySetup', () => {
  it('offers a bounded initialization action without promising to commit files', () => {
    const markup = String(RepositorySetup({}));
    expect(markup).toContain('data-step="initialize"');
    expect(markup).not.toContain('This folder is not a Git repository');
    expect(markup).toContain('data-action="initialize-repository"');
    expect(markup).toContain('Initialize Git repository');
    expect(markup).toContain('will not stage or commit');
  });

  it('offers provider-neutral origin setup without publishing project contents', () => {
    const markup = String(RepositorySetup({ step: 'remote', error: 'origin could not be added' }));
    expect(markup).toContain('data-action="connect-repository-remote"');
    expect(markup).toContain('name="repository-remote"');
    expect(markup).toContain('data-action="skip-repository-remote"');
    expect(markup).toContain('Add origin');
    expect(markup).toContain('will not stage, commit, or push');
    expect(markup).toContain('role="alert"');
    expect(markup).toContain('origin could not be added');
  });

  it('HS2-GAJSX9 submits the origin through a Kerf Form, not a raw form (KUI-L303)', () => {
    const markup = String(RepositorySetup({ step: 'remote' }));
    expect(markup).toMatch(/<form class="kui-form" data-component="form" data-action="connect-repository-remote"/u);
    expect(markup.match(/<form\b/gu)).toHaveLength(1);
    expect(markup).toMatch(/^<div class="repository-setup" data-component="repository-setup" data-step="remote">/u);
  });

  it('exposes deterministic busy copy and a narrow responsive layout', () => {
    expect(String(RepositorySetup({ busy: true }))).toContain('Initializing…');
    expect(String(RepositorySetup({ step: 'remote', busy: true }))).toContain('Adding origin…');
    const css = readFileSync(resolve(import.meta.dirname, 'repository-setup.css'), 'utf8');
    expect(css).not.toContain('.repository-setup {');
    expect(css).not.toContain('--wa-space-');
    expect(css).toContain('.repository-setup .repository-setup__icon {');
    for (const step of ['initialize', 'remote'] as const) {
      const markup = String(RepositorySetup({ step }));
      expect(markup).toContain('data-content-width="compact"');
      expect(markup).toContain('data-content-align="center"');
      expect(markup).toContain('data-inset="document"');
      expect(markup).toContain('data-gap="true"');
    }
    // Kerf StateBanner, Row, List, and Text own the error, footers, and copy (HS2-AP13JM).
    expect(css).not.toMatch(/__error|footer|__message|__lead|@media/);
    // The initialize lead is centered quiet Kerf Text (HS2-J7BTAG).
    expect(String(RepositorySetup({}))).toMatch(
      /<p class="kui-text" data-component="text" data-tone="quiet"[^>]*data-align="center"/u,
    );
  });
});
