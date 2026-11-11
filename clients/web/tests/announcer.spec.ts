import { expect, test } from '@playwright/test';

import { openDemoProject } from './mobile-project-fixture';

test('HS2-6PZ35D keeps the phone announcer toggle in the sidebar rail', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openDemoProject(page);
  await expect(page.locator('[data-project-dialog]')).not.toBeVisible();
  const launch = page.getByRole('button', { name: 'Announcer', exact: true });
  // A phone keeps the announcer in the overlay rail, leaving its one-row toolbar to search (HS2-RDFY4A).
  await expect(launch).toBeHidden();
  await expect(page.locator('[data-component="project-tab-bar"] [data-announcer-action="open"]')).toHaveCount(0);
  await page.getByRole('button', { name: 'Show Project Sidebar' }).click();
  await expect(launch).toBeVisible();
  expect(await launch.evaluate((node) => Boolean(node.closest('[data-region-id="app-left-rail"]')))).toBe(true);
  await expect(launch).toHaveAttribute('aria-pressed', 'false');
  if (process.env.ANNOUNCER_VISUAL_CAPTURE)
    await page.screenshot({
      path: `${process.env.ANNOUNCER_VISUAL_CAPTURE}/hs2-announcer-launch-narrow.png`,
      animations: 'disabled',
    });
  await launch.click();
  await expect(launch).toHaveAttribute('aria-pressed', 'true');
  await expect(page.locator('[data-component="announcer-overlay"]')).toContainText('No activity');
  await expect(page.locator('[data-component="announcer-overlay"] [aria-label="Close Announcer"]')).toHaveCount(0);
  await launch.click();
  await expect(launch).toHaveAttribute('aria-pressed', 'false');
  await expect(page.locator('[data-component="announcer-overlay"]')).toHaveCount(0);
});

