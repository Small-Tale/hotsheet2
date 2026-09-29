import { expect, test } from '@playwright/test';

test('serves installable PWA identity and decodable branding assets', async ({ page, request }) => {
  await page.goto('/', { waitUntil: 'domcontentloaded' });
  const lightTheme = page.locator('meta[name="theme-color"][media="(prefers-color-scheme: light)"]');
  const darkTheme = page.locator('meta[name="theme-color"][media="(prefers-color-scheme: dark)"]');
  await expect(lightTheme).toHaveAttribute('content', '#ffffff');
  await expect(darkTheme).toHaveAttribute('content', '#1c1c1e');
  await page.emulateMedia({ colorScheme: 'light' });
  expect(await lightTheme.evaluate((meta: HTMLMetaElement) => matchMedia(meta.media).matches)).toBe(true);
  expect(await darkTheme.evaluate((meta: HTMLMetaElement) => matchMedia(meta.media).matches)).toBe(false);
  const surfaceColor = () =>
    page.evaluate(() => {
      const sample = document.createElement('div');
      sample.style.background = 'var(--wa-color-surface-default)';
      document.body.append(sample);
      const color = getComputedStyle(sample).backgroundColor;
      sample.remove();
      return color;
    });
  expect(await surfaceColor()).toBe('rgb(255, 255, 255)');
  await page.screenshot({ path: '/private/tmp/hs2-h229wa-light-wide.png', fullPage: true });
  await page.emulateMedia({ colorScheme: 'dark' });
  expect(await lightTheme.evaluate((meta: HTMLMetaElement) => matchMedia(meta.media).matches)).toBe(false);
  expect(await darkTheme.evaluate((meta: HTMLMetaElement) => matchMedia(meta.media).matches)).toBe(true);
  expect(await surfaceColor()).toBe('rgb(28, 28, 30)');
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({ path: '/private/tmp/hs2-h229wa-dark-narrow.png', fullPage: true });
  await expect(page.locator('link[rel="manifest"]')).toHaveAttribute('href', '/manifest.webmanifest');
  await expect(page.locator('link[rel="apple-touch-icon"]')).toHaveAttribute('href', '/apple-touch-icon.png');

  const manifestResponse = await request.get('/manifest.webmanifest');
  expect(manifestResponse.ok()).toBe(true);
  expect(manifestResponse.headers()['content-type']).toContain('application/manifest+json');
  const manifest = (await manifestResponse.json()) as { display: string; icons: Array<{ src: string }> };
  expect(manifest.display).toBe('standalone');
  expect(manifest.icons.map((icon) => icon.src)).toEqual([
    '/app-icon-192.png',
    '/app-icon-512.png',
    '/app-icon-maskable-512.png',
  ]);

  for (const src of [
    '/favicon.svg',
    '/favicon-32.png',
    '/favicon-256.png',
    '/apple-touch-icon.png',
    ...manifest.icons.map((icon) => icon.src),
  ]) {
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
