import { expect, type Page, test } from '@playwright/test';

import { openMockedProject } from './open-mocked-project';
import { resizeTo, syntheticTool } from './orchestration-real-fixtures';
import { proxyFetch, realTicketServer } from './real-ticket-server';

/**
 * HS2-KXKA7X: Triggers… through the real shell against a real server. The project orchestrates itself ("Self");
 * a template is checked in through the server, and a trigger is set up through the shipped Orchestration header,
 * saved as a dry run, given a manual delivery (the `fire` route), turned on through its review, paused and
 * resumed. A second manual delivery applies the template as the trigger, which waits for permission, so a third
 * is skipped as overlapping. Synthetic tickets only.
 */
// A click a toast or an overlay blocks fails fast instead of waiting out the test.
test.use({ actionTimeout: 15_000 });

type Server = Awaited<ReturnType<typeof realTicketServer>>;
const TARGET = '01ARZ3NDEKTSV4RRFFQ69G5FJ8';
const person = { role: 'human', id: 'maintainer' };

async function send(server: Server, method: string, path: string, body?: unknown) {
  const response = await fetch(`${server.url}${path}`, {
    method,
    headers: { 'content-type': 'application/json', 'X-Hotsheet-Secret': server.secret },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await response.text();
  if (!response.ok) throw new Error(`${method} ${path}: ${response.status} ${text}`);
  return JSON.parse(text) as Record<string, unknown>;
}

/** An orchestration project that declares itself as its one target ("Self"), bound on this machine. */
async function orchestrateSelf(server: Server) {
  const base = `/checkouts/${server.checkoutId}/orchestration`;
  const added = await send(server, 'POST', `${base}/targets`, {
    target_id: TARGET,
    label: 'Self',
    expected_revision: null,
  });
  const candidate = await send(server, 'POST', `${base}/inspect`, {
    destination: { kind: 'local', checkout_id: server.checkoutId },
  });
  await send(server, 'PUT', `${base}/targets/${TARGET}/binding`, {
    target: { owner_checkout_id: server.checkoutId, target_id: TARGET },
    expected_manifest_revision: added.revision,
    expected_binding_revision: 0,
    previous_destination: null,
    destination: candidate.destination,
  });
  await send(server, 'PUT', `/checkouts/${server.checkoutId}/orchestration/project-type`, {
    project_type: 'orchestration',
    actor: { role: 'human' },
  });
}

async function checkInTemplate(server: Server) {
  await send(server, 'POST', `/checkouts/${server.checkoutId}/orchestration/templates/set`, {
    templates: [
      {
        name: 'Repair in Self',
        inputs: [
          { key: 'feature', label: 'Feature', kind: 'text', required: true },
          { key: 'project', label: 'Project', kind: 'target', default: TARGET },
        ],
        goal: { title: '{{feature}} in Self' },
        steps: [{ key: 'fix', target: '{{project}}', title: 'Repair {{feature}}' }],
      },
    ],
    actor: person,
  });
}

async function openProject(page: Page, server: Server) {
  const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
  await page.routeWebSocket(/.*/, (route) => {
    void route.close();
  });
  await page.route(/\/__hotsheet\/(?:remote\/)?projects\/open$/, (route) =>
    route.fulfill({
      status: 201,
      json: { id: server.checkoutId, root: server.root, name: 'Launch', stores: [server.store], apiPath },
    }),
  );
  await page.route(`**${apiPath}/**`, async (route) => {
    const incoming = new URL(route.request().url());
    try {
      const response = await proxyFetch(route, {
        url: `${server.url}${incoming.pathname.slice(apiPath.length)}${incoming.search}`,
        headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
        timeout: 60_000,
      });
      await route.fulfill({ response });
    } catch {
      await route.abort().catch(() => undefined);
    }
  });
  await page.goto('/');
  await openMockedProject(page);
}

// A waiting permission card keeps its own attention animation running; captures freeze animations instead of
// waiting them out.
const capture = async (page: Page, name: string) => {
  await page.screenshot({ path: test.info().outputPath(`triggers-impl-${name}.png`), animations: 'disabled' });
};

/** Wide, medium, phone and wide again, light and dark. */
async function captureSizes(page: Page, name: string, phone: () => Promise<void>) {
  for (const [width, height] of [
    [1440, 900],
    [1100, 800],
  ]) {
    await resizeTo(page, { width, height });
    await capture(page, `${name}-${width}-light`);
    await page.emulateMedia({ colorScheme: 'dark' });
    await capture(page, `${name}-${width}-dark`);
    await page.emulateMedia({ colorScheme: 'light' });
  }
  await resizeTo(page, { width: 390, height: 844 });
  await phone();
  await capture(page, `${name}-390-light`);
  await page.emulateMedia({ colorScheme: 'dark' });
  await capture(page, `${name}-390-dark`);
  await page.emulateMedia({ colorScheme: 'light' });
  await resizeTo(page, { width: 1440, height: 900 });
}

/** A toast over the footer would take a click meant for Pause or Resume: wait until each one is gone. */
async function dismissToasts(page: Page) {
  await expect(page.locator('wa-toast-item')).toHaveCount(0, { timeout: 30_000 });
}

/** On a phone, drill from the list into the trigger unless its detail is already pushed. */
async function drillIn(page: Page) {
  const row = dialogOf(page).locator('[data-trigger-id]');
  if (await row.isVisible()) await row.click();
}

const dialogOf = (page: Page) => page.locator('dialog[data-dialog-component="orchestration-trigger-dialog"]');

test('HS2-KXKA7X Triggers… sets up a dry run, records a delivery, turns it on, pauses, resumes and skips an overlap', async ({
  page,
}) => {
  test.setTimeout(360_000);
  const server = await realTicketServer({ seed: ({ env }) => syntheticTool(env.HOTSHEET_HOME!) });
  const base = `/checkouts/${server.checkoutId}/orchestration/triggers`;
  try {
    await orchestrateSelf(server);
    await checkInTemplate(server);
    await resizeTo(page, { width: 1440, height: 900 });
    await openProject(page, server);
    const toolbar = page.locator('[data-orchestration-toolbar="no_plan"]');
    await toolbar.getByRole('button', { name: 'Triggers…' }).click();
    const dialog = dialogOf(page);
    await expect(dialog).toBeVisible();
    await expect(dialog).toContainText('No Triggers Yet');

    // New Trigger…: nothing can be saved until it is named and runs something.
    await dialog.getByRole('button', { name: 'New Trigger…' }).click();
    await expect(dialog.getByRole('heading', { name: 'New Trigger' })).toBeVisible();
    const save = dialog.getByRole('button', { name: 'Save as Dry Run' });
    await expect(save).toBeDisabled();
    await dialog.getByLabel('Name', { exact: true }).fill('Nightly repair');
    await dialog.getByRole('combobox', { name: 'Template' }).click();
    await dialog.getByRole('option', { name: 'Repair in Self' }).click();
    await dialog.locator('[data-trigger-input="feature"]').getByLabel('Value').fill('Nightly');
    await expect(save).toBeEnabled();
    // The live review: what it changes and what asks.
    await expect(dialog.locator('[data-trigger-review]')).toContainText('Create Tickets in Self');
    await expect(dialog.locator('[data-trigger-review]')).toContainText('Asks through the permission prompt');
    await expect(dialog.locator('[data-trigger-review]')).toContainText('Launch (goal ticket) and Self tickets');
    await expect(dialog.locator('[data-trigger-review]')).toContainText('Push and Deploy');
    await captureSizes(page, 'setup', async () => {
      await expect(dialog.locator('[data-trigger-setup="new"]')).toBeVisible();
    });

    await save.click();
    await expect(dialog.getByRole('heading', { name: 'Nightly repair' })).toBeVisible();
    const row = dialog.locator('[data-trigger-id]');
    await expect(row.locator('[data-trigger-badge="dry_run"]')).toHaveText('Dry Run');
    await expect(dialog.locator('[data-trigger-banner="dry_run"]')).toContainText('0 events so far');
    const listed = await send(server, 'POST', `${base}/list`, { actor: person });
    const trigger = (listed.triggers as Array<{ trigger_id: string; state: string; source: { kind: string } }>)[0];
    expect(trigger).toMatchObject({ state: 'dry_run', source: { kind: 'schedule' } });

    // A manual delivery while it is a dry run records what would have run, and writes nothing.
    const fired = await send(server, 'POST', `${base}/fire`, {
      trigger_id: trigger.trigger_id,
      operation_id: 'manual-1',
      actor: person,
    });
    expect(fired.outcome).toBe('dry_run');
    const goalsBefore = (await server.request<Array<{ title: string }>>('/tickets')).length;
    await dialog.locator('[data-dialog-close]').first().click();
    await expect(dialog).toBeHidden();
    await toolbar.getByRole('button', { name: 'Triggers…' }).click();
    await expect(dialog.locator('[data-trigger-run="dry_run"]')).toContainText(
      'Run by hand · would open “Nightly in Self” with 1 step · would ask in Self',
    );
    await expect(dialog.locator('[data-trigger-banner="dry_run"]')).toContainText('1 event so far');
    await captureSizes(page, 'dry-run', async () => {
      await drillIn(page);
      await expect(dialog.getByRole('button', { name: 'Turn On…' })).toBeVisible();
    });

    // Turn On… asks for a confirmation, then records the review.
    await dismissToasts(page);
    await dialog.getByRole('button', { name: 'Turn On…' }).click();
    await page.getByRole('button', { name: 'Turn On', exact: true }).click();
    await expect(row.locator('[data-trigger-badge="on"]')).toHaveText('On');
    await expect(dialog.locator('[data-trigger-detail]')).toContainText('reviewed by you');
    await captureSizes(page, 'enabled', async () => {
      await drillIn(page);
      await expect(dialog.getByRole('button', { name: 'Pause' })).toBeVisible();
    });

    // Pause, then Resume back to On (its review still covers it).
    await dismissToasts(page);
    await dialog.getByRole('button', { name: 'Pause' }).click();
    await expect(row.locator('[data-trigger-badge="paused"]')).toHaveText('Paused');
    await dismissToasts(page);
    await dialog.getByRole('button', { name: 'Resume' }).click();
    await expect(row.locator('[data-trigger-badge="on"]')).toHaveText('On');
    const audit = await send(server, 'GET', `/checkouts/${server.checkoutId}/orchestration/audit`);
    const actions = (audit.entries as Array<{ action: string }>).map((entry) => entry.action);
    for (const action of ['trigger_set', 'trigger_on', 'trigger_pause', 'trigger_resume'])
      expect(actions).toContain(action);

    // On: a delivery applies the template as the trigger, which waits for a person's permission; the
    // next delivery while that run waits is skipped, not run twice.
    const applied = await send(server, 'POST', `${base}/fire`, {
      trigger_id: trigger.trigger_id,
      operation_id: 'manual-2',
      actor: person,
    });
    expect(applied).toMatchObject({ outcome: 'applied', reason: 'permission_required' });
    // A redelivery of the applied one is already handled.
    const again = await send(server, 'POST', `${base}/fire`, {
      trigger_id: trigger.trigger_id,
      operation_id: 'manual-2',
      actor: person,
    });
    expect(again.outcome).toBe('already_handled');
    const skipped = await send(server, 'POST', `${base}/fire`, {
      trigger_id: trigger.trigger_id,
      operation_id: 'manual-3',
      actor: person,
    });
    expect(skipped.outcome).toBe('skipped_overlap');
    expect((await server.request<Array<{ title: string }>>('/tickets')).length).toBe(goalsBefore);
    // The trigger's apply asks on the ordinary permission card; allowing it lets the server resume the apply
    // (HS2-AMJNER), which opens the goal the skipped delivery waits for.
    const card = page.locator('[data-component="permission-request-popup"]');
    await card.getByRole('button', { name: 'Allow Once' }).click();
    await expect(card).toBeHidden();
    await expect
      .poll(async () => (await server.request<Array<{ title: string }>>('/tickets')).length, { timeout: 30_000 })
      .toBe(goalsBefore + 1);
    await dismissToasts(page);
    await dialog.locator('[data-dialog-close]').first().click();
    await expect(dialog).toBeHidden();
    await toolbar.getByRole('button', { name: 'Triggers…' }).click();
    await expect(dialog.locator('[data-trigger-run="skipped_overlap"]')).toBeVisible();
    await expect(dialog.locator('[data-trigger-overlap]')).toContainText(
      'Skipped 1 Run: The Last Run Is Still Working',
    );
    const goal = dialog.locator('[data-trigger-overlap]').getByRole('button', { name: /^Open / });
    await expect(goal).toBeVisible();
    await expect(dialog.locator('[data-trigger-run="already_handled"]')).toContainText('already handled');
    // The applied run shows the goal its resumed apply opened.
    await expect(dialog.locator('[data-trigger-run="applied"]')).toContainText('Ran · goal ');
    await captureSizes(page, 'overlap', async () => {
      await drillIn(page);
      await expect(dialog.locator('[data-trigger-run="skipped_overlap"]')).toBeVisible();
    });
    // Open <goal> opens the goal ticket of the run the skip waits for, and closes the dialog.
    await goal.click();
    await expect(dialog).toBeHidden();
  } finally {
    await server.stop(page);
  }
});
