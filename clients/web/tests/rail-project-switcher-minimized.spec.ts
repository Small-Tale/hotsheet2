import { expect, type Page, test } from '@playwright/test';

import { appRequests } from './app-requests';
import { capabilities } from './mobile-project-fixture';

// HS2-2609BW: the workspace grid ticket rail's project switcher lists open projects directly and
// gathers minimized ones under a final Minimized Projects submenu. The shipped app restores four
// remembered projects; the test minimizes two through the tabs' own Minimize Tab action, then picks
// minimized projects from the rail by pointer and by keyboard.
const BINDING = '01ARZ3NDEKTSV4RRFFQ69G5FAV';
const REMOTE_ID = `remote:${BINDING}:launch-checkout`;
const projects = [
  { id: 'alpha-project', root: '/work/alpha', name: 'Alpha' },
  { id: 'beta-project', root: '/work/beta', name: 'Beta' },
  { id: 'gamma-project', root: '/work/gamma', name: 'Gamma' },
  { id: REMOTE_ID, root: REMOTE_ID, name: 'Build host · Launch', remoteHost: 'Build host' },
].map((item) => ({
  ...item,
  stores: item.remoteHost ? [] : [`${item.root}.hs2`],
  apiPath: `/__hotsheet/project-api/${encodeURIComponent(item.id)}`,
}));

const ticket = (id: string, title: string) => ({
  connection_id: 'git',
  native_id: id,
  qualified_id: `git:${id}`,
  id,
  slug: `HS2-${id}`,
  title,
  category: 'task',
  priority: 'default',
  status: 'not_started',
  up_next: true,
  feedback_needed: false,
  tags: [],
  blocked_by: [],
  claim_count: 0,
  created_at: '2026-11-01T00:00:00Z',
  updated_at: '2026-11-01T00:01:00Z',
});
const rows: Record<string, ReturnType<typeof ticket>[]> = {
  'alpha-project': [ticket('ALPHA1', 'Alpha work')],
  'beta-project': [ticket('BETA01', 'Beta work')],
  'gamma-project': [ticket('GAMMA1', 'Gamma work'), ticket('GAMMA2', 'More gamma work')],
  [REMOTE_ID]: [ticket('LAUNCH', 'Launch work')],
};

function ticketPage(items: ReturnType<typeof ticket>[]) {
  return {
    items,
    counts: {
      total: items.length,
      queued: items.length,
      backlog: 0,
      archive: 0,
      trash: 0,
      open: items.length,
      up_next: items.length,
      active: 0,
      started: 0,
      completed_today: 0,
      completion_trend: [],
    },
  };
}

async function installProjects(page: Page) {
  await page.addInitScript(
    ({ roots }) => {
      if (sessionStorage.getItem('hs2-2609bw-seeded')) return;
      sessionStorage.setItem('hs2-2609bw-seeded', 'true');
      localStorage.setItem('hotsheet.open-projects', JSON.stringify(roots));
      localStorage.removeItem('hotsheet.layout.minimized-projects.v1');
    },
    { roots: projects.map((item) => item.root) },
  );
  await page.routeWebSocket(/\/ws\/sync(?:\?.*)?$/, () => undefined);
  await page.route(appRequests, async (route) => {
    const request = route.request(),
      path = decodeURIComponent(new URL(request.url()).pathname);
    if (/^\/__hotsheet\/(?:remote\/)?projects\/open$/.test(path)) {
      const root = (request.postDataJSON() as { root: string }).root;
      const item = projects.find((candidate) => candidate.root === root);
      return item
        ? route.fulfill({ status: 201, json: item })
        : route.fulfill({ status: 404, json: { error: 'Unknown synthetic project' } });
    }
    if (path === '/__hotsheet/remotes') return route.fulfill({ json: [] });
    const item = projects.find((candidate) => path.startsWith(`/__hotsheet/project-api/${candidate.id}/`));
    if (!item) return path.startsWith('/__hotsheet/') ? route.fulfill({ status: 404, json: {} }) : route.continue();
    if (path.endsWith('/ws/poll')) return route.fulfill({ json: { cursor: 1, events: [], overflow: false } });
    if (path.endsWith('/providers'))
      return route.fulfill({
        json: [
          {
            connection_id: 'git',
            provider: 'git',
            display_name: 'Hot Sheet Git',
            locator: `${item.root}.hs2`,
            default: true,
            capabilities,
          },
        ],
      });
    if (path.endsWith('/repository/status'))
      return route.fulfill({
        json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true },
      });
    if (path.endsWith('/terminal-settings')) return route.fulfill({ json: { inherit_global_shell_history: false } });
    if (path.endsWith('/tickets') && request.method() === 'GET')
      return route.fulfill({ json: ticketPage(rows[item.id] ?? []) });
    if (
      /\/(?:provider-connections|corrupt-tickets|commands|command-runs|views|terminals|permissions|connections|sessions)$/.test(
        path,
      )
    )
      return route.fulfill({ json: [] });
    return route.fulfill({ status: 404, json: { error: `Unhandled ${request.method()} ${path}` } });
  });
}

