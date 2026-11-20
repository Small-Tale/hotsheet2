import { expect, type Page, test } from '@playwright/test';

import { appRequests } from './app-requests';
import { openMockedProject, projectTab } from './open-mocked-project';
import { ticketsJson } from './ticket-page-fixture';

/**
 * HS2-Q8WF64: minimizing ticket readers without closing them, through the shipped application.
 * Every ticket and note here is synthetic.
 */

const projects = {
  alpha: {
    id: 'alpha-project',
    root: '/work/alpha',
    name: 'Alpha Work',
    stores: ['/work/alpha.hs2'],
    apiPath: '/__hotsheet/project-api/alpha-project',
  },
  beta: {
    id: 'beta-project',
    root: '/work/beta',
    name: 'Beta Work',
    stores: ['/work/beta.hs2'],
    apiPath: '/__hotsheet/project-api/beta-project',
  },
} as const;
const base = {
  priority: 'high',
  status: 'started',
  up_next: false,
  feedback_needed: false,
  tags: ['client'],
  blocked_by: [],
  claim_count: 0,
  created_at: '2026-11-01T00:00:00Z',
  updated_at: '2026-11-01T00:01:00Z',
};
const ticket = (projectId: string, id: string, slug: string, title: string, category: string) => ({
  ...base,
  connection_id: `git-${projectId}`,
  native_id: id,
  qualified_id: `git-${projectId}:${id}`,
  id,
  slug,
  title,
  category,
  details: `Synthetic details for ${slug}.`,
});
const rows: Record<string, ReturnType<typeof ticket>[]> = {
  'alpha-project': [
    ticket('alpha-project', 'demo', 'HS2-DEMO01', 'Keep reader drafts when switching projects', 'feature'),
    ticket('alpha-project', 'start', 'HS2-START02', 'Retry a failed attachment upload', 'bug'),
    ticket('alpha-project', 'next', 'HS2-NEXT01', 'Show CI run duration in the ticket timeline', 'task'),
  ],
  'beta-project': [ticket('beta-project', 'other', 'HS2-OTHER1', 'Another project ticket', 'task')],
};
const request = {
  id: 'REQUEST-1',
  kind: 'feedback_needed',
  created_at: '2026-11-01T00:00:00Z',
  edited_at: '2026-11-01T00:00:00Z',
  text: 'FEEDBACK NEEDED: which synthetic option?',
};
const projectByRoot = new Map<string, (typeof projects)[keyof typeof projects]>(
  Object.values(projects).map((project) => [project.root, project]),
);

async function mockProjects(page: Page, mutations: Record<string, unknown>[] = []) {
  const chooser = ['/work/beta'];
  await page.route(appRequests, async (route) => {
    const call = route.request(),
      url = new URL(call.url()),
      path = decodeURIComponent(url.pathname);
    if (/^\/__hotsheet\/(?:remote\/)?projects\/open$/.test(path)) {
      const root = (call.postDataJSON() as { root: string }).root;
      return route.fulfill({ status: 201, json: projectByRoot.get(root) ?? projects.alpha });
    }
    if (path === '/__hotsheet/folders/choose') return route.fulfill({ json: { path: chooser.shift() } });
    const projectId = path.match(/project-api\/([^/]+)/)?.[1],
      project = Object.values(projects).find((item) => item.id === projectId),
      tickets = projectId ? (rows[projectId] ?? []) : [];
    if (path.endsWith('/providers'))
      return route.fulfill({
        json: [
          {
            connection_id: `git-${projectId ?? 'alpha-project'}`,
            provider: 'git',
            display_name: `${project?.name ?? 'Project'} git`,
            locator: project?.stores[0] ?? '',
            default: true,
            capabilities: {
              create: true,
              update: true,
              close: true,
              notes: true,
              note_edit: true,
              note_delete: true,
              attachments: true,
              attachment_edit: true,
              up_next: true,
              close_reasons: true,
              claims: true,
              history: true,
              watch: true,
              query_fields: [],
            },
          },
        ],
      });
    if (
      path.endsWith('/provider-connections') ||
      path.endsWith('/corrupt-tickets') ||
      path.endsWith('/commands') ||
      path.endsWith('/terminals') ||
      path.endsWith('/permissions') ||
      path.endsWith('/connections') ||
      path.endsWith('/drive/sessions')
    )
      return route.fulfill({ json: [] });
    if (path.endsWith('/repository/status'))
      return route.fulfill({
        json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true },
      });
    if (path.endsWith('/terminal-settings')) return route.fulfill({ json: { inherit_global_shell_history: false } });
    if (path.endsWith('/ws/poll')) {
      // A genuine long poll that answers with no change (HS2-Q8WF64 issues no polling of its own).
      await new Promise((resolve) => setTimeout(resolve, 500));
      return route.fulfill({ json: { cursor: 0, events: [], overflow: false } });
    }
    if (path.endsWith('/tickets') && call.method() === 'GET')
      return route.fulfill({ json: ticketsJson(call, tickets) });
    const ticketId = path.match(/\/tickets\/([^/]+)$/)?.[1],
      found = tickets.find((item) => [item.id, item.native_id, item.qualified_id].includes(ticketId ?? ''));
    if (ticketId && call.method() === 'PATCH') {
      mutations.push(call.postDataJSON() as Record<string, unknown>);
      if (!found) return route.fulfill({ status: 404, json: { error: `No ${ticketId}` } });
      return route.fulfill({
        json: {
          store: found.connection_id,
          ...found,
          blocked_reason: null,
          concurrency_token: `token-${found.id}-${mutations.length}`,
          notes: found.id === 'start' ? [request] : [],
          attachments: [],
        },
      });
    }
    if (ticketId && call.method() === 'GET')
      return found
        ? route.fulfill({
            json: {
              store: found.connection_id,
              ...found,
              blocked_reason: null,
              concurrency_token: `token-${found.id}`,
              notes: found.id === 'start' ? [request] : [],
              attachments: [],
            },
          })
        : route.fulfill({ status: 404, json: { error: `No ${ticketId}` } });
    if (!path.startsWith('/__hotsheet/')) return route.continue();
    return route.fulfill({ status: 404, json: { error: `Unhandled ${call.method()} ${path}` } });
  });
}

