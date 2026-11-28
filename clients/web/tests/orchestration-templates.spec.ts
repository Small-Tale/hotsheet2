import { writeFile } from 'node:fs/promises';
import { join } from 'node:path';

import { expect, type Page, test } from '@playwright/test';

import { openMockedProject } from './open-mocked-project';
import { resizeTo, syntheticTool } from './orchestration-real-fixtures';
import { proxyFetch, realTicketServer } from './real-ticket-server';

/**
 * HS2-A4RVYA: Use Template… through the real shell against a real server. The project orchestrates itself
 * ("Self"); a template is checked in through the server, applied through the shipped Orchestration header,
 * and the planner (the synthetic tool, a real terminal) takes the new goal. Synthetic tickets only.
 */
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

/** The design's template, checked in through the server: a step that searches first, then a create step. */
async function checkInTemplate(server: Server) {
  const set = await send(server, 'POST', `/checkouts/${server.checkoutId}/orchestration/templates/set`, {
    templates: [
      {
        name: 'Feature in Self',
        description: 'Build the feature, then document it on top of its green CI.',
        inputs: [
          { key: 'feature', label: 'Feature', kind: 'text', required: true },
          { key: 'project', label: 'Project', kind: 'target', default: TARGET },
          { key: 'priority', label: 'Priority', kind: 'priority', default: 'high' },
        ],
        goal: { title: '{{feature}} in Self' },
        steps: [
          { key: 'build', target: '{{project}}', title: 'Build {{feature}}', priority: '{{priority}}' },
          {
            key: 'docs',
            target: '{{project}}',
            title: 'Document {{feature}}',
            ticket: 'create',
            after: [{ step: 'build', gates: 'ci' }],
          },
        ],
      },
    ],
    actor: person,
  });
  return (set.templates as Array<{ template_id: string }>)[0].template_id;
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

const capture = async (page: Page, name: string) => {
  await page.waitForFunction(() =>
    document
      .getAnimations()
      .every((animation) => animation.playState !== 'running' || animation.effect?.getTiming().iterations === Infinity),
  );
  await page.screenshot({ path: test.info().outputPath(`hs2-a4rvya-${name}.png`) });
};

const dialogOf = (page: Page) => page.locator('dialog[data-dialog-component="orchestration-template-dialog"]');

test('HS2-A4RVYA Use Template… previews a template, applies it once and hands the new goal to the planner', async ({
  page,
}) => {
  test.setTimeout(120_000);
  // The planner runs in an installed tool (Auto); the synthetic tool keeps that independent of the host.
  const server = await realTicketServer({ seed: ({ env }) => syntheticTool(env.HOTSHEET_HOME!) });
  try {
    await orchestrateSelf(server);
    const templateId = await checkInTemplate(server);
    await resizeTo(page, { width: 1440, height: 900 });
    await openProject(page, server);
    const toolbar = page.locator('[data-orchestration-toolbar="no_plan"]');
    // Offered in the header and in the No Plan Yet empty state, beside Propose Plan.
    await expect(
      page.locator('[data-orchestration-empty="no-plan"]').getByRole('button', { name: 'Use Template…' }),
    ).toBeVisible();
    await toolbar.getByRole('button', { name: 'Use Template…' }).click();
    const dialog = dialogOf(page);
    await expect(dialog).toBeVisible();
    await expect(dialog.getByRole('heading', { name: 'Feature in Self' })).toBeVisible();
    // A required input is empty: Apply stays disabled and the banner says why.
    const apply = dialog.getByRole('button', { name: 'Apply Template' });
    await expect(apply).toBeDisabled();
    await expect(dialog).toContainText('1 Problem Keeps Apply Disabled');
    await expect(dialog).toContainText('Feature needs a value.');
    // The server's pre-run summary: the steps, the project's ticket writes ask, propose first.
    await expect(dialog.locator('[data-template-step="docs"]')).toContainText(
      'Create “Document ” · after step 1 once its CI passes',
    );
    await expect(dialog).toContainText('Asks through the permission prompt');
    await expect(dialog).toContainText('Proposes a plan for the new goal (Propose Plan)');
    await capture(page, 'problems-1440-light');

    const feature = dialog.getByLabel('Feature', { exact: true });
    await feature.fill('Rate limits');
    // Apply is enabled at once; a pause in typing reads the summary again, and leaving the field asks
    // nothing more.
    await expect(apply).toBeEnabled();
    await expect(dialog.locator('[data-template-step="build"]')).toContainText(
      'Find or create “Build Rate limits” · high priority',
    );
    // Switch the search-first step to create; the summary follows.
    await dialog.getByLabel('Create a New Ticket for Step 1').check();
    await expect(dialog.locator('[data-template-step="build"]')).toContainText('Create “Build Rate limits”');
    await dialog.getByLabel('Create a New Ticket for Step 1').uncheck();
    await expect(dialog.locator('[data-template-step="build"]')).toContainText('Find or create “Build Rate limits”');
    await expect(dialog).not.toContainText('Keeps Apply Disabled');
    await capture(page, 'use-1440-light');
    await page.emulateMedia({ colorScheme: 'dark' });
    await capture(page, 'use-1440-dark');
    await page.emulateMedia({ colorScheme: 'light' });
    await resizeTo(page, { width: 1100, height: 800 });
    await expect(dialog).toBeVisible();
    await capture(page, 'use-1100-light');
    // A phone shows the list first (the dialog stays open across the change), drills into the detail
    // and back.
    await resizeTo(page, { width: 390, height: 844 });
    const row = dialog.getByRole('button', { name: /Feature in Self/ });
    await expect(row).toBeVisible();
    await capture(page, 'list-390-light');
    await row.click();
    await expect(dialog.getByRole('button', { name: 'Apply Template' })).toBeVisible();
    // Selecting the template again starts from its defaults.
    await expect(dialog.getByLabel('Feature', { exact: true })).toHaveValue('');
    await dialog.getByLabel('Feature', { exact: true }).fill('Rate limits');
    await expect(dialog.locator('[data-template-step="build"]')).toContainText('“Build Rate limits”');
    await dialog.getByLabel('Feature', { exact: true }).blur();
    await capture(page, 'use-390-light');
    await page.emulateMedia({ colorScheme: 'dark' });
    await capture(page, 'use-390-dark');
    await page.emulateMedia({ colorScheme: 'light' });
    await dialog.getByRole('button', { name: 'Back to Templates' }).click();
    await expect(row).toBeVisible();
    await row.click();
    await dialog.getByLabel('Feature', { exact: true }).fill('Rate limits');
    await resizeTo(page, { width: 1440, height: 900 });

    await dialog.getByRole('button', { name: 'Apply Template' }).click();
    await expect(dialog).toContainText('The Planner Is Adapting the Template');
    const listed = await send(server, 'POST', `/checkouts/${server.checkoutId}/orchestration/templates/list`, {
      actor: person,
    });
    const applied = (listed.applies as Array<{ state: string; template_id: string; goal?: { slug?: string } }>)[0];
    // The planner took it once: the apply is recorded as planning, under the template applied.
    expect(applied).toMatchObject({ state: 'planning', template_id: templateId });
    const slug = applied.goal?.slug ?? '';
    // The goal ticket reaches the listing once the index catches up with the store (HS2-N91NYP).
    await expect
      .poll(
        async () =>
          (await server.request<Array<{ slug: string; title: string }>>('/tickets')).find(
            (ticket) => ticket.slug === slug,
          )?.title,
        { timeout: 15_000 },
      )
      .toBe('Rate limits in Self');
    await expect(dialog).toContainText(slug);
    await capture(page, 'applied-1440-light');
    await page.emulateMedia({ colorScheme: 'dark' });
    await capture(page, 'applied-1440-dark');
    await page.emulateMedia({ colorScheme: 'light' });
    const planning = await send(server, 'POST', `/checkouts/${server.checkoutId}/orchestration/planning/show`, {});
    expect(planning.state).toBe('planning');
    // Open the new goal: the dialog closes and the goal ticket opens.
    await dialog.getByRole('button', { name: `Open ${slug}` }).click();
    await expect(dialog).toBeHidden();
  } finally {
    await server.stop(page);
  }
});

test('HS2-A4RVYA Save a Goal as Template… saves a goal’s plan steps and selects the new template', async ({ page }) => {
  test.setTimeout(120_000);
  const server = await realTicketServer({ seed: ({ env }) => syntheticTool(env.HOTSHEET_HOME!) });
  try {
    await orchestrateSelf(server);
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    const goal = await server.request<{ id: string; slug: string }>('/tickets', 'POST', {
      title: 'Ship the synthetic sign-on',
    });
    const first = await server.request<{ id: string; connection_id: string }>('/tickets', 'POST', {
      title: 'Add the synthetic token endpoint',
    });
    const second = await server.request<{ id: string }>('/tickets', 'POST', { title: 'Add the synthetic login page' });
    const define = (native: string, prerequisites: object[]) =>
      send(server, 'POST', `${base}/readiness/define`, {
        ticket: { native_id: goal.id },
        target_id: TARGET,
        connection_id: first.connection_id,
        native_id: native,
        prerequisites,
        actor: { role: 'ai', id: 'planner-1' },
      });
    await define(first.id, []);
    await define(second.id, [
      { kind: 'ticket', target_id: TARGET, connection_id: first.connection_id, native_id: first.id, gates: 'none' },
    ]);
    await resizeTo(page, { width: 1440, height: 900 });
    await openProject(page, server);
    const toolbar = page.locator('[data-orchestration-toolbar="proposed"]');
    await toolbar.getByRole('button', { name: 'Use Template…' }).click();
    const dialog = dialogOf(page);
    await expect(dialog).toContainText('No Templates Yet');
    await capture(page, 'empty-1440-light');
    await dialog.getByRole('button', { name: 'Save a Goal as Template…' }).click();
    await expect(dialog.getByRole('heading', { name: 'Save a Goal as Template' })).toBeVisible();
    // HS2-M7RXCB: the goal is named by its title, then its slug and step count.
    await expect(dialog.locator('[data-template-save]')).toContainText(
      `Ship the synthetic sign-on · ${goal.slug} · 2 steps`,
    );
    const save = dialog.getByRole('button', { name: 'Save Template' });
    await expect(save).toBeDisabled();
    await expect(dialog).toContainText('Name the template.');
    await dialog.getByLabel('Template Name').fill('Sign-on again');
    await dialog.getByLabel('Make Each Project an Input').check();
    await expect(save).toBeEnabled();
    await capture(page, 'save-1440-light');
    await save.click();
    await expect(dialog.getByRole('heading', { name: 'Sign-on again' })).toBeVisible();
    await expect(dialog.locator('[data-template-step="step-2"]')).toContainText(
      'Find or create “Add the synthetic login page” · after step 1 once it is finished',
    );
    const listed = await send(server, 'POST', `${base}/templates/list`, { actor: person });
    const saved = (listed.templates as Array<{ name: string; steps: unknown[]; inputs: Array<{ kind: string }> }>)[0];
    expect(saved.name).toBe('Sign-on again');
    expect(saved.steps).toHaveLength(2);
    expect(saved.inputs.map((input) => input.kind)).toEqual(['target']);
    // Escape closes it; nothing else changed.
    await page.keyboard.press('Escape');
    await expect(dialog).toBeHidden();
  } finally {
    await server.stop(page);
  }
});

test('HS2-DTP7QR an open select in Use Template… stays open while a preview answer removes a problem above it', async ({
  page,
}) => {
  test.setTimeout(120_000);
  const server = await realTicketServer({ seed: ({ env }) => syntheticTool(env.HOTSHEET_HOME!) });
  try {
    await orchestrateSelf(server);
    await checkInTemplate(server);
    await resizeTo(page, { width: 1440, height: 900 });
    await openProject(page, server);
    await page.locator('[data-orchestration-toolbar="no_plan"]').getByRole('button', { name: 'Use Template…' }).click();
    const dialog = dialogOf(page);
    await expect(dialog).toContainText('1 Problem Keeps Apply Disabled');
    // Preview answers carry a problem for the project input until `clear`; the next one is held until a select is
    // open, then answers without it, which removes the banner above the inputs.
    let problem = true;
    let release: (() => void) | undefined;
    await page.route('**/orchestration/templates/preview', async (route) => {
      if (!problem)
        await new Promise<void>((resolve) => {
          release = resolve;
        });
      const incoming = new URL(route.request().url());
      const response = await proxyFetch(route, {
        url: `${server.url}${incoming.pathname.slice(`/__hotsheet/project-api/${server.checkoutId}`.length)}`,
        headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
        timeout: 60_000,
      });
      const body = (await response.json()) as { problems?: unknown[] };
      const problems = problem ? [{ code: 'input_invalid', at: 'inputs.project' }] : [];
      await route.fulfill({ response, json: { ...body, problems: [...(body.problems ?? []), ...problems] } });
    });
    const feature = dialog.getByLabel('Feature', { exact: true });
    await feature.fill('Rate limits');
    const banner = dialog.locator('[data-template-detail] > :not([hidden])').first();
    await expect(banner).toContainText('1 Problem Keeps Apply Disabled');
    await expect(banner).toContainText('Project is not a value it accepts.');
    // The next answer is held and clears the problem; the person opens the project select meanwhile.
    problem = false;
    await feature.fill('Rate limits, again');
    await expect.poll(() => release !== undefined).toBe(true);
    const project = dialog.locator('[data-template-input="project"]').getByRole('combobox', { name: 'Project' });
    await project.click();
    await expect(page.getByRole('option', { name: 'Self', exact: true })).toBeVisible();
    release?.();
    await expect(dialog).not.toContainText('Keeps Apply Disabled');
    await expect(project).toHaveAttribute('aria-expanded', 'true');
    await expect(page.getByRole('option', { name: 'Self', exact: true })).toBeVisible();
    await page.keyboard.press('Escape');
    await page.unroute('**/orchestration/templates/preview');
  } finally {
    await server.stop(page);
  }
});

test('HS2-4BQACQ Use Template… says why the machine refused the planner, with its limit', async ({ page }) => {
  test.setTimeout(120_000);
  // The machine's own admission file allows no AI session, so the planner's start is refused (no real load).
  const server = await realTicketServer({
    seed: async ({ env }) => {
      await syntheticTool(env.HOTSHEET_HOME!);
      await writeFile(
        join(env.HOTSHEET_HOME!, 'orchestration-admission.json'),
        JSON.stringify({ schema_version: 1, admission: { max_sessions: 0 } }),
      );
    },
  });
  try {
    await orchestrateSelf(server);
    await checkInTemplate(server);
    await resizeTo(page, { width: 1440, height: 900 });
    await openProject(page, server);
    await page.locator('[data-orchestration-toolbar="no_plan"]').getByRole('button', { name: 'Use Template…' }).click();
    const dialog = dialogOf(page);
    await dialog.getByLabel('Feature', { exact: true }).fill('Rate limits');
    const apply = dialog.getByRole('button', { name: 'Apply Template' });
    await expect(apply).toBeEnabled();
    await apply.click();
    await expect(dialog).toContainText('Waiting for the Planner');
    await expect(dialog).toContainText('the machine did not start the planner.');
    await expect(dialog).toContainText('The machine’s session limit is 0, so no AI session may start.');
    await expect(dialog).toContainText('Settings › Orchestration › Machine Defaults');
    await expect(dialog).not.toContainText('session admission refused');
    // The apply record keeps the refusal's reason and limit, so a later read says the same.
    const listed = await send(server, 'POST', `/checkouts/${server.checkoutId}/orchestration/templates/list`, {
      actor: person,
    });
    const [{ operation_id }] = listed.applies as Array<{ operation_id: string }>;
    // HS2-DMA5DR: the server's resume pass never asks again for an apply still asking, so the record is settled at once.
    const shown = await send(server, 'POST', `/checkouts/${server.checkoutId}/orchestration/templates/apply_show`, {
      operation_id,
    });
    expect(shown).toMatchObject({
      state: 'waiting_planner',
      attempts: 1,
      proposal_id: operation_id,
      reason: 'session_admission_refused',
      admission: { reason: 'session_cap_reached', setting: 'max_sessions', limit: 0 },
    });
    await capture(page, 'refused-1440-light');
    await page.emulateMedia({ colorScheme: 'dark' });
    await capture(page, 'refused-1440-dark');
    await page.emulateMedia({ colorScheme: 'light' });
    await resizeTo(page, { width: 390, height: 844 });
    await expect(dialog).toContainText('The machine’s session limit is 0, so no AI session may start.');
    await capture(page, 'refused-390-light');
  } finally {
    await server.stop(page);
  }
});
