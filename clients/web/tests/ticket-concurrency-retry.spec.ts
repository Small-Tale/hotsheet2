import { expect, type Locator, type Page, test } from '@playwright/test';

import {
  controlInspectorResizeDelivery,
  expectPendingInspectorResize,
  releaseInspectorResizeDelivery,
} from './inspector-resize-delivery';
import { realTicketServer } from './real-ticket-server';

// HS2-9S8CV1: a real Markdown checkout 409 names the ticket in its message (`ticket '<slug>' changed
// since it was read`), so the editors recognize the stale-token refusal by its typed `code`. Every
// conflict here comes from the real rebuilt server: another writer lands on the ticket in the instant
// before the browser's PATCH reaches it, exactly as an AI renewing its lease or adding a note would.

type Server = Awaited<ReturnType<typeof realTicketServer>>;
interface Seeded {
  id: string;
  slug: string;
}
interface Attempt {
  path: string;
  status: number;
  body: { error?: string; code?: string };
}

async function openRealProject(
  page: Page,
  server: Server,
  beforePatch: (attempt: number, path: string) => Promise<void>,
) {
  const apiPath = `/__hotsheet/project-api/${server.checkoutId}`,
    attempts: Attempt[] = [];
  await page.routeWebSocket(/.*/, (route) => {
    void route.close();
  });
  await page.route('**/__hotsheet/projects/open', (route) =>
    route.fulfill({
      status: 201,
      json: { id: server.checkoutId, root: server.root, name: 'conflicts', stores: [server.store], apiPath },
    }),
  );
  await page.route(`**${apiPath}/**`, async (route) => {
    const request = route.request(),
      incoming = new URL(request.url()),
      path = incoming.pathname.slice(apiPath.length),
      patch = request.method() === 'PATCH' && /\/checkouts\/[^/]+\/tickets\/[^/]+$/u.test(path);
    try {
      if (patch) await beforePatch(attempts.length, decodeURIComponent(path));
      const response = await route.fetch({
        url: `${server.url}${path}${incoming.search}`,
        headers: { ...request.headers(), 'X-Hotsheet-Secret': server.secret },
        timeout: 60_000,
      });
      // Recorded after the response is read, so it can trail the server's own write: assertions poll it.
      if (patch)
        attempts.push({
          path: decodeURIComponent(path),
          status: response.status(),
          body: (await response.json()) as Attempt['body'],
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
  return attempts;
}

async function seed(server: Server, title: string, details: string) {
  return server.request<Seeded>('/tickets', 'POST', { title, details });
}

async function stored(server: Server, ticket: Seeded) {
  return server.request<{ details: string; notes: Array<{ text: string }> }>(`/tickets/${ticket.id}`);
}

async function editDetails(page: Page, slug: string, value: string) {
  await page.locator(`[data-action="select-ticket-row"][data-ticket-slug="${slug}"]`).click();
  const inspector = page.locator('#app-right-rail');
  await inspector.getByRole('button', { name: 'Edit Ticket details' }).click({ position: { x: 4, y: 4 } });
  const editor = inspector.getByRole('textbox', { name: 'Ticket details' });
  await expect(editor).toBeVisible();
  await editor.fill(value);
  return editor;
}

function expectRealStaleRefusal(attempt: Attempt | undefined, slug: string) {
  expect(attempt?.status).toBe(409);
  expect(attempt?.body).toEqual({ error: `ticket '${slug}' changed since it was read`, code: 'ticket_changed' });
}

test('rebases and retries a real Markdown slug-bearing 409 instead of failing the edit (HS2-9S8CV1)', async ({
  page,
}) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    const ticket = await seed(server, 'Real retry', 'Intro\nMiddle\nEnd');
    const attempts = await openRealProject(page, server, async (attempt) => {
      // An AI appends a line and records a note between the client's read and its save.
      if (attempt === 0) {
        await server.request(`/tickets/${ticket.id}`, 'PATCH', { details: 'Intro\nMiddle\nEnd\nAppended by the AI' });
        await server.request(`/tickets/${ticket.id}`, 'PATCH', { note: 'Lease renewed by the AI' });
      }
    });
    const editor = await editDetails(page, ticket.slug, 'Intro, edited locally\nMiddle\nEnd');
    await editor.blur();
    await expect
      .poll(async () => (await stored(server, ticket)).details)
      .toBe('Intro, edited locally\nMiddle\nEnd\nAppended by the AI');
    expectRealStaleRefusal(attempts[0], ticket.slug);
    await expect.poll(() => attempts.map((attempt) => attempt.status)).toEqual([409, 200]);
    expect((await stored(server, ticket)).notes.map((note) => note.text)).toEqual(['Lease renewed by the AI']);
    const inspector = page.locator('#app-right-rail');
    await expect(inspector.locator('[data-component="ticket-field-conflict"]')).toHaveCount(0);
    await expect(page.locator('.app-error')).toHaveCount(0);
    await inspector.getByRole('button', { name: 'Edit Ticket details' }).click({ position: { x: 4, y: 4 } });
    await expect(editor).toHaveValue('Intro, edited locally\nMiddle\nEnd\nAppended by the AI');
  } finally {
    await page.unrouteAll({ behavior: 'ignoreErrors' });
    await server.stop();
  }
});

test('finishes a held real 409 retry for the inactive owner without touching the newer owner draft (HS2-9S8CV1)', async ({
  page,
}) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    const owner = await seed(server, 'Inactive owner', 'First\nSecond'),
      next = await seed(server, 'Newer owner', 'Base of the newer owner');
    let releaseRetry!: () => void, signalRetry!: () => void;
    const retryHeld = new Promise<undefined>((resolve) => {
        releaseRetry = () => {
          resolve(undefined);
        };
      }),
      retryArrived = new Promise<undefined>((resolve) => {
        signalRetry = () => {
          resolve(undefined);
        };
      });
    const attempts = await openRealProject(page, server, async (attempt, path) => {
      if (!path.includes(owner.id)) return;
      if (attempt === 0) await server.request(`/tickets/${owner.id}`, 'PATCH', { note: 'Concurrent AI note' });
      // Hold the owner's rebased retry until the user has a newer draft open on another ticket.
      if (attempt === 1) {
        signalRetry();
        await retryHeld;
      }
    });
    await editDetails(page, owner.slug, 'First, edited\nSecond');
    // Selecting another ticket blurs the owner's editor: its save hits a real 409 and retries with no editor open.
    await page.locator(`[data-action="select-ticket-row"][data-ticket-slug="${next.slug}"]`).click();
    await retryArrived;
    const inspector = page.locator('#app-right-rail');
    await expect(inspector).toContainText('Newer owner');
    await inspector.getByRole('button', { name: 'Edit Ticket details' }).click({ position: { x: 4, y: 4 } });
    const editor = inspector.getByRole('textbox', { name: 'Ticket details' });
    await expect(editor).toHaveValue('Base of the newer owner');
    await editor.fill('Newer owner draft, still typing');
    const recovery = () =>
      page.evaluate((id) => {
        const key = Object.keys(localStorage).find(
          (item) => item.startsWith('hotsheet.ticket-draft:') && item.includes(id) && item.endsWith(':details'),
        );
        return key ? (JSON.parse(localStorage.getItem(key) ?? '{}') as { base?: string; draft?: string }) : undefined;
      }, next.id);
    await expect
      .poll(recovery)
      .toMatchObject({ base: 'Base of the newer owner', draft: 'Newer owner draft, still typing' });

    releaseRetry();
    await expect.poll(async () => (await stored(server, owner)).details).toBe('First, edited\nSecond');
    expectRealStaleRefusal(attempts[0], owner.slug);
    await expect.poll(() => attempts.map((attempt) => attempt.status)).toEqual([409, 200]);
    // Both attempts addressed the owner's qualified id, never the newly selected ticket.
    for (const attempt of attempts) {
      expect(attempt.path).toContain(owner.id);
      expect(attempt.path).not.toContain(next.id);
    }
    expect((await stored(server, owner)).notes.map((note) => note.text)).toEqual(['Concurrent AI note']);
    // The newer owner's draft, focus, recovery copy and base are untouched by the late completion.
    await expect(editor).toHaveValue('Newer owner draft, still typing');
    await expect(editor).toBeFocused();
    await expect(inspector).toContainText('Newer owner');
    await expect
      .poll(recovery)
      .toMatchObject({ base: 'Base of the newer owner', draft: 'Newer owner draft, still typing' });
    expect((await stored(server, next)).details).toBe('Base of the newer owner');
    await expect(page.locator('.app-error')).toHaveCount(0);

    // The newer owner then saves normally, its recovery copy clears, and a further edit saves on top.
    await editor.blur();
    await expect.poll(async () => (await stored(server, next)).details).toBe('Newer owner draft, still typing');
    await expect.poll(recovery).toBeUndefined();
    await inspector.getByRole('button', { name: 'Edit Ticket details' }).click({ position: { x: 4, y: 4 } });
    await editor.fill('Newer owner draft, saved twice');
    await editor.blur();
    await expect.poll(async () => (await stored(server, next)).details).toBe('Newer owner draft, saved twice');
    await expect
      .poll(() => attempts.filter((attempt) => attempt.path.includes(next.id)).map((attempt) => attempt.status))
      .toEqual([200, 200]);
    expect((await stored(server, owner)).details).toBe('First, edited\nSecond');
  } finally {
    await page.unrouteAll({ behavior: 'ignoreErrors' });
    await server.stop();
  }
});

test('surfaces a true same-place conflict from a real 409 without a blind retry (HS2-9S8CV1)', async ({ page }) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    const ticket = await seed(server, 'True conflict', 'Intro\nMiddle');
    const attempts = await openRealProject(page, server, async (attempt) => {
      if (attempt === 0) await server.request(`/tickets/${ticket.id}`, 'PATCH', { details: 'Intro by the AI\nMiddle' });
    });
    const editor = await editDetails(page, ticket.slug, 'Intro by me\nMiddle');
    await editor.blur();
    const conflict = page.locator('#app-right-rail [data-component="ticket-field-conflict"]');
    await expect(conflict).toBeVisible();
    await expect(conflict).toContainText('Intro by the AI');
    expect(attempts).toHaveLength(1);
    expectRealStaleRefusal(attempts[0], ticket.slug);
    expect((await stored(server, ticket)).details).toBe('Intro by the AI\nMiddle');
    await conflict.getByRole('textbox', { name: 'Merged details' }).fill('Intro by me and the AI\nMiddle');
    await conflict.getByRole('button', { name: 'Apply Merged Value' }).click();
    await expect(conflict).toHaveCount(0);
    await expect.poll(async () => (await stored(server, ticket)).details).toBe('Intro by me and the AI\nMiddle');
    await expect.poll(() => attempts.at(-1)?.status).toBe(200);
  } finally {
    await page.unrouteAll({ behavior: 'ignoreErrors' });
    await server.stop();
  }
});

