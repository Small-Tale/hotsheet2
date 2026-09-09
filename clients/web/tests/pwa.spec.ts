import { expect, test } from '@playwright/test';

test('serves installable PWA identity and decodable branding assets', async ({ page, request }) => {
  await page.goto('/', { waitUntil: 'domcontentloaded' });
  await expect(page.locator('meta[name="theme-color"]')).toHaveAttribute('content', '#f2f2f7');
  await expect(page.locator('link[rel="manifest"]')).toHaveAttribute('href', '/manifest.webmanifest');
  await expect(page.locator('link[rel="apple-touch-icon"]')).toHaveAttribute('href', '/apple-touch-icon.png');

  const manifestResponse = await request.get('/manifest.webmanifest');
  expect(manifestResponse.ok()).toBe(true);
  expect(manifestResponse.headers()['content-type']).toContain('application/manifest+json');
  const manifest = await manifestResponse.json() as { display: string; icons: Array<{ src: string }> };
  expect(manifest.display).toBe('standalone');
  expect(manifest.icons.map(icon => icon.src)).toEqual([
    '/app-icon-192.png',
    '/app-icon-512.png',
    '/app-icon-maskable-512.png',
  ]);

  for (const src of ['/favicon.svg', '/favicon-32.png', '/favicon-256.png', '/apple-touch-icon.png', ...manifest.icons.map(icon => icon.src)]) {
    const image = await request.get(src);
    expect(image.ok(), src).toBe(true);
    expect(image.headers()['content-type'], src).toMatch(/^image\/(?:svg\+xml|png)/);
  }

  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/app-icon-512.png');
  await expect(page.locator('img')).toHaveJSProperty('complete', true);
  await page.screenshot({ path: '/private/tmp/hs2-z8mk4g-app-icon-wide.png', fullPage: true });

  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/favicon-256.png');
  await expect(page.locator('img')).toHaveJSProperty('complete', true);
  await page.screenshot({ path: '/private/tmp/hs2-z8mk4g-favicon-narrow.png', fullPage: true });
});
