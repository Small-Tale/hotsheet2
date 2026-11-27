import { expect, type Locator, type Page, test } from '@playwright/test';

import { openMockedProject } from './open-mocked-project';
import { resizeTo } from './orchestration-real-fixtures';
import { proxyFetch, realTicketServer } from './real-ticket-server';

/**
 * HS2-1XCW6N: the visual template editor of Settings › Orchestration › Templates through the real shell
 * against a real server. The project orchestrates itself ("Self"). A new template is built part by part,
 * its problems are shown in place, it is saved through `templates/set` with the list's revision, a save
 * over a template changed elsewhere is refused and reloaded, and Retire asks first. Synthetic data only.
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

type Listed = { revision?: string; templates: Array<Record<string, unknown>> };
const list = async (server: Server) =>
  (await send(server, 'POST', `/checkouts/${server.checkoutId}/orchestration/templates/list`, {
    actor: person,
  })) as unknown as Listed;

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
  await expect(
    page.getByRole('group', { name: 'View mode' }).getByRole('button', { name: 'Orchestration View' }),
  ).toBeVisible();
}

async function showTemplatesSettings(page: Page) {
  await page.getByLabel('Settings View').click();
  await page
    .getByRole('navigation', { name: 'Project Settings' })
    .getByRole('button', { name: 'Orchestration', exact: true })
    .click();
  const section = page.locator('[data-component="orchestration-template-settings"]');
  await expect(section).toBeVisible();
  return section;
}

const capture = async (page: Page, name: string) => {
  await page.waitForFunction(() =>
    document
      .getAnimations()
      .every((animation) => animation.playState !== 'running' || animation.effect?.getTiming().iterations === Infinity),
  );
  await page.screenshot({ path: test.info().outputPath(`hs2-1xcw6n-${name}.png`) });
};

const editorOf = (page: Page) => page.locator('dialog[data-dialog-component="orchestration-template-editor"]');
const fieldOf = (editor: Locator, name: string) => editor.locator(`[data-template-field="${name}"]`);
const partOf = (editor: Locator, label: string | RegExp) =>
  editor.locator('[data-template-part]').filter({ hasText: label }).first();

/** Choose a Kerf Select's option by its label. */
async function choose(page: Page, select: Locator, option: string) {
  await select.getByRole('combobox').click();
  await page.getByRole('option', { name: option, exact: true }).click();
}

/** Type into a text field and leave it, which commits the value and checks the draft. */
async function fill(field: Locator, value: string) {
  const input = field.locator('input, textarea').first();
  await input.fill(value);
  await input.blur();
}

/**
 * HS2-324VN2: a select's problem is its own error: shown under it, `aria-invalid` on its combobox, and
 * read with it through `aria-describedby`.
 */
async function expectSelectError(editor: Locator, suffix: string, words: string) {
  const select = editor.locator(`[data-template-field$="${suffix}"]`);
  await expect(select).toContainText(words);
  const combobox = select.getByRole('combobox');
  await expect(combobox).toHaveAttribute('aria-invalid', 'true');
  await expect
    .poll(() =>
      combobox.evaluate((element) =>
        (element.getAttribute('aria-describedby') ?? '')
          .split(/\s+/)
          .filter(Boolean)
          .map((id) => document.getElementById(id)?.textContent ?? '')
          .join(' '),
      ),
    )
    .toContain(words);
}

const toast = (page: Page, text: string) => page.locator('.app-toast').filter({ hasText: text });