test('bounds real stale-token retries, then saves the next edit after the churn stops (HS2-9S8CV1)', async ({
  page,
}) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    const ticket = await seed(server, 'Bounded retry', 'Original');
    let churning = true;
    const attempts = await openRealProject(page, server, async (attempt) => {
      // Every save loses the race to another writer until the churn stops.
      if (churning) await server.request(`/tickets/${ticket.id}`, 'PATCH', { note: `Churn ${attempt}` });
    });
    const editor = await editDetails(page, ticket.slug, 'Never lands');
    // A later background refresh replaces the shared error banner (HS2-4Q44N2), so record every message it shows.
    await page.evaluate(() => {
      const shown: string[] = [];
      (window as unknown as { hs2ErrorsShown: string[] }).hs2ErrorsShown = shown;
      new MutationObserver(() => {
        const text = document.querySelector('.app-error')?.textContent ?? '';
        if (text && shown.at(-1) !== text) shown.push(text);
      }).observe(document.body, { subtree: true, childList: true, characterData: true });
    });
    await editor.blur();
    // The first attempt plus four bounded retries, every one a real refusal, then the failure is shown. Each
    // attempt is a real write, re-read and refusal, so the whole sequence waits on attempts, not a fixed delay.
    await expect.poll(() => attempts.length, { timeout: 60_000 }).toBe(5);
    await expect
      .poll(() => page.evaluate(() => (window as unknown as { hs2ErrorsShown: string[] }).hs2ErrorsShown.join('\n')))
      .toContain(`ticket '${ticket.slug}' changed since it was read`);
    expect(attempts).toHaveLength(5);
    for (const attempt of attempts) expectRealStaleRefusal(attempt, ticket.slug);
    expect((await stored(server, ticket)).details).toBe('Original');
    // The refused draft stays in the open editor; once the churn stops, a further edit saves.
    await expect(editor).toHaveValue('Never lands');
    churning = false;
    await editor.fill('Lands after reset');
    await editor.blur();
    await expect.poll(async () => (await stored(server, ticket)).details).toBe('Lands after reset');
    await expect.poll(() => attempts.at(-1)?.status).toBe(200);
    await expect(page.locator('#app-right-rail [data-component="ticket-field-conflict"]')).toHaveCount(0);
  } finally {
    await page.unrouteAll({ behavior: 'ignoreErrors' });
    await server.stop();
  }
});

