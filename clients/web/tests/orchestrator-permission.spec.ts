import { expect, type Page, test } from '@playwright/test';

import { desktopApp } from './desktop-draft-files-fixture';

const TARGET = '01ARZ3NDEKTSV4RRFFQ69G5FAZ';

/** Declare the project as its own target, bind it, and file one AI write against it. */
async function orchestratorRequest(server: Awaited<ReturnType<typeof desktopApp>>['server'], operation: string) {
  const checkout = server.checkoutId,
    base = `/checkouts/${checkout}/orchestration`;
  const declared = await server.request<{ targets: { target_id: string }[]; revision: string | null }>(
    `${base}/targets`,
  );
  if (!declared.targets.some((target) => target.target_id === TARGET)) {
    const added = await server.request<{ revision: string }>(`${base}/targets`, 'POST', {
      target_id: TARGET,
      label: 'API',
      expected_revision: declared.revision,
    });
    const candidate = await server.request<{ destination: unknown }>(`${base}/inspect`, 'POST', {
      destination: { kind: 'local', checkout_id: checkout },
    });
    await server.request(`${base}/targets/${TARGET}/binding`, 'PUT', {
      target: { owner_checkout_id: checkout, target_id: TARGET },
      expected_manifest_revision: added.revision,
      expected_binding_revision: 0,
      previous_destination: null,
      destination: candidate.destination,
    });
  }
  const checkouts = await server.request<{ id: string; sources: { connection_id: string }[] }[]>('/checkouts');
  const connection = checkouts.find((item) => item.id === checkout)?.sources[0]?.connection_id ?? '';
  const ticket = await server.request<{ slug: string }>('/tickets', 'POST', { title: 'Coordinate' });
  const response = await fetch(`${server.url}${base}/tickets/create`, {
    method: 'POST',
    headers: { 'content-type': 'application/json', 'X-Hotsheet-Secret': server.secret },
    body: JSON.stringify({
      ticket: { native_id: ticket.slug },
      target_id: TARGET,
      connection_id: connection,
      operation_id: operation,
      title: 'Planned work',
      actor: { role: 'ai', id: 'worker-1' },
    }),
  });
  const refused = (await response.json()) as { code?: string };
  expect(response.status).toBe(403);
  expect(refused.code).toBe('permission_required');
}

async function openPermissionSettings(page: Page) {
  await page.getByLabel('Settings View').click();
  const showSidebar = page.getByRole('button', { name: 'Show Settings Sidebar' });
  if (await showSidebar.isVisible()) await showSidebar.click();
  await page.getByRole('button', { name: 'Permissions', exact: true }).click();
  const hideSidebar = page.getByRole('button', { name: 'Hide Settings Sidebar' });
  if (await hideSidebar.isVisible()) await hideSidebar.click();
}

// HS2-XHQ1EH: an orchestrator's request reaches people as the ordinary permission card,
// headed by the orchestrator and the target project, with Allow Once, Always Allow and
// Deny only. Always Allow is reviewed and remembered as a rule that Settings → Permissions
// lists beside the other answers, naming the target project and the orchestrator.
// HS2-W4VB4Z: the same states in the dark scheme, on the real backdrop.
const capturePath = (state: string, width: number, scheme: 'light' | 'dark') =>
  scheme === 'light'
    ? `target/visual-captures/hs2-xhq1eh-${state}-${width}.png`
    : `target/visual-captures/hs2-w4vb4z-${state}-${width}-dark.png`;