test('HS2-1XCW6N builds a new template part by part, shows its problems in place and saves it', async ({ page }) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    await orchestrateSelf(server);
    await resizeTo(page, { width: 1440, height: 900 });
    await openProject(page, server);
    const section = await showTemplatesSettings(page);
    await expect(section).toContainText('No templates yet.');
    await section.getByRole('button', { name: 'New Template' }).click();
    const editor = editorOf(page);
    await expect(editor.getByRole('heading', { name: 'Template', exact: true })).toBeVisible();
    // The first step starts in the one declared project.
    await expect(partOf(editor, '1 · step-1')).toContainText('Self');

    // Saving an empty template is refused with every problem in place.
    await editor.getByRole('button', { name: 'Save Template' }).click();
    await expect(toast(page, 'Fix 3 problems before saving.')).toBeVisible();
    await expect(editor).toContainText('Not Saved');
    await expect(fieldOf(editor, 'name')).toContainText('Name the template in one line');
    await expect(partOf(editor, 'Goal')).toContainText('1 problem');
    await expect(partOf(editor, '1 · step-1')).toContainText('1 problem');
    await capture(page, 'problems-1440-light');
    await fill(fieldOf(editor, 'name'), 'Rate limits');
    await expect(fieldOf(editor, 'name')).not.toContainText('Name the template');

    // A placeholder naming no input is a problem of the goal title until the input exists.
    await partOf(editor, 'Goal').click();
    await fill(fieldOf(editor, 'goal.title'), 'Rate limits: {{feature}}');
    await expect(fieldOf(editor, 'goal.title')).toContainText('names no input of this template');
    await editor.getByRole('button', { name: 'Add Input' }).click();
    await expect(editor.getByRole('heading', { name: 'input-1' })).toBeVisible();
    const label = editor.locator('[data-template-field$=".label"]');
    await fill(label, 'Feature');
    await fill(editor.locator('[data-template-field$=".key"]'), 'feature');
    await expect(partOf(editor, 'Goal')).not.toContainText('problem');
    await expect(partOf(editor, 'Feature')).toContainText('Text · required');

    // Step 1: its title and priority; then a second step that waits for it once it is finished.
    await partOf(editor, '1 · step-1').click();
    await fill(editor.locator('[data-template-field$=".title"]'), 'Add {{feature}} headers');
    await choose(page, editor.locator('[data-template-field$=".priority"]'), 'High');
    await expect(partOf(editor, '1 · Add {{feature}} headers')).not.toContainText('problem');
    await editor.getByRole('button', { name: 'Add Step' }).click();
    await expect(editor.getByRole('heading', { name: '2 · step-2' })).toBeVisible();
    await fill(editor.locator('[data-template-field$=".title"]'), 'Show {{feature}} errors');
    await expect(editor.locator('[data-template-after="0"]')).toContainText('1 · Add {{feature}} headers');
    await choose(page, editor.locator('[data-template-field$=".after.0.gates"]'), 'Finished');
    await editor.locator('[data-template-part-actions]').scrollIntoViewIfNeeded();
    await capture(page, 'step-1440-light');

    await editor.getByRole('button', { name: 'Save Template' }).click();
    await expect(toast(page, 'Saved Rate limits.')).toBeVisible();
    await expect(editor).toBeHidden();
    const row = section.locator('[data-orchestration-template]').filter({ hasText: 'Rate limits' });
    await expect(row).toContainText('1 input · 2 steps · version 1');
    const saved = (await list(server)).templates[0];
    expect(saved).toMatchObject({
      name: 'Rate limits',
      state: 'active',
      version: 1,
      inputs: [{ key: 'feature', label: 'Feature', kind: 'text', required: true }],
      goal: { title: 'Rate limits: {{feature}}' },
      steps: [
        { key: 'step-1', target: TARGET, title: 'Add {{feature}} headers', priority: 'high' },
        {
          key: 'step-2',
          target: TARGET,
          title: 'Show {{feature}} errors',
          after: [{ step: 'step-1', gates: 'none' }],
        },
      ],
    });
    // Use Template… offers it at once.
    await page.getByRole('group', { name: 'View mode' }).getByRole('button', { name: 'Orchestration View' }).click();
    await page.getByRole('button', { name: 'Use Template…' }).first().click();
    const use = page.locator('dialog[data-dialog-component="orchestration-template-dialog"]');
    await expect(use.getByRole('heading', { name: 'Rate limits' })).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(use).toBeHidden();
  } finally {
    await server.stop(page);
  }
});

