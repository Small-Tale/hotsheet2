import { execFile } from 'node:child_process';
import { existsSync } from 'node:fs';
import { mkdir } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { promisify } from 'node:util';

import { expect, type Page, test } from '@playwright/test';

import { proxyFetch, realTicketServer, routeProjectChangeSocket } from './real-ticket-server';

/**
 * HS2-W27N1D: Project Type and Settings → Orchestration through the real shell, against a real
 * isolated server with three registered checkouts. Only the bridge's project-open answer is
 * routed; every orchestration read and write reaches the real Rust workflows.
 */
async function openProjects(
  page: Page,
  options: {
    /** More checkouts of the same ticket store, after Web and Docs. */
    names?: readonly string[];
    /** HS2-E9VVQF: checkouts of a second, separate ticket store. */
    otherStore?: readonly string[];
    /** How many projects open as tabs at first; Launch and Web by default. */
    open?: number;
  } = {},
) {
  const other = { store: '' };
  const server = await realTicketServer(
    options.otherStore?.length
      ? {
          // A second standalone store, initialised from a directory of its own so nothing else is linked.
          seed: async ({ root, env, cli }) => {
            const directory = join(dirname(root), 'other');
            await mkdir(directory, { recursive: true });
            other.store = join(dirname(root), 'other-tickets');
            await promisify(execFile)(
              cli,
              ['--test-fixture-credentials', 'init', '--standalone', '--at', other.store, '--prefix', 'OTH'],
              { cwd: directory, env },
            );
          },
        }
      : {},
  );
  const projects: { id: string; root: string; name: string }[] = [
    { id: server.checkoutId, root: server.root, name: 'Launch' },
  ];
  for (const [name, store] of [
    ...['Web', 'Docs', ...(options.names ?? [])].map((name) => [name, server.store] as const),
    ...(options.otherStore ?? []).map((name) => [name, other.store] as const),
  ]) {
    const root = join(dirname(server.root), name.toLowerCase());
    await mkdir(root, { recursive: true });
    const checkout = await server.request<{ id: string }>('/checkouts', 'POST', { root, stores: [store] });
    projects.push({ id: checkout.id, root, name });
  }
  await page.addInitScript(
    (roots) => {
      localStorage.setItem('hotsheet.open-projects', JSON.stringify(roots));
    },
    projects.slice(0, options.open ?? 2).map((project) => project.root),
  );
  await page.route(/\/__hotsheet\/(?:remote\/)?projects\/open$/, async (route) => {
    const { root } = route.request().postDataJSON() as { root: string };
    const project = projects.find((item) => item.root === root);
    if (!project) return route.fulfill({ status: 404, json: { error: 'unknown project' } });
    return route.fulfill({
      status: 201,
      json: { ...project, stores: [server.store], apiPath: `/__hotsheet/project-api/${project.id}` },
    });
  });
  await page.route('**/__hotsheet/remotes', (route) => route.fulfill({ json: [] }));
  await page.route('**/__hotsheet/project-api/**', async (route) => {
    const incoming = new URL(route.request().url()),
      path = incoming.pathname.replace(/^\/__hotsheet\/project-api\/[^/]+/, '');
    try {
      const response = await proxyFetch(route, {
        url: `${server.url}${path}${incoming.search}`,
        headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
        timeout: 60_000,
      });
      await route.fulfill({ response });
    } catch {
      await route.abort().catch(() => undefined);
    }
  });
  await routeProjectChangeSocket(page, server, server.checkoutId);
  return { server, projects };
}

async function showProjectSettings(page: Page, category: string) {
  await page.getByLabel('Settings View').click();
  await page
    .getByRole('navigation', { name: 'Project Settings' })
    .getByRole('button', { name: category, exact: true })
    .click();
}

async function chooseProjectType(page: Page, label: 'Software' | 'Orchestration') {
  const general = page.locator('[data-component="project-general-settings"]');
  await general.getByRole('combobox', { name: 'Project Type' }).click();
  await general.getByRole('option', { name: label, exact: true }).click();
}

const orchestrationNav = (page: Page) =>
  page.getByRole('navigation', { name: 'Project Settings' }).getByRole('button', { name: 'Orchestration' });

/**
 * Open the Launch project that is already an orchestration project. The shell learns the type
 * asynchronously and then inserts Orchestration View into the view-mode group, shifting the
 * buttons after it; a click aimed at Settings View before that lands on Notifications View
 * (HS2-NDN2NT). Wait for the settled group before any toolbar click.
 */
/** HS2-25KT42: the Limits form's Advanced disclosure, opened when it is closed. */
async function openAdvancedLimits(page: Page) {
  const advanced = page.locator('[data-component="orchestration-project-limits"] [data-orchestration-advanced-limits]');
  if (!(await advanced.evaluate((element) => (element as HTMLDetailsElement).open)))
    await advanced.locator('summary').first().click();
  await expect(advanced).toHaveJSProperty('open', true);
}

/** HS2-1X1TXC, HS2-NPAZQW: the chosen names of one Limits field of chosen values, in order. */
async function chosenValues(page: Page, field: string) {
  return page
    .locator(`[data-component="orchestration-pattern-field"][data-limits-field="${field}"] [data-item-id]`)
    .evaluateAll((chips) => chips.map((chip) => (chip as HTMLElement).dataset.itemId));
}

async function openOrchestrationProject(page: Page) {
  await page.goto('/');
  await page.getByRole('tab', { name: /Launch/ }).click();
  await expect(
    page.getByRole('group', { name: 'View mode' }).getByRole('button', { name: 'Orchestration View' }),
  ).toBeVisible();
}

test('HS2-W27N1D Project Type adds the Orchestration category, which adds, details and removes projects through the real workflows', async ({
  page,
}) => {
  test.setTimeout(240_000);
  const { server, projects } = await openProjects(page);
  let stopped = false;
  try {
    const [owner, web, docs] = projects;
    const base = `/checkouts/${owner.id}/orchestration`;
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('/');
    await page.getByRole('tab', { name: /Launch/ }).click();
    await showProjectSettings(page, 'General');
    // Software by default: no Orchestration category, and nothing declared.
    await expect(page.locator('[data-component="project-general-settings"]')).toHaveAttribute(
      'data-project-type',
      'software',
    );
    await expect(orchestrationNav(page)).toHaveCount(0);
    await chooseProjectType(page, 'Orchestration');
    await expect(orchestrationNav(page)).toBeVisible();
    expect((await server.request<{ project_type: string }>(`${base}/project-type`)).project_type).toBe('orchestration');
    expect(existsSync(join(owner.root, '.hotsheet2/orchestration.json'))).toBe(true);

    // Orchestration: empty, then + Add Project → an open project.
    await orchestrationNav(page).click();
    const settings = page.locator('[data-component="orchestration-settings"]');
    await expect(settings.getByText('No projects yet.')).toBeVisible();
    await settings.getByRole('button', { name: 'Add Project' }).click();
    const menu = page.getByRole('menu');
    await expect(menu.getByRole('menuitemcheckbox', { name: /Launch/ }).or(menu.getByText('Launch'))).toHaveCount(0);
    await menu
      .getByRole('menuitemcheckbox', { name: /Web/ })
      .or(menu.getByRole('menuitem', { name: /Web/ }))
      .click();
    const webRow = settings.locator('[data-orchestration-project]').filter({ hasText: 'Web' });
    await expect(webRow).toContainText('Local on This Computer');
    const declared = await server.request<{ targets: { target_id: string; label: string }[] }>(`${base}/targets`);
    expect(declared.targets.map((target) => target.label)).toEqual(['Web']);
    const webTarget = declared.targets[0].target_id;
    expect(
      (await server.request<{ destination: { checkout_id: string } }>(`${base}/targets/${webTarget}/binding`))
        .destination.checkout_id,
    ).toBe(web.id);

    // A project added by another client (the CLI's workflow) arrives through the change stream.
    await server.request(`${base}/projects`, 'POST', {
      destination: { kind: 'local', checkout_id: docs.id },
      label: 'Docs',
      actor: { role: 'human' },
    });
    await expect(settings.locator('[data-orchestration-project]').filter({ hasText: 'Docs' })).toBeVisible();

    // A row opens its detail in the inspector; each part reads through the existing routes.
    await webRow.click();
    const detail = page.locator('[data-component="orchestration-project-detail"]');
    await expect(detail).toBeVisible();
    await expect(detail.getByRole('region', { name: 'Binding' })).toContainText('Connected');
    await expect(detail.getByRole('region', { name: 'Sessions' })).toContainText(
      'The orchestrator runs no sessions here.',
    );
    await expect(detail.getByRole('region', { name: 'Held Resources' })).toContainText(
      'The policy requires no resources here.',
    );
    await page.screenshot({ path: test.info().outputPath('orchestration-detail-1440-light.png') });

    // The limits-only policy: no action or approval lists, and Apply stores the limits.
    const policy = page.locator('[data-component="orchestration-policy-settings"]');
    await expect(policy).not.toContainText('Ask a Person');
    await policy.getByLabel('Sessions per Machine').fill('3');
    await policy.getByLabel('Sessions per Machine').blur();
    // HS2-NCMATS: the redo switch is off by default and stored when turned on.
    const redo = policy.getByRole('switch', { name: 'Redo reopened work on its own' });
    await expect(redo).not.toBeChecked();
    await redo.click();
    await expect(redo).toBeChecked();
    await policy.getByRole('button', { name: 'Apply Policy' }).click();
    await expect(page.locator('.app-toast').filter({ hasText: 'Policy saved.' })).toBeVisible();
    const stored = await server.request<{
      policy: { machine: { max_sessions: number }; speculation: { redo_reopened: boolean } };
    }>(`${base}/policy`);
    expect(stored.policy.machine.max_sessions).toBe(3);
    expect(stored.policy.speculation.redo_reopened).toBe(true);

    // Remove from the detail, after the shared confirmation.
    await detail.getByRole('button', { name: 'Stop Orchestrating…' }).click();
    await page.getByRole('button', { name: 'Stop Orchestrating', exact: true }).click();
    // By its id: Docs' Add a Site menu now offers the open Web tab, so the text alone matches its row too (HS2-E9VVQF).
    await expect(settings.locator(`[data-orchestration-project="${webTarget}"]`)).toHaveCount(0);
    expect(
      (await server.request<{ targets: { label: string; state: string }[] }>(`${base}/targets`)).targets.map(
        (target) => `${target.label}:${target.state}`,
      ),
    ).toEqual(['Web:removed', 'Docs:active']);

    // Back to Software with Docs still added: asked first, and Docs stays declared.
    await showProjectSettings(page, 'General');
    await chooseProjectType(page, 'Software');
    await expect(page.getByText('Change this project to Software?')).toBeVisible();
    await page.getByRole('button', { name: 'Change to Software', exact: true }).click();
    await expect(orchestrationNav(page)).toHaveCount(0);
    const after = await server.request<{ project_type: string; active_projects: number }>(`${base}/project-type`);
    expect(after).toMatchObject({ project_type: 'software', active_projects: 1 });
    await expect(page.locator('[data-component="project-general-settings"]')).toContainText('1 project stays');
    await server.stop(page);
    stopped = true;
  } finally {
    if (!stopped) await server.stop();
  }
});

/** Visual QA captures for HS2-W27N1D: the real shell at wide, medium and narrow widths, both schemes. */
test('HS2-W27N1D captures Project Type, Orchestration, the Add Project menu and a project detail', async ({
  page,
  browserName,
}) => {
  test.setTimeout(300_000);
  const { server, projects } = await openProjects(page);
  try {
    const [owner, web, docs] = projects;
    const base = `/checkouts/${owner.id}/orchestration`;
    await server.request(`${base}/project-type`, 'PUT', { project_type: 'orchestration', actor: { role: 'human' } });
    for (const [project, label] of [
      [web, 'Web'],
      [docs, 'Docs'],
    ] as const)
      await server.request(`${base}/projects`, 'POST', {
        destination: { kind: 'local', checkout_id: project.id },
        label,
        actor: { role: 'human' },
      });
    const directory = join('target', 'visual-captures', 'hs2-w27n1d');
    await mkdir(directory, { recursive: true });
    const capture = (name: string) =>
      page.screenshot({ path: join(directory, `hs2-w27n1d-${name}-${browserName}.png`), animations: 'disabled' });
    const wide = { width: 1440, height: 900 };
    const sizes = [wide, { width: 1100, height: 900 }, { width: 390, height: 844 }];
    const settings = page.locator('[data-component="orchestration-settings"]');
    const hideRails = async (names: string[]) => {
      for (const name of names) {
        const hide = page.getByRole('button', { name, exact: true });
        // A rail may already be sliding closed after the resize; then the click finds nothing to
        // press, which is fine: the capture only needs the rail closed.
        if (await hide.isVisible()) await hide.click({ timeout: 3_000 }).catch(() => undefined);
        await expect(hide).toBeHidden();
      }
    };
    const fits = async () => {
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
    };
    await page.setViewportSize(wide);
    await openOrchestrationProject(page);
    for (const scheme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      // Each state is set up wide, then shown wide, medium and narrow, and wide again.
      await page.setViewportSize(wide);
      await showProjectSettings(page, 'General');
      await expect(page.locator('[data-component="project-general-settings"]')).toHaveAttribute(
        'data-project-type',
        'orchestration',
      );
      for (const size of sizes) {
        await page.setViewportSize(size);
        if (size.width === 390) await hideRails(['Hide Settings Sidebar', 'Hide Ticket Inspector']);
        await fits();
        await capture(`project-type-${String(size.width)}-${scheme}`);
      }
      const showNavigation = page.getByRole('button', { name: 'Show Settings Sidebar', exact: true });
      await showNavigation.click();
      await expect(orchestrationNav(page)).toBeVisible();
      await capture(`settings-navigation-390-${scheme}`);
      await page.setViewportSize(wide);
      await capture(`project-type-1440-after-390-${scheme}`);
      await orchestrationNav(page).click();
      await expect(settings.locator('[data-orchestration-project]')).toHaveCount(2);
      for (const size of sizes) {
        await page.setViewportSize(size);
        if (size.width === 390) await hideRails(['Hide Settings Sidebar', 'Hide Ticket Inspector']);
        await fits();
        await capture(`orchestration-settings-${String(size.width)}-${scheme}`);
        await settings.getByRole('button', { name: 'Add Project' }).click();
        await expect(page.getByRole('menu').getByText('Open Another Project…')).toBeVisible();
        await capture(`add-project-menu-${String(size.width)}-${scheme}`);
        await page.keyboard.press('Escape');
        await expect(page.getByRole('menu')).toBeHidden();
      }
      await page.setViewportSize(wide);
      await settings.locator('[data-orchestration-project]').filter({ hasText: 'Web' }).click();
      const detail = page.locator('[data-component="orchestration-project-detail"]');
      await expect(detail.getByRole('region', { name: 'Binding' })).toContainText('Connected');
      for (const size of sizes) {
        await page.setViewportSize(size);
        if (size.width === 390) {
          await hideRails(['Hide Settings Sidebar']);
          const show = page.getByRole('button', { name: 'Show Project Detail', exact: true });
          if (await show.isVisible()) await show.click();
        }
        await expect(detail).toBeVisible();
        await fits();
        await capture(`project-detail-${String(size.width)}-${scheme}`);
      }
      await page.setViewportSize(wide);
      await expect(settings).toBeVisible();
      await fits();
      await capture(`orchestration-settings-1440-after-390-${scheme}`);
    }
  } finally {
    await server.stop(page);
  }
});

