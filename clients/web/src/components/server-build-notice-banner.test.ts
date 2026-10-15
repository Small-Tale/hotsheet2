import { describe, expect, it } from 'vitest';

import { ServerBuildNoticeBanner } from './server-build-notice-banner';

describe('ServerBuildNoticeBanner (HS2-46BKH7)', () => {
  it('explains the mismatched running server as an info strip with a dismiss action', () => {
    const markup = String(
      ServerBuildNoticeBanner({ detail: 'It is busy with active work; this host will restart it when idle.' }),
    );
    expect(markup).toContain('data-component="server-build-notice-banner"');
    expect(markup).toContain('class="shell-banner"');
    expect(markup).toContain('data-tone="info"');
    expect(markup).toContain('Server build differs from this host');
    expect(markup).toContain('this host will restart it when idle');
    expect(markup).toContain('data-lucide="server-cog"');
    expect(markup).toContain('<button type="button" data-action="dismiss-server-build-notice">Dismiss</button>');
    expect(markup).not.toContain('server:rebuild');
  });
});