test('HS2-1XCW6N edits a template with compare-and-set, reloads after a change elsewhere, and retires it', async ({
  page,
}) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    await orchestrateSelf(server);
    const base = `/checkouts/${server.checkoutId}/orchestration/templates`;
    const definition = {
      name: 'Feature in Self',
      inputs: [
        { key: 'feature', label: 'Feature', kind: 'text', required: true },
        { key: 'docs', label: 'Write Docs', kind: 'boolean', default: true },
      ],
      goal: { title: '{{feature}} in Self' },
      steps: [
        { key: 'build', target: TARGET, title: 'Build {{feature}}' },
        { key: 'docs', target: TARGET, title: 'Document {{feature}}', when: '{{docs}}', after: [{ step: 'build' }] },
      ],
    };
    const set = await send(server, 'POST', `${base}/set`, { templates: [definition], actor: person });
    const templateId = (set.templates as Array<{ template_id: string }>)[0].template_id;
    await resizeTo(page, { width: 1440, height: 900 });
    await openProject(page, server);
    const section = await showTemplatesSettings(page);
    const row = section.locator(`[data-orchestration-template="${templateId}"]`);
    await expect(row).toContainText('2 inputs · 2 steps · version 1');
    await row.locator('button[data-item-id]').click();
    const editor = editorOf(page);
    await expect(fieldOf(editor, 'name').locator('input')).toHaveValue('Feature in Self');
    // Unchanged: nothing to save.
    await expect(editor.getByRole('button', { name: 'Save Template' })).toBeDisabled();
    await partOf(editor, '2 · Document {{feature}}').click();
    await expect(editor.locator('[data-template-field$=".when"]')).toContainText('When Write Docs Is On');
    await partOf(editor, 'Template').click();
    await fill(fieldOf(editor, 'name'), 'Feature in Self, renamed');

    // Meanwhile the template changes elsewhere: the save is refused rather than overwriting it.
    await send(server, 'POST', `${base}/set`, {
      templates: [{ ...definition, template_id: templateId, description: 'Changed by another session.' }],
      actor: person,
    });
    await editor.getByRole('button', { name: 'Save Template' }).click();
    await expect(editor).toContainText('Changed Elsewhere');
    await expect(toast(page, 'The templates changed meanwhile.')).toBeVisible();
    await capture(page, 'conflict-1440-light');
    await editor.getByRole('button', { name: 'Reload Templates' }).click();
    await expect(editor).not.toContainText('Changed Elsewhere');
    await expect(fieldOf(editor, 'name').locator('input')).toHaveValue('Feature in Self');
    await expect(fieldOf(editor, 'description').locator('textarea')).toHaveValue('Changed by another session.');
    // Edited again over the latest revision, it saves as the next version.
    await fill(fieldOf(editor, 'name'), 'Feature in Self, renamed');
    await editor.getByRole('button', { name: 'Save Template' }).click();
    await expect(toast(page, 'Saved Feature in Self, renamed.')).toBeVisible();
    await expect(row).toContainText('version 3');
    expect((await list(server)).templates[0]).toMatchObject({
      name: 'Feature in Self, renamed',
      description: 'Changed by another session.',
      version: 3,
    });

    // Cancel with changes asks first; Keep Editing keeps the draft, Discard Changes drops it.
    await row.locator('button[data-item-id]').click();
    await fill(fieldOf(editor, 'name'), 'Not kept');
    await editor.getByRole('button', { name: 'Cancel' }).click();
    const confirm = page.getByRole('alertdialog');
    await expect(confirm).toContainText('Discard your changes?');
    await confirm.getByRole('button', { name: 'Discard Changes' }).click();
    await expect(editor).toBeHidden();
    expect((await list(server)).templates[0].name).toBe('Feature in Self, renamed');

    // Retire asks first, then the row says Retired and the template opens read-only.
    await row.getByRole('button', { name: /Actions for Feature in Self/ }).click();
    await page.getByRole('menuitem', { name: 'Retire Template…' }).click();
    await expect(confirm).toContainText('Retire Feature in Self, renamed?');
    await capture(page, 'retire-1440-light');
    await confirm.getByRole('button', { name: 'Retire Template' }).click();
    await expect(toast(page, 'Retired Feature in Self, renamed.')).toBeVisible();
    await expect(row).toContainText('Retired');
    expect((await list(server)).templates[0].state).toBe('retired');
    await row.locator('button[data-item-id]').click();
    await expect(editor).toContainText('A retired template can no longer be applied');
    await expect(fieldOf(editor, 'name').locator('input')).toBeDisabled();
    await expect(editor.getByRole('button', { name: 'Save Template' })).toHaveCount(0);
    await editor.locator('[data-dialog-close]').filter({ hasText: 'Close' }).click();
    await expect(editor).toBeHidden();
  } finally {
    await server.stop(page);
  }
});

