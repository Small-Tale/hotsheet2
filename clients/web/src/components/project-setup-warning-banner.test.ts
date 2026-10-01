import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { ProjectSetupWarningBanner } from './project-setup-warning-banner';

describe('ProjectSetupWarningBanner (HS2-0TXM8S)', () => {
  it('explains the skipped setup with a dismiss action and owns its styles through Kerf tokens', () => {
    const markup = String(ProjectSetupWarningBanner({ detail: 'Run cargo build -p hotsheet-cli.' }));
    expect(markup).toContain('data-component="project-setup-warning-banner"');
    expect(markup).toContain('Project setup was skipped');
    expect(markup).toContain('Run cargo build -p hotsheet-cli.');
    expect(markup).toContain('data-lucide="triangle-alert"');
    expect(markup).toContain('<button type="button" data-action="dismiss-project-setup-warning">Dismiss</button>');
    const css = readFileSync(new URL('./project-setup-warning-banner.css', import.meta.url), 'utf8');
    expect(css).not.toContain('.kui-');
    expect(css).toContain('--kui-state-banner-foreground: var(--wa-color-warning-on-quiet)');
  });
});
