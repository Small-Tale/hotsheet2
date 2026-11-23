import { describe, expect, it } from 'vitest';

import { RemoteProjectPlaceholder } from './remote-project-placeholder';
import { RemoteReconnectingBanner } from './remote-reconnecting-banner';

describe('HS2-EJ5K09 HS2-WNK73S remote project placeholder', () => {
  it('shows a busy opening panel while the first open runs', () => {
    const markup = String(
      RemoteProjectPlaceholder({ projectId: 'remote:a', name: 'Studio · alpha', phase: 'loading' }),
    );
    expect(markup).toContain('data-remote-project-placeholder="loading"');
    expect(markup).toContain('Opening Studio · alpha…');
    expect(markup).toContain('aria-busy="true"');
    expect(markup).not.toContain('retry-remote-project');
  });

  it('says the server is not answering and offers an immediate retry', () => {
    const markup = String(
      RemoteProjectPlaceholder({ projectId: 'remote:a', name: 'Studio · alpha', phase: 'reconnecting' }),
    );
    expect(markup).toContain('data-remote-project-placeholder="reconnecting"');
    expect(markup).toContain('Reconnecting to Studio · alpha');
    expect(markup).toContain('data-action="retry-remote-project"');
    expect(markup).toContain('data-project-id="remote:a"');
    expect(markup).toContain('Retry now');
    expect(markup).not.toContain('aria-busy="true"');
  });

  it('disables the retry while one is in flight', () => {
    const markup = String(
      RemoteProjectPlaceholder({
        projectId: 'remote:a',
        name: 'Studio · alpha',
        phase: 'reconnecting',
        connecting: true,
      }),
    );
    expect(markup).toContain('Connecting…');
    expect(markup).toContain('aria-busy="true"');
    expect(markup).toMatch(/disabled/);
  });

  it('names the host in the mid-session reconnecting banner', () => {
    const markup = String(RemoteReconnectingBanner({ host: 'studio' }));
    expect(markup).toContain('data-component="remote-reconnecting-banner"');
    expect(markup).toContain('Reconnecting to studio');
  });
});

describe('HS2-4B0KSD expired, revoked and failing remote tabs', () => {
  const render = (props: Partial<Parameters<typeof RemoteProjectPlaceholder>[0]>) =>
    String(
      RemoteProjectPlaceholder({
        projectId: 'remote:a',
        name: 'Studio · alpha',
        phase: 'expired',
        bindingId: 'binding-a',
        ...props,
      }),
    );

  it('offers Renew for an expired approval, not reconnecting', () => {
    const markup = render({});
    expect(markup).toContain('data-remote-project-placeholder="expired"');
    expect(markup).toContain('Renew Access to Studio · alpha');
    expect(markup).toContain('data-action="renew-remote-approval"');
    expect(markup).toContain('data-remote-scope="binding-a"');
    expect(markup).toContain('Check Again');
    expect(markup).not.toContain('Reconnecting');
    const requested = render({ renewalRequested: true });
    expect(requested).toContain('Renewal requested.');
    expect(requested).not.toContain('data-action="renew-remote-approval"');
  });

  it('says a revoked approval was revoked and offers pairing again', () => {
    const markup = render({ phase: 'revoked' });
    expect(markup).toContain('Access Revoked by Studio · alpha');
    expect(markup).toContain('data-action="review-remote-server"');
    expect(markup).not.toContain('data-action="renew-remote-approval"');
  });

  it('shows another failure’s text while it keeps reconnecting', () => {
    const markup = render({ phase: 'reconnecting', error: 'The remote server’s certificate is not trusted.' });
    expect(markup).toContain('certificate is not trusted');
    expect(markup).toContain('Reconnecting to Studio · alpha');
    expect(render({ phase: 'reconnecting', error: 'The server did not answer.' })).toContain(
      'The remote server is not answering.',
    );
  });
});