async function openProjects(page: Page) {
  await page.goto('/');
  await openMockedProject(page);
  await expect(projectTab(page, 'Alpha Work')).toHaveAttribute('aria-selected', 'true');
}

async function openReader(page: Page, slug: string) {
  await page.locator(`[data-component="summary-row"][data-ticket-slug="${slug}"]`).dblclick();
  const dialog = page.getByRole('dialog', { name: new RegExp(`^Read and edit ${slug} in `) });
  await expect(dialog).toBeVisible();
  return dialog;
}

async function writeNote(dialog: ReturnType<Page['getByRole']>, text: string) {
  await dialog.locator('[data-action="add-ticket-note"]').first().click();
  const composer = dialog.getByRole('textbox', { name: 'New note' });
  await expect(composer).toBeFocused();
  await composer.pressSequentially(text);
  return composer;
}

const minimizedControl = (page: Page) => page.locator('[data-minimized-tickets-control]');
const anyModal = (page: Page) => page.evaluate(() => Boolean(document.querySelector('dialog:modal')));

test('HS2-Q8WF64 minimizes the workspace reader with its reply in progress and restores it modal again', async ({
  page,
}) => {
  const mutations: Record<string, unknown>[] = [];
  await mockProjects(page, mutations);
  await page.setViewportSize({ width: 1440, height: 900 });
  await openProjects(page);
  const dialog = await openReader(page, 'HS2-START02');
  const response = dialog.getByRole('textbox', { name: 'Feedback response' });
  await response.fill('Half-written synthetic reply');
  await dialog.getByRole('button', { name: 'Minimize Ticket Reader' }).click();
  await expect(page.locator('dialog[data-reader-dialog][open]')).toHaveCount(0);
  expect(await anyModal(page)).toBe(false);
  const restore = page.getByRole('button', {
    name: 'Restore HS2-START02 Retry a failed attachment upload (Reply in progress)',
  });
  await expect(restore).toBeVisible();
  // The control sits in the terminal drawer's restore corner, before the drawer button.
  const corner = page.getByRole('toolbar', { name: 'Terminal Drawer Controls' });
  await expect(corner.locator('[data-minimized-tickets-control="single"]')).toBeVisible();
  // The workspace is usable: another ticket can be selected while the reader waits.
  await page.locator('[data-component="summary-row"][data-ticket-slug="HS2-NEXT01"]').click();
  await expect(page.locator('#app-right-rail [data-component="ticket-inspector-header"]')).toHaveAttribute(
    'data-ticket-slug',
    'HS2-NEXT01',
  );
  // Neither typing nor minimizing wrote the reply to the server (HS2-RE1PS6).
  expect(mutations).toEqual([]);
  await restore.click();
  const restored = page.getByRole('dialog', { name: /^Read and edit HS2-START02 in / });
  await expect(restored).toBeVisible();
  expect(await anyModal(page)).toBe(true);
  await expect(restored.getByRole('textbox', { name: 'Feedback response' })).toHaveValue(
    'Half-written synthetic reply',
  );
  await expect(minimizedControl(page)).toHaveCount(0);
  // The workspace selection stayed where the person left it.
  await expect(page.locator('#app-right-rail [data-component="ticket-inspector-header"]')).toHaveAttribute(
    'data-ticket-slug',
    'HS2-NEXT01',
  );
  expect(mutations).toEqual([]);
});

