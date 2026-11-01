import { describe, expect, it } from 'vitest';

import { githubAccountEndpoint } from './github-account-endpoint';

describe('HS2-WV6QCR saved GitHub endpoint transport', () => {
  it.each([
    'https://github.com:443/api/v3',
    'http://github.com:80/api/v3',
    'https://ghe.test:8443/custom/API/v3',
    'HTTPS://GHE.TEST:443/Gateway/API/v3',
    'https://api.github.com',
    'https://ghe.test/api/v%33/',
    'http://127.0.0.1:9012/bridge',
    'http://[::1]:9012/bridge',
    'https://internal/bridge',
  ])('preserves the complete explicit endpoint %s', (base_url) => {
    const selected = githubAccountEndpoint({ base_url, host: 'ignored.test' });
    expect(selected).not.toHaveProperty('error');
    expect(selected).toHaveProperty('apiBase', base_url);
  });

  it('retains default ports in the separate presentation origin', () => {
    expect(githubAccountEndpoint({ base_url: ' HTTPS://GHE.TEST:443/Path/Case ', host: 'ghe.test' })).toEqual({
      apiBase: 'HTTPS://GHE.TEST:443/Path/Case',
      webOrigin: 'https://ghe.test:443',
    });
  });

  it.each(['github.com', ' GITHUB.COM '])('omits the endpoint only for proven public default %s', (host) => {
    expect(githubAccountEndpoint({ host })).toEqual({});
  });

  it.each(['https://api.github.com', 'HTTPS://GITHUB.COM/api/v3'])(
    'keeps ordinary GitHub presentation while transporting explicit public endpoint %s',
    (base_url) => {
      expect(githubAccountEndpoint({ base_url, host: 'github.com' })).toEqual({ apiBase: base_url });
    },
  );

  it.each(['', 'ghe.test', 'github.com:443', 'https://github.com', 'github.com/unknown', 'gitlab.com'])(
    'refuses to guess an absent endpoint from %j',
    (host) => {
      expect(githubAccountEndpoint({ host })).toHaveProperty('error');
    },
  );

  it.each([undefined, null])('refuses missing/null host metadata accepted by the existing account parser', (host) => {
    expect(githubAccountEndpoint({ host })).toHaveProperty('error');
  });

  it.each([
    null,
    '',
    ' ',
    'ghe.test/api/v3',
    'ftp://ghe.test/api/v3',
    'https://',
    'https://user:synthetic-secret@ghe.test/api/v3',
    'https://ghe.test/api/v3?query',
    'https://ghe.test/api/v3#fragment',
    'https://ghe.test\\api/v3',
    'https://ghe.test:invalid/api/v3',
    'https://ghe.test:65536/api/v3',
    'https://git hub.test/api/v3',
    'https://ghe.test/api v3',
    '\thttps://ghe.test/api/v3',
    'https://ghe.test/api/v3\n',
    'https://ghe.test/\u0000api/v3',
    'https://ghe.test/\u007fapi/v3',
    'https://ghe.test/\u0085api/v3',
    '\u00a0https://ghe.test/api/v3',
  ])('rejects malformed present metadata without public fallback %j', (base_url) => {
    const selected = githubAccountEndpoint({ base_url, host: 'github.com' });
    expect(selected).toHaveProperty('error');
    expect(selected).not.toHaveProperty('apiBase');
    expect(JSON.stringify(selected)).not.toContain('synthetic-secret');
  });

  it('does not reconcile a syntactically valid legacy endpoint without provenance (HS2-6H8ZY6)', () => {
    expect(githubAccountEndpoint({ base_url: 'HTTPS://GITHUB.COM/api/v3', host: 'github.com' })).toHaveProperty(
      'apiBase',
      'HTTPS://GITHUB.COM/api/v3',
    );
  });
});