test('HS2-1XCW6N the Templates section and editor fit at wide, medium and phone widths in both schemes', async ({
  page,
}) => {
  test.setTimeout(180_000);
  const server = await realTicketServer();
  try {
    await orchestrateSelf(server);
    await send(server, 'POST', `/checkouts/${server.checkoutId}/orchestration/templates/set`, {
      templates: [
        {
          name: 'Feature in Self',
          description: 'Build the feature, then document it on top of its green CI.',
          inputs: [
            { key: 'feature', label: 'Feature', kind: 'text', required: true, hint: 'Such as Rate limits' },
            { key: 'docs', label: 'Write Docs', kind: 'boolean', default: true },
          ],
          goal: { title: '{{feature}} in Self' },
          steps: [
            { key: 'build', target: TARGET, title: 'Build {{feature}}', priority: 'high', validation: ['smoke'] },
            {
              key: 'docs',
              target: TARGET,
              title: 'Document {{feature}} for the release notes of this quarter',
              when: '{{docs}}',
              after: [{ step: 'build', gates: 'ci' }],
            },
          ],
        },
      ],
      actor: person,
    });
    await resizeTo(page, { width: 1440, height: 900 });
    await openProject(page, server);
    for (const scheme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      const section = await showTemplatesSettings(page);
      for (const [index, size] of [
        { width: 1440, height: 900 },
        { width: 1100, height: 900 },
        { width: 390, height: 844 },
        { width: 1440, height: 900 },
      ].entries()) {
        await resizeTo(page, size);
        const suffix = `${String(size.width)}${index === 3 ? '-after-390' : ''}-${scheme}`;
        const row = section.locator('[data-orchestration-template]').first();
        await row.scrollIntoViewIfNeeded();
        await expect(row).toBeVisible();
        await capture(page, `settings-${suffix}`);
        await row.locator('button[data-item-id]').click();
        const editor = editorOf(page);
        await expect(editor).toBeVisible();
        if (size.width === 390) {
          // The compact drill-down: the parts first, then one part over them.
          await partOf(editor, '2 · Document').click();
          await expect(editor.getByRole('button', { name: 'Back to Parts' })).toBeVisible();
        } else await partOf(editor, '2 · Document').click();
        await expect(editor.locator('[data-template-field$=".when"]')).toContainText('When Write Docs Is On');
        // Nothing spills past the viewport sideways.
        const fits = await editor.evaluate((dialog) => {
          const box = dialog.getBoundingClientRect();
          return box.left >= 0 && box.right <= window.innerWidth + 0.5 && dialog.scrollWidth <= dialog.clientWidth + 1;
        });
        expect(fits).toBe(true);
        await capture(page, `editor-step-${suffix}`);
        await editor.locator('[data-template-part-actions]').scrollIntoViewIfNeeded();
        const waits = await editor.locator('[data-template-after="0"]').evaluate((row) => {
          const box = row.getBoundingClientRect(),
            pane = row.closest('dialog')!.getBoundingClientRect();
          return box.left >= pane.left && box.right <= pane.right + 0.5;
        });
        expect(waits).toBe(true);
        await capture(page, `editor-waits-${suffix}`);
        await editor.locator('[data-dialog-close]').filter({ hasText: 'Cancel' }).click();
        await expect(editor).toBeHidden();
      }
    }
  } finally {
    await server.stop(page);
  }
});