test('HS2-Q8WF64 lists several minimized tickets with draft indicators, restores all, and closes all after asking', async ({
  page,
}) => {
  const mutations: Record<string, unknown>[] = [];
  await mockProjects(page, mutations);
  await page.setViewportSize({ width: 1440, height: 900 });
  await openProjects(page);
  const first = await openReader(page, 'HS2-DEMO01');
  await writeNote(first, 'Unsent synthetic note');
  // Pressing Minimize keeps the unsent note as a draft instead of submitting it.
  await first.getByRole('button', { name: 'Minimize Ticket Reader' }).click();
  await expect(minimizedControl(page)).toHaveAttribute('data-minimized-tickets-control', 'single');
  const second = await openReader(page, 'HS2-START02');
  await second.getByRole('textbox', { name: 'Feedback response' }).fill('Synthetic reply in progress');
  await second.getByRole('button', { name: 'Minimize Ticket Reader' }).click();
  const third = await openReader(page, 'HS2-NEXT01');
  await third.getByRole('button', { name: 'Minimize Ticket Reader' }).click();
  await expect(page.locator('dialog[data-reader-dialog][open]')).toHaveCount(0);

  const trigger = page.getByRole('button', { name: '3 Minimized Tickets' });
  await expect(trigger).toBeVisible();
  await expect(trigger.locator('[data-placement="corner"]')).toHaveText('3');
  await trigger.click();
  const menu = page.getByRole('menu', { name: '3 Minimized Tickets' });
  await expect(menu).toBeVisible();
  const demo = menu.getByRole('menuitem', { name: /HS2-DEMO01 Keep reader drafts/ });
  await expect(demo).toContainText('Unsent note');
  await expect(menu.getByRole('menuitem', { name: /HS2-START02/ })).toContainText('Reply in progress');
  await expect(menu.getByRole('menuitem', { name: /HS2-NEXT01/ })).not.toContainText('Unsent');
  await expect(menu.getByRole('menuitem', { name: 'Close All…' })).toBeVisible();

  // Restore one from the list; the other two stay minimized.
  await demo.click();
  const restored = page.getByRole('dialog', { name: /^Read and edit HS2-DEMO01 in / });
  await expect(restored).toBeVisible();
  await expect(restored.getByRole('textbox', { name: 'New note' })).toHaveValue('Unsent synthetic note');
  await restored.getByRole('button', { name: 'Minimize Ticket Reader' }).click();
  await expect(page.getByRole('button', { name: '3 Minimized Tickets' })).toBeVisible();

  // Restore All stacks every reader, the newest on top.
  await page.getByRole('button', { name: '3 Minimized Tickets' }).click();
  await page.getByRole('menuitem', { name: 'Restore All' }).click();
  await expect(page.locator('dialog[data-reader-dialog][open]')).toHaveCount(3);
  await expect(page.locator('dialog[data-reader-dialog][data-reader-active="true"]')).toContainText('HS2-DEMO01');
  await expect(minimizedControl(page)).toHaveCount(0);
  for (let remaining = 3; remaining > 0; remaining -= 1)
    await page
      .locator('dialog[data-reader-dialog][data-reader-active="true"]')
      .getByRole('button', { name: 'Minimize Ticket Reader' })
      .click();
  await expect(page.getByRole('button', { name: '3 Minimized Tickets' })).toBeVisible();

  // Close All asks because drafts would be lost; Cancel keeps everything.
  await page.getByRole('button', { name: '3 Minimized Tickets' }).click();
  await page.getByRole('menuitem', { name: 'Close All…' }).click();
  const confirmation = page.getByRole('alertdialog', { name: 'Close 3 minimized tickets?' });
  await expect(confirmation).toBeVisible();
  await expect(confirmation).toContainText('2 of them have unsent text that will be discarded.');
  await confirmation.getByRole('button', { name: 'Cancel' }).click();
  await expect(page.getByRole('button', { name: '3 Minimized Tickets' })).toBeVisible();
  await page.getByRole('button', { name: '3 Minimized Tickets' }).click();
  await page.getByRole('menuitem', { name: 'Close All…' }).click();
  await page
    .getByRole('alertdialog', { name: 'Close 3 minimized tickets?' })
    .getByRole('button', { name: 'Close All' })
    .click();
  await expect(minimizedControl(page)).toHaveCount(0);
  // No unsent note or reply was ever submitted (HS2-RE1PS6).
  expect(mutations).toEqual([]);
});

