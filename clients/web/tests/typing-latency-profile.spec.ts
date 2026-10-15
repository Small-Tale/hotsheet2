import { expect, type Page, test } from '@playwright/test';

import { realTicketServer } from './real-ticket-server';

// HS2-EQTXTJ profiling harness: per-keystroke Event Timing (input delay + handlers + next paint) while
// typing into each text field of a realistically sized project. Opt-in: HOTSHEET_TYPING_PROFILE=1.
test.skip(!process.env.HOTSHEET_TYPING_PROFILE, 'opt-in typing latency profile');

interface KeyTiming {
  name: string;
  duration: number;
  inputDelay: number;
  processing: number;
  presentation: number;
}

async function startRecording(page: Page) {
  await page.evaluate(() => {
    const state = window as typeof window & { __keyTimings?: KeyTiming[]; __keyObserver?: PerformanceObserver };
    state.__keyTimings = [];
    state.__keyObserver?.disconnect();
    state.__keyObserver = new PerformanceObserver((list) => {
      for (const entry of list.getEntries() as PerformanceEventTiming[]) {
        if (!['keydown', 'keypress', 'keyup', 'input', 'beforeinput'].includes(entry.name)) continue;
        state.__keyTimings!.push({
          name: entry.name,
          duration: entry.duration,
          inputDelay: entry.processingStart - entry.startTime,
          processing: entry.processingEnd - entry.processingStart,
          presentation: entry.startTime + entry.duration - entry.processingEnd,
        });
      }
    });
    state.__keyObserver.observe({ type: 'event', durationThreshold: 16, buffered: false } as PerformanceObserverInit);
  });
}

async function stopRecording(page: Page): Promise<KeyTiming[]> {
  await page.waitForTimeout(500);
  return page.evaluate(() => {
    const state = window as typeof window & { __keyTimings?: KeyTiming[]; __keyObserver?: PerformanceObserver };
    state.__keyObserver?.disconnect();
    return state.__keyTimings ?? [];
  });
}

function summarize(label: string, timings: KeyTiming[], keystrokes: number) {
  const keydowns = timings.filter((entry) => entry.name === 'keydown'),
    sorted = [...keydowns].sort((left, right) => left.duration - right.duration),
    pick = (fraction: number) =>
      sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * fraction))]?.duration ?? 0,
    average = (key: keyof KeyTiming) =>
      keydowns.length ? keydowns.reduce((sum, entry) => sum + (entry[key] as number), 0) / keydowns.length : 0;
  const inputs = timings.filter((entry) => entry.name === 'input'),
    inputAverage = (key: keyof KeyTiming) =>
      inputs.length ? inputs.reduce((sum, entry) => sum + (entry[key] as number), 0) / inputs.length : 0;
  console.log(
    `${label} input: ${inputs.length} over 16ms; avg duration ${inputAverage('duration').toFixed(1)} handlers ${inputAverage('processing').toFixed(1)} paint ${inputAverage('presentation').toFixed(1)}`,
  );
  console.log(
    `${label}: ${keydowns.length}/${keystrokes} keydowns over 16ms; p50 ${pick(0.5).toFixed(0)} p90 ${pick(0.9).toFixed(0)} max ${pick(1).toFixed(0)} ms; avg delay ${average('inputDelay').toFixed(1)} handlers ${average('processing').toFixed(1)} paint ${average('presentation').toFixed(1)}`,
  );
}