test('HS2-BS0XFK docks the announcer under a resizable separator in the sidebar footer', async ({ page }) => {
  const captureDir = process.env.ANNOUNCER_VISUAL_CAPTURE;
  await page.setViewportSize({ width: 1280, height: 800 });
  await openDemoProject(page);
  await page.route('**/checkouts/demo-checkout/activity**', (route) =>
    route.fulfill({
      json: [
        {
          store_id: 'synthetic-store',
          event: {
            id: 'dock-1',
            ts: '2026-11-01T01:00:00Z',
            tool: 'codex',
            kind: 'decision',
            summary: 'codex decided: keep the synthetic sample short',
            importance: 'high',
          },
        },
      ],
    }),
  );
  const rail = page.locator('aside[data-region-id="app-left-rail"]');
  const launch = page.getByRole('button', { name: 'Announcer', exact: true });
  await launch.click();
  const overlay = page.locator('[data-component="announcer-overlay"]');
  await expect(overlay).toHaveAttribute('data-presentation', 'docked');
  await expect(overlay).toContainText('keep the synthetic sample short');
  const dock = rail.getByRole('region', { name: 'Announcer Dock' });
  const handle = page.getByRole('separator', { name: 'Resize Announcer Dock' });
  await expect(dock).toBeVisible();
  // The separator sits between the drive row and the section, and the section reaches the rail's bottom.
  const drive = await rail
    .getByRole('button', { name: /Drive with/ })
    .first()
    .boundingBox();
  const handleBox = await handle.boundingBox();
  const dockBox = await dock.boundingBox();
  const railBox = await rail.boundingBox();
  if (!drive || !handleBox || !dockBox || !railBox) throw new Error('Docked announcer must be measurable');
  // The separator's hit area straddles its line; the line (the handle's center) lies below the drive row.
  expect(handleBox.y + handleBox.height / 2).toBeGreaterThanOrEqual(drive.y + drive.height);
  expect(Math.abs(handleBox.y + handleBox.height / 2 - dockBox.y)).toBeLessThanOrEqual(2);
  expect(Math.round(dockBox.y + dockBox.height)).toBeLessThanOrEqual(Math.round(railBox.y + railBox.height));
  expect(dockBox.width).toBeGreaterThan(railBox.width - 24);
  const initial = Number(await handle.getAttribute('aria-valuenow'));
  expect(initial).toBe(232);
  if (captureDir) await page.screenshot({ path: `${captureDir}/hs2-bs0xfk-docked-wide.png` });
  // Keyboard resize grows the section upward, by step and to its bound.
  await handle.focus();
  await page.keyboard.press('ArrowUp');
  await expect(handle).toHaveAttribute('aria-valuenow', String(initial + 16));
  await page.keyboard.press('Shift+ArrowUp');
  await expect(handle).toHaveAttribute('aria-valuenow', String(initial + 80));
  await expect.poll(async () => Math.round((await dock.boundingBox())?.height ?? 0)).toBe(initial + 80);
  // Pointer drag on the separator: down shrinks.
  const grip = await handle.boundingBox();
  if (!grip) throw new Error('Separator must be measurable');
  await page.mouse.move(grip.x + grip.width / 2, grip.y + grip.height / 2);
  // The separator stays on top of the section it resizes (a press at its line reaches the handle).
  expect(
    await page.evaluate(
      ([x, y]) => document.elementFromPoint(x, y)?.getAttribute('aria-label'),
      [grip.x + grip.width / 2, grip.y + grip.height / 2],
    ),
  ).toBe('Resize Announcer Dock');
  await page.mouse.down();
  await page.mouse.move(grip.x + grip.width / 2, grip.y + grip.height / 2 + 60, { steps: 6 });
  await page.mouse.up();
  await expect(handle).toHaveAttribute('aria-valuenow', String(initial + 20));
  // The height is the device's: it survives closing, reopening, and a reload.
  await launch.click();
  await expect(overlay).toHaveCount(0);
  await launch.click();
  await expect(handle).toHaveAttribute('aria-valuenow', String(initial + 20));
  await page.reload();
  await page.getByRole('button', { name: 'Announcer', exact: true }).click();
  await expect(page.getByRole('separator', { name: 'Resize Announcer Dock' })).toHaveAttribute(
    'aria-valuenow',
    String(initial + 20),
  );
  // At a short window the sidebar's views stay reachable above the section.
  await page.setViewportSize({ width: 1280, height: 600 });
  const queue = rail.getByRole('navigation', { name: 'Ticket Views' }).getByText('Queue', { exact: true });
  await queue.scrollIntoViewIfNeeded();
  await expect(queue).toBeInViewport();
  const views = await queue.boundingBox();
  const shortDock = await dock.boundingBox();
  if (!views || !shortDock) throw new Error('Views and dock must be measurable');
  expect(views.y + views.height).toBeLessThanOrEqual(shortDock.y);
  if (captureDir) await page.screenshot({ path: `${captureDir}/hs2-bs0xfk-docked-short.png` });
  // The phone drawer docks it while open; closing the drawer leaves the corner card on screen.
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(page.locator('[data-component="app-shell"]')).toHaveAttribute('data-mobile', 'true');
  await expect(overlay).toHaveAttribute('data-presentation', 'corner');
  await expect
    .poll(() => overlay.evaluate((node) => node.getBoundingClientRect().right <= window.innerWidth))
    .toBe(true);
  if (captureDir) {
    // Captures wait for the Workbench's rail motion across the desktop/phone boundary to settle.
    await page.waitForTimeout(500);
    await page.screenshot({ path: `${captureDir}/hs2-bs0xfk-corner-narrow.png` });
  }
  await page.getByRole('button', { name: 'Show Project Sidebar' }).click();
  await expect(overlay).toHaveAttribute('data-presentation', 'docked');
  await expect(overlay).toHaveCount(1);
  if (captureDir) {
    await page.waitForTimeout(400);
    await page.screenshot({ path: `${captureDir}/hs2-bs0xfk-docked-narrow.png` });
  }
  await page.setViewportSize({ width: 1280, height: 800 });
  await expect(page.locator('[data-component="app-shell"]')).toHaveAttribute('data-mobile', 'false');
  await expect(overlay).toHaveCount(1);
  if (captureDir) {
    await page.waitForTimeout(500);
    await page.screenshot({ path: `${captureDir}/hs2-bs0xfk-return-wide.png` });
  }
});