for (const scheme of ['light', 'dark'] as const)
  for (const width of [1440, 1100, 390]) {
    test(`shows an orchestrator permission request and its remembered rule at ${width}px${scheme === 'dark' ? ' in the dark scheme (HS2-W4VB4Z)' : ''} (HS2-XHQ1EH)`, async ({
      context,
      baseURL,
    }) => {
      test.setTimeout(120_000);
      const app = await desktopApp(context, baseURL!);
      try {
        const page = await app.launch();
        await page.emulateMedia({ colorScheme: scheme });
        await page.setViewportSize({ width, height: 900 });
        await page.getByRole('button', { name: 'Open Project' }).click();
        await page.getByRole('button', { name: 'Local Project' }).click();
        await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
        await expect(page.getByRole('button', { name: 'New ticket…' })).toBeVisible();
        await orchestratorRequest(app.server, 'op-browser-1');
        const popup = page.locator('[data-component="permission-request-popup"]');
        await expect(popup).toContainText('Orchestrator: Create or change tickets in API');
        await expect(popup).toContainText('orchestration');
        await expect(popup).toContainText('Target project: API');
        await expect(popup.getByRole('button', { name: 'Allow This AI Session' })).toHaveCount(0);
        await expect(popup.getByRole('button', { name: 'Allow Once' })).toBeVisible();
        await expect(popup.getByRole('button', { name: 'Deny' })).toBeVisible();
        await popup.screenshot({ path: capturePath('card', width, scheme) });
        // The card over the real workspace backdrop it floats on.
        await page.screenshot({ path: capturePath('card-page', width, scheme) });
        if (width === 1440) {
          // Wide to narrow to wide: the decision row keeps every button visible.
          await page.setViewportSize({ width: 390, height: 900 });
          await expect(popup.getByRole('button', { name: 'Always Allow' })).toBeVisible();
          await page.setViewportSize({ width: 1440, height: 900 });
          await expect(popup.getByRole('button', { name: 'Always Allow' })).toBeVisible();
        }
        await popup.getByRole('button', { name: 'Always Allow' }).click();
        await expect(popup).toContainText('The orchestrator of');
        await expect(popup).toContainText('Persistent, until revoked');
        await expect(popup.getByLabel('Permission rule type')).toHaveCount(0);
        await popup.screenshot({ path: capturePath('review', width, scheme) });
        await popup.getByRole('button', { name: 'Confirm Always Allow' }).click();
        await expect(popup).toHaveCount(0);
        await openPermissionSettings(page);
        const preview = page.getByRole('list', { name: 'Recent Remembered Answers' });
        await expect(preview).toContainText('Create or change tickets in API');
        await expect(preview).toContainText('Applies to: The orchestrator of');
        await expect(preview).toContainText('Target project: API');
        await expect(preview).not.toContainText('All AI sessions');
        await preview.scrollIntoViewIfNeeded();
        // The settings sidebar slides away; capture once the list is fully on screen.
        await expect(preview).toBeInViewport({ ratio: 1 });
        await preview.screenshot({ path: capturePath('rule', width, scheme) });
        await page.screenshot({ path: capturePath('settings', width, scheme) });
      } finally {
        await app.stop();
      }
    });
  }

// HS2-42WS4Z: a project's Auto-allow decides AI tool requests only. With Auto-allow after
// 0 seconds, an AI tool's request would never be presented; the orchestrator's card is
// presented, shows no countdown, and waits for a person.
test('keeps an orchestrator card out of the project auto-allow (HS2-42WS4Z)', async ({ context, baseURL }) => {
  test.setTimeout(120_000);
  const app = await desktopApp(context, baseURL!);
  try {
    await app.server.request(`/checkouts/${app.server.checkoutId}/permission-automation`, 'PUT', {
      action: 'allow',
      delayMs: 0,
    });
    const page = await app.launch();
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.getByRole('button', { name: 'Open Project' }).click();
    await page.getByRole('button', { name: 'Local Project' }).click();
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    await expect(page.getByRole('button', { name: 'New ticket…' })).toBeVisible();
    await orchestratorRequest(app.server, 'op-browser-auto');
    const popup = page.locator('[data-component="permission-request-popup"]');
    await expect(popup).toContainText('Orchestrator: Create or change tickets in API');
    for (const width of [1440, 1100, 390, 1440]) {
      await page.setViewportSize({ width, height: 900 });
      await expect(popup.getByRole('button', { name: 'Allow Once' })).toBeVisible();
      await expect(popup).not.toContainText('Auto-allow in');
      await expect(popup.getByRole('button', { name: 'Stop auto-allow Countdown' })).toHaveCount(0);
      await popup.screenshot({ path: `target/visual-captures/hs2-42ws4z-card-${width}.png` });
    }
    // Still waiting on the server: nothing answered it automatically.
    const pending = await app.server.request<{ orchestrator?: unknown }[]>('/permissions');
    expect(pending.filter((request) => request.orchestrator)).toHaveLength(1);
    await popup.getByRole('button', { name: 'Allow Once' }).click();
    await expect(popup).toHaveCount(0);
  } finally {
    await app.stop();
  }
});