test('retries a real 409 from a linked reader through the same typed recognition (HS2-9S8CV1)', async ({ page }) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    const linked = await seed(server, 'Linked target', 'Top\nBottom'),
      host = await seed(server, 'Linking host', 'Host');
    await server.request(`/tickets/${host.id}`, 'PATCH', { note: `See ${linked.slug}.` });
    let raced = false;
    const attempts = await openRealProject(page, server, async () => {
      if (!raced) {
        raced = true;
        await server.request(`/tickets/${linked.id}`, 'PATCH', { details: 'Top\nBottom\nAppended by the AI' });
      }
    });
    await page.locator(`[data-action="select-ticket-row"][data-ticket-slug="${host.slug}"]`).click();
    await page.locator('#app-right-rail [data-action="open-linked-ticket"]').first().click();
    const reader = page.getByRole('dialog', { name: /^Read and edit / }).last();
    await expect(reader).toBeVisible();
    await reader.getByRole('button', { name: 'Edit Ticket details' }).click({ position: { x: 4, y: 4 } });
    const editor = reader.getByRole('textbox', { name: 'Ticket details' });
    await editor.fill('Top, edited in the reader\nBottom');
    await editor.blur();
    await expect
      .poll(async () => (await stored(server, linked)).details)
      .toBe('Top, edited in the reader\nBottom\nAppended by the AI');
    expectRealStaleRefusal(attempts[0], linked.slug);
    await expect.poll(() => attempts.map((attempt) => attempt.status)).toEqual([409, 200]);
    await expect(reader.locator('[data-component="ticket-field-conflict"]')).toHaveCount(0);
  } finally {
    await page.unrouteAll({ behavior: 'ignoreErrors' });
    await server.stop();
  }
});

