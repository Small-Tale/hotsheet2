import { describe, expect, it } from 'vitest';

import { githubSignInOrigin } from './github-sign-in-origin';

describe('HS2-VKNA12 GitHub sign-in origin identity', () => {
  it.each([
    ['github.com', 'https://github.com'],
    [' HTTPS://GITHUB.COM/// ', 'https://github.com'],
    ['https://github.com:443', 'https://github.com:443'],
    ['HTTPS://GITHUB.COM:8443/', 'https://github.com:8443'],
    ['http://github.com:80', 'http://github.com:80'],
    ['http://github.com', 'http://github.com'],
    ['https://api.github.com', 'https://api.github.com'],
    ['GHE.EXAMPLE.TEST:443/', 'https://ghe.example.test:443'],
    ['http://127.0.0.1:8080', 'http://127.0.0.1:8080'],
    ['https://github.com.example.test', 'https://github.com.example.test'],
    ['HTTPS://\u00C4.EXAMPLE.TEST', 'https://\u00C4.example.test'],
  ])('preserves configured origin %s', (typed, origin) => {
    expect(githubSignInOrigin(typed)).toBe(origin);
  });

  it.each([
    '',
    'github',
    'https://',
    'ftp://github.example.test',
    'https://user:password@github.example.test',
    'https://github.example.test/path',
    'https://github.example.test/..',
    'https://github.example.test?query',
    'https://github.example.test#fragment',
    'https://github.example.test\\path',
    'https://github.example.test:65536',
    'https://github.example.test:invalid',
    'https://git hub.example.test',
    'https://github.example.test\u0000',
    'https://github.example.test\u007f',
  ])('rejects invalid server address %j', (typed) => {
    expect(githubSignInOrigin(typed)).toBeUndefined();
  });
});