test('HS2-1XCW6N the catalog demo runs the production editor and shows each editor state', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=orchestration-settings');
  const examples = page.getByRole('region', { name: 'OrchestrationSettings Examples' });
  const interactive = examples.getByRole('region', { name: 'Interactive' });
  const rows = interactive.locator('[data-orchestration-template]');
  await expect(rows).toHaveCount(2);
  await expect(rows.nth(1)).toContainText('Retired');
  // New Template opens the production editor (modal); a new template saves and is listed.
  await interactive.getByRole('button', { name: 'New Template' }).click();
  const modal = page.locator(
    'dialog[data-dialog-component="orchestration-template-editor"][data-presentation="modal"]',
  );
  await expect(modal).toBeVisible();
  await fill(fieldOf(modal, 'name'), 'Demo template');
  await partOf(modal, 'Goal').click();
  await fill(fieldOf(modal, 'goal.title'), 'Demo goal');
  await partOf(modal, '1 · step-1').click();
  await fill(modal.locator('[data-template-field$=".title"]'), 'Demo step');
  await modal.getByRole('button', { name: 'Save Template' }).click();
  await expect(modal).toBeHidden();
  await expect(rows).toHaveCount(3);
  await expect(rows.filter({ hasText: 'Demo template' })).toContainText('0 inputs · 1 step · version 1');
  // Every in-place editor state renders.
  for (const label of [
    'Template Editor: Template',
    'Template Editor: Goal',
    'Template Editor: Input',
    'Template Editor: Step',
    'Template Editor: New Template With Problems',
    'Template Editor: Step Problems',
    'Template Editor: Changed Elsewhere',
    'Template Editor: Saving',
    'Template Editor: Retired',
    'Template Editor: Phone',
    'Templates: None Yet',
    'Templates: Loading',
    'Templates: Unreadable',
    'Templates: Retiring',
  ])
    await expect(examples.getByRole('region', { name: label, exact: true })).toBeVisible();
  await expect(examples.getByRole('region', { name: 'Template Editor: Step Problems' })).toContainText(
    'The steps wait for each other in a circle.',
  );
  expect(errors).toEqual([]);
});

test('HS2-4EBCNH an open select in a step stays open while a validation answer removes the problem banner above it', async ({
  page,
}) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    await orchestrateSelf(server);
    await resizeTo(page, { width: 1440, height: 900 });
    await openProject(page, server);
    const section = await showTemplatesSettings(page);
    await section.getByRole('button', { name: 'New Template' }).click();
    const editor = editorOf(page);
    await partOf(editor, '1 · step-1').click();
    // Validation answers carry a problem for the step's project (shown in the part's banner) until `problem` is
    // cleared; the next answer is then held until the priority select is open, and removes the banner.
    let problem = true;
    let release: (() => void) | undefined;
    const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
    await page.route('**/orchestration/templates/validate', async (route) => {
      if (!problem)
        await new Promise<void>((resolve) => {
          release = resolve;
        });
      const incoming = new URL(route.request().url());
      const response = await proxyFetch(route, {
        url: `${server.url}${incoming.pathname.slice(apiPath.length)}`,
        headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
        timeout: 60_000,
      });
      const body = (await response.json()) as { problems?: unknown[] };
      const added = problem
        ? [
            { code: 'target_unknown', at: 'steps[0].target' },
            { code: 'step_cycle', at: 'steps.step-1' },
          ]
        : [];
      await route.fulfill({ response, json: { ...body, problems: [...(body.problems ?? []), ...added] } });
    });
    const title = editor.locator('[data-template-field$=".title"]');
    await fill(title, 'Add headers');
    const banner = editor.locator('[data-template-part-problems]');
    await expect(banner).toBeVisible();
    // HS2-324VN2: the project's problem sits under its select; the banner keeps only the circular wait.
    await expectSelectError(editor, '.target', 'This project is not one this orchestration project includes.');
    await expect(banner).toContainText('The steps wait for each other in a circle.');
    await expect(banner).not.toContainText('This project is not one');
    problem = false;
    await fill(title, 'Add rate limit headers');
    await expect.poll(() => release !== undefined).toBe(true);
    const priority = editor.locator('[data-template-field$=".priority"]').getByRole('combobox');
    await priority.click();
    await expect(page.getByRole('option', { name: 'High', exact: true })).toBeVisible();
    release?.();
    await expect(banner).toHaveCount(0);
    await expect(editor.locator('[data-template-field$=".target"]').getByRole('combobox')).not.toHaveAttribute(
      'aria-invalid',
      'true',
    );
    await expect(priority).toHaveAttribute('aria-expanded', 'true');
    await page.getByRole('option', { name: 'High', exact: true }).click();
    await expect(priority).toContainText('High');
    await page.unroute('**/orchestration/templates/validate');
  } finally {
    await server.stop(page);
  }
});

