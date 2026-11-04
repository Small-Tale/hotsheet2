import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { describe, expect, it } from 'vitest';

import { CLIENT_BUILD_META_NAME, clientBuildIdFromHtml, loadedClientBuildId, validClientBuildId } from './client-build';
import { servedClientBuildId } from './client-build-host';

describe('client build id (HS2-KPJWBK)', () => {
  it('accepts short printable ids only', () => {
    expect(validClientBuildId('bundle-0123abcd')).toBe('bundle-0123abcd');
    expect(validClientBuildId('dev-lx2k3-ab12cd34')).toBe('dev-lx2k3-ab12cd34');
    expect(validClientBuildId('')).toBeUndefined();
    expect(validClientBuildId('has space')).toBeUndefined();
    expect(validClientBuildId('<script>')).toBeUndefined();
    expect(validClientBuildId('x'.repeat(129))).toBeUndefined();
    expect(validClientBuildId(7)).toBeUndefined();
    expect(validClientBuildId(undefined)).toBeUndefined();
  });

  it('reads the stamped meta tag from built markup in either attribute order', () => {
    expect(clientBuildIdFromHtml(`<head><meta name="${CLIENT_BUILD_META_NAME}" content="bundle-a"></head>`)).toBe(
      'bundle-a',
    );
    expect(clientBuildIdFromHtml(`<meta content='bundle-b' name='${CLIENT_BUILD_META_NAME}' />`)).toBe('bundle-b');
    expect(
      clientBuildIdFromHtml(
        `<meta name="application-name" content="Hot Sheet"><meta name="${CLIENT_BUILD_META_NAME}" content="bundle-c">`,
      ),
    ).toBe('bundle-c');
    expect(clientBuildIdFromHtml('<meta name="application-name" content="Hot Sheet">')).toBeUndefined();
    expect(clientBuildIdFromHtml(`<meta name="${CLIENT_BUILD_META_NAME}" content="bad id">`)).toBeUndefined();
  });

  it('reads the loaded bundle id from the page document', () => {
    const root = {
      querySelector: (selector: string) =>
        selector === `meta[name="${CLIENT_BUILD_META_NAME}"]`
          ? ({ getAttribute: () => 'bundle-loaded' } as unknown as HTMLMetaElement)
          : null,
    } as Pick<Document, 'querySelector'>;
    expect(loadedClientBuildId(root)).toBe('bundle-loaded');
    expect(loadedClientBuildId({ querySelector: () => null })).toBeUndefined();
    expect(loadedClientBuildId(undefined)).toBeUndefined();
  });

  it('reads the served id from a host dist root and reports nothing without one', () => {
    const dist = mkdtempSync(join(tmpdir(), 'hotsheet-client-build-'));
    try {
      expect(servedClientBuildId(dist)).toBeUndefined();
      writeFileSync(join(dist, 'index.html'), '<html><head></head></html>');
      expect(servedClientBuildId(dist)).toBeUndefined();
      writeFileSync(join(dist, 'index.html'), `<meta name="${CLIENT_BUILD_META_NAME}" content="bundle-dist">`);
      expect(servedClientBuildId(dist)).toBe('bundle-dist');
    } finally {
      rmSync(dist, { recursive: true, force: true });
    }
  });
});