async function exerciseResponsiveConflict(page: Page, controlledResize: boolean) {
  test.setTimeout(180_000);
  if (controlledResize) await controlInspectorResizeDelivery(page);
  const server = await realTicketServer();
  const widths = [
      ['wide', 1440, 900],
      ['medium', 1024, 800],
      ['narrow', 390, 844],
      ['wide-again', 1440, 900],
    ] as const,
    capture = async (state: string, subject: () => Locator) => {
      const rail = page.locator('#app-right-rail');
      for (const [label, width, height] of widths) {
        const controlled = controlledResize && state === 'conflict' && label === 'narrow';
        if (controlled) await page.evaluate(() => window.dispatchEvent(new Event('hs2-zv9m6r-arm-resize')));
        await page.setViewportSize({ width, height });
        if (controlled) await expectPendingInspectorResize(page);
        const readRail = async (name: string) => {
          const value = await rail.getAttribute(name);
          if (controlled) await releaseInspectorResizeDelivery(page);
          return value;
        };
        // HS2-ZV9M6R/HS2-3HJP95: resize delivery can follow setViewportSize.
        // Observe the new presentation before its collapse state; desktop return
        // also restores the saved rail preference in a later animation frame.
        await expect.poll(() => readRail('data-presentation')).toBe(width < 1024 ? 'overlay' : 'inline');
        if (width < 1024) {
          await expect(rail).toHaveAttribute('data-collapsed', 'true');
          await page.getByRole('button', { name: 'Show Ticket Inspector' }).click();
        }
        await expect(rail).toHaveAttribute('data-collapsed', 'false');
        // Rails slide after a breakpoint change; wait until no finite animation is still running.
        await expect
          .poll(() =>
            page.evaluate(
              () =>
                document
                  .getAnimations()
                  .filter(
                    (animation) =>
                      animation.playState === 'running' &&
                      animation.effect?.getComputedTiming().iterations !== Infinity,
                  ).length,
            ),
          )
          .toBe(0);
        await subject().scrollIntoViewIfNeeded();
        await expect(subject()).toBeInViewport();
        await expect
          .poll(() => subject().evaluate((node) => node.getBoundingClientRect().right <= innerWidth + 1))
          .toBe(true);
        if (width >= 1024)
          await expect
            .poll(() => page.locator('#app-left-rail').evaluate((node) => node.getBoundingClientRect().left))
            .toBe(0);
        const captureSlug = controlledResize ? 'hs2-zv9m6r-controlled' : 'hs2-zv9m6r';
        await page.screenshot({ path: `target/visual-captures/${captureSlug}-${state}-${label}.png` });
      }
    };
  try {
    const ticket = await seed(
      server,
      'Real conflict outcome with a long title that has to wrap at narrow widths',
      'Intro\nMiddle',
    );
    const attempts = await openRealProject(page, server, async (attempt) => {
      if (attempt === 0) await server.request(`/tickets/${ticket.id}`, 'PATCH', { details: 'Intro by the AI\nMiddle' });
    });
    const editor = await editDetails(page, ticket.slug, 'Intro by me\nMiddle');
    await editor.blur();
    const conflict = page.locator('#app-right-rail [data-component="ticket-field-conflict"]');
    await expect(conflict).toBeVisible();
    expectRealStaleRefusal(attempts[0], ticket.slug);
    await capture('conflict', () => conflict);
    await expect(conflict).toBeVisible();
    await expect(conflict).toContainText('Intro by the AI');
    await conflict.getByRole('textbox', { name: 'Merged details' }).fill('Intro by me and the AI\nMiddle');
    await conflict.getByRole('button', { name: 'Apply Merged Value' }).click();
    await expect(conflict).toHaveCount(0);
    await expect.poll(async () => (await stored(server, ticket)).details).toBe('Intro by me and the AI\nMiddle');
    await expect(page.locator('#app-right-rail')).toContainText('Intro by me and the AI');
    const saved = page.locator('#app-right-rail').getByRole('textbox', { name: 'Ticket details' });
    await expect(saved).toHaveValue('Intro by me and the AI\nMiddle');
    await capture('saved', () => saved);
    await expect(saved).toHaveValue('Intro by me and the AI\nMiddle');
    await expect(page.locator('.app-error')).toHaveCount(0);
  } finally {
    if (controlledResize)
      await page.evaluate(() => window.dispatchEvent(new Event('hs2-zv9m6r-restore-resize'))).catch(() => undefined);
    await page.unrouteAll({ behavior: 'ignoreErrors' });
    await server.stop();
  }
}

test('keeps the real-409 conflict and saved outcomes readable from wide to narrow and back (HS2-9S8CV1)', async ({
  page,
}) => {
  await exerciseResponsiveConflict(page, false);
});

test('delivers a pending real resize before opening the inspector (HS2-ZV9M6R)', async ({ page }) => {
  await exerciseResponsiveConflict(page, true);
});
