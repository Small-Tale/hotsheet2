import { expect, it } from 'vitest';

import { assertStandaloneNode } from './desktop-node-runtime.mjs';

it('HS2-T7HVHA accepts a Node runtime linked only to macOS system libraries', () => {
  expect(() =>
    assertStandaloneNode(
      '/fixture/node',
      '/fixture/node:\n\t/System/Library/Frameworks/Security.framework/Versions/A/Security (compatibility version 1.0.0)\n\t/usr/lib/libc++.1.dylib (compatibility version 1.0.0)\n',
      'darwin',
    ),
  ).not.toThrow();
});

it('HS2-T7HVHA rejects a Node stub that needs missing runtime libraries', () => {
  expect(() =>
    assertStandaloneNode(
      '/fixture/node',
      '/fixture/node:\n\t@rpath/libnode.147.dylib (compatibility version 0.0.0)\n\t/third-party/lib/libuv.dylib (compatibility version 1.0.0)\n',
      'darwin',
    ),
  ).toThrow(/standalone Node executable/);
});

it('HS2-KY5X6J executes a Windows Node .exe without invoking macOS otool', () => {
  const calls = [];
  assertStandaloneNode('C:/synthetic/node.exe', undefined, 'win32', (...args) => {
    calls.push(args);
    return 'v22.0.0';
  });
  expect(calls).toEqual([['C:/synthetic/node.exe', ['--version'], { encoding: 'utf8' }]]);
  expect(() => assertStandaloneNode('C:/synthetic/node.cmd', undefined, 'win32', () => '')).toThrow(/\.exe/);
  expect(() =>
    assertStandaloneNode('C:/synthetic/node.exe', undefined, 'win32', () => {
      throw new Error('missing runtime');
    }),
  ).toThrow(/standalone Windows executable/);
});

it('HS2-D80Q2V executes an extensionless Linux Node without invoking macOS otool', () => {
  const calls = [];
  assertStandaloneNode('/synthetic/node', undefined, 'linux', (...args) => {
    calls.push(args);
    return 'v22.0.0';
  });
  expect(calls).toEqual([['/synthetic/node', ['--version'], { encoding: 'utf8' }]]);
  expect(() =>
    assertStandaloneNode('/synthetic/node', undefined, 'linux', () => {
      throw new Error('missing runtime');
    }),
  ).toThrow(/standalone Linux executable/);
});
