import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { ShellBanner } from './shell-banner';

describe('ShellBanner (HS2-BF8C76)', () => {
  it('composes a stacked status StateBanner inside a toned full-width strip', () => {
    const markup = String(
      ShellBanner({
        component: 'example-banner',
        tone: 'success',
        title: 'Done',
        detail: 'All set.',
        actionPlacement: 'below',
      }),
    );
    expect(markup).toContain('class="shell-banner" data-component="example-banner" data-tone="success"');
    expect(markup).toContain('data-component="state-banner"');
    expect(markup).toContain('data-tone="success"');
    expect(markup).toContain('Done');
    expect(markup).toContain('All set.');
    expect(markup).not.toContain('role="alert"');
  });

  it('configures StateBanner only through public tokens', () => {
    const css = readFileSync(new URL('./shell-banner.css', import.meta.url), 'utf8');
    expect(css).not.toContain('.kui-');
    expect(css).toContain('--kui-state-banner-border: transparent');
    expect(css).toContain('--kui-layout-rounded-radius: 0');
  });
});
