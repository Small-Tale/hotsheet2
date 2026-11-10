import { expect, test } from '@playwright/test';

import { openDemoProject } from './mobile-project-fixture';

test('HS2-6PZ35D keeps the sidebar announcer toggle clear of phone drawer restore', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openDemoProject(page);
  await expect(page.locator('[data-project-dialog]')).not.toBeVisible();
  const launch = page.getByRole('button', { name: 'Announcer', exact: true });
  const restore = page.getByRole('button', { name: 'Show Terminal Drawer' });
  await expect(launch).toBeVisible();
  await expect(launch).toHaveAttribute('aria-pressed', 'false');
  await expect(page.locator('[data-component="project-tab-bar"] [data-announcer-action="open"]')).toHaveCount(0);
  await expect(restore).toBeVisible();
  const launchBox = await launch.boundingBox();
  const restoreBox = await restore.boundingBox();
  if (!launchBox || !restoreBox) throw new Error('Announcer and drawer controls must be measurable');
  expect(
    launchBox.x < restoreBox.x + restoreBox.width &&
      launchBox.x + launchBox.width > restoreBox.x &&
      launchBox.y < restoreBox.y + restoreBox.height &&
      launchBox.y + launchBox.height > restoreBox.y,
  ).toBe(false);
  if (process.env.ANNOUNCER_VISUAL_CAPTURE)
    await page.screenshot({ path: `${process.env.ANNOUNCER_VISUAL_CAPTURE}/hs2-announcer-launch-narrow.png` });
  await restore.click();
  await launch.click();
  await expect(launch).toHaveAttribute('aria-pressed', 'true');
  await expect(page.locator('[data-component="announcer-overlay"]')).toContainText('No activity');
  await expect(page.locator('[data-component="announcer-overlay"] [aria-label="Close Announcer"]')).toHaveCount(0);
  await launch.click();
  await expect(launch).toHaveAttribute('aria-pressed', 'false');
  await expect(page.locator('[data-component="announcer-overlay"]')).toHaveCount(0);
});

