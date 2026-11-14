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
