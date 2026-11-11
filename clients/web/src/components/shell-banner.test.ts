import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { ShellBanner } from './shell-banner';

describe('ShellBanner (HS2-BF8C76)', () => {
  it('composes a stacked status StateBanner inset by a Kerf List (HS2-YCB15Z)', () => {
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
    expect(markup).toMatch(
      /data-component="list"[^>]*data-control-insets="trl"|data-control-insets="trl"[^>]*data-component="list"/,
    );
  });

  it('keeps the inset rounded StateBanner and only paints the header surface behind it (HS2-YCB15Z)', () => {
    const css = readFileSync(new URL('./shell-banner.css', import.meta.url), 'utf8');
    expect(css).not.toContain('.kui-');
    expect(css).not.toMatch(/--kui-(?:state-banner|layout)-/);
    expect(css).toContain('background: var(--kui-color-surface)');
  });
});
