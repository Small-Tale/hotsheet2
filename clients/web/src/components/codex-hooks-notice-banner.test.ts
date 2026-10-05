import { describe, expect, it } from 'vitest';

import { CodexHooksNoticeBanner } from './codex-hooks-notice-banner';

describe('CodexHooksNoticeBanner', () => {
  it('explains successful setup, identifies the hooks path and gives a checkout-specific review action', () => {
    const markup = String(CodexHooksNoticeBanner({ path: '.codex/hooks.json' }));
    expect(markup).toContain('Review updated Codex hooks');
    expect(markup).toContain('Hot Sheet updated .codex/hooks.json');
    expect(markup).toContain('Run /hooks in Codex for this checkout');
    expect(markup).not.toContain('setup was skipped');
    expect(markup).toContain('data-action="dismiss-codex-hooks-notice"');
    expect(markup).toContain('data-component="state-banner"');
  });
});