test('HS2-EYW4H8 HS2-YF8P38 shows toggle states and keeps two spaced control groups', async ({ page }) => {
  const captureDir = process.env.ANNOUNCER_VISUAL_CAPTURE;
  await page.setViewportSize({ width: 1280, height: 800 });
  await openDemoProject(page);
  await page.route('**/checkouts/demo-checkout/activity**', (route) => route.fulfill({ json: [] }));
  await page.getByRole('button', { name: 'Announcer', exact: true }).click();
  const overlay = page.locator('[data-component="announcer-overlay"]');
  const controls = overlay.locator('[data-component="toolbar"][aria-label="Announcer Controls"]');
  const live = controls.getByRole('button', { name: 'Live', exact: true });
  const audio = controls.getByRole('button', { name: 'Audio', exact: true });
  const paint = (button: typeof live) =>
    button.evaluate((node) => {
      const style = getComputedStyle(node);
      return `${style.backgroundColor}|${style.boxShadow}`;
    });
  // Off and on differ in pressed state and painted chrome, and Audio's icon changes with it.
  await expect(live).toHaveAttribute('aria-pressed', 'false');
  await expect(audio).toHaveAttribute('aria-pressed', 'true');
  const liveOff = await paint(live);
  const audioOn = await paint(audio);
  expect(liveOff).not.toBe(audioOn);
  await expect(audio.locator('[data-lucide="volume-2"]')).toHaveCount(1);
  await live.click();
  await audio.click();
  await expect(live).toHaveAttribute('aria-pressed', 'true');
  await expect(audio).toHaveAttribute('aria-pressed', 'false');
  await expect(audio.locator('[data-lucide="volume-x"]')).toHaveCount(1);
  // Away from the pointer's hover surface, once the chrome transition settles, the states swap paint.
  await page.mouse.move(0, 0);
  await expect.poll(() => paint(live)).toBe(audioOn);
  await expect.poll(() => paint(audio)).toBe(liveOff);
  if (captureDir) await overlay.screenshot({ path: `${captureDir}/hs2-eyw4h8-toggles-on-off.png` });
  await live.click();
  await audio.click();
  await expect(live).toHaveAttribute('aria-pressed', 'false');
  await expect(audio).toHaveAttribute('aria-pressed', 'true');
  // Playback hugs the start and the toggles hug the end; widening only grows the gap between them.
  const measure = async () => {
    const bar = await controls.boundingBox();
    const playback = await controls.getByRole('group', { name: 'Playback' }).boundingBox();
    const narration = await controls.getByRole('group', { name: 'Narration' }).boundingBox();
    if (!bar || !playback || !narration) throw new Error('Announcer controls must be measurable');
    // One row: the groups share a vertical center even in the narrow docked section.
    expect(Math.abs(playback.y + playback.height / 2 - (narration.y + narration.height / 2))).toBeLessThanOrEqual(1);
    return {
      start: playback.x - bar.x,
      end: bar.x + bar.width - (narration.x + narration.width),
      gap: narration.x - (playback.x + playback.width),
      playback: Math.round(playback.width),
      narration: Math.round(narration.width),
    };
  };
  const docked = await measure();
  await page.getByRole('button', { name: 'Hide Project Sidebar' }).click();
  await overlay.getByRole('button', { name: 'Expand Announcer' }).click();
  await expect(overlay).toHaveAttribute('data-expanded', 'true');
  const expanded = await measure();
  expect(expanded.gap).toBeGreaterThan(docked.gap + 200);
  expect(expanded.playback).toBe(docked.playback);
  expect(expanded.narration).toBe(docked.narration);
  for (const layout of [docked, expanded]) {
    expect(layout.start).toBeLessThanOrEqual(16);
    expect(layout.end).toBeLessThanOrEqual(16);
  }
  if (captureDir) await overlay.screenshot({ path: `${captureDir}/hs2-yf8p38-expanded-groups.png` });
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
  // Shown beside an open sidebar, the announcer docks in its footer without an expand control (HS2-BS0XFK).
  expect(await overlay.evaluate((node) => Boolean(node.closest('[data-region-id="app-left-rail"]')))).toBe(true);
  await expect(overlay.getByRole('button', { name: 'Expand Announcer' })).toHaveCount(0);
  // Hiding the sidebar keeps the announcer visible as its corner card, which still expands.
  await page.getByRole('button', { name: 'Hide Project Sidebar' }).click();
  await expect(overlay).toHaveCount(1);
  expect(await overlay.evaluate((node) => Boolean(node.closest('[data-region-id="app-left-rail"]')))).toBe(false);
  await expect(overlay).toHaveAttribute('data-presentation', 'corner');
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
  await page.getByRole('button', { name: 'Show Project Sidebar' }).click();
  await expect(overlay).toHaveAttribute('data-presentation', 'docked');
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
  // The phone's closed overlay rail holds the toggle, off the one-row toolbar (HS2-RDFY4A).
  await expect(launch).toBeHidden();
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