test('HS2-5PSQJQ plays a retained cross-project digest in the real shell across sizes', async ({ page }) => {
  const captureDir = process.env.ANNOUNCER_VISUAL_CAPTURE;
  const settleViewport = async (mobile: boolean) => {
    await expect(page.locator('[data-component="app-shell"]')).toHaveAttribute('data-mobile', String(mobile));
    if (mobile)
      await expect
        .poll(() =>
          page
            .locator('[data-component="project-tab-bar"]')
            .evaluate((node) => Math.round(node.getBoundingClientRect().left)),
        )
        .toBe(0);
    else
      await expect
        .poll(() =>
          page
            .locator('[data-workbench-rail="left"]')
            .evaluate((node) => Math.round(node.getBoundingClientRect().left)),
        )
        .toBe(0);
    await page.evaluate(() => {
      if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
      window.scrollTo(0, 0);
    });
    await expect.poll(() => page.evaluate(() => window.scrollX)).toBe(0);
    // Workbench slides the left rail across the desktop/mobile boundary; capture after the motion ends.
    await page.waitForTimeout(500);
  };
  await page.addInitScript(() => {
    const spoken: string[] = [];
    Object.defineProperty(window, '__announcerSpoken', { value: spoken });
    Object.defineProperty(window, 'SpeechSynthesisUtterance', {
      configurable: true,
      value: class {
        onstart?: () => void;
        onend?: () => void;
        onerror?: () => void;
        constructor(readonly text: string) {}
      },
    });
    Object.defineProperty(window, 'speechSynthesis', {
      configurable: true,
      value: {
        cancel() {},
        speak(utterance: { text: string; onstart?: () => void; onend?: () => void }) {
          spoken.push(utterance.text);
          utterance.onstart?.();
          queueMicrotask(() => utterance.onend?.());
        },
      },
    });
    Object.defineProperty(URL, 'createObjectURL', { configurable: true, value: () => 'blob:synthetic-speech' });
    Object.defineProperty(URL, 'revokeObjectURL', { configurable: true, value: () => {} });
    Object.defineProperty(window, 'Audio', {
      configurable: true,
      value: class {
        onended?: () => void;
        onerror?: () => void;
        constructor(readonly src: string) {}
        play() {
          spoken.push(`server:${this.src}`);
          return Promise.resolve();
        }
        pause() {}
      },
    });
  });
  await openDemoProject(page);
  await page.route('**/tts/providers', (route) => route.fulfill({ json: ['fake'] }));
  await page.route('**/tts/synthesize', (route) =>
    route.fulfill({ status: 200, contentType: 'audio/wav', body: Buffer.from('synthetic audio') }),
  );
  await page.route('**/checkouts/demo-checkout/activity**', (route) =>
    route.fulfill({
      json: [
        {
          store_id: 'synthetic-store',
          event: {
            id: 'A',
            ts: '2026-11-01T01:00:00Z',
            tool: 'codex',
            kind: 'edit',
            summary: 'Updated the synthetic sample file',
            detail: { path: 'src/sample.ts', operation: 'fileChange', before: 'const n = 1;', after: 'const n = 2;' },
            importance: 'normal',
          },
        },
      ],
    }),
  );
  let diffReads = 0;
  await page.route('**/checkouts/demo-checkout/activity/current-diff', (route) => {
    diffReads += 1;
    return route.fulfill({
      json: { diff: 'diff --git a/src/sample.ts b/src/sample.ts\n+const n = 2;', truncated: false },
    });
  });

  const launch = page.getByRole('button', { name: 'Announcer', exact: true });
  await expect(launch).toBeVisible();
  expect(await launch.evaluate((node) => Boolean(node.closest('[data-region-id="app-left-rail"]')))).toBe(true);
  await launch.click();
  await expect(launch).toHaveAttribute('aria-pressed', 'true');
  const overlay = page.locator('[data-component="announcer-overlay"]');
  await expect(overlay).toContainText('Updated the synthetic sample file');
  await expect(overlay).toContainText('src/sample.ts');
  await expect(overlay).not.toContainText('const n = 1;');
  await expect(overlay).not.toContainText('const n = 2;');
  expect(diffReads).toBe(0);
  const currentDiffRequest = page.waitForRequest('**/checkouts/demo-checkout/activity/current-diff');
  await overlay.getByRole('button', { name: 'View current diff' }).click();
  expect((await currentDiffRequest).postDataJSON()).toEqual({ path: 'src/sample.ts' });
  expect(diffReads).toBe(1);
  await expect(overlay.getByLabel('Current working-tree diff')).toContainText('const n = 2;');
  await expect(overlay).toContainText('may differ from this edit');
  await overlay.getByRole('button', { name: 'Play Narration' }).click();
  await expect
    .poll(() => page.evaluate(() => (window as unknown as { __announcerSpoken: string[] }).__announcerSpoken))
    .toEqual(['Updated the synthetic sample file']);
  await overlay.getByRole('button', { name: 'Expand Announcer' }).click();
  await overlay.getByRole('textbox', { name: 'Announcer voice name' }).fill('synthetic-voice');
  await overlay.getByRole('textbox', { name: 'Announcer voice name' }).blur();
  await overlay.getByRole('combobox', { name: 'Announcer voice provider' }).selectOption('fake');
  const serverSpeech = page.waitForRequest('**/tts/synthesize');
  await overlay.getByRole('button', { name: 'Play Narration' }).click();
  expect((await serverSpeech).postDataJSON()).toMatchObject({
    provider: 'fake',
    text: 'Updated the synthetic sample file',
    voice: 'synthetic-voice',
  });
  await expect
    .poll(() => page.evaluate(() => (window as unknown as { __announcerSpoken: string[] }).__announcerSpoken))
    .toContain('server:blob:synthetic-speech');
  await expect(overlay.getByRole('list', { name: 'Cross-Project Activity Reel' })).toContainText('demo');
  await overlay.getByRole('button', { name: 'Shrink Announcer' }).click();
  await settleViewport(false);
  if (captureDir) {
    await page.screenshot({ path: `${captureDir}/hs2-announcer-wide.png` });
    await overlay.screenshot({ path: `${captureDir}/hs2-announcer-panel-wide.png` });
  }

  await page.setViewportSize({ width: 768, height: 900 });
  await settleViewport(true);
  await expect
    .poll(() => overlay.evaluate((node) => node.getBoundingClientRect().right <= window.innerWidth))
    .toBe(true);
  if (captureDir) {
    await page.screenshot({ path: `${captureDir}/hs2-announcer-medium.png` });
    await overlay.screenshot({ path: `${captureDir}/hs2-announcer-panel-medium.png` });
  }

  await page.setViewportSize({ width: 390, height: 844 });
  await settleViewport(true);
  await expect(launch).toBeVisible();
  await expect
    .poll(() => overlay.evaluate((node) => node.getBoundingClientRect().right <= window.innerWidth))
    .toBe(true);
  if (captureDir) {
    await page.screenshot({ path: `${captureDir}/hs2-announcer-narrow.png` });
    await overlay.screenshot({ path: `${captureDir}/hs2-announcer-panel-narrow.png` });
  }
  await page.setViewportSize({ width: 1280, height: 800 });
  await settleViewport(false);
  if (captureDir) await page.screenshot({ path: `${captureDir}/hs2-announcer-return.png` });
  await expect(overlay.getByRole('button', { name: 'Close Announcer' })).toHaveCount(0);
  await launch.click();
  await expect(launch).toHaveAttribute('aria-pressed', 'false');
  await expect(overlay).toHaveCount(0);
  await launch.click();
  await expect(launch).toHaveAttribute('aria-pressed', 'true');
  await expect(overlay).toContainText('src/sample.ts');
  await expect(overlay).not.toContainText('const n = 2;');
  expect(diffReads).toBe(1);
  await page.getByRole('button', { name: 'Hide Project Sidebar' }).click();
  await expect(launch).toBeVisible();
  expect(await launch.evaluate((node) => Boolean(node.closest('[data-region-id="app-left-rail"]')))).toBe(false);
  await launch.click();
  await expect(launch).toHaveAttribute('aria-pressed', 'false');
  await expect(overlay).toHaveCount(0);
  await launch.click();
  await expect(overlay).toContainText('src/sample.ts');
  await page.getByRole('button', { name: 'Show Project Sidebar' }).click();
  await expect(launch).toBeVisible();
  expect(await launch.evaluate((node) => Boolean(node.closest('[data-region-id="app-left-rail"]')))).toBe(true);
});