test('HS2-Q8WF64 closes all at once when no draft would be lost', async ({ page }) => {
  await mockProjects(page);
  await page.setViewportSize({ width: 1100, height: 900 });
  await openProjects(page);
  for (const slug of ['HS2-DEMO01', 'HS2-NEXT01']) {
    const dialog = await openReader(page, slug);
    await dialog.getByRole('button', { name: 'Minimize Ticket Reader' }).click();
  }
  await page.getByRole('button', { name: '2 Minimized Tickets' }).click();
  await page.getByRole('menuitem', { name: 'Close All', exact: true }).click();
  await expect(minimizedControl(page)).toHaveCount(0);
  await expect(page.getByRole('alertdialog', { name: /minimized tickets\?/ })).toHaveCount(0);
});

test('HS2-Q8WF64 keeps minimized tickets per project tab and across a reload', async ({ page }) => {
  await mockProjects(page);
  await page.setViewportSize({ width: 1440, height: 900 });
  await openProjects(page);
  const dialog = await openReader(page, 'HS2-DEMO01');
  await writeNote(dialog, 'Survives a reload');
  await page.keyboard.press('ControlOrMeta+Alt+Shift+KeyM');
  await expect(minimizedControl(page)).toBeVisible();

  await page.getByRole('button', { name: 'Add Project' }).click();
  await page.getByRole('button', { name: 'Local Project' }).click();
  await expect(projectTab(page, 'Beta Work')).toHaveAttribute('aria-selected', 'true');
  await expect(minimizedControl(page)).toHaveCount(0);
  await projectTab(page, 'Alpha Work').click();
  await expect(minimizedControl(page)).toBeVisible();

  await page.reload();
  await expect(projectTab(page, 'Alpha Work')).toBeVisible();
  await projectTab(page, 'Alpha Work').click();
  const restore = page.getByRole('button', { name: /^Restore HS2-DEMO01 .*\(Unsent note\)$/ });
  await expect(restore).toBeVisible();
  await restore.click();
  const restored = page.getByRole('dialog', { name: /^Read and edit HS2-DEMO01 in / });
  await expect(restored).toBeVisible();
  await expect(restored.getByRole('textbox', { name: 'New note' })).toHaveValue('Survives a reload');
});

test('HS2-Q8WF64 minimizes and restores from the keyboard', async ({ page }) => {
  await mockProjects(page);
  await page.setViewportSize({ width: 1440, height: 900 });
  await openProjects(page);
  await openReader(page, 'HS2-DEMO01');
  await page.keyboard.press('ControlOrMeta+Alt+Shift+KeyM');
  await expect(page.locator('dialog[data-reader-dialog][open]')).toHaveCount(0);
  await expect(minimizedControl(page)).toBeVisible();
  await page.keyboard.press('ControlOrMeta+Alt+Shift+KeyM');
  await expect(page.getByRole('dialog', { name: /^Read and edit HS2-DEMO01 in / })).toBeVisible();
  await expect(minimizedControl(page)).toHaveCount(0);
});

test('HS2-Q8WF64 minimize and restore animate a stand-in, and cross-fade under reduced motion', async ({ page }) => {
  await mockProjects(page);
  await page.setViewportSize({ width: 1440, height: 900 });
  await openProjects(page);
  await page.evaluate(() => {
    const seen: { kind: string; transforms: boolean }[] = [];
    (window as unknown as { motion: typeof seen }).motion = seen;
    new MutationObserver((records) => {
      for (const record of records)
        for (const node of record.addedNodes)
          if (node instanceof HTMLElement && node.dataset.readerMotionGhost) {
            const frames = node
              .getAnimations()
              .flatMap((animation) => (animation.effect as KeyframeEffect | null)?.getKeyframes() ?? []);
            seen.push({
              kind: node.dataset.readerMotionGhost,
              transforms: frames.some((frame) => typeof frame.transform === 'string' && frame.transform !== 'none'),
            });
          }
    }).observe(document.body, { childList: true });
  });
  const dialog = await openReader(page, 'HS2-DEMO01');
  await dialog.getByRole('button', { name: 'Minimize Ticket Reader' }).click();
  await expect(minimizedControl(page)).toBeVisible();
  await minimizedControl(page).click();
  await expect(page.getByRole('dialog', { name: /^Read and edit HS2-DEMO01 in / })).toBeVisible();
  await expect(page.locator('[data-reader-motion-ghost]')).toHaveCount(0);
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page
    .getByRole('dialog', { name: /^Read and edit HS2-DEMO01 in / })
    .getByRole('button', { name: 'Minimize Ticket Reader' })
    .click();
  await expect(minimizedControl(page)).toBeVisible();
  await expect(page.locator('[data-reader-motion-ghost]')).toHaveCount(0);
  expect(await page.evaluate(() => (window as unknown as { motion: unknown[] }).motion)).toEqual([
    { kind: 'minimize', transforms: true },
    { kind: 'restore', transforms: true },
    { kind: 'minimize', transforms: false },
  ]);
});