/**
 * HS2-Y3N3PH: adding a project is the consent to read it (HS2-XHQ1EH), so an added project's detail
 * reads capacity and admission through the real gate without anyone being asked. Checked in the
 * real shell against the real server, with captures at wide, medium and narrow widths.
 */
test('HS2-Y3N3PH the project detail reads capacity and admission for an added project', async ({
  page,
  browserName,
}) => {
  test.setTimeout(240_000);
  const { server, projects } = await openProjects(page);
  try {
    const [owner, web, docs] = projects;
    const base = `/checkouts/${owner.id}/orchestration`;
    await server.request(`${base}/project-type`, 'PUT', { project_type: 'orchestration', actor: { role: 'human' } });
    const targets: Record<string, string> = {};
    for (const [project, label] of [
      [web, 'Web'],
      [docs, 'Docs'],
    ] as const)
      targets[label] = (
        await server.request<{ target_id: string }>(`${base}/projects`, 'POST', {
          destination: { kind: 'local', checkout_id: project.id },
          label,
          actor: { role: 'human' },
        })
      ).target_id;
    // The policy lets Codex sessions run on Web; Docs lists no tool.
    await server.request(`${base}/policy`, 'PUT', {
      expected_revision: null,
      policy: {
        tools: [{ tool: 'codex', max_sessions: 2 }],
        targets: [{ target_id: targets.Web, tools: [{ tool: 'codex' }], max_sessions: 1 }],
      },
      actor: { role: 'human' },
    });
    const wide = { width: 1440, height: 900 };
    await page.setViewportSize(wide);
    await openOrchestrationProject(page);
    await showProjectSettings(page, 'General');
    await expect(page.locator('[data-component="project-general-settings"]')).toHaveAttribute(
      'data-project-type',
      'orchestration',
    );
    await orchestrationNav(page).click();
    const settings = page.locator('[data-component="orchestration-settings"]');
    const detail = page.locator('[data-component="orchestration-project-detail"]');
    const open = async (label: string) => {
      await settings.locator('[data-orchestration-project]').filter({ hasText: label }).click();
      await expect(detail).toHaveAttribute('data-target-id', targets[label]);
    };
    await open('Web');
    const capacity = detail.getByRole('region', { name: 'Capacity and Admission' });
    // Capacity and admission are read, not refused.
    await expect(capacity.getByText('Free Memory')).toBeVisible();
    await expect(capacity.getByText('Codex Admission')).toBeVisible();
    await expect(capacity).toContainText(/Can start|Refused/);
    // HS2-ZY4Z4G: what the machine is, as the real server reports it.
    await expect(capacity.getByText('System', { exact: true })).toBeVisible();
    await expect(capacity).toContainText(/(macOS|Linux|Windows)[^,]*, (arm64|x86_64)/);
    await expect(capacity.getByText('Toolchains', { exact: true })).toBeVisible();
    await expect(detail).not.toContainText('is not available');
    await expect(detail).not.toContainText('not allowed it yet');
    // Nobody was asked: no permission request was filed for these reads.
    const permissions = await server.request<{ requests: unknown[] }>(`${base}/permissions`);
    expect(permissions.requests).toEqual([]);
    // HS2-N5BDE4: a project the policy does not list is Auto, so admission is asked for every usable tool.
    await open('Docs');
    await expect(capacity.getByText('Free Memory')).toBeVisible();
    await expect(capacity.getByText('Codex Admission')).toBeVisible();
    await expect(capacity).not.toContainText('No AI tool is allowed here yet');
    await expect(detail).not.toContainText('is not available');
    await open('Web');
    await expect(capacity.getByText('Codex Admission')).toBeVisible();

    const directory = join('target', 'visual-captures', 'hs2-y3n3ph');
    await mkdir(directory, { recursive: true });
    for (const [index, size] of [wide, { width: 1100, height: 900 }, { width: 390, height: 844 }, wide].entries()) {
      await page.setViewportSize(size);
      if (size.width === 390) {
        const hide = page.getByRole('button', { name: 'Hide Settings Sidebar', exact: true });
        if (await hide.isVisible()) await hide.click({ timeout: 3_000 }).catch(() => undefined);
        const show = page.getByRole('button', { name: 'Show Project Detail', exact: true });
        if (await show.isVisible()) await show.click();
      }
      await expect(capacity.getByText('Codex Admission')).toBeVisible();
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
      await page.screenshot({
        path: join(
          directory,
          `hs2-y3n3ph-project-detail-${String(size.width)}${index === 3 ? '-after-390' : ''}-${browserName}.png`,
        ),
        animations: 'disabled',
      });
    }
  } finally {
    await server.stop(page);
  }
});

/**
 * HS2-7WF6JD: a project's own limits in its detail and a tool's accounts in the policy form, applied
 * through the real policy route; only that project's entry and that tool's accounts change.
 */
test('HS2-7WF6JD HS2-YSY7AR edits a project limits, models, resources and a tool account through the real policy route', async ({
  page,
  browserName,
}) => {
  test.setTimeout(240_000);
  const { server, projects } = await openProjects(page);
  try {
    const [owner, web, docs] = projects;
    const base = `/checkouts/${owner.id}/orchestration`;
    await server.request(`${base}/project-type`, 'PUT', { project_type: 'orchestration', actor: { role: 'human' } });
    const targets: Record<string, string> = {};
    for (const [project, label] of [
      [web, 'Web'],
      [docs, 'Docs'],
    ] as const)
      targets[label] = (
        await server.request<{ target_id: string }>(`${base}/projects`, 'POST', {
          destination: { kind: 'local', checkout_id: project.id },
          label,
          actor: { role: 'human' },
        })
      ).target_id;
    // Docs is listed with Codex; Web is not listed yet.
    await server.request(`${base}/policy`, 'PUT', {
      expected_revision: null,
      policy: {
        tools: [{ tool: 'codex', max_sessions: 2 }],
        targets: [{ target_id: targets.Docs, tools: [{ tool: 'codex' }], max_sessions: 1 }],
      },
      actor: { role: 'human' },
    });
    type Stored = {
      policy: {
        tools: {
          tool: string;
          accounts?: { account: string; max_sessions: number; budget_cents_per_day?: number }[];
        }[];
        targets: {
          target_id: string;
          tools: { tool: string }[];
          max_sessions: number;
          machine?: { max_sessions: number };
        }[];
      };
    };
    const wide = { width: 1440, height: 900 };
    await page.setViewportSize(wide);
    await openOrchestrationProject(page);
    await showProjectSettings(page, 'General');
    await expect(page.locator('[data-component="project-general-settings"]')).toHaveAttribute(
      'data-project-type',
      'orchestration',
    );
    await orchestrationNav(page).click();
    const settings = page.locator('[data-component="orchestration-settings"]');
    await settings.locator('[data-orchestration-project]').filter({ hasText: 'Web' }).click();
    const limits = page.locator('[data-component="orchestration-project-limits"]');
    // HS2-N5BDE4: an unlisted project is Auto: no tool checked, Sessions Here shows Auto, and admission
    // is asked for every usable tool.
    await expect(limits).toContainText('None checked is Auto');
    // HS2-25KT42: Sessions Here is one of the Auto limits under Advanced.
    await openAdvancedLimits(page);
    await expect(limits.getByRole('spinbutton', { name: 'Sessions Here' })).toHaveValue('');
    await expect(limits.getByRole('spinbutton', { name: 'Sessions Here' })).toHaveAttribute('placeholder', 'Auto');
    const capacity = page.getByRole('region', { name: 'Capacity and Admission' });
    await expect(capacity.getByText('Codex Admission')).toBeVisible();
    // Allow Codex here with its own machine limits, and apply.
    await limits.getByRole('checkbox', { name: 'Codex' }).check();
    await openAdvancedLimits(page);
    await limits.getByRole('checkbox', { name: 'Use Different Machine Limits' }).check();
    const machineSessions = limits.getByLabel('Sessions per Machine');
    await machineSessions.fill('3');
    await machineSessions.blur();
    const directory = join('target', 'visual-captures', 'hs2-7wf6jd');
    await mkdir(directory, { recursive: true });
    const capture = (name: string) =>
      page.screenshot({ path: join(directory, `hs2-7wf6jd-${name}-${browserName}.png`), animations: 'disabled' });
    await limits.scrollIntoViewIfNeeded();
    await capture('limits-draft-1440');
    await limits.getByRole('button', { name: 'Apply Limits' }).click();
    await expect(page.locator('.app-toast').filter({ hasText: 'Limits for Web saved.' })).toBeVisible();
    let stored = await server.request<Stored>(`${base}/policy`);
    expect(stored.policy.targets.map((entry) => entry.target_id)).toEqual([targets.Docs, targets.Web]);
    expect(stored.policy.targets[1]).toMatchObject({
      tools: [{ tool: 'codex' }],
      machine: { max_sessions: 3 },
    });
    // Sessions Here left empty is Auto, stored by leaving it out.
    expect(stored.policy.targets[1]).not.toHaveProperty('max_sessions');
    // Admission is now asked for Codex here.
    await expect(capacity.getByText('Codex Admission')).toBeVisible();
    await expect(limits).not.toContainText('does not list this project');

    // HS2-YSY7AR: the models Codex may use here and the resources work here must hold.
    // HS2-1X1TXC, HS2-NPAZQW: each is chosen or typed in its combobox and shows as a chip.
    const models = limits.getByRole('combobox', { name: 'Codex Models' });
    await models.fill('gpt-5-codex, gpt-5');
    await models.blur();
    await expect.poll(() => chosenValues(page, 'limits.models.codex')).toEqual(['gpt-5-codex', 'gpt-5']);
    const resources = limits.getByRole('combobox', { name: 'Required Resources' });
    await resources.fill('vm/macos-vm');
    await resources.press('Enter');
    await expect.poll(() => chosenValues(page, 'limits.resources')).toEqual(['vm/macos-vm']);
    const ysy = join('target', 'visual-captures', 'hs2-ysy7ar');
    await mkdir(ysy, { recursive: true });
    const captureModels = async (name: string) => {
      await resources.scrollIntoViewIfNeeded();
      await page.screenshot({ path: join(ysy, `hs2-ysy7ar-${name}-${browserName}.png`), animations: 'disabled' });
    };
    await captureModels('limits-draft-1440');
    await limits.getByRole('button', { name: 'Apply Limits' }).click();
    // The toast already says the first save; wait for the stored policy itself.
    type WithResources = Stored & {
      policy: {
        placement?: { resources?: { target_id: string; resources: { namespace: string; resource: string }[] }[] };
      };
    };
    await expect
      .poll(async () => (await server.request<WithResources>(`${base}/policy`)).policy.placement?.resources)
      .toEqual([{ target_id: targets.Web, resources: [{ namespace: 'vm', resource: 'macos-vm' }] }]);
    const withResources = await server.request<WithResources>(`${base}/policy`);
    expect(withResources.policy.targets[1].tools).toEqual([{ tool: 'codex', models: ['gpt-5-codex', 'gpt-5'] }]);
    // The detail reads held resources in the namespace the policy now requires here.
    await expect(page.getByRole('region', { name: 'Held Resources' })).not.toContainText(
      'The policy requires no resources here.',
    );
    expect(await chosenValues(page, 'limits.models.codex')).toEqual(['gpt-5-codex', 'gpt-5']);
    for (const [index, size] of [{ width: 1100, height: 900 }, { width: 390, height: 844 }, wide].entries()) {
      await page.setViewportSize(size);
      if (size.width === 390) {
        const hide = page.getByRole('button', { name: 'Hide Settings Sidebar', exact: true });
        if (await hide.isVisible()) await hide.click({ timeout: 3_000 }).catch(() => undefined);
        const show = page.getByRole('button', { name: 'Show Project Detail', exact: true });
        if (await show.isVisible()) await show.click();
      }
      await openAdvancedLimits(page);
      await expect(resources).toBeVisible();
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
      await captureModels(`limits-${String(size.width)}${index === 2 ? '-after-390' : ''}`);
    }
    await page.setViewportSize(wide);

    // A tool account with a daily budget in the policy form.
    const policy = page.locator('[data-component="orchestration-policy-settings"]');
    await policy.getByRole('button', { name: 'Add Account' }).first().click();
    const account = policy.locator('[data-policy-account]').first();
    await account.getByRole('textbox', { name: 'Account', exact: true }).fill('team');
    await account.getByRole('textbox', { name: 'Account', exact: true }).blur();
    await account.getByLabel('Daily Budget ($)').fill('12.50');
    await account.getByLabel('Daily Budget ($)').blur();
    await account.scrollIntoViewIfNeeded();
    await capture('policy-account-1440');
    await policy.getByRole('button', { name: 'Apply Policy' }).click();
    await expect(page.locator('.app-toast').filter({ hasText: 'Policy saved.' })).toBeVisible();
    stored = await server.request<Stored>(`${base}/policy`);
    expect(stored.policy.tools[0].accounts).toEqual([{ account: 'team', max_sessions: 1, budget_cents_per_day: 1250 }]);
    // The project entries the policy form does not show are unchanged.
    expect(stored.policy.targets.map((entry) => entry.target_id)).toEqual([targets.Docs, targets.Web]);

    for (const [index, size] of [wide, { width: 1100, height: 900 }, { width: 390, height: 844 }, wide].entries()) {
      await page.setViewportSize(size);
      if (size.width === 390) {
        const hide = page.getByRole('button', { name: 'Hide Settings Sidebar', exact: true });
        if (await hide.isVisible()) await hide.click({ timeout: 3_000 }).catch(() => undefined);
        const show = page.getByRole('button', { name: 'Show Project Detail', exact: true });
        if (await show.isVisible()) await show.click();
      }
      await limits.scrollIntoViewIfNeeded();
      await expect(limits.getByRole('button', { name: 'Apply Limits' })).toBeVisible();
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
      await capture(`limits-${String(size.width)}${index === 3 ? '-after-390' : ''}`);
    }
    // The policy form's accounts at the medium and narrow widths.
    for (const size of [
      { width: 1100, height: 900 },
      { width: 390, height: 844 },
    ]) {
      await page.setViewportSize(size);
      if (size.width === 390) {
        const hide = page.getByRole('button', { name: 'Hide Project Detail', exact: true });
        if (await hide.isVisible()) await hide.click({ timeout: 3_000 }).catch(() => undefined);
      }
      await account.scrollIntoViewIfNeeded();
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
      // The tool rows hold their content, whatever wraps inside them: nothing spills into the next row.
      const rows = policy.locator('.kui-framed-list__row');
      await expect
        .poll(() => rows.evaluateAll((elements) => elements.every((row) => row.scrollHeight <= row.clientHeight + 1)))
        .toBe(true);
      await capture(`policy-account-${String(size.width)}`);
    }
    // Both schemes on the real backdrop, wide.
    await page.setViewportSize(wide);
    await page.emulateMedia({ colorScheme: 'dark' });
    await account.scrollIntoViewIfNeeded();
    await capture('policy-account-1440-dark');
    await limits.scrollIntoViewIfNeeded();
    await capture('limits-1440-dark');
  } finally {
    await server.stop(page);
  }
});

