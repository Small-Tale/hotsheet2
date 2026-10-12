import { describe, expect, it } from 'vitest';

import { ProjectSetupWarningBanner } from './project-setup-warning-banner';

describe('ProjectSetupWarningBanner (HS2-0TXM8S)', () => {
  it('explains the skipped setup with a dismiss action in the shared warning shell strip', () => {
    const markup = String(ProjectSetupWarningBanner({ detail: 'Run cargo build -p hotsheet-cli.' }));
    expect(markup).toContain('data-component="project-setup-warning-banner"');
    expect(markup).toContain('class="shell-banner"');
    expect(markup).toContain('data-tone="warning"');
    expect(markup).toContain('Project setup needs attention');
    expect(markup).toContain('Run cargo build -p hotsheet-cli.');
    expect(markup).toContain('data-lucide="triangle-alert"');
    expect(markup).toContain('<button type="button" data-action="dismiss-project-setup-warning">Dismiss</button>');
  });
});
