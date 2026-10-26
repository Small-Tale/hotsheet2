import { expect, it } from 'vitest';

import { assertStandaloneNode } from './desktop-node-runtime.mjs';

it('HS2-T7HVHA accepts a Node runtime linked only to macOS system libraries', () => {
  expect(() =>
    assertStandaloneNode(
      '/fixture/node',
      '/fixture/node:\n\t/System/Library/Frameworks/Security.framework/Versions/A/Security (compatibility version 1.0.0)\n\t/usr/lib/libc++.1.dylib (compatibility version 1.0.0)\n',
    ),
  ).not.toThrow();
});

it('HS2-T7HVHA rejects a Node stub that needs missing runtime libraries', () => {
  expect(() =>
    assertStandaloneNode(
      '/fixture/node',
      '/fixture/node:\n\t@rpath/libnode.147.dylib (compatibility version 0.0.0)\n\t/third-party/lib/libuv.dylib (compatibility version 1.0.0)\n',
    ),
  ).toThrow(/standalone Node executable/);
});