/**
 * HS2-N3HWP9: each tool's Sessions and Weight are Auto when empty, and 0 turns the tool off. Checked in
 * the real shell against the real policy route, with captures of the Tools section wide and narrow.
 */
test('HS2-N3HWP9 tool Sessions and Weight show Auto when empty and store 0 to turn a tool off', async ({
  page,
  browserName,
}) => {
  test.setTimeout(180_000);
  const { server, projects } = await openProjects(page);
  try {
    const [owner] = projects;
    const base = `/checkouts/${owner.id}/orchestration`;
    await server.request(`${base}/project-type`, 'PUT', { project_type: 'orchestration', actor: { role: 'human' } });
    // Codex listed with nothing set: Auto in both fields.
    await server.request(`${base}/policy`, 'PUT', {
      expected_revision: null,
      policy: { tools: [{ tool: 'codex' }] },
      actor: { role: 'human' },
    });
    const wide = { width: 1440, height: 900 };
    await page.setViewportSize(wide);
    await openOrchestrationProject(page);
    await showProjectSettings(page, 'Orchestration');
    const policy = page.locator('[data-component="orchestration-policy-settings"]');
    await expect(policy).toContainText('Empty is Auto');
    await expect(policy).not.toContainText('Leave Sessions empty');
    const sessions = policy.getByRole('spinbutton', { name: 'Sessions', exact: true });
    const weights = policy.getByRole('spinbutton', { name: 'Weight', exact: true });
    expect(await sessions.count()).toBeGreaterThan(0);
    // Every tool reads Auto: an empty value with Auto as its placeholder.
    for (const field of [...(await sessions.all()), ...(await weights.all())]) {
      await expect(field).toHaveValue('');
      await expect(field).toHaveAttribute('placeholder', 'Auto');
    }
    const directory = join('target', 'visual-captures', 'hs2-n3hwp9');
    await mkdir(directory, { recursive: true });
    const capture = (name: string) =>
      page.screenshot({ path: join(directory, `hs2-n3hwp9-${name}-${browserName}.png`), animations: 'disabled' });
    await sessions.first().scrollIntoViewIfNeeded();
    await capture('tools-auto-1440');

    // 0 turns the first tool off; a number on the second is a cap. Weight stays Auto.
    await sessions.nth(0).fill('0');
    await sessions.nth(0).blur();
    const two = (await sessions.count()) > 1;
    if (two) {
      await sessions.nth(1).fill('3');
      await sessions.nth(1).blur();
    }
    await policy.getByRole('button', { name: 'Apply Policy' }).click();
    // With no project added the empty state is a status too; find the save message by its words.
    await expect(page.locator('.app-toast').filter({ hasText: 'Policy saved.' })).toBeVisible();
    const stored = await server.request<{ policy: { tools: Record<string, unknown>[] } }>(`${base}/policy`);
    expect(stored.policy.tools[0]).toMatchObject({ max_sessions: 0 });
    expect(stored.policy.tools.every((tool) => !('weight' in tool))).toBe(true);
    if (two) expect(stored.policy.tools[1]).toMatchObject({ max_sessions: 3 });
    expect(stored.policy.tools).toHaveLength(two ? 2 : 1);
    await expect(sessions.nth(0)).toHaveValue('0');
    await expect(weights.nth(0)).toHaveValue('');
    for (const [index, size] of [wide, { width: 1100, height: 900 }, { width: 390, height: 844 }, wide].entries()) {
      await page.setViewportSize(size);
      if (size.width === 390) {
        const hide = page.getByRole('button', { name: 'Hide Settings Sidebar', exact: true });
        if (await hide.isVisible()) await hide.click({ timeout: 3_000 }).catch(() => undefined);
      }
      await sessions.first().scrollIntoViewIfNeeded();
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
      await capture(`tools-${String(size.width)}${index === 3 ? '-after-390' : ''}`);
    }
    await page.emulateMedia({ colorScheme: 'dark' });
    await capture('tools-1440-dark');
  } finally {
    await server.stop(page);
  }
});

test('HS2-TK9ADC turns a project coordination sessions on and off through the real policy route', async ({
  page,
  browserName,
}) => {
  test.setTimeout(180_000);
  const { server, projects } = await openProjects(page);
  try {
    const [owner, , docs] = projects;
    const base = `/checkouts/${owner.id}/orchestration`;
    await server.request(`${base}/project-type`, 'PUT', { project_type: 'orchestration', actor: { role: 'human' } });
    const target = (
      await server.request<{ target_id: string }>(`${base}/projects`, 'POST', {
        destination: { kind: 'local', checkout_id: docs.id },
        label: 'Docs',
        actor: { role: 'human' },
      })
    ).target_id;
    await server.request(`${base}/policy`, 'PUT', {
      expected_revision: null,
      policy: {
        machine: { max_sessions: 4 },
        tools: [{ tool: 'codex', max_sessions: 3 }],
        targets: [{ target_id: target, tools: [{ tool: 'codex' }], max_sessions: 3 }],
      },
      actor: { role: 'human' },
    });
    type Stored = { policy: { placement?: { role_sessions?: { target_id: string; roles: string[] }[] } } };
    const roles = async () => (await server.request<Stored>(`${base}/policy`)).policy.placement?.role_sessions;
    const wide = { width: 1440, height: 900 };
    await page.setViewportSize(wide);
    await openOrchestrationProject(page);
    await showProjectSettings(page, 'General');
    await orchestrationNav(page).click();
    const settings = page.locator('[data-component="orchestration-settings"]');
    await settings.locator('[data-orchestration-project]').filter({ hasText: 'Docs' }).click();
    const limits = page.locator('[data-component="orchestration-project-limits"]');
    await openAdvancedLimits(page);
    const dispatcher = limits.getByRole('checkbox', { name: 'Dispatcher' }),
      integrator = limits.getByRole('checkbox', { name: 'Integrator' });
    // HS2-R2N33M: not listed, so Auto: both shown checked, and nothing is stored until a person chooses.
    await expect(dispatcher).toBeChecked();
    await expect(integrator).toBeChecked();
    await expect(limits).toContainText('Auto: follows placement, both when they help.');
    await integrator.uncheck();
    const directory = join('target', 'visual-captures', 'hs2-tk9adc');
    await mkdir(directory, { recursive: true });
    const capture = async (name: string) => {
      await integrator.scrollIntoViewIfNeeded();
      await page.screenshot({ path: join(directory, `hs2-tk9adc-${name}-${browserName}.png`), animations: 'disabled' });
    };
    await capture('limits-draft-1440');
    await limits.getByRole('button', { name: 'Apply Limits' }).click();
    await expect.poll(roles).toEqual([{ target_id: target, roles: ['dispatcher'] }]);
    await expect(dispatcher).toBeChecked();
    await expect(integrator).not.toBeChecked();
    for (const [index, size] of [{ width: 1100, height: 900 }, { width: 390, height: 844 }, wide].entries()) {
      await page.setViewportSize(size);
      if (size.width === 390) {
        const hide = page.getByRole('button', { name: 'Hide Settings Sidebar', exact: true });
        if (await hide.isVisible()) await hide.click({ timeout: 3_000 }).catch(() => undefined);
        const show = page.getByRole('button', { name: 'Show Project Detail', exact: true });
        if (await show.isVisible()) await show.click();
      }
      await expect(integrator).toBeVisible();
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
      await capture(`limits-${String(size.width)}${index === 2 ? '-after-390' : ''}`);
    }
    await page.emulateMedia({ colorScheme: 'dark' });
    await capture('limits-1440-dark');
    await page.emulateMedia({ colorScheme: 'light' });
    // A further edit: the dispatcher off too; the choice of none is kept (HS2-R2N33M).
    await dispatcher.uncheck();
    await limits.getByRole('button', { name: 'Apply Limits' }).click();
    await expect.poll(roles).toEqual([{ target_id: target, roles: [] }]);
    await expect(dispatcher).not.toBeChecked();
    await expect(limits).not.toContainText('Auto: follows placement');
    // Both checked again is Auto: the entry goes.
    await dispatcher.check();
    await integrator.check();
    await limits.getByRole('button', { name: 'Apply Limits' }).click();
    await expect.poll(roles).toBeUndefined();
    await expect(limits).toContainText('Auto: follows placement, both when they help.');
  } finally {
    await server.stop(page);
  }
});

/**
 * HS2-92FJXM: the orchestration project is open only as a remote tab of the fixture server, whose
 * checkout-scoped paths the routed bridge rewrites to the native checkout as the real bridge does.
 * Web and Docs are registered on that server but not open here.
 */
/** The Orchestration view's read-only POSTs: where planning stands, the dry-run schedule, the decisions. */
// HS2-1XCW6N: Settings › Orchestration also lists the checked-in templates, a read.
const READ_ONLY_POSTS =
  /\/orchestration\/(?:planning\/show|scheduler\/plan|scheduler\/decisions|templates\/list|triggers\/list)$/;

async function openRemoteOrchestration(page: Page) {
  const server = await realTicketServer();
  const binding = '01ARZ3NDEKTSV4RRFFQ69G5FAV';
  const others: { id: string; name: string }[] = [];
  for (const name of ['Web', 'Docs']) {
    const root = join(dirname(server.root), name.toLowerCase());
    await mkdir(root, { recursive: true });
    const checkout = await server.request<{ id: string }>('/checkouts', 'POST', {
      root,
      stores: [server.store],
      alias: name,
    });
    others.push({ id: checkout.id, name });
  }
  const remoteId = `remote:${binding}:${server.checkoutId}`,
    encoded = encodeURIComponent(remoteId),
    remotePath = `/__hotsheet/project-api/${encoded}`;
  await page.addInitScript(
    (roots) => {
      localStorage.setItem('hotsheet.open-projects', JSON.stringify(roots));
    },
    [remoteId],
  );
  await page.route(/\/__hotsheet\/(?:remote\/)?projects\/open$/, (route) =>
    route.fulfill({
      status: 201,
      json: {
        id: remoteId,
        root: remoteId,
        name: 'Build host · Launch',
        remoteHost: 'Build host',
        stores: [],
        apiPath: remotePath,
      },
    }),
  );
  await page.route('**/__hotsheet/remotes', (route) => route.fulfill({ json: [] }));
  const posted: string[] = [];
  // A context route: a page route's POST here was answered by the dev bridge before the handler did.
  await page.context().route(`**${remotePath}/**`, async (route) => {
    const incoming = new URL(route.request().url());
    const path = incoming.pathname
      .slice(remotePath.length)
      .replace(`/checkouts/${encoded}`, `/checkouts/${server.checkoutId}`);
    // The Orchestration view an orchestration project opens in (HS2-BRRSYM) reads its plan and decisions
    // through POSTs that change nothing; only the writes are recorded.
    if (route.request().method() === 'POST' && !READ_ONLY_POSTS.test(path)) posted.push(path);
    if (route.request().method() === 'DELETE') posted.push(`DELETE ${path}`);
    try {
      const response = await proxyFetch(route, {
        url: `${server.url}${path}${incoming.search}`,
        headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
        timeout: 60_000,
      });
      await route.fulfill({ response });
    } catch {
      await route.abort().catch(() => undefined);
    }
  });
  return { server, others, remoteId, posted };
}

