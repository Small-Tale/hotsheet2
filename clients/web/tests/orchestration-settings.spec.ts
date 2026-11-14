import { existsSync } from 'node:fs';
import { mkdir } from 'node:fs/promises';
import { dirname, join } from 'node:path';

import { expect, type Page, test } from '@playwright/test';

import { realTicketServer, routeProjectChangeSocket } from './real-ticket-server';

/**
 * HS2-W27N1D: Project Type and Settings → Orchestration through the real shell, against a real
 * isolated server with three registered checkouts. Only the bridge's project-open answer is
 * routed; every orchestration read and write reaches the real Rust workflows.
 */
async function openProjects(page: Page) {
  const server = await realTicketServer();
  const projects: { id: string; root: string; name: string }[] = [
    { id: server.checkoutId, root: server.root, name: 'Launch' },
  ];
  for (const name of ['Web', 'Docs']) {
    const root = join(dirname(server.root), name.toLowerCase());
    await mkdir(root, { recursive: true });
    const checkout = await server.request<{ id: string }>('/checkouts', 'POST', { root, stores: [server.store] });
    projects.push({ id: checkout.id, root, name });
  }
  await page.addInitScript(
    (roots) => {
      localStorage.setItem('hotsheet.open-projects', JSON.stringify(roots));
    },
    projects.slice(0, 2).map((project) => project.root),
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
    await webRow.getByRole('button', { name: /Web/ }).first().click();
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
    await policy.getByRole('button', { name: 'Apply Policy' }).click();
    await expect(settings.getByRole('status')).toContainText('Policy saved.');
    const stored = await server.request<{ policy: { machine: { max_sessions: number } } }>(`${base}/policy`);
    expect(stored.policy.machine.max_sessions).toBe(3);

    // Remove from the detail, after the shared confirmation.
    await detail.getByRole('button', { name: 'Stop Orchestrating…' }).click();
    await page.getByRole('button', { name: 'Stop Orchestrating', exact: true }).click();
    await expect(webRow).toHaveCount(0);
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
    await server.stop();
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
    await page.goto('/');
    await page.getByRole('tab', { name: /Launch/ }).click();
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
      await settings
        .locator('[data-orchestration-project]')
        .filter({ hasText: 'Web' })
        .getByRole('button', { name: /Web/ })
        .first()
        .click();
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
    await server.stop();
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
  await expect(interactive.getByRole('status')).toHaveText('Mobile added.');
  await page.screenshot({ path: test.info().outputPath('orchestration-settings-demo.png'), fullPage: true });
  expect(errors).toEqual([]);
});