test('profiles per-keystroke latency in every editor of a 400-ticket project', async ({ page }) => {
  test.setTimeout(300_000);
  const server = await realTicketServer();
  try {
    const created: Array<{ id: string; slug: string }> = [];
    for (let start = 0; start < 400; start += 25)
      created.push(
        ...(await Promise.all(
          Array.from({ length: 25 }, (_, offset) =>
            server.request<{ id: string; slug: string }>('/tickets', 'POST', {
              title: `Profile ticket ${start + offset} with a realistic, moderately long title`,
              details: `Details for ticket ${start + offset}.\n\n- one\n- two\n\nSome **markdown** text.`,
              tags: ['profile', `group-${(start + offset) % 7}`],
            }),
          ),
        )),
      );
    // Each note links another ticket, so the profile can type into a stacked linked reader (HS2-WV0VPE).
    for (const ticket of created.slice(0, 40))
      await server.request(`/tickets/${ticket.id}`, 'PATCH', {
        note: `An existing note with some text. See ${created[(created.indexOf(ticket) + 1) % created.length].slug}.`,
      });
    const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
    await page.routeWebSocket(/.*/, (route) => {
      void route.close();
    });
    await page.route('**/__hotsheet/projects/open', (route) =>
      route.fulfill({
        status: 201,
        json: { id: server.checkoutId, root: server.root, name: 'profile', stores: [server.store], apiPath },
      }),
    );
    await page.route(`**${apiPath}/**`, async (route) => {
      const incoming = new URL(route.request().url()),
        path = incoming.pathname.slice(apiPath.length);
      try {
        const response = await route.fetch({
          url: `${server.url}${path}${incoming.search}`,
          headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
          timeout: 60_000,
        });
        await route.fulfill({ response });
      } catch {
        await route.abort().catch(() => undefined);
      }
    });
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('/');
    await page.getByRole('button', { name: 'Open Project' }).click();
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    const rows = page.locator('[data-action="select-ticket-row"]');
    await expect(rows.first()).toBeVisible({ timeout: 30_000 });
    await page.waitForTimeout(1500);
    const text = 'the quick brown fox jumps over the lazy dog';
    const type = async (label: string, field: ReturnType<Page['locator']>) => {
      await field.click();
      await page.waitForTimeout(300);
      await startRecording(page);
      await page.keyboard.type(text, { delay: 60 });
      summarize(label, await stopRecording(page), text.length);
    };

    // Search field.
    const toolbar = page.locator('[data-component="toolbar"][aria-label="Workspace Toolbar"]');
    await toolbar.getByRole('button', { name: 'Search tickets' }).click();
    await type('search', toolbar.getByRole('searchbox', { name: 'Search tickets' }));
    await page.keyboard.press('Escape');
    await toolbar
      .getByRole('button', { name: 'Clear search' })
      .click()
      .catch(() => undefined);
    await page.waitForTimeout(800);

    // Select a ticket with notes and type into its title, details, and a new note.
    await rows.first().click();
    const inspector = page.locator('#app-right-rail');
    await inspector.locator('[data-action="edit-ticket-title"]').first().dblclick();
    await expect(inspector.locator('[name="ticket-title"]').first()).toBeVisible();
    await type('title', inspector.locator('[name="ticket-title"]').first());
    await page.keyboard.press('Tab');
    await page.waitForTimeout(500);
    const details = inspector.locator('[data-action="edit-markdown"]').first();
    await details.click();
    const source = inspector.locator('[name="markdown-source"]').first();
    await expect(source).toBeVisible();
    await type('details', source);
    await source.evaluate((field) => {
      (field as HTMLTextAreaElement).blur();
    });
    await page.waitForTimeout(800);
    await inspector.getByRole('button', { name: 'Add Note', exact: true }).last().click();
    await type('new note', inspector.locator('[name="new-note-body"]').first());
    await page.keyboard.press('Escape');
    await page.waitForTimeout(800);

    // A stacked linked reader's details editor.
    await inspector.locator('[data-action="open-linked-ticket"]').first().click();
    const linked = page.getByRole('dialog', { name: /^Read and edit / }).last();
    await expect(linked).toBeVisible();
    await page.waitForTimeout(800);
    await linked
      .locator('[data-action="edit-markdown"]')
      .first()
      .click({ position: { x: 4, y: 4 } });
    const linkedSource = linked.locator('[name="markdown-source"]').first();
    await expect(linkedSource).toBeVisible();
    await type('linked details', linkedSource);
  } finally {
    await server.stop();
  }
});