test('HS2-92FJXM a remote orchestration project adds the projects open on its server', async ({
  page,
  browserName,
}) => {
  test.setTimeout(180_000);
  const { server, others, posted } = await openRemoteOrchestration(page);
  try {
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    await server.request(`${base}/project-type`, 'PUT', { project_type: 'orchestration', actor: { role: 'human' } });
    const wide = { width: 1440, height: 900 };
    await page.setViewportSize(wide);
    await page.goto('/');
    await page.getByRole('tab', { name: /Launch/ }).click();
    await expect(
      page.getByRole('group', { name: 'View mode' }).getByRole('button', { name: 'Orchestration View' }),
    ).toBeVisible();
    await showProjectSettings(page, 'General');
    await orchestrationNav(page).click();
    const settings = page.locator('[data-component="orchestration-settings"]');
    // No longer the old explanation: + Add Project is enabled and lists that server's open projects.
    await expect(settings).not.toContainText('Add projects to it on the computer it runs on');
    const add = settings.getByRole('button', { name: 'Add Project' });
    await expect(add).toBeEnabled();
    await add.click();
    const menu = page.getByRole('menu');
    await expect(menu).toContainText('Open on Build host');
    for (const other of others) await expect(menu.getByRole('menuitemcheckbox', { name: other.name })).toBeVisible();
    await expect(menu).not.toContainText('Open Another Project');
    const directory = join('target', 'visual-captures', 'hs2-92fjxm');
    await mkdir(directory, { recursive: true });
    const capture = (name: string) =>
      page.screenshot({ path: join(directory, `hs2-92fjxm-${name}-${browserName}.png`), animations: 'disabled' });
    await capture('menu-1440');
    // Choosing Web adds it through the hosted route; the server binds it there.
    await menu.getByRole('menuitemcheckbox', { name: 'Web' }).click();
    await expect(page.locator('.app-toast').filter({ hasText: 'Web added.' })).toBeVisible();
    expect(posted).toEqual([`${base}/hosted-projects`]);
    await expect(settings.locator('[data-orchestration-project]').filter({ hasText: 'Web' })).toBeVisible();
    const hosted = await server.request<{ checkout_id: string; added: boolean }[]>(`${base}/hosted-projects`);
    expect(hosted.find((choice) => choice.checkout_id === others[0].id)?.added).toBe(true);
    // The menu now checks Web; choosing it again is the same project.
    await add.click();
    await expect(menu.getByRole('menuitemcheckbox', { name: 'Web' })).toBeChecked();
    await capture('menu-added-1440');
    await menu.getByRole('menuitemcheckbox', { name: 'Web' }).click();
    await expect(page.locator('.app-toast').filter({ hasText: 'Web is already added.' })).toBeVisible();
    for (const [index, size] of [{ width: 1100, height: 900 }, { width: 390, height: 844 }, wide].entries()) {
      await page.setViewportSize(size);
      if (size.width === 390) {
        const hide = page.getByRole('button', { name: 'Hide Settings Sidebar', exact: true });
        if (await hide.isVisible()) await hide.click({ timeout: 3_000 }).catch(() => undefined);
      }
      await expect(add).toBeVisible();
      await add.click();
      await expect(menu.getByRole('menuitemcheckbox', { name: 'Docs' })).toBeVisible();
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
      await capture(`menu-${String(size.width)}${index === 2 ? '-after-390' : ''}`);
      await page.keyboard.press('Escape');
    }
    await page.emulateMedia({ colorScheme: 'dark' });
    await add.click();
    await capture('menu-1440-dark');
  } finally {
    await server.stop(page);
  }
});

/**
 * HS2-89YNGN: for an orchestration project open from another server, + Add Project asks about a duplicate of an
 * added project's ticket store, and a project row's Add a Site offers that server's open checkouts of the same
 * store and adds one as a site through the hosted route. Web and Docs share the owner's store.
 */
test('HS2-89YNGN a remote orchestration project adds a site from the projects open on its server', async ({
  page,
  browserName,
}) => {
  test.setTimeout(240_000);
  const { server, others, posted } = await openRemoteOrchestration(page);
  try {
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    await server.request(`${base}/project-type`, 'PUT', { project_type: 'orchestration', actor: { role: 'human' } });
    const web = await server.request<{ target_id: string }>(`${base}/hosted-projects`, 'POST', {
      checkout_id: others[0].id,
      label: 'Web',
      actor: { role: 'human' },
    });
    const listed = await server.request<{ checkout_id: string; checkout_authority?: string }[]>(
      `${base}/hosted-projects`,
    );
    expect(listed.every((choice) => typeof choice.checkout_authority === 'string')).toBe(true);
    const wide = { width: 1440, height: 900 };
    await page.setViewportSize(wide);
    await page.goto('/');
    await page.getByRole('tab', { name: /Launch/ }).click();
    await expect(
      page.getByRole('group', { name: 'View mode' }).getByRole('button', { name: 'Orchestration View' }),
    ).toBeVisible();
    await showProjectSettings(page, 'General');
    await orchestrationNav(page).click();
    const settings = page.locator('[data-component="orchestration-settings"]');
    const rows = settings.locator('[data-orchestration-project]');
    await expect(rows).toHaveCount(1);
    const directory = join('target', 'visual-captures', 'hs2-89yngn');
    await mkdir(directory, { recursive: true });
    const capture = (name: string) =>
      page.screenshot({ path: join(directory, `hs2-89yngn-${name}-${browserName}.png`), animations: 'disabled' });

    // + Add Project → Docs, a duplicate of Web's store: asked, and Cancel adds nothing.
    await settings.getByRole('button', { name: 'Add Project' }).click();
    await page.getByRole('menu', { name: 'Add Project' }).getByRole('menuitemcheckbox', { name: /Docs/ }).click();
    const question = page.getByRole('alertdialog', { name: 'Is Docs another site of Web?' });
    await expect(question).toBeVisible();
    await capture('duplicate-1440-light');
    await question.getByRole('button', { name: 'Cancel', exact: true }).click();
    await expect(question).toHaveCount(0);
    expect(posted.filter((path) => path.endsWith('/hosted-projects'))).toEqual([]);

    // Web's "…" → Add a Site: the server's open checkouts of its store, without Open Another Project….
    const openSites = async () => {
      await rows.nth(0).click({ button: 'right' });
      await page.getByRole('menuitem', { name: 'Add a Site' }).click();
      const submenu = page.getByRole('menu', { name: 'Add a Site' });
      await expect(submenu).toBeVisible();
      await expect(submenu.getByRole('menuitem', { name: /Docs/ })).toContainText('Build host');
      await expect(submenu.getByRole('menuitem', { name: 'Open Another Project…' })).toHaveCount(0);
      return submenu;
    };
    for (const scheme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      for (const [index, size] of [wide, { width: 1100, height: 900 }, { width: 390, height: 844 }, wide].entries()) {
        await page.setViewportSize(size);
        if (size.width === 390) {
          const hide = page.getByRole('button', { name: 'Hide Settings Sidebar', exact: true });
          if (await hide.isVisible()) await hide.click({ timeout: 3_000 }).catch(() => undefined);
        }
        const submenu = await openSites();
        const box = await submenu.boundingBox();
        expect(box!.x).toBeGreaterThanOrEqual(0);
        expect(box!.x + box!.width).toBeLessThanOrEqual(size.width + 1);
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
        await capture(`add-site-${String(size.width)}${index === 3 ? '-after-390' : ''}-${scheme}`);
        await page.keyboard.press('Escape');
        await page.keyboard.press('Escape');
        await expect(submenu).toBeHidden();
      }
    }
    await page.emulateMedia({ colorScheme: 'light' });
    await page.setViewportSize(wide);
    const submenu = await openSites();
    await submenu.getByRole('menuitem', { name: /Docs/ }).click();
    // Action results arrive as a toast, never an inline status line (HS2-1K6122).
    await expect(page.locator('.app-toast').filter({ hasText: 'Docs added as a site of Web.' })).toBeVisible();
    expect(posted).toContain(`${base}/hosted-projects`);
    const declared = await server.request<{ targets: { label: string; site_of?: string }[] }>(`${base}/targets`);
    expect(declared.targets.map((target) => [target.label, target.site_of ?? null])).toEqual([
      ['Web', null],
      ['Docs', web.target_id],
    ]);
    await expect(rows).toHaveCount(2);
    await expect(rows.nth(1)).toContainText('Site · ');
    await capture('added-1440-light');
  } finally {
    await server.stop(page);
  }
});

/**
 * HS2-40CTDS: a Settings › Orchestration change's outcome is the app's toast. A refused add is toasted and also
 * kept in the message line until the next change; a success is a toast only, with nothing inlined under
 * + Add Project. The refusal is the server's own failure body, answered once in place of the real route.
 */
test('HS2-40CTDS a refused add toasts and keeps its record; a success is only a toast', async ({
  page,
  browserName,
}) => {
  test.setTimeout(180_000);
  const { server } = await openRemoteOrchestration(page);
  try {
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    await server.request(`${base}/project-type`, 'PUT', { project_type: 'orchestration', actor: { role: 'human' } });
    const wide = { width: 1440, height: 900 };
    await page.setViewportSize(wide);
    await page.goto('/');
    await page.getByRole('tab', { name: /Launch/ }).click();
    await showProjectSettings(page, 'General');
    await orchestrationNav(page).click();
    const settings = page.locator('[data-component="orchestration-settings"]');
    const add = settings.getByRole('button', { name: 'Add Project' });
    await expect(add).toBeEnabled();
    let refused = false;
    await page.route(/\/orchestration\/hosted-projects$/, async (route) => {
      if (route.request().method() !== 'POST' || refused) return route.fallback();
      refused = true;
      await route.fulfill({
        status: 409,
        json: { error: 'orchestration declaration changed', code: 'revision_conflict' },
      });
    });
    await add.click();
    await page.getByRole('menu').getByRole('menuitemcheckbox', { name: 'Docs' }).click();
    const failure = 'Could not add Docs. orchestration declaration changed';
    await expect(page.locator('.app-toast').filter({ hasText: failure })).toBeVisible();
    // The refusal stays in the message line after its toast.
    const record = settings.getByText(failure);
    await expect(record).toBeVisible();
    const directory = join('target', 'visual-captures', 'hs2-40ctds');
    await mkdir(directory, { recursive: true });
    const capture = (name: string) =>
      page.screenshot({ path: join(directory, `hs2-40ctds-${name}-${browserName}.png`), animations: 'disabled' });
    for (const scheme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      for (const [index, size] of [wide, { width: 1100, height: 900 }, { width: 390, height: 844 }, wide].entries()) {
        await page.setViewportSize(size);
        if (size.width === 390) {
          const hide = page.getByRole('button', { name: 'Hide Settings Sidebar', exact: true });
          if (await hide.isVisible()) await hide.click({ timeout: 3_000 }).catch(() => undefined);
        }
        await expect(record).toBeVisible();
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
        await capture(`refused-${String(size.width)}${index === 3 ? '-after-390' : ''}-${scheme}`);
      }
    }
    await page.emulateMedia({ colorScheme: 'light' });
    // The next change clears the record; its success is the toast, and nothing is inlined under + Add Project.
    // Where + Add Project sits within the settings, not the window: a connection banner above the settings
    // (a remote project reconnecting) may come and go meanwhile and move them all.
    const placed = async () => {
      const [button, within] = await Promise.all([add.boundingBox(), settings.boundingBox()]);
      return button && within ? { x: button.x - within.x, y: button.y - within.y, width: button.width } : undefined;
    };
    const box = await placed();
    await add.click();
    await page.getByRole('menu').getByRole('menuitemcheckbox', { name: 'Docs' }).click();
    await expect(page.locator('.app-toast').filter({ hasText: 'Docs added.' })).toBeVisible();
    await expect(settings.locator('[data-orchestration-project]').filter({ hasText: 'Docs' })).toBeVisible();
    await expect(record).toHaveCount(0);
    await expect(settings.getByText('Docs added.')).toHaveCount(0);
    expect(await placed()).toEqual(box);
    await capture('added-1440-light');
  } finally {
    await server.stop(page);
  }
});

test('HS2-0G80WD a remote orchestration project stops orchestrating a project after the shared confirmation', async ({
  page,
  browserName,
}) => {
  test.setTimeout(180_000);
  const { server, others, posted } = await openRemoteOrchestration(page);
  try {
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    await server.request(`${base}/project-type`, 'PUT', { project_type: 'orchestration', actor: { role: 'human' } });
    const added = await server.request<{ target_id: string }>(`${base}/hosted-projects`, 'POST', {
      checkout_id: others[0].id,
      label: 'Web',
      actor: { role: 'human' },
    });
    const wide = { width: 1440, height: 900 };
    await page.setViewportSize(wide);
    await page.goto('/');
    await page.getByRole('tab', { name: /Launch/ }).click();
    await expect(
      page.getByRole('group', { name: 'View mode' }).getByRole('button', { name: 'Orchestration View' }),
    ).toBeVisible();
    await showProjectSettings(page, 'General');
    await orchestrationNav(page).click();
    const settings = page.locator('[data-component="orchestration-settings"]');
    const webRow = settings.locator('[data-orchestration-project]').filter({ hasText: 'Web' });
    await expect(webRow).toBeVisible();
    const directory = join('target', 'visual-captures', 'hs2-0g80wd');
    await mkdir(directory, { recursive: true });
    const capture = (name: string) =>
      page.screenshot({ path: join(directory, `hs2-0g80wd-${name}-${browserName}.png`), animations: 'disabled' });
    // HS2-DXAB4K: the checkout is on the serving machine, so the row names that server, never this computer.
    await expect(webRow).toContainText('On Build host');
    await expect(webRow).not.toContainText('This Computer');
    await capture('row-1440');
    await page.emulateMedia({ colorScheme: 'dark' });
    await capture('row-1440-dark');
    await page.emulateMedia({ colorScheme: 'light' });
    await webRow.click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Stop Orchestrating…' }).click();
    const dialog = page.getByRole('alertdialog');
    await expect(dialog).toContainText('Stop orchestrating Web?');
    await capture('confirm-1440');
    // Cancelled: nothing is sent.
    await dialog.getByRole('button', { name: 'Cancel' }).click();
    expect(posted.filter((path) => path.includes(added.target_id))).toEqual([]);
    for (const [index, size] of [{ width: 1100, height: 900 }, { width: 390, height: 844 }, wide].entries()) {
      await page.setViewportSize(size);
      if (size.width === 390) {
        const hide = page.getByRole('button', { name: 'Hide Settings Sidebar', exact: true });
        if (await hide.isVisible()) await hide.click({ timeout: 3_000 }).catch(() => undefined);
      }
      await expect(webRow).toContainText('On Build host');
      await capture(`row-${String(size.width)}${index === 2 ? '-after-390' : ''}`);
      await webRow.click({ button: 'right' });
      await page.getByRole('menuitem', { name: 'Stop Orchestrating…' }).click();
      await expect(dialog).toBeVisible();
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
      await capture(`confirm-${String(size.width)}${index === 2 ? '-after-390' : ''}`);
      if (index < 2) await dialog.getByRole('button', { name: 'Cancel' }).click();
    }
    // Confirmed: removed through the hosted route on the serving machine.
    await page.getByRole('button', { name: 'Stop Orchestrating', exact: true }).click();
    await expect(webRow).toHaveCount(0);
    await expect(page.locator('.app-toast').filter({ hasText: 'Web removed.' })).toBeVisible();
    expect(posted).toContain(`DELETE ${base}/hosted-projects/${added.target_id}`);
    const declared = await server.request<{ targets: { label: string; state: string }[] }>(`${base}/targets`);
    expect(declared.targets.map((target) => `${target.label}:${target.state}`)).toEqual(['Web:removed']);
    await capture('removed-1440');
  } finally {
    await server.stop(page);
  }
});