test.use({ deviceScaleFactor: 2 });

test('HS2-2609BW the rail switcher gathers minimized projects under a Minimized Projects submenu', async ({
  page,
}, testInfo) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await installProjects(page);
  await page.goto('/');
  const strip = page.locator('.project-tab-bar:not(.project-tab-bar--mobile)'),
    tab = (id: string) => strip.locator(`[data-tab-kind="project"][data-project-id="${id}"]`);
  await expect(strip.locator('[data-tab-kind="project"]')).toHaveCount(4);

  const rail = page.locator('#app-right-rail'),
    switcher = rail.locator('[data-terminal-rail-project]'),
    trigger = switcher.locator('button[aria-haspopup="menu"]').first(),
    menu = switcher.locator('[role="menu"]').first(),
    group = menu.locator('[data-rail-minimized-projects="true"]'),
    railRow = (id: string) => menu.locator(`[data-project-id="${id}"]`);

  // With nothing minimized, every open project is a top-level choice and the group is absent.
  await page.getByRole('button', { name: 'Workspace Grid' }).click();
  await expect(switcher).toBeVisible();
  await trigger.click();
  await expect(menu).toBeVisible();
  await expect(menu.locator(':scope > [role="menuitemcheckbox"]')).toHaveCount(4);
  await expect(group).toHaveCount(0);
  await page.keyboard.press('Escape');

  // Minimize Beta and the remote project through their tabs' Minimize Tab action.
  for (const id of ['beta-project', REMOTE_ID]) {
    await tab(id).first().click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Minimize Tab' }).click();
    await expect(tab(id)).toHaveCount(0);
  }
  await expect(strip.locator('[data-tab-kind="project"]')).toHaveCount(2);

  // The top level keeps the open projects; the minimized ones sit only in the final submenu.
  await trigger.click();
  await expect(menu).toBeVisible();
  // Kerf hosts a submenu parent in a role="none" wrapper beside its nested menu.
  const topLevel = menu.locator(':scope > [role^="menuitem"], :scope > [role="none"] > [role^="menuitem"]');
  await expect(topLevel).toHaveCount(3);
  await expect(topLevel.nth(0)).toHaveText('Alpha');
  await expect(topLevel.nth(1)).toHaveText('Gamma');
  await expect(topLevel.nth(2)).toContainText('Minimized Projects');
  await expect(menu.locator(':scope > [data-project-id="beta-project"]')).toHaveCount(0);
  await group.hover();
  const beta = railRow('beta-project'),
    launch = railRow(REMOTE_ID);
  await expect(beta).toBeVisible();
  await expect(launch).toBeVisible();
  // Submenu rows reuse the Restore Project presentation: folder or cloud icon, and the work badge.
  await expect(beta.locator('[data-lucide="folder"]')).toHaveCount(1);
  await expect(launch.locator('[data-lucide="cloud"]')).toBeVisible();
  await expect(beta.locator('.project-tab__work-count')).toHaveText('1');

  const capture = async (name: string) => {
    await beta.hover();
    await page.screenshot({
      path: testInfo.outputPath(`HS2-2609BW-rail-switcher-${name}.png`),
      animations: 'disabled',
    });
  };
  await capture('wide-light');
  await page.emulateMedia({ colorScheme: 'dark' });
  await capture('wide-dark');
  await page.emulateMedia({ colorScheme: 'light' });
  for (const [label, width] of [
    ['medium', 1100],
    ['narrow', 900],
    ['wide-return', 1440],
  ] as const) {
    await page.keyboard.press('Escape');
    await page.keyboard.press('Escape');
    await page.setViewportSize({ width, height: 900 });
    // At the narrow width the Workbench collapses the rail; show it through its own toggle.
    if (label === 'narrow') await page.getByRole('button', { name: 'Show ticket rail' }).first().click();
    await expect(switcher).toBeVisible();
    await trigger.click();
    await group.hover();
    await expect(beta).toBeVisible();
    await capture(label);
  }
  await page.emulateMedia({ colorScheme: 'dark' });
  await capture('wide-return-dark');
  await page.emulateMedia({ colorScheme: 'light' });

  // Pointer: picking a minimized project switches the rail to it without restoring its tab, as the
  // switcher always treated a minimized project.
  await beta.click();
  await expect(switcher).toHaveAttribute('data-terminal-rail-project', 'beta-project');
  await expect(trigger).toContainText('Beta');
  await expect(rail.getByText('Beta work')).toBeVisible();
  await expect(tab('beta-project')).toHaveCount(0);
  await expect(strip.getByRole('button', { name: 'Minimized Projects' })).toBeVisible();

  // The shown minimized project is checked in the submenu, and its group carries the check.
  await trigger.click();
  await expect(group).toHaveAttribute('aria-checked', 'true');
  await group.hover();
  await expect(beta).toHaveAttribute('aria-checked', 'true');
  await expect(launch).toHaveAttribute('aria-checked', 'false');
  await page.keyboard.press('Escape');
  await page.keyboard.press('Escape');

  // Keyboard: open the switcher, move to the group, open its submenu, and choose the remote project.
  await trigger.focus();
  await page.keyboard.press('Enter');
  await expect(menu).toBeVisible();
  await page.keyboard.press('End');
  await expect(group).toBeFocused();
  await page.keyboard.press('ArrowRight');
  await expect(beta).toBeFocused();
  await page.keyboard.press('ArrowDown');
  await expect(launch).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(switcher).toHaveAttribute('data-terminal-rail-project', REMOTE_ID);
  await expect(rail.getByText('Launch work')).toBeVisible();
  await expect(tab(REMOTE_ID)).toHaveCount(0);

  // An open project from the top level still switches the rail.
  await trigger.click();
  await railRow('gamma-project').click();
  await expect(switcher).toHaveAttribute('data-terminal-rail-project', 'gamma-project');
  await expect(rail.getByText('More gamma work')).toBeVisible();

  // Restoring every minimized project removes the group.
  for (const name of ['Beta', 'Build host · Launch']) {
    await page
      .locator('.project-tab-bar:not(.project-tab-bar--mobile)')
      .getByRole('button', { name: 'Minimized Projects' })
      .click();
    await page.locator('[data-minimized-projects="true"] [role="menu"]').getByRole('menuitem', { name }).click();
  }
  await expect(strip.locator('[data-tab-kind="project"]')).toHaveCount(4);
  await page.getByRole('button', { name: 'Workspace Grid' }).click();
  await trigger.click();
  await expect(menu.locator(':scope > [role="menuitemcheckbox"]')).toHaveCount(4);
  await expect(group).toHaveCount(0);
});