// HS2-4K43DK: while the AI permission vault is locked, the full rule list answers 503 and
// Settings shows "Remembered Answers Locked". The orchestrator's rules are stored in the
// orchestration ledger, so they stay listed below that notice and can still be revoked.
test('keeps orchestrator answers listed and revocable while the vault is locked (HS2-4K43DK)', async ({
  context,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const app = await desktopApp(context, baseURL!);
  try {
    const page = await app.launch();
    // The vault lock as the server reports it: only the full list, not the orchestrator subset.
    await page.route(
      (url) => url.pathname.endsWith('/permissions/rules') && url.search === '',
      (route) =>
        route.request().method() === 'GET'
          ? route.fulfill({ status: 503, json: { error: 'permission-rule store locked or unavailable' } })
          : route.continue(),
    );
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.getByRole('button', { name: 'Open Project' }).click();
    await page.getByRole('button', { name: 'Local Project' }).click();
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    await expect(page.getByRole('button', { name: 'New ticket…' })).toBeVisible();
    await orchestratorRequest(app.server, 'op-locked-1');
    const popup = page.locator('[data-component="permission-request-popup"]');
    await popup.getByRole('button', { name: 'Always Allow' }).click();
    await popup.getByRole('button', { name: 'Confirm Always Allow' }).click();
    await expect(popup).toHaveCount(0);
    await openPermissionSettings(page);
    const remembered = page.locator('[data-component="permission-rules-settings"]');
    await expect(remembered).toContainText('Remembered Answers Locked');
    await expect(remembered).toContainText('The orchestrator answers below are stored separately and still apply.');
    const preview = page.getByRole('list', { name: 'Recent Remembered Answers' });
    await expect(preview).toContainText('Create or change tickets in API');
    await expect(preview).toContainText('Target project: API');
    for (const width of [1440, 1100, 390, 1440]) {
      await page.setViewportSize({ width, height: 900 });
      if (width === 390) {
        const hideSidebar = page.getByRole('button', { name: 'Hide Settings Sidebar' });
        if (await hideSidebar.isVisible()) await hideSidebar.click();
      }
      await remembered.getByText('Remembered Answers Locked').evaluate((element) => {
        element.scrollIntoView({ block: 'start' });
      });
      // Panels slide when the width changes; capture once the list is fully on screen.
      await expect(preview).toBeInViewport({ ratio: 1 });
      await page.waitForFunction(() =>
        document.getAnimations().every((animation) => animation.playState !== 'running'),
      );
      await expect
        .poll(() => remembered.evaluate((element) => element.getBoundingClientRect().right))
        .toBeLessThanOrEqual(width);
      await page.screenshot({ path: `target/visual-captures/hs2-4k43dk-locked-${width}.png` });
    }
    const row = preview.locator('[data-permission-rule]').first();
    await row.locator('[data-action="select-permission-rule-row"]').click();
    await page
      .locator('[data-permission-selection-surface="preview"][data-permission-bulk-menu]')
      .getByRole('button', { name: 'Selected Answer Actions' })
      .click();
    await page.getByRole('menuitem', { name: 'Delete Selected' }).click();
    await page.getByRole('button', { name: 'Confirm Delete' }).click();
    await expect(preview).toHaveCount(0);
    await expect(remembered).toContainText('Remembered Answers Locked');
    await expect(remembered).not.toContainText('Could not refresh');
    const left = await app.server.request<unknown[]>('/permissions/rules?source=orchestrator');
    expect(left).toEqual([]);
  } finally {
    await app.stop();
  }
});

// HS2-7NGC69: a remembered orchestrator rule's action is a sentence, set in body type in
// Settings → Permissions; a command keeps code type. Light and dark alike.
test('sets an orchestrator rule sentence in body type (HS2-7NGC69)', async ({ context, baseURL }) => {
  test.setTimeout(120_000);
  const app = await desktopApp(context, baseURL!);
  try {
    const page = await app.launch();
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.getByRole('button', { name: 'Open Project' }).click();
    await page.getByRole('button', { name: 'Local Project' }).click();
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    await expect(page.getByRole('button', { name: 'New ticket…' })).toBeVisible();
    await orchestratorRequest(app.server, 'op-type-1');
    await app.server.request(`/checkouts/${app.server.checkoutId}/orchestration/permission-rules`, 'POST', {
      target_id: TARGET,
      action: 'tickets_write',
      actor: { role: 'human', id: 'maintainer' },
    });
    await openPermissionSettings(page);
    const preview = page.getByRole('list', { name: 'Recent Remembered Answers' });
    const sentence = preview.locator('[data-exact-action]').first();
    await expect(sentence).toHaveText('Create or change tickets in API');
    await expect(sentence.locator('code')).toHaveCount(0);
    const family = await sentence.evaluate((element) => getComputedStyle(element).fontFamily);
    expect(family).not.toMatch(/mono/iu);
    for (const scheme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      for (const width of [1440, 1100, 390, 1440]) {
        await page.setViewportSize({ width, height: 900 });
        if (width === 390) {
          const hideSidebar = page.getByRole('button', { name: 'Hide Settings Sidebar' });
          if (await hideSidebar.isVisible()) await hideSidebar.click();
        }
        await preview.scrollIntoViewIfNeeded();
        await expect(preview).toBeInViewport({ ratio: 1 });
        await page.waitForFunction(() =>
          document.getAnimations().every((animation) => animation.playState !== 'running'),
        );
        await preview.screenshot({ path: `target/visual-captures/hs2-7ngc69-rule-${scheme}-${width}.png` });
      }
    }
  } finally {
    await app.stop();
  }
});
