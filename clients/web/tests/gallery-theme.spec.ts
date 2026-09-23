import { expect, test } from '@playwright/test';

for (const width of [1280, 390]) {
  test(`keeps inverse gallery chrome stable through device and app themes at ${width}px (HS2-1CACB4)`, async ({
    page,
  }, testInfo) => {
    await page.setViewportSize({ width, height: 844 });
    await page.emulateMedia({ colorScheme: 'light' });
    await page.goto('/ux-demo?component=attachment-gallery&dev-review=false');
    const gallery = page.locator('[data-component="attachment-gallery"]');
    await expect(gallery).toBeVisible();
    await gallery.getByRole('button', { name: 'Annotate media, 1 annotation' }).click();
    const palette = () =>
      gallery.evaluate((node) =>
        [
          node,
          ...node.querySelectorAll(
            '.attachment-gallery__toolbar, .attachment-gallery__filename, .attachment-gallery__footer, .attachment-gallery__annotation, .attachment-gallery__annotation-label, .attachment-gallery__volume-popup, .attachment-gallery__timeline input, [data-component="toolbar-control-group"]',
          ),
        ].map((element) => {
          const style = getComputedStyle(element);
          return {
            color: style.color,
            background: style.backgroundColor,
            border: style.borderColor,
            shadow: style.boxShadow,
            accent: style.accentColor,
          };
        }),
      );
    const imagePalette = await palette();
    await expect(gallery.locator('.attachment-gallery__filename')).toHaveCSS('color', 'rgb(255, 255, 255)');
    await expect(gallery.locator('img')).toHaveCSS('filter', 'none');
    await gallery.screenshot({ path: testInfo.outputPath(`gallery-light-${width}.png`), animations: 'disabled' });
    // Exercise real system preference changes while the modal is open. Removing
    // the catalog override lets the same shared foundation follow the device.
    await page.evaluate(() => {
      delete document.documentElement.dataset.theme;
    });
    const surrounding = () => page.locator('html').evaluate((node) => getComputedStyle(node).backgroundColor);
    const lightSurface = await surrounding();
    for (const colorScheme of ['dark', 'light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme });
      if (colorScheme === 'dark') await expect.poll(surrounding).not.toBe(lightSurface);
      else await expect.poll(surrounding).toBe(lightSurface);
      await expect.poll(palette).toEqual(imagePalette);
    }
    await gallery.screenshot({ path: testInfo.outputPath(`gallery-dark-${width}.png`), animations: 'disabled' });
    await gallery.getByRole('button', { name: 'Close image gallery' }).click();
    await expect(gallery).toHaveCount(0);
    // Explicit app theme toggles must keep working outside the gallery.
    await page.getByRole('button', { name: 'Use dark theme', exact: true }).click();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    await page.getByRole('button', { name: 'Open gallery', exact: true }).click();
    await gallery.getByRole('button', { name: 'Annotate media, 1 annotation' }).click();
    await expect.poll(palette).toEqual(imagePalette);
    await gallery.getByRole('button', { name: 'Next image' }).click();
    await gallery.getByRole('button', { name: 'Next image' }).click();
    await expect(gallery.locator('video')).toBeVisible();
    // The catalog's missing standalone video asset is tracked by HS2-DRDH36.
    // Use a real decoded local video for theme/legibility evidence, as the
    // existing gallery playback test does, instead of accepting an empty box.
    const videoBytes = await page.evaluate(async () => {
      const canvas = document.createElement('canvas');
      canvas.width = 640;
      canvas.height = 360;
      const context = canvas.getContext('2d')!;
      context.fillStyle = '#7857a4';
      context.fillRect(0, 0, 640, 360);
      context.fillStyle = 'white';
      context.font = '32px sans-serif';
      context.fillText('Original video colors', 160, 190);
      const stream = canvas.captureStream(5);
      const recorder = new MediaRecorder(stream, { mimeType: 'video/webm' });
      const chunks: Blob[] = [];
      recorder.ondataavailable = (event) => chunks.push(event.data);
      recorder.start();
      await new Promise((resolve) => setTimeout(resolve, 250));
      const stopped = new Promise<void>((resolve) => {
        recorder.onstop = () => {
          resolve();
        };
      });
      recorder.stop();
      await stopped;
      stream.getTracks().forEach((track) => {
        track.stop();
      });
      return Array.from(new Uint8Array(await new Blob(chunks, { type: 'video/webm' }).arrayBuffer()));
    });
    await page.route('**/ux-gallery-video.mp4', (route) =>
      route.fulfill({ contentType: 'video/webm', body: Buffer.from(videoBytes) }),
    );
    await gallery.locator('video').evaluate((video: HTMLVideoElement) => {
      video.load();
    });
    await expect(gallery.locator('video')).toHaveJSProperty('videoWidth', 640);
    await expect(gallery.locator('video')).toHaveCSS('filter', 'none');
    await gallery.getByRole('button', { name: 'Volume controls' }).click();
    await expect
      .poll(() => gallery.locator('video').evaluate((video: HTMLVideoElement) => video.readyState))
      .toBeGreaterThanOrEqual(2);
    const videoPalette = await palette();
    await gallery.screenshot({ path: testInfo.outputPath(`gallery-video-dark-${width}.png`), animations: 'disabled' });
    // A live explicit light override, with the device still dark, also leaves
    // playback, range, popup and annotation colors intact.
    await page.evaluate(() => {
      document.documentElement.dataset.theme = 'light';
    });
    await expect.poll(palette).toEqual(videoPalette);
    await gallery.getByRole('button', { name: 'Mute video' }).click();
    await expect(gallery.getByRole('button', { name: 'Unmute video' })).toBeVisible();
    await gallery.getByRole('button', { name: 'Close video gallery' }).focus();
    await page.keyboard.press('Enter');
    await expect(gallery).toHaveCount(0);
    await page.getByRole('button', { name: 'Use light theme', exact: true }).click();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
    await expect.poll(surrounding).toBe(lightSurface);
  });
}