test('HS2-7SHGQH a remote orchestration project row shows its project open from the same server', async ({ page }) => {
  test.setTimeout(180_000);
  const { server, others, remoteId } = await openRemoteOrchestration(page);
  try {
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    await server.request(`${base}/project-type`, 'PUT', { project_type: 'orchestration', actor: { role: 'human' } });
    await server.request(`${base}/hosted-projects`, 'POST', {
      checkout_id: others[0].id,
      label: 'Web',
      actor: { role: 'human' },
    });
    // The Web checkout is also open here, as a tab of the same remote server.
    const binding = remoteId.split(':')[1],
      webId = `remote:${binding}:${others[0].id}`,
      webPath = `/__hotsheet/project-api/${encodeURIComponent(webId)}`;
    await page.addInitScript(
      (roots) => {
        localStorage.setItem('hotsheet.open-projects', JSON.stringify(roots));
      },
      [remoteId, webId],
    );
    await page.route(/\/__hotsheet\/(?:remote\/)?projects\/open$/, (route) => {
      const root = (route.request().postDataJSON() as { root?: string } | null)?.root;
      const web = root === webId;
      return route.fulfill({
        status: 201,
        json: {
          id: web ? webId : remoteId,
          root: web ? webId : remoteId,
          name: web ? 'Build host · Web' : 'Build host · Launch',
          remoteHost: 'Build host',
          stores: [],
          apiPath: web ? webPath : `/__hotsheet/project-api/${encodeURIComponent(remoteId)}`,
        },
      });
    });
    await page.context().route(`**${webPath}/**`, async (route) => {
      const incoming = new URL(route.request().url());
      const path = incoming.pathname
        .slice(webPath.length)
        .replace(`/checkouts/${encodeURIComponent(webId)}`, `/checkouts/${others[0].id}`);
      try {
        const response = await proxyFetch(route, {
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
    await page.getByRole('tab', { name: /Launch/ }).click();
    await showProjectSettings(page, 'General');
    await orchestrationNav(page).click();
    const settings = page.locator('[data-component="orchestration-settings"]');
    const webRow = settings.locator('[data-orchestration-project]').filter({ hasText: 'Web' });
    await expect(webRow).toBeVisible();
    await webRow.click({ button: 'right' });
    // The row is the serving server's local checkout; here it is the remote Web tab.
    const show = page.getByRole('menuitem', { name: 'Show Project Tab' });
    await expect(show).toBeEnabled();
    await show.click();
    await expect(page.getByRole('tab', { name: /Web/ })).toHaveAttribute('aria-selected', 'true');
  } finally {
    await server.stop(page);
  }
});

/**
 * HS2-SHH7WW: two checkouts of one ticket store declared as sites of one project. Settings lists the
 * other site right after its project, labelled with the project, its machine and connection, through
 * the real routes; captured wide, medium, narrow and wide again in both schemes.
 */
test('HS2-SHH7WW Settings lists a project with its other sites after it', async ({ page, browserName }) => {
  test.setTimeout(240_000);
  const { server, projects } = await openProjects(page);
  try {
    const [owner, web, docs] = projects;
    const base = `/checkouts/${owner.id}/orchestration`;
    await server.request(`${base}/project-type`, 'PUT', { project_type: 'orchestration', actor: { role: 'human' } });
    const targets: Record<string, string> = {};
    for (const [project, label] of [
      [web, 'Widgets'],
      [docs, 'Widgets second clone'],
    ] as const)
      targets[label] = (
        await server.request<{ target_id: string }>(`${base}/projects`, 'POST', {
          destination: { kind: 'local', checkout_id: project.id },
          label,
          actor: { role: 'human' },
        })
      ).target_id;
    const { revision } = await server.request<{ revision: string }>(`${base}/targets`);
    // Both checkouts reach the same ticket store, so the second may be a site of the first.
    await server.request(`${base}/targets/${targets['Widgets second clone']}`, 'PATCH', {
      expected_revision: revision,
      label: 'Widgets second clone',
      site_of: targets.Widgets,
      actor: { role: 'human' },
    });
    const sites = await server.request<{ projects: { project_id: string; sites: { target_id: string }[] }[] }>(
      `${base}/sites`,
    );
    expect(sites.projects).toHaveLength(1);
    expect(sites.projects[0].sites.map((site) => site.target_id)).toEqual([
      targets.Widgets,
      targets['Widgets second clone'],
    ]);

    const wide = { width: 1440, height: 900 };
    await page.setViewportSize(wide);
    await openOrchestrationProject(page);
    await showProjectSettings(page, 'Orchestration');
    const settings = page.locator('[data-component="orchestration-settings"]');
    const rows = settings.locator('[data-orchestration-project]');
    await expect(rows).toHaveCount(2);
    await expect(rows.nth(0)).toContainText('Widgets');
    // HS2-M1T6KN: the project is a tree group naming its sites' machines, its home site first.
    await expect(settings.locator('[data-orchestration-project-group]')).toContainText('2 sites · This Computer');
    await expect(rows.nth(0)).toContainText('Home Site · Local on This Computer');
    await expect(rows.nth(1)).toContainText('Widgets second clone');
    await expect(rows.nth(1)).toContainText('Site · Local on This Computer');

    const directory = join('target', 'visual-captures', 'hs2-shh7ww');
    await mkdir(directory, { recursive: true });
    const hideRails = async (names: string[]) => {
      for (const name of names) {
        const hide = page.getByRole('button', { name, exact: true });
        if (await hide.isVisible()) await hide.click({ timeout: 3_000 }).catch(() => undefined);
        await expect(hide).toBeHidden();
      }
    };
    for (const scheme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      for (const size of [wide, { width: 1100, height: 900 }, { width: 390, height: 844 }, wide]) {
        await page.setViewportSize(size);
        if (size.width === 390) await hideRails(['Hide Settings Sidebar', 'Hide Ticket Inspector']);
        await expect(rows.nth(1)).toBeVisible();
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
        await page.screenshot({
          path: join(directory, `hs2-shh7ww-sites-${String(size.width)}-${scheme}-${browserName}.png`),
          animations: 'disabled',
        });
      }
    }
  } finally {
    await server.stop(page);
  }
});

/**
 * HS2-M1T6KN (Design v1 Concept A): Settings → Orchestration → Projects is a tree. Widgets has two sites, so it
 * is a group of its home site and its other site; Writer has one site and stays a leaf. Every group opens by
 * default, a person's close or open is remembered across a reload, the rows' actions open from a right-click,
 * from the context-menu key and from the inspector's actions menu, and the keyboard walks the tree. Through the
 * real shell and the real orchestration routes; captured wide, medium, narrow and wide again in both schemes.
 */
test('HS2-M1T6KN Projects is a tree of each project and its sites, remembered and keyboard driven', async ({
  page,
  browserName,
}) => {
  test.setTimeout(300_000);
  const { server, projects } = await openProjects(page, { names: ['Mobile', 'Writer'], open: 5 });
  try {
    const [owner, web, docs, , tablet] = projects;
    const base = `/checkouts/${owner.id}/orchestration`;
    await server.request(`${base}/project-type`, 'PUT', { project_type: 'orchestration', actor: { role: 'human' } });
    const targets: Record<string, string> = {};
    for (const [project, label] of [
      [web, 'Widgets'],
      [docs, 'Docs'],
      [tablet, 'Writer'],
    ] as const)
      targets[label] = (
        await server.request<{ target_id: string }>(`${base}/projects`, 'POST', {
          destination: { kind: 'local', checkout_id: project.id },
          label,
          actor: { role: 'human' },
        })
      ).target_id;
    const { revision } = await server.request<{ revision: string }>(`${base}/targets`);
    await server.request(`${base}/targets/${targets.Docs}`, 'PATCH', {
      expected_revision: revision,
      label: 'Docs',
      site_of: targets.Widgets,
      actor: { role: 'human' },
    });

    const wide = { width: 1440, height: 900 };
    await page.setViewportSize(wide);
    await openOrchestrationProject(page);
    await showProjectSettings(page, 'Orchestration');
    const settings = page.locator('[data-component="orchestration-settings"]');
    const tree = settings.getByRole('tree', { name: 'Orchestrated Projects' });
    const group = tree.locator('[data-orchestration-project-group]');
    const row = (label: string) =>
      tree.locator('[data-orchestration-project]').filter({ has: page.getByText(label, { exact: true }) });
    const items = tree.getByRole('treeitem');

    // Expanded by default: the group, its home site, its other site, then the single-site project as a leaf.
    await expect(group).toHaveAttribute('aria-expanded', 'true');
    await expect(group).toContainText('2 sites · This Computer');
    await expect(items).toHaveCount(4);
    await expect(items.nth(1)).toContainText('Home Site · Local on This Computer');
    await expect(items.nth(1)).toHaveAttribute('aria-level', '2');
    await expect(items.nth(2)).toContainText('Docs');
    await expect(items.nth(2)).toContainText('Site · Local on This Computer');
    await expect(items.nth(3)).toContainText('Writer');
    await expect(items.nth(3)).toHaveAttribute('aria-level', '1');
    await expect(items.nth(3)).not.toHaveAttribute('aria-expanded', /.*/);
    for (const index of [1, 2, 3]) await expect(items.nth(index)).toContainText('Bound');
    // No row carries a "…" button: the actions are in each row's context menu and the inspector.
    await expect(tree.getByRole('button', { name: /^Actions for / })).toHaveCount(0);

    // Closed with its arrow, then remembered across a reload, in this browser.
    await group.locator('[data-action="toggle-orchestration-project-group"]').click();
    await expect(group).toHaveAttribute('aria-expanded', 'false');
    await expect(items).toHaveCount(2);
    await expect(group).toContainText('2 sites · This Computer');
    await page.reload();
    await showProjectSettings(page, 'Orchestration');
    await expect(group).toHaveAttribute('aria-expanded', 'false');
    await expect(items).toHaveCount(2);

    // Choosing the group opens its home site's detail; the inspector's toolbar has the same actions.
    await group.click();
    await expect(group).toHaveAttribute('aria-selected', 'true');
    const detail = page.locator('[data-component="orchestration-project-detail"]');
    await expect(detail).toHaveAttribute('data-target-id', targets.Widgets);
    // The keyboard: Right opens the group and is remembered, Down walks into it, Left goes back to the group.
    await expect(group).toBeFocused();
    await page.keyboard.press('ArrowRight');
    await expect(group).toHaveAttribute('aria-expanded', 'true');
    await expect(items).toHaveCount(4);
    await page.keyboard.press('ArrowDown');
    await expect(row('Widgets')).toBeFocused();
    await page.keyboard.press('ArrowDown');
    await expect(row('Docs')).toBeFocused();
    // The context-menu key opens the focused row's actions; Escape closes them and keeps the row focused.
    await page.keyboard.press('Shift+F10');
    const docsMenu = page.getByRole('menu', { name: 'Actions for Docs' });
    await expect(docsMenu.getByRole('menuitem', { name: 'Make Docs the Home Site' })).toBeVisible();
    await expect(docsMenu.getByRole('menuitem', { name: 'Separate from Widgets' })).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(docsMenu).toBeHidden();
    await expect(row('Docs')).toBeFocused();
    await page.keyboard.press('ArrowLeft');
    await expect(group).toBeFocused();
    await page.keyboard.press('Shift+F10');
    const widgetsMenu = page.getByRole('menu', { name: 'Actions for Widgets' });
    await expect(widgetsMenu.getByRole('menuitem', { name: 'Add a Site' })).toBeVisible();
    await expect(widgetsMenu.getByRole('menuitem', { name: /the Home Site/ })).toHaveCount(0);
    await page.keyboard.press('Escape');
    await expect(widgetsMenu).toBeHidden();
    await expect(group).toBeFocused();
    await page.keyboard.press('End');
    await expect(row('Writer')).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(detail).toHaveAttribute('data-target-id', targets.Writer);
    await page.reload();
    await showProjectSettings(page, 'Orchestration');
    await expect(group).toHaveAttribute('aria-expanded', 'true');

    const directory = join('target', 'visual-captures', 'hs2-m1t6kn');
    await mkdir(directory, { recursive: true });
    const hideRails = async (names: string[]) => {
      for (const name of names) {
        const hide = page.getByRole('button', { name, exact: true });
        if (await hide.isVisible()) await hide.click({ timeout: 3_000 }).catch(() => undefined);
        await expect(hide).toBeHidden();
      }
    };
    const capture = async (state: string) => {
      for (const scheme of ['light', 'dark'] as const) {
        await page.emulateMedia({ colorScheme: scheme });
        for (const [index, size] of [wide, { width: 1100, height: 900 }, { width: 390, height: 844 }, wide].entries()) {
          await page.setViewportSize(size);
          if (size.width === 390) await hideRails(['Hide Settings Sidebar', 'Hide Ticket Inspector']);
          await expect(group).toBeVisible();
          expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
          await page.screenshot({
            path: join(
              directory,
              `hs2-m1t6kn-${state}-${String(size.width)}${index === 3 ? '-again' : ''}-${scheme}-${browserName}.png`,
            ),
            animations: 'disabled',
          });
        }
      }
      await page.emulateMedia({ colorScheme: 'light' });
    };
    await capture('expanded');

    // A right-click on the closed group: Add a Site → Mobile opens the group again beside the toast.
    await group.locator('[data-action="toggle-orchestration-project-group"]').click();
    await expect(group).toHaveAttribute('aria-expanded', 'false');
    await capture('collapsed');
    await group.click({ button: 'right' });
    await widgetsMenu.getByRole('menuitem', { name: 'Add a Site' }).click();
    const submenu = page.getByRole('menu', { name: 'Add a Site' });
    await submenu.getByRole('menuitem', { name: /Mobile/ }).click();
    await expect(page.locator('.app-toast').filter({ hasText: 'Mobile added as a site of Widgets.' })).toBeVisible();
    await expect(group).toHaveAttribute('aria-expanded', 'true');
    await expect(group).toContainText('3 sites · This Computer');
    await expect(items).toHaveCount(5);
    await capture('added');

    // A right-click on a site: Separate from Widgets makes Mobile a leaf of its own.
    await row('Mobile').click({ button: 'right' });
    await page
      .getByRole('menu', { name: 'Actions for Mobile' })
      .getByRole('menuitem', { name: 'Separate from Widgets' })
      .click();
    await expect(page.locator('.app-toast').filter({ hasText: 'Mobile is its own project again.' })).toBeVisible();
    await expect(group).toContainText('2 sites');
    await expect(row('Mobile')).toHaveAttribute('aria-level', '1');

    // The inspector's actions menu: Make Docs the Home Site moves the home, and the group follows it.
    await row('Docs').click();
    await expect(detail).toHaveAttribute('data-target-id', targets.Docs);
    await page.getByRole('button', { name: 'Actions for Docs' }).click();
    await page
      .getByRole('menu', { name: 'Actions for Docs' })
      .getByRole('menuitem', { name: 'Make Docs the Home Site' })
      .click();
    await expect(
      page.locator('.app-toast').filter({ hasText: 'Docs is the home site of the project now.' }),
    ).toBeVisible();
    await expect(group).toHaveAttribute('data-orchestration-project-group', targets.Docs);
    // Mobile, a project of its own again, sorts first; the group's children follow it.
    const children = tree.locator('[role="treeitem"][aria-level="2"]');
    await expect(children.nth(0)).toContainText('Docs');
    await expect(children.nth(0)).toContainText('Home Site · ');
    await expect(children.nth(1)).toContainText('Widgets');
  } finally {
    await server.stop(page);
  }
});

/**
 * HS2-SYZB33: coordination sessions start only on a project's home site. A site's Limits points at
 * the home; a role a stored policy still lists on the site is shown with a note that it has no
 * effect and can only be cleared, through the real policy route. The home's Limits are unchanged.
 */
test('HS2-SYZB33 a site Limits point Coordination Sessions at the project home', async ({ page, browserName }) => {
  test.setTimeout(240_000);
  const { server, projects } = await openProjects(page);
  try {
    const [owner, web, docs] = projects;
    const base = `/checkouts/${owner.id}/orchestration`;
    await server.request(`${base}/project-type`, 'PUT', { project_type: 'orchestration', actor: { role: 'human' } });
    const targets: Record<string, string> = {};
    for (const [project, label] of [
      [web, 'Widgets'],
      [docs, 'Widgets second clone'],
    ] as const)
      targets[label] = (
        await server.request<{ target_id: string }>(`${base}/projects`, 'POST', {
          destination: { kind: 'local', checkout_id: project.id },
          label,
          actor: { role: 'human' },
        })
      ).target_id;
    const { revision } = await server.request<{ revision: string }>(`${base}/targets`);
    await server.request(`${base}/targets/${targets['Widgets second clone']}`, 'PATCH', {
      expected_revision: revision,
      label: 'Widgets second clone',
      site_of: targets.Widgets,
      actor: { role: 'human' },
    });
    const site = targets['Widgets second clone'];
    // A policy stored before the grouping: the site still lists a dispatcher.
    await server.request(`${base}/policy`, 'PUT', {
      expected_revision: null,
      policy: {
        machine: { max_sessions: 4 },
        tools: [{ tool: 'codex', max_sessions: 3 }],
        targets: [
          { target_id: targets.Widgets, tools: [{ tool: 'codex' }], max_sessions: 2 },
          { target_id: site, tools: [{ tool: 'codex' }], max_sessions: 2 },
        ],
        placement: { role_sessions: [{ target_id: site, roles: ['dispatcher'] }] },
      },
      actor: { role: 'human' },
    });
    const validated = await server.request<{ valid: boolean; warnings?: { code: string; subject: string }[] }>(
      `${base}/policy/validate`,
      'POST',
      {},
    );
    expect(validated.valid).toBe(true);
    expect(validated.warnings?.map((warning) => [warning.code, warning.subject])).toEqual([
      ['role_sessions_not_home', site],
    ]);
    type Stored = { policy: { placement?: { role_sessions?: { target_id: string; roles: string[] }[] } } };
    const roles = async () => (await server.request<Stored>(`${base}/policy`)).policy.placement?.role_sessions;

    const wide = { width: 1440, height: 900 };
    await page.setViewportSize(wide);
    await openOrchestrationProject(page);
    await showProjectSettings(page, 'Orchestration');
    const settings = page.locator('[data-component="orchestration-settings"]');
    const open = (label: string) =>
      settings
        .locator('[data-orchestration-project]')
        .filter({ has: page.getByText(label, { exact: true }) })
        .click();
    await open('Widgets second clone');
    const limits = page.locator('[data-component="orchestration-project-limits"]');
    await openAdvancedLimits(page);
    const dispatcher = limits.getByRole('checkbox', { name: 'Dispatcher' }),
      integrator = limits.getByRole('checkbox', { name: 'Integrator' });
    await expect(limits).toContainText("Coordination sessions start only on Widgets, this project's home site.");
    await expect(limits).toContainText('The roles checked here have no effect; clear them.');
    await expect(dispatcher).toBeChecked();
    await expect(dispatcher).toBeEnabled();

    const directory = join('target', 'visual-captures', 'hs2-syzb33');
    await mkdir(directory, { recursive: true });
    const capture = async (name: string) => {
      await integrator.scrollIntoViewIfNeeded();
      await page.screenshot({ path: join(directory, `hs2-syzb33-${name}-${browserName}.png`), animations: 'disabled' });
    };
    await capture('site-stored-1440');
    // Cleared: the entry keeps the choice of none (HS2-R2N33M), and the choice is unavailable, pointing at
    // the home.
    await dispatcher.uncheck();
    await limits.getByRole('button', { name: 'Apply Limits' }).click();
    await expect.poll(roles).toEqual([{ target_id: site, roles: [] }]);
    await expect(limits).not.toContainText('no effect');
    await expect(dispatcher).not.toBeChecked();
    await expect(dispatcher).toBeDisabled();
    await expect(integrator).toBeDisabled();
    for (const [index, size] of [{ width: 1100, height: 900 }, { width: 390, height: 844 }, wide].entries()) {
      await page.setViewportSize(size);
      if (size.width === 390) {
        const hide = page.getByRole('button', { name: 'Hide Settings Sidebar', exact: true });
        if (await hide.isVisible()) await hide.click({ timeout: 3_000 }).catch(() => undefined);
        const show = page.getByRole('button', { name: 'Show Project Detail', exact: true });
        if (await show.isVisible()) await show.click();
      }
      await expect(integrator).toBeVisible();
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
      await capture(`site-${String(size.width)}${index === 2 ? '-after-390' : ''}`);
    }
    await page.emulateMedia({ colorScheme: 'dark' });
    await capture('site-1440-dark');
    await page.emulateMedia({ colorScheme: 'light' });

    // The home's choice is as before.
    await open('Widgets');
    await expect(limits).toContainText('Auto: follows placement, both when they help.');
    await expect(dispatcher).toBeEnabled();
    // HS2-R2N33M: the home is Auto (both); choosing the dispatcher only stores it.
    await expect(dispatcher).toBeChecked();
    await integrator.uncheck();
    await limits.getByRole('button', { name: 'Apply Limits' }).click();
    await expect.poll(roles).toEqual([
      { target_id: site, roles: [] },
      { target_id: targets.Widgets, roles: ['dispatcher'] },
    ]);
  } finally {
    await server.stop(page);
  }
});

/**
 * HS2-D4G81Q: what each site's machine is, in Settings › Orchestration. Two checkouts of one ticket store
 * grouped as sites of one project; the sites view reads each one's machine through the real gate, the
 * rows add its OS and the detail lists every site with the machine rows, which the capacity region then
 * leaves out so they show in one place.
 */
test('HS2-D4G81Q Settings shows each site OS and tools', async ({ page, browserName }) => {
  test.setTimeout(240_000);
  const { server, projects } = await openProjects(page);
  try {
    const [owner, web, docs] = projects;
    const base = `/checkouts/${owner.id}/orchestration`;
    await server.request(`${base}/project-type`, 'PUT', { project_type: 'orchestration', actor: { role: 'human' } });
    const targets: Record<string, string> = {};
    for (const [project, label] of [
      [web, 'Widgets'],
      [docs, 'Widgets second clone'],
    ] as const)
      targets[label] = (
        await server.request<{ target_id: string }>(`${base}/projects`, 'POST', {
          destination: { kind: 'local', checkout_id: project.id },
          label,
          actor: { role: 'human' },
        })
      ).target_id;
    const { revision } = await server.request<{ revision: string }>(`${base}/targets`);
    await server.request(`${base}/targets/${targets['Widgets second clone']}`, 'PATCH', {
      expected_revision: revision,
      label: 'Widgets second clone',
      site_of: targets.Widgets,
      actor: { role: 'human' },
    });
    // The headless read the client shares: each bound site's machine, through the gate.
    const system = /(macOS|Linux|Windows)[^,·]*, (arm64|x86_64)/;
    const read = await server.request<{
      projects: { sites: { target_id: string; capabilities?: { os: string; arch: string } }[] }[];
    }>(`${base}/sites`, 'POST', { capabilities: true, actor: { role: 'human', id: 'maintainer' } });
    expect(read.projects[0].sites.map((site) => [site.target_id, Boolean(site.capabilities?.os)])).toEqual([
      [targets.Widgets, true],
      [targets['Widgets second clone'], true],
    ]);
    // Nobody was asked for these reads.
    expect((await server.request<{ requests: unknown[] }>(`${base}/permissions`)).requests).toEqual([]);

    const wide = { width: 1440, height: 900 };
    await page.setViewportSize(wide);
    await openOrchestrationProject(page);
    await showProjectSettings(page, 'Orchestration');
    const settings = page.locator('[data-component="orchestration-settings"]');
    const rows = settings.locator('[data-orchestration-project]');
    await expect(rows).toHaveCount(2);
    // Each site row adds its machine's OS once the sites view is read.
    await expect(rows.nth(0)).toContainText('Home Site · Local on This Computer · ');
    await expect(rows.nth(0)).toContainText(system);
    await expect(rows.nth(1)).toContainText('Site · Local on This Computer · ');
    await expect(rows.nth(1)).toContainText(system);

    // The home's detail lists both sites with the machine rows of the capacity region.
    await rows.nth(0).click();
    const detail = page.locator('[data-component="orchestration-project-detail"]');
    await expect(detail).toHaveAttribute('data-target-id', targets.Widgets);
    const sites = detail.getByRole('region', { name: 'Sites' });
    const tables = sites.locator('[data-orchestration-site]');
    await expect(tables).toHaveCount(2);
    await expect(tables.nth(0)).toContainText('Widgets · Home');
    await expect(tables.nth(1)).toContainText('Widgets second clone');
    for (const index of [0, 1]) {
      const table = tables.nth(index);
      await expect(table).toContainText('Local on This Computer');
      await expect(table).toContainText(system);
      for (const label of ['System', 'GPU', 'AI Tools', 'Toolchains'])
        await expect(table.getByText(label, { exact: true })).toBeVisible();
    }
    // One place for what a machine is: with several sites the capacity region does not repeat it.
    const capacityRegion = detail.getByRole('region', { name: 'Capacity and Admission' });
    await expect(capacityRegion).toContainText('Free Memory');
    await expect(capacityRegion).not.toContainText(system);
    await expect(capacityRegion.getByText('Toolchains', { exact: true })).toHaveCount(0);
    // The site's own detail shows the same Sites section.
    await rows.nth(1).click();
    await expect(detail).toHaveAttribute('data-target-id', targets['Widgets second clone']);
    await expect(tables).toHaveCount(2);
    await rows.nth(0).click();
    await expect(detail).toHaveAttribute('data-target-id', targets.Widgets);

    const directory = join('target', 'visual-captures', 'hs2-d4g81q');
    await mkdir(directory, { recursive: true });
    for (const scheme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      for (const [index, size] of [wide, { width: 1100, height: 900 }, { width: 390, height: 844 }, wide].entries()) {
        await page.setViewportSize(size);
        const name = `${String(size.width)}${index === 3 ? '-after-390' : ''}-${scheme}-${browserName}`;
        if (size.width === 390) {
          const hide = page.getByRole('button', { name: 'Hide Settings Sidebar', exact: true });
          if (await hide.isVisible()) await hide.click({ timeout: 3_000 }).catch(() => undefined);
          const hideInspector = page.getByRole('button', { name: 'Hide Project Detail', exact: true });
          if (await hideInspector.isVisible()) await hideInspector.click({ timeout: 3_000 }).catch(() => undefined);
          // The rows first, then the detail sheet with the Sites section.
          await expect(rows.nth(1)).toContainText(system);
          expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
          await page.screenshot({ path: join(directory, `hs2-d4g81q-rows-${name}.png`), animations: 'disabled' });
          const show = page.getByRole('button', { name: 'Show Project Detail', exact: true });
          if (await show.isVisible()) await show.click();
          // The capacity region, which leaves the machine rows to the Sites tables, at the same width.
          const capacity = detail.getByRole('region', { name: 'Capacity and Admission' });
          await capacity.getByText('Free Memory', { exact: true }).scrollIntoViewIfNeeded();
          await page.screenshot({ path: join(directory, `hs2-d4g81q-capacity-${name}.png`), animations: 'disabled' });
        } else {
          await expect(rows.nth(1)).toContainText(system);
          await page.screenshot({ path: join(directory, `hs2-d4g81q-rows-${name}.png`), animations: 'disabled' });
        }
        await expect(tables.nth(1)).toContainText(system);
        // The Sites header with the end of the capacity region above it.
        await sites.evaluate((element) => {
          element.scrollIntoView({ block: 'start' });
          let scroller = element.parentElement;
          while (scroller && scroller.scrollHeight <= scroller.clientHeight) scroller = scroller.parentElement;
          scroller?.scrollBy(0, -120);
        });
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
        await page.screenshot({ path: join(directory, `hs2-d4g81q-sites-${name}.png`), animations: 'disabled' });
      }
    }
  } finally {
    await server.stop(page);
  }
});

/**
 * HS2-NAA3HV: a site row's Make <site> the Home Site through the real shell and the real workflow: the
 * site becomes the project's home at one manifest revision, the old home becomes its site, and the policy's
 * per-target entry moves with the home. Captured wide, medium, narrow and wide again in both schemes.
 */
test('HS2-NAA3HV makes a site the home site of its project from Settings', async ({ page, browserName }) => {
  test.setTimeout(240_000);
  const { server, projects } = await openProjects(page);
  try {
    const [owner, web, docs] = projects;
    const base = `/checkouts/${owner.id}/orchestration`;
    const person = { role: 'human' };
    await server.request(`${base}/project-type`, 'PUT', { project_type: 'orchestration', actor: person });
    const widgets = (
      await server.request<{ target_id: string }>(`${base}/projects`, 'POST', {
        destination: { kind: 'local', checkout_id: web.id },
        label: 'Widgets',
        actor: person,
      })
    ).target_id;
    const laptop = (
      await server.request<{ target_id: string }>(`${base}/projects`, 'POST', {
        destination: { kind: 'local', checkout_id: docs.id },
        label: 'Widgets laptop',
        site_of: widgets,
        actor: person,
      })
    ).target_id;
    await server.request(`${base}/policy`, 'PUT', {
      expected_revision: null,
      actor: person,
      policy: { targets: [{ target_id: widgets, max_sessions: 1 }] },
    });
    const declared = async () =>
      (await server.request<{ targets: { target_id: string; site_of?: string }[] }>(`${base}/targets`)).targets.map(
        (target) =>
          `${target.target_id === widgets ? 'Widgets' : 'Laptop'}<${target.site_of === widgets ? 'Widgets' : target.site_of === laptop ? 'Laptop' : ''}`,
      );
    expect(await declared()).toEqual(['Widgets<', 'Laptop<Widgets']);

    const wide = { width: 1440, height: 900 };
    await page.setViewportSize(wide);
    await openOrchestrationProject(page);
    await showProjectSettings(page, 'Orchestration');
    const settings = page.locator('[data-component="orchestration-settings"]');
    const rows = settings.locator('[data-orchestration-project]');
    await expect(rows).toHaveCount(2);
    await expect(rows.nth(1)).toContainText('Site · ');
    const directory = join('target', 'visual-captures', 'hs2-naa3hv');
    await mkdir(directory, { recursive: true });
    // A project row has no Make … the Home Site; a site row has it beside Separate from Widgets.
    await rows.nth(0).click({ button: 'right' });
    await expect(page.getByRole('menuitem', { name: /the Home Site/ })).toHaveCount(0);
    await page.keyboard.press('Escape');
    const openSiteMenu = async () => {
      await rows.nth(1).click({ button: 'right' });
      const menu = page.getByRole('menu', { name: 'Actions for Widgets laptop' });
      await expect(menu.getByRole('menuitem', { name: 'Make Widgets laptop the Home Site' })).toBeVisible();
      await expect(menu.getByRole('menuitem', { name: 'Separate from Widgets' })).toBeVisible();
      return menu;
    };
    for (const scheme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      for (const [index, size] of [wide, { width: 1100, height: 900 }, { width: 390, height: 844 }, wide].entries()) {
        await page.setViewportSize(size);
        if (size.width === 390)
          for (const name of ['Hide Settings Sidebar', 'Hide Project Detail']) {
            const hide = page.getByRole('button', { name, exact: true });
            if (await hide.isVisible()) await hide.click({ timeout: 3_000 }).catch(() => undefined);
          }
        const menu = await openSiteMenu();
        const box = await menu.boundingBox();
        expect(box!.x + box!.width).toBeLessThanOrEqual(size.width + 1);
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
        await page.screenshot({
          path: join(
            directory,
            `hs2-naa3hv-menu-${String(size.width)}${index === 3 ? '-after-390' : ''}-${scheme}-${browserName}.png`,
          ),
          animations: 'disabled',
        });
        await page.keyboard.press('Escape');
        await expect(menu).toBeHidden();
      }
    }
    await page.emulateMedia({ colorScheme: 'light' });
    await page.setViewportSize(wide);
    const menu = await openSiteMenu();
    await menu.getByRole('menuitem', { name: 'Make Widgets laptop the Home Site' }).click();
    // No confirmation: the same action on the old home undoes it.
    // Action results arrive as a toast, never an inline status line (HS2-1K6122).
    await expect(
      page.locator('.app-toast').filter({ hasText: 'Widgets laptop is the home site of the project now.' }),
    ).toBeVisible();
    await expect(page.getByRole('alertdialog')).toHaveCount(0);
    expect(await declared()).toEqual(['Widgets<Laptop', 'Laptop<']);
    await expect(rows.nth(0)).toContainText('Widgets laptop');
    await expect(settings.locator('[data-orchestration-project-group]')).toContainText('2 sites');
    await expect(rows.nth(0)).toContainText('Home Site · ');
    await expect(rows.nth(1)).toContainText('Site · ');
    // The policy's entry for the old home moved with the home.
    const policy = await server.request<{ policy: { targets: { target_id: string }[] } }>(`${base}/policy`);
    expect(policy.policy.targets.map((target) => target.target_id)).toEqual([laptop]);
    for (const scheme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      await page.screenshot({
        path: join(directory, `hs2-naa3hv-made-home-1440-${scheme}-${browserName}.png`),
        animations: 'disabled',
      });
    }
  } finally {
    await server.stop(page);
  }
});

/**
 * HS2-E9VVQF: Add a Site on a project row, Separate from <project> on a site row, and + Add Project's
 * duplicate question, through the real shell and the real workflows. Launch orchestrates; Web, Docs and
 * Clone are checkouts of one ticket store and Other of a second one.
 */
test('HS2-E9VVQF adds and separates sites from Settings and asks only for a duplicate', async ({
  page,
  browserName,
}) => {
  test.setTimeout(300_000);
  const { server, projects } = await openProjects(page, { names: ['Clone'], otherStore: ['Other'], open: 5 });
  try {
    const [owner, web] = projects;
    const base = `/checkouts/${owner.id}/orchestration`;
    await server.request(`${base}/project-type`, 'PUT', { project_type: 'orchestration', actor: { role: 'human' } });
    const widgets = (
      await server.request<{ target_id: string }>(`${base}/projects`, 'POST', {
        destination: { kind: 'local', checkout_id: web.id },
        label: 'Widgets',
        actor: { role: 'human' },
      })
    ).target_id;
    const declared = async () =>
      (
        await server.request<{ targets: { label: string; state: string; site_of?: string }[] }>(`${base}/targets`)
      ).targets
        .filter((target) => target.state === 'active')
        .map(
          (target) =>
            `${target.label}${target.site_of ? ` < ${target.site_of === widgets ? 'Widgets' : target.site_of}` : ''}`,
        );

    const wide = { width: 1440, height: 900 };
    await page.setViewportSize(wide);
    await openOrchestrationProject(page);
    await showProjectSettings(page, 'Orchestration');
    const settings = page.locator('[data-component="orchestration-settings"]');
    const rows = settings.locator('[data-orchestration-project]');
    await expect(rows).toHaveCount(1);
    const directory = join('target', 'visual-captures', 'hs2-e9vvqf');
    await mkdir(directory, { recursive: true });

    // A project row's "…" → Add a Site: the open projects of its ticket store only.
    const openSites = async (row = rows.nth(0)) => {
      await row.click({ button: 'right' });
      await page.getByRole('menuitem', { name: 'Add a Site' }).click();
      const submenu = page.getByRole('menu', { name: 'Add a Site' });
      await expect(submenu).toBeVisible();
      return submenu;
    };
    let submenu = await openSites();
    await expect(submenu.getByRole('group', { name: 'Open Projects in the Same Ticket Store' })).toBeVisible();
    await expect(submenu.getByRole('menuitem', { name: /Docs/ })).toBeVisible();
    await expect(submenu.getByRole('menuitem', { name: /Clone/ })).toBeVisible();
    await expect(submenu.getByRole('menuitem', { name: /Other/ })).toHaveCount(0);
    // Web is already added, so it is not offered.
    await expect(submenu.getByRole('menuitem', { name: /Web/ })).toHaveCount(0);
    await page.screenshot({
      path: join(directory, `hs2-e9vvqf-add-site-1440-light-${browserName}.png`),
      animations: 'disabled',
    });
    await page.keyboard.press('Escape');
    await page.keyboard.press('Escape');
    await expect(submenu).toBeHidden();
    // Visual QA: the Add a Site submenu at each width, and back, in both schemes.
    for (const scheme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      for (const [index, size] of [wide, { width: 1100, height: 900 }, { width: 390, height: 844 }, wide].entries()) {
        await page.setViewportSize(size);
        if (size.width === 390)
          for (const name of ['Hide Settings Sidebar', 'Hide Project Detail']) {
            const hide = page.getByRole('button', { name, exact: true });
            if (await hide.isVisible()) await hide.click({ timeout: 3_000 }).catch(() => undefined);
          }
        const name = `${String(size.width)}${index === 3 ? '-after-390' : ''}-${scheme}-${browserName}`;
        await expect(rows.first()).toBeVisible();
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
        await page.screenshot({ path: join(directory, `hs2-e9vvqf-rows-${name}.png`), animations: 'disabled' });
        submenu = await openSites();
        // The submenu stays inside the viewport.
        const box = await submenu.boundingBox();
        expect(box).not.toBeNull();
        expect(box!.x).toBeGreaterThanOrEqual(0);
        expect(box!.x + box!.width).toBeLessThanOrEqual(size.width + 1);
        await page.screenshot({ path: join(directory, `hs2-e9vvqf-add-site-${name}.png`), animations: 'disabled' });
        await page.keyboard.press('Escape');
        await page.keyboard.press('Escape');
        await expect(submenu).toBeHidden();
      }
    }
    await page.emulateMedia({ colorScheme: 'light' });
    await page.setViewportSize(wide);
    submenu = await openSites();
    // Show All Projects keeps the row's menu open; Add a Site then lists the other store's project, marked.
    await submenu.getByRole('menuitemcheckbox', { name: 'Show All Projects' }).click();
    const rowMenu = page.getByRole('menu', { name: 'Actions for Widgets' });
    await expect(rowMenu).toBeVisible();
    await rowMenu.getByRole('menuitem', { name: 'Add a Site' }).click();
    await expect(submenu.getByRole('menuitemcheckbox', { name: 'Show All Projects' })).toHaveAttribute(
      'aria-checked',
      'true',
    );
    await expect(submenu.getByRole('menuitem', { name: /Other/ })).toContainText('Another ticket store');
    await page.screenshot({
      path: join(directory, `hs2-e9vvqf-show-all-1440-light-${browserName}.png`),
      animations: 'disabled',
    });
    // Another store is refused by the server, before anything is declared, in the message line.
    await submenu.getByRole('menuitem', { name: /Other/ }).click();
    await expect(settings.getByRole('alert')).toHaveText(
      'Could not add Other as a site of Widgets. It reaches a different ticket store than this project.',
    );
    expect(await declared()).toEqual(['Widgets']);

    // Docs, of the same store: declared and bound as a site of Widgets in one step.
    submenu = await openSites();
    await submenu.getByRole('menuitem', { name: /Docs/ }).click();
    await expect(page.locator('.app-toast').filter({ hasText: 'Docs added as a site of Widgets.' })).toBeVisible();
    await expect(settings.getByRole('alert')).toHaveCount(0);
    // HS2-1K6122: the outcome is the toast; nothing about it is inlined under + Add Project.
    await expect(settings.getByText('Docs added as a site of Widgets.')).toHaveCount(0);
    await page.waitForFunction(() =>
      document
        .getAnimations()
        .every(
          (animation) => animation.playState !== 'running' || animation.effect?.getTiming().iterations === Infinity,
        ),
    );
    await page.screenshot({ path: test.info().outputPath('hs2-1k6122-site-added-toast.png') });
    expect(await declared()).toEqual(['Widgets', 'Docs < Widgets']);
    await expect(rows).toHaveCount(2);
    await expect(rows.nth(1)).toContainText('Site · ');

    // A site row offers Separate from Widgets, not Add a Site; its context menu has the same entries.
    await rows.nth(1).click({ button: 'right' });
    const siteMenu = page.getByRole('menu', { name: 'Actions for Docs' });
    await expect(siteMenu.getByRole('menuitem', { name: 'Separate from Widgets' })).toBeVisible();
    await expect(siteMenu.getByRole('menuitem', { name: 'Add a Site' })).toHaveCount(0);
    await page.screenshot({
      path: join(directory, `hs2-e9vvqf-separate-1440-light-${browserName}.png`),
      animations: 'disabled',
    });
    await siteMenu.getByRole('menuitem', { name: 'Separate from Widgets' }).click();
    // No confirmation: the grouping is undone at once.
    await expect(page.locator('.app-toast').filter({ hasText: 'Docs is its own project again.' })).toBeVisible();
    // Neither row is a site any more, so neither lists a site's machine.
    await expect(rows.filter({ hasText: /arm64|x86_64/ })).toHaveCount(0);
    await expect(page.getByRole('alertdialog')).toHaveCount(0);
    expect(await declared()).toEqual(['Widgets', 'Docs']);
    await expect(rows.nth(0)).not.toContainText('sites');

    // + Add Project → Clone, a duplicate of an added project's store: asked, and Cancel adds nothing.
    const addClone = async () => {
      await settings.getByRole('button', { name: 'Add Project' }).click();
      await page.getByRole('menu', { name: 'Add Project' }).getByRole('menuitemcheckbox', { name: /Clone/ }).click();
      const question = page.getByRole('alertdialog', { name: 'Is Clone another site of Docs?' });
      await expect(question).toBeVisible();
      return question;
    };
    let question = await addClone();
    const buttons = question.getByRole('button');
    await expect(buttons.filter({ hasText: /^(Cancel|Add as a Separate Project|Add as a Site of Docs)$/ })).toHaveText([
      'Cancel',
      'Add as a Separate Project',
      'Add as a Site of Docs',
    ]);
    await page.screenshot({
      path: join(directory, `hs2-e9vvqf-duplicate-1440-light-${browserName}.png`),
      animations: 'disabled',
    });
    {
      const confirmBox = await question
        .getByRole('button', { name: 'Add as a Site of Docs', exact: true })
        .boundingBox();
      const dialogBox = await question.boundingBox();
      expect(confirmBox!.x + confirmBox!.width).toBeLessThanOrEqual(dialogBox!.x + dialogBox!.width);
    }
    await page.setViewportSize({ width: 390, height: 844 });
    const confirmBox = await question.getByRole('button', { name: 'Add as a Site of Docs', exact: true }).boundingBox();
    const dialogBox = await question.boundingBox();
    expect(confirmBox!.x + confirmBox!.width).toBeLessThanOrEqual(dialogBox!.x + dialogBox!.width);
    await page.screenshot({
      path: join(directory, `hs2-e9vvqf-duplicate-390-light-${browserName}.png`),
      animations: 'disabled',
    });
    await page.setViewportSize(wide);
    await question.getByRole('button', { name: 'Cancel', exact: true }).click();
    await expect(question).toHaveCount(0);
    expect(await declared()).toEqual(['Widgets', 'Docs']);
    // Add as a Site of Docs.
    question = await addClone();
    await question.getByRole('button', { name: 'Add as a Site of Docs', exact: true }).click();
    await expect(page.locator('.app-toast').filter({ hasText: 'Clone added as a site of Docs.' })).toBeVisible();
    expect((await declared()).sort()).toEqual(['Clone < ' + (await docsId()), 'Docs', 'Widgets'].sort());

    // + Add Project → Other, whose store no added project uses: added at once, no question.
    await settings.getByRole('button', { name: 'Add Project' }).click();
    await page.getByRole('menu', { name: 'Add Project' }).getByRole('menuitemcheckbox', { name: /Other/ }).click();
    await expect(page.locator('.app-toast').filter({ hasText: 'Other added.' })).toBeVisible();
    await expect(page.getByRole('alertdialog')).toHaveCount(0);
  } finally {
    await server.stop(page);
  }

  async function docsId() {
    const { targets } = await server.request<{ targets: { target_id: string; label: string }[] }>(
      `/checkouts/${projects[0].id}/orchestration/targets`,
    );
    return targets.find((target) => target.label === 'Docs')!.target_id;
  }
});

test('HS2-W27N1D catalog demos render the production components and run their controller', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.setViewportSize({ width: 1280, height: 900 });
  for (const [component, name] of [
    ['project-general-settings', 'ProjectGeneralSettings'],
    ['orchestration-policy-settings', 'OrchestrationPolicySettings'],
    ['orchestration-project-detail', 'OrchestrationProjectDetail'],
  ]) {
    await page.goto(`/ux-demo?component=${component}`);
    await expect(page.getByRole('region', { name: `${name} Examples` })).toBeVisible();
  }
  await page.goto('/ux-demo?component=orchestration-settings');
  const interactive = page
    .getByRole('region', { name: 'OrchestrationSettings Examples' })
    .getByRole('region', { name: 'Interactive' });
  await expect(interactive.locator('[data-orchestration-project]')).toHaveCount(3);
  await interactive.getByRole('button', { name: 'Add Project' }).click();
  await page.getByRole('menu').getByText('Mobile').click();
  await expect(interactive.locator('[data-orchestration-project]')).toHaveCount(4);
  // HS2-E9VVQF: Mobile reaches API's ticket store, so the demo's duplicate question adds it as a site.
  // HS2-1K6122: the outcome is the app's toast, which the catalog demo does not host; the row shows the site.
  // HS2-M1T6KN: API is now a group of its home site and Mobile.
  await expect(interactive.locator('[data-orchestration-project-group]')).toHaveCount(1);
  await expect(interactive.locator('[data-orchestration-project]').filter({ hasText: 'Mobile' })).toContainText(
    'Site ·',
  );
  // HS2-DXAB4K: a project open from another server names that server on its rows.
  const hosted = page
    .getByRole('region', { name: 'OrchestrationSettings Examples' })
    .getByRole('region', { name: 'Open From Another Server' });
  await expect(
    hosted
      .locator('[data-orchestration-project]')
      .filter({ hasText: /On Build Host/ })
      .first(),
  ).toBeVisible();
  await expect(hosted).not.toContainText('This Computer');
  await page.screenshot({ path: test.info().outputPath('orchestration-settings-demo.png'), fullPage: true });
  expect(errors).toEqual([]);
});

/**
 * HS2-Y737GM, HS2-25KT42, HS2-NGP2AK, HS2-1X1TXC, HS2-NPAZQW: a project detail of a project with two
 * sites, through the real shell and policy route: Binding names every site, the Limits help explains
 * each limit, the Auto limits stay closed under Advanced, a model family and a resource pattern are
 * chosen in their comboboxes, and the server stores both. Captured wide and narrow, light and dark.
 */
test('HS2-Y737GM HS2-25KT42 HS2-NGP2AK HS2-1X1TXC HS2-NPAZQW the project detail limits read Auto and take patterns', async ({
  page,
  browserName,
}) => {
  test.setTimeout(300_000);
  const { server, projects } = await openProjects(page);
  try {
    const [owner, web, docs] = projects;
    const base = `/checkouts/${owner.id}/orchestration`;
    await server.request(`${base}/project-type`, 'PUT', { project_type: 'orchestration', actor: { role: 'human' } });
    const targets: Record<string, string> = {};
    for (const [project, label] of [
      [web, 'Widgets'],
      [docs, 'Widgets second clone'],
    ] as const)
      targets[label] = (
        await server.request<{ target_id: string }>(`${base}/projects`, 'POST', {
          destination: { kind: 'local', checkout_id: project.id },
          label,
          actor: { role: 'human' },
        })
      ).target_id;
    const { revision } = await server.request<{ revision: string }>(`${base}/targets`);
    await server.request(`${base}/targets/${targets['Widgets second clone']}`, 'PATCH', {
      expected_revision: revision,
      label: 'Widgets second clone',
      site_of: targets.Widgets,
      actor: { role: 'human' },
    });
    // The second site requires an exact VM, which declares it for the Required Resources choices.
    await server.request(`${base}/policy`, 'PUT', {
      expected_revision: null,
      policy: {
        tools: [{ tool: 'codex', max_sessions: 2 }],
        targets: [{ target_id: targets['Widgets second clone'], tools: [{ tool: 'codex' }] }],
        placement: {
          resources: [
            { target_id: targets['Widgets second clone'], resources: [{ namespace: 'vm', resource: 'macos-vm' }] },
          ],
        },
      },
      actor: { role: 'human' },
    });

    const wide = { width: 1440, height: 900 },
      narrow = { width: 390, height: 844 };
    await page.setViewportSize(wide);
    await openOrchestrationProject(page);
    await showProjectSettings(page, 'Orchestration');
    const settings = page.locator('[data-component="orchestration-settings"]');
    await settings.locator('[data-orchestration-project-group]').filter({ hasText: '2 sites' }).click();
    const detail = page.locator('[data-component="orchestration-project-detail"]');
    const binding = detail.getByRole('region', { name: 'Binding' });
    // HS2-NGP2AK: one row per site, the home site marked.
    await expect(binding).toContainText('2 sites');
    await expect(binding).toContainText('Widgets (Home)');
    await expect(binding).toContainText('Widgets second clone');

    const limits = page.locator('[data-component="orchestration-project-limits"]');
    const advanced = limits.locator('[data-orchestration-advanced-limits]');
    // HS2-25KT42: the default view needs no input: every Advanced limit is Auto and closed.
    await expect(advanced).toHaveJSProperty('open', false);
    await expect(advanced.locator('summary')).toContainText('All Auto');
    await expect(limits.getByRole('spinbutton', { name: 'Sessions Here' })).toBeHidden();

    const directory = join('target', 'visual-captures', 'hs2-orchpanel');
    await mkdir(directory, { recursive: true });
    const capture = (name: string) =>
      page.screenshot({ path: join(directory, `${name}-${browserName}.png`), animations: 'disabled' });
    const showDetail = async (size: { width: number; height: number }) => {
      await page.setViewportSize(size);
      if (size.width === narrow.width) {
        const hide = page.getByRole('button', { name: 'Hide Settings Sidebar', exact: true });
        if (await hide.isVisible()) await hide.click({ timeout: 3_000 }).catch(() => undefined);
        const show = page.getByRole('button', { name: 'Show Project Detail', exact: true });
        if (await show.isVisible()) await show.click();
      }
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
    };
    for (const scheme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      for (const size of [wide, narrow, wide]) {
        await showDetail(size);
        await binding.scrollIntoViewIfNeeded();
        await capture(`hs2-ngp2ak-binding-${String(size.width)}-${scheme}`);
        await limits.scrollIntoViewIfNeeded();
        await capture(`hs2-25kt42-limits-auto-${String(size.width)}-${scheme}`);
      }
    }
    await page.emulateMedia({ colorScheme: 'light' });

    // HS2-Y737GM: the help explains Sessions Here and Build Ahead beside the Limits header.
    for (const [size, scheme] of [
      [wide, 'light'],
      [narrow, 'dark'],
    ] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      await showDetail(size);
      await limits.getByRole('button', { name: 'About Limits' }).click();
      const help = page.getByRole('dialog', { name: 'About Limits' });
      await expect(help).toContainText('Sessions Here');
      await expect(help).toContainText('The most AI sessions the orchestrator runs in this project at once');
      await expect(help).toContainText('Build Ahead');
      await capture(`hs2-y737gm-help-${String(size.width)}-${scheme}`);
      await page.keyboard.press('Escape');
      await expect(help).toBeHidden();
    }
    await page.emulateMedia({ colorScheme: 'light' });
    await showDetail(wide);

    // HS2-1X1TXC: a model family typed as a pattern becomes a chip.
    await limits.getByRole('checkbox', { name: 'Codex' }).check();
    const models = limits.getByRole('combobox', { name: 'Codex Models' });
    await models.fill('gpt-*');
    await models.press('Enter');
    await expect.poll(() => chosenValues(page, 'limits.models.codex')).toEqual(['gpt-*']);
    await models.fill('gpt-5-codex');
    await capture('hs2-1x1txc-models-typing-1440-light');
    await models.press('Enter');
    await expect.poll(() => chosenValues(page, 'limits.models.codex')).toEqual(['gpt-*', 'gpt-5-codex']);
    // A chip's remove button removes just that one.
    await limits.getByRole('button', { name: 'Remove gpt-5-codex' }).click();
    await expect.poll(() => chosenValues(page, 'limits.models.codex')).toEqual(['gpt-*']);

    // HS2-NPAZQW: Required Resources offers the declared VM and its namespace pattern.
    await openAdvancedLimits(page);
    await expect(advanced).toContainText('Whether work may start before a ticket it depends on has passed CI');
    const resources = limits.getByRole('combobox', { name: 'Required Resources' });
    await resources.click();
    await resources.fill('vm');
    const option = page.getByRole('option', { name: 'vm/*', exact: true });
    await expect(option).toBeVisible();
    await expect(page.getByRole('option', { name: 'vm/macos-vm', exact: true })).toBeVisible();
    await capture('hs2-npazqw-resources-choices-1440-light');
    await option.click();
    await expect.poll(() => chosenValues(page, 'limits.resources')).toEqual(['vm/*']);
    await expect(advanced.locator('summary')).toContainText('1 Changed');
    await limits.getByRole('button', { name: 'Apply Limits' }).click();
    type Stored = {
      policy: {
        targets: { target_id: string; tools?: { tool: string; models?: string[] }[] }[];
        placement?: { resources?: { target_id: string; resources: { namespace: string; resource: string }[] }[] };
      };
    };
    await expect
      .poll(async () =>
        (await server.request<Stored>(`${base}/policy`)).policy.placement?.resources?.find(
          (entry) => entry.target_id === targets.Widgets,
        ),
      )
      .toEqual({ target_id: targets.Widgets, resources: [{ namespace: 'vm', resource: '*' }] });
    const stored = await server.request<Stored>(`${base}/policy`);
    expect(stored.policy.targets.find((entry) => entry.target_id === targets.Widgets)?.tools).toEqual([
      { tool: 'codex', models: ['gpt-*'] },
    ]);
    for (const scheme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      for (const size of [wide, narrow, wide]) {
        await showDetail(size);
        await openAdvancedLimits(page);
        await models.scrollIntoViewIfNeeded();
        await capture(`hs2-1x1txc-models-${String(size.width)}-${scheme}`);
        await resources.scrollIntoViewIfNeeded();
        await capture(`hs2-npazqw-y737gm-advanced-${String(size.width)}-${scheme}`);
      }
    }
  } finally {
    await server.stop(page);
  }
});

/**
 * HS2-RD16CQ: the project detail's inspector panel in the catalog, with its toolbar's Actions for <name> menu
 * (HS2-M1T6KN): a home project offers Show Project Tab and Add a Site; a site, whose project tab is not open,
 * has Show Project Tab disabled and offers Make <site> the Home Site and Separate from <project>.
 */
/** Every finite animation (a menu opening) has finished. */
const settled = (page: Page) =>
  page.waitForFunction(() =>
    document
      .getAnimations()
      .every((animation) => animation.playState !== 'running' || animation.effect?.getTiming().iterations === Infinity),
  );

test('HS2-RD16CQ the catalog shows the project detail toolbar’s Actions menu for a project and a site', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=orchestration-project-detail');
  const examples = page.getByRole('region', { name: 'OrchestrationProjectDetail Examples' });
  const home = examples.getByRole('region', { name: 'Panel Toolbar, Home Project' });
  // The panel is the inspector's landmark, its toolbar naming the project and holding its actions.
  const panel = home.getByRole('complementary', { name: 'Web Detail' });
  await expect(panel).toContainText('Local on This Computer');
  await expect(panel.locator('[data-component="orchestration-project-detail"]')).toBeVisible();
  await panel.getByRole('group', { name: 'Project Actions' }).getByRole('button', { name: 'Actions for Web' }).click();
  const menu = page.getByRole('menu').last();
  await expect(menu.getByRole('menuitem', { name: 'Show Project Tab' })).toBeEnabled();
  await expect(menu.getByRole('menuitem', { name: 'Add a Site' })).toBeVisible();
  await expect(menu.getByRole('menuitem', { name: 'Stop Orchestrating…' })).toBeVisible();
  await expect(menu.getByRole('menuitem', { name: /Home Site/ })).toHaveCount(0);
  await settled(page);
  await page.screenshot({ path: test.info().outputPath('hs2-rd16cq-home-menu.png') });
  await page.keyboard.press('Escape');
  await expect(menu).toBeHidden();

  const site = examples.getByRole('region', { name: 'Panel Toolbar, Site' });
  await site
    .getByRole('complementary', { name: 'Web second clone Detail' })
    .getByRole('button', { name: 'Actions for Web second clone' })
    .click();
  const siteMenu = page.getByRole('menu').last();
  await expect(siteMenu.getByRole('menuitem', { name: 'Show Project Tab' })).toBeDisabled();
  await expect(siteMenu.getByRole('menuitem', { name: 'Make Web second clone the Home Site' })).toBeEnabled();
  await expect(siteMenu.getByRole('menuitem', { name: 'Separate from Web' })).toBeEnabled();
  await expect(siteMenu.getByRole('menuitem', { name: 'Add a Site' })).toHaveCount(0);
  await site.scrollIntoViewIfNeeded();
  await settled(page);
  await page.screenshot({ path: test.info().outputPath('hs2-rd16cq-site-menu.png') });
  await page.keyboard.press('Escape');
  await expect(siteMenu).toBeHidden();
});