test('HS2-324VN2 step and input select problems sit under their selects at every width in both schemes', async ({
  page,
}) => {
  test.setTimeout(180_000);
  const server = await realTicketServer();
  try {
    await orchestrateSelf(server);
    await send(server, 'POST', `/checkouts/${server.checkoutId}/orchestration/templates/set`, {
      templates: [
        {
          name: 'Feature in Self',
          inputs: [
            { key: 'feature', label: 'Feature', kind: 'text', required: true },
            { key: 'urgency', label: 'Urgency', kind: 'priority' },
          ],
          goal: { title: '{{feature}} in Self' },
          steps: [
            { key: 'build', target: TARGET, title: 'Build {{feature}}' },
            { key: 'docs', target: TARGET, title: 'Document {{feature}}', after: [{ step: 'build', gates: 'ci' }] },
          ],
        },
      ],
      actor: person,
    });
    await resizeTo(page, { width: 1440, height: 900 });
    await openProject(page, server);
    // Registered after the project's proxy, so it answers first.
    // Every validation answer adds select problems of the second step and the second input, and the
    // step's circular wait, which no control shows.
    const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
    await page.route('**/orchestration/templates/validate', async (route) => {
      const incoming = new URL(route.request().url());
      const response = await proxyFetch(route, {
        url: `${server.url}${incoming.pathname.slice(apiPath.length)}`,
        headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
        timeout: 60_000,
      });
      const body = (await response.json()) as { problems?: unknown[] };
      await route.fulfill({
        response,
        json: {
          ...body,
          problems: [
            ...(body.problems ?? []),
            { code: 'target_unknown', at: 'steps[1].target' },
            { code: 'step_gates_invalid', at: 'steps[1].after[0].gates' },
            { code: 'step_cycle', at: 'steps.docs' },
            { code: 'input_default_invalid', at: 'inputs[1].default' },
          ],
        },
      });
    });
    let edits = 0;
    for (const scheme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      const section = await showTemplatesSettings(page);
      for (const [index, size] of [
        { width: 1440, height: 900 },
        { width: 1100, height: 900 },
        { width: 390, height: 844 },
        { width: 1440, height: 900 },
      ].entries()) {
        await resizeTo(page, size);
        const suffix = `${String(size.width)}${index === 3 ? '-after-390' : ''}-${scheme}`;
        const row = section.locator('[data-orchestration-template]').first();
        await row.scrollIntoViewIfNeeded();
        await row.locator('button[data-item-id]').click();
        const editor = editorOf(page);
        await expect(editor).toBeVisible();
        await partOf(editor, '2 · Document').click();
        edits += 1;
        await fill(editor.locator('[data-template-field$=".title"]'), `Document {{feature}} ${String(edits)}`);
        await expectSelectError(editor, '.target', 'This project is not one this orchestration project includes.');
        await expectSelectError(editor, '.after.0.gates', 'Choose what to wait for.');
        const banner = editor.locator('[data-template-part-problems]');
        await expect(banner).toContainText('1 Problem');
        await expect(banner).toContainText('The steps wait for each other in a circle.');
        await expect(banner).not.toContainText('This project is not one');
        await expect(banner).not.toContainText('Choose what to wait for.');
        const fits = await editor.evaluate((dialog) => {
          const box = dialog.getBoundingClientRect();
          return box.left >= 0 && box.right <= window.innerWidth + 0.5 && dialog.scrollWidth <= dialog.clientWidth + 1;
        });
        expect(fits).toBe(true);
        await capture(page, `select-errors-step-${suffix}`);
        await editor.locator('[data-template-after="0"]').scrollIntoViewIfNeeded();
        await capture(page, `select-errors-waits-${suffix}`);
        if (size.width === 390) await editor.getByRole('button', { name: 'Back to Parts' }).click();
        await partOf(editor, 'Urgency').click();
        await expectSelectError(editor, '.default', 'This default is not a value the input accepts.');
        await expect(editor.locator('[data-template-part-problems]')).toHaveCount(0);
        await capture(page, `select-errors-input-${suffix}`);
        await editor.locator('[data-dialog-close]').filter({ hasText: 'Cancel' }).click();
        // The edited title is discarded after asking.
        await page.getByRole('button', { name: 'Discard Changes', exact: true }).click();
        await expect(editor).toBeHidden();
      }
    }
    await page.unroute('**/orchestration/templates/validate');
  } finally {
    await server.stop(page);
  }
});