test('HS2-5PSQJQ narrates a new live activity once after a replayable stream event', async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(window, 'WebSocket', { configurable: true, value: undefined });
    const spoken: string[] = [];
    Object.defineProperty(window, '__announcerSpoken', { value: spoken });
    Object.defineProperty(window, 'SpeechSynthesisUtterance', {
      configurable: true,
      value: class {
        onstart?: () => void;
        onend?: () => void;
        constructor(readonly text: string) {}
      },
    });
    Object.defineProperty(window, 'speechSynthesis', {
      configurable: true,
      value: {
        cancel() {},
        speak(utterance: { text: string; onstart?: () => void }) {
          spoken.push(utterance.text);
          utterance.onstart?.();
        },
      },
    });
  });
  await openDemoProject(page);
  await page.route('**/checkouts/demo-checkout/activity**', (route) => route.fulfill({ json: [] }));
  await page.route('**/checkouts/demo-checkout/activity/current-diff', (route) =>
    route.fulfill({ json: { diff: '+const live = false;', truncated: false } }),
  );
  let send = false;
  await page.route('**/ws/poll**', (route) => {
    if (!send) return route.fulfill({ json: { cursor: 0, events: [], overflow: false } });
    send = false;
    return route.fulfill({
      json: {
        cursor: 1,
        overflow: false,
        events: [
          {
            cursor: 1,
            store: 'synthetic-store',
            kind: 'activity',
            id: 'live-1',
            slug: '',
            activity: {
              id: 'live-1',
              ts: '2026-11-01T01:01:00Z',
              tool: 'codex',
              kind: 'edit',
              summary: 'Changed the synthetic live file',
              detail: { path: 'src/live.ts', diff: '+const live = true;' },
              importance: 'high',
            },
          },
        ],
      },
    });
  });
  await page.getByRole('button', { name: 'Announcer' }).click();
  const overlay = page.locator('[data-component="announcer-overlay"]');
  await overlay.getByRole('button', { name: 'Live' }).click();
  send = true;
  await expect(overlay).toContainText('Changed the synthetic live file');
  await expect(overlay).not.toContainText('+const live = true;');
  await overlay.getByRole('button', { name: 'View current diff' }).click();
  await expect(overlay.getByLabel('Current working-tree diff')).toContainText('+const live = false;');
  await expect
    .poll(() => page.evaluate(() => (window as unknown as { __announcerSpoken: string[] }).__announcerSpoken))
    .toEqual(['Changed the synthetic live file']);
});
