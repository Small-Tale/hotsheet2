import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

const webRoot = resolve(import.meta.dirname, '..');
const publicRoot = resolve(webRoot, 'public');

function pngSize(filename: string): { width: number; height: number } {
  const png = readFileSync(resolve(publicRoot, filename));
  expect(png.subarray(1, 4).toString()).toBe('PNG');
  return { width: png.readUInt32BE(16), height: png.readUInt32BE(20) };
}

describe('PWA identity', () => {
  it('publishes matching browser, installed-app, and Apple metadata', () => {
    const html = readFileSync(resolve(webRoot, 'index.html'), 'utf8');
    expect(html).toContain('<meta name="theme-color" content="#f2f2f7">');
    expect(html).toContain('<link rel="manifest" href="/manifest.webmanifest">');
    expect(html).toContain('<link rel="icon" href="/favicon.svg" type="image/svg+xml">');
    expect(html).toContain('<link rel="icon" href="/favicon-256.png" type="image/png" sizes="256x256">');
    expect(html).toContain('<link rel="icon" href="/favicon-32.png" type="image/png" sizes="32x32">');
    expect(html).toContain('<link rel="apple-touch-icon" href="/apple-touch-icon.png" sizes="180x180">');
    expect(html).toContain('<meta name="apple-mobile-web-app-capable" content="yes">');
    expect(readFileSync(resolve(publicRoot, 'favicon.svg'), 'utf8')).toContain('<title>favicon</title>');
  });

  it('declares installable app identity and correctly sized raster assets', () => {
    const manifest = JSON.parse(readFileSync(resolve(publicRoot, 'manifest.webmanifest'), 'utf8')) as {
      name: string;
      short_name: string;
      display: string;
      theme_color: string;
      background_color: string;
      icons: Array<{ src: string; sizes: string; purpose: string }>;
    };
    expect(manifest).toMatchObject({
      name: 'Hot Sheet 2',
      short_name: 'Hot Sheet',
      display: 'standalone',
      theme_color: '#f2f2f7',
      background_color: '#f2f2f7',
    });
    expect(manifest.icons).toEqual([
      { src: '/app-icon-192.png', sizes: '192x192', type: 'image/png', purpose: 'any' },
      { src: '/app-icon-512.png', sizes: '512x512', type: 'image/png', purpose: 'any' },
      { src: '/app-icon-maskable-512.png', sizes: '512x512', type: 'image/png', purpose: 'maskable' },
    ]);
    expect(pngSize('favicon-32.png')).toEqual({ width: 32, height: 32 });
    expect(pngSize('favicon-256.png')).toEqual({ width: 256, height: 256 });
    expect(pngSize('apple-touch-icon.png')).toEqual({ width: 180, height: 180 });
    expect(pngSize('app-icon-192.png')).toEqual({ width: 192, height: 192 });
    expect(pngSize('app-icon-512.png')).toEqual({ width: 512, height: 512 });
    expect(pngSize('app-icon-maskable-512.png')).toEqual({ width: 512, height: 512 });
  });
});
