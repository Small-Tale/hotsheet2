import { mkdirSync } from 'node:fs';
import { join } from 'node:path';

import { expect, type Locator, type Page, test } from '@playwright/test';

import type { FullTicket } from '../src/api';
import { realTicketServer } from './real-ticket-server';

/**
 * Ticket groups in the real app against a private real server (HS2-3XNTSD): rendering in the list
 * and the columns, membership through the menu, the dialog, and drag and drop, group rename and
 * ungroup, and group search. Tickets store group ids whose names live in the project's group
 * registry (HS2-9KZSD4), so the spec seeds through checkout routes and finds groups by display
 * name. Every ticket is synthetic.
 */
type Server = Awaited<ReturnType<typeof realTicketServer>>;
const BOARD = 'Board drag & drop',
  SEARCH = 'Search syntax';
const captures = join('target', 'visual-captures', 'hs2-3xntsd');
mkdirSync(captures, { recursive: true });
const capture = (page: Page, name: string) =>
  page.screenshot({ path: join(captures, `${name}.png`), animations: 'disabled' });

interface Seed {
  title: string;
  category: string;
  status?: string;
  group?: string;
  up_next?: boolean;
}
const SEEDS: readonly Seed[] = [
  { title: 'Drag tickets between columns keeps order', category: 'bug', group: BOARD },
  { title: 'Persist column widths per view', category: 'feature', status: 'started', group: BOARD, up_next: true },
  { title: 'Fix flicker when board reconnects', category: 'bug', status: 'started', group: BOARD },
  { title: 'Terminal drawer remembers last tab', category: 'feature' },
  { title: 'Group headers collapse with keyboard', category: 'feature', up_next: true },
  { title: 'Document group: search syntax', category: 'task', group: SEARCH },
  { title: 'Search parser accepts quoted field values', category: 'task', status: 'started', group: SEARCH },
  { title: 'Onboarding checklist copy review', category: 'task' },
  { title: 'Inspector shows group membership', category: 'feature', status: 'completed' },
];

async function seed(server: Server) {
  const tickets: Record<string, FullTicket> = {},
    checkout = `/checkouts/${server.checkoutId}`;
  for (const item of SEEDS) {
    // A checkout route resolves a group name through the registry, creating its entry (HS2-9KZSD4).
    const created = await server.request<FullTicket>(`${checkout}/tickets`, 'POST', {
      title: item.title,
      category: item.category,
      tags: [],
      group: item.group,
      up_next: item.up_next,
    });
    tickets[item.title] = item.status
      ? await server.request<FullTicket>(`${checkout}/tickets/${created.id}`, 'PATCH', { status: item.status })
      : created;
  }
  return tickets;
}

/** Wait until finite layout animations (such as a rail collapsing after a resize) have finished. */
async function settle(page: Page): Promise<void> {
  await expect
    .poll(() =>
      page.evaluate(async () => {
        await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
        return document
          .getAnimations()
          .filter(
            (animation) =>
              animation.playState === 'running' && animation.effect?.getComputedTiming().iterations !== Infinity,
          ).length;
      }),
    )
    .toBe(0);
}

async function open(
  page: Page,
  server: Server,
  { rewrite }: { rewrite?: (path: string, body: string) => string | undefined } = {},
) {
  const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
  await page.routeWebSocket(/.*/, (route) => {
    void route.close();
  });
  await page.route(/\/__hotsheet\/(?:remote\/)?projects\/open$/, (route) =>
    route.fulfill({
      status: 201,
      json: { id: server.checkoutId, root: server.root, name: 'synthetic', stores: [server.store], apiPath },
    }),
  );
  await page.route(`**${apiPath}/**`, async (route) => {
    const incoming = new URL(route.request().url());
    try {
      const response = await route.fetch({
        url: `${server.url}${incoming.pathname.slice(apiPath.length)}${incoming.search}`,
        headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
        timeout: 60_000,
      });
      const path = incoming.pathname.slice(apiPath.length),
        body = rewrite && response.ok() ? rewrite(path, await response.text()) : undefined;
      await route.fulfill(body === undefined ? { response } : { response, body });
    } catch {
      await route.abort().catch(() => undefined);
    }
  });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('button', { name: 'Local Project' }).click();
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
  await expect(page.locator('[data-component="ticket-list"] [data-ticket-slug]').first()).toBeVisible();
}

const list = (page: Page) => page.locator('[data-component="ticket-list"]');
const board = (page: Page) => page.locator('[data-component="ticket-board"]');
/** A group container by its display name; its `data-ticket-group` is the group's id (HS2-9KZSD4). */
const group = (scope: Locator, name: string, column?: string) =>
  scope.locator(
    `[data-component="ticket-group"][aria-label="${name} group"]${column ? `[data-ticket-group-scope="${column}"]` : ''}`,
  );
const rowOf = (scope: Locator, slug: string) => scope.locator(`[data-ticket-slug="${slug}"]`);
const slugsIn = (scope: Locator) =>
  scope.locator('[data-ticket-slug]').evaluateAll((rows) => rows.map((row) => (row as HTMLElement).dataset.ticketSlug));
interface GroupSummary {
  id: string;
  name: string;
  count: number;
}
const groupsOf = (server: Server) =>
  server.request<{ groups: GroupSummary[] }>(`/checkouts/${server.checkoutId}/groups`).then((result) => result.groups);
/** The project's groups as `[name, count]` pairs, sorted by name. */
const groupCounts = (server: Server) =>
  groupsOf(server).then((groups) => groups.map((item) => [item.name, item.count] as const));
/** The id the registry gives a group name. */
const groupIdOf = (server: Server, name: string) =>
  groupsOf(server).then((groups) => groups.find((item) => item.name === name)?.id);
/** A ticket's group display name, resolved through the registry, or null when ungrouped. */
async function groupOf(server: Server, id: string): Promise<string | null> {
  const ticket = await server.request<FullTicket>(`/tickets/${id}`);
  if (!ticket.group) return null;
  return (await groupsOf(server)).find((item) => item.id === ticket.group)?.name ?? ticket.group;
}
/** A ticket's status and group display name. */
const statusAndGroupOf = async (server: Server, id: string) =>
  [(await server.request<FullTicket>(`/tickets/${id}`)).status, await groupOf(server, id)] as const;

/** The x of a group title's first glyph and of the first ticket type icon below it. */
async function titleAndIconLeft(container: Locator) {
  const title = await container.locator('[data-action="toggle-ticket-group"]').evaluate((node) => {
    const walker = document.createTreeWalker(node, NodeFilter.SHOW_TEXT);
    let text = walker.nextNode();
    while (text && !text.textContent?.trim()) text = walker.nextNode();
    const range = document.createRange();
    range.selectNodeContents(text!);
    return range.getBoundingClientRect().left;
  });
  const icon = await container
    .locator('.ticket-list-row__category')
    .first()
    .evaluate((node) => node.getBoundingClientRect().left);
  return { title, icon };
}

async function withServer(run: (server: Server, tickets: Record<string, FullTicket>) => Promise<void>, page: Page) {
  const server = await realTicketServer();
  try {
    await run(server, await seed(server));
  } finally {
    await page.unrouteAll({ behavior: 'ignoreErrors' });
    await server.stop();
  }
}

test('HS2-WZ3JGZ renders list groups as segments, aligned and collapsible', async ({ page }) => {
  test.setTimeout(180_000);
  await withServer(async (server, tickets) => {
    await open(page, server);
    const view = list(page),
      boardGroup = group(view, BOARD),
      searchGroup = group(view, SEARCH);
    await expect(boardGroup).toBeVisible();
    await expect(searchGroup).toBeVisible();
    // Each group gathers its members; ungrouped runs are separate cards.
    await expect(boardGroup.locator('[data-ticket-slug]')).toHaveCount(3);
    await expect(searchGroup.locator('[data-ticket-slug]')).toHaveCount(2);
    await expect(boardGroup.getByRole('button', { name: `${BOARD}, 3 tickets` })).toHaveAttribute(
      'aria-expanded',
      'true',
    );
    // Rows appear in one visual order with every group contiguous.
    const order = await slugsIn(view);
    const boardId = await groupIdOf(server, BOARD);
    // The group carries its registry id, not its name (HS2-9KZSD4).
    expect(boardId).toMatch(/^[0-9A-HJKMNP-TV-Z]{10}$/);
    await expect(boardGroup).toHaveAttribute('data-ticket-group', boardId!);
    const members = Object.values(tickets)
      .filter((ticket) => ticket.group === boardId)
      .map((ticket) => ticket.slug);
    const positions = members.map((slug) => order.indexOf(slug)).sort((a, b) => a - b);
    expect(positions[2] - positions[0]).toBe(2);
    // No list surface is drawn behind a group; each segment carries its own.
    expect(await view.evaluate((node) => getComputedStyle(node).backgroundColor)).toBe('rgba(0, 0, 0, 0)');
    const segment = view.locator('.ticket-list__segment').first();
    expect(await segment.evaluate((node) => getComputedStyle(node).backgroundColor)).not.toBe('rgba(0, 0, 0, 0)');
    // Groups keep clear space above and below.
    const gap = await view.evaluate((node) => {
      const groupBox = node.querySelector('[data-component="ticket-group"]')!.getBoundingClientRect(),
        next = node.querySelector('[data-component="ticket-group"]')!.nextElementSibling!.getBoundingClientRect();
      return next.top - groupBox.bottom;
    });
    expect(gap).toBeGreaterThanOrEqual(12);
    // Edge rows of each segment round with their own card.
    await expect(boardGroup.locator('[data-ticket-slug]').first()).toHaveAttribute('data-list-edge', 'start');
    await expect(boardGroup.locator('[data-ticket-slug]').last()).toHaveAttribute('data-list-edge', 'end');

    for (const theme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: theme });
      for (const width of [1440, 900, 390, 1440]) {
        await page.setViewportSize({ width, height: 900 });
        await settle(page);
        await expect(boardGroup).toBeVisible();
        const { title, icon } = await titleAndIconLeft(boardGroup);
        expect(Math.abs(title - icon), `title/icon alignment at ${width}px ${theme}`).toBeLessThanOrEqual(1);
        // The group never overflows the list horizontally.
        const fits = await boardGroup.evaluate((node) => {
          const box = node.getBoundingClientRect(),
            parent = node.closest('[data-component="ticket-list"]')!.getBoundingClientRect();
          return box.left >= parent.left - 0.5 && box.right <= parent.right + 0.5 && box.width > 200;
        });
        expect(fits, `group fits the list at ${width}px`).toBe(true);
        if (width !== 900) await capture(page, `list-groups-${theme}-${width}`);
      }
    }
    await page.emulateMedia({ colorScheme: 'light' });

    // Collapse with the keyboard: the header and count stay, the rows go, and focus stays put.
    const toggle = boardGroup.locator('[data-action="toggle-ticket-group"]');
    await toggle.focus();
    await page.keyboard.press('Enter');
    await expect(toggle).toHaveAttribute('aria-expanded', 'false');
    await expect(boardGroup.locator('[data-ticket-slug]')).toHaveCount(0);
    await expect(boardGroup.getByRole('button', { name: `${BOARD}, 3 tickets` })).toBeFocused();
    await capture(page, 'list-collapsed-group-light-1440');
    // Arrow keys skip the hidden members.
    const visible = await slugsIn(view);
    expect(visible).toHaveLength(SEEDS.length - 3);
    await rowOf(view, visible[0]!).click();
    for (let index = 1; index < visible.length; index += 1) {
      await page.keyboard.press('ArrowDown');
      await expect(rowOf(view, visible[index]!)).toBeFocused();
    }
    // The preference survives a reload and is per view.
    // The reloaded page restores the open project by itself.
    await page.reload();
    await expect(group(list(page), BOARD).locator('[data-action="toggle-ticket-group"]')).toHaveAttribute(
      'aria-expanded',
      'false',
    );
    await group(list(page), BOARD).locator('[data-action="toggle-ticket-group"]').click();
    await expect(group(list(page), BOARD).locator('[data-ticket-slug]')).toHaveCount(3);
  }, page);
});

test('HS2-HV8S4Y shows each column’s members of a group and collapses it everywhere', async ({ page }) => {
  test.setTimeout(180_000);
  await withServer(async (server) => {
    await open(page, server);
    await page.getByLabel('Columns View').click();
    const view = board(page);
    await expect(group(view, BOARD, 'not-started').locator('[data-ticket-slug]')).toHaveCount(1);
    await expect(group(view, BOARD, 'started').locator('[data-ticket-slug]')).toHaveCount(2);
    await expect(group(view, SEARCH, 'not-started').locator('[data-ticket-slug]')).toHaveCount(1);
    await expect(group(view, SEARCH, 'started').locator('[data-ticket-slug]')).toHaveCount(1);
    await expect(group(view, BOARD, 'completed')).toHaveCount(0);
    await expect(group(view, BOARD, 'started').getByRole('button', { name: `${BOARD}, 2 tickets` })).toBeVisible();
    for (const theme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: theme });
      for (const width of [1440, 1100]) {
        await page.setViewportSize({ width, height: 900 });
        await settle(page);
        for (const column of ['not-started', 'started']) {
          const container = group(view, BOARD, column);
          await expect(container).toBeVisible();
          const { title, icon } = await titleAndIconLeft(container);
          expect(Math.abs(title - icon), `${column} title/icon alignment at ${width}px`).toBeLessThanOrEqual(1);
          const fits = await container.evaluate((node) => {
            const box = node.getBoundingClientRect(),
              column = node.closest('[data-component="ticket-board-column"]')!.getBoundingClientRect();
            return box.left >= column.left - 0.5 && box.right <= column.right + 0.5 && box.width > 150;
          });
          expect(fits, `${column} group fits its column at ${width}px`).toBe(true);
        }
        await capture(page, `columns-groups-${theme}-${width}`);
      }
    }
    // Collapsing in one column collapses the group in every column.
    await group(view, SEARCH, 'started').locator('[data-action="toggle-ticket-group"]').click();
    await expect(group(view, SEARCH, 'not-started').locator('[data-action="toggle-ticket-group"]')).toHaveAttribute(
      'aria-expanded',
      'false',
    );
    await expect(group(view, SEARCH, 'not-started').locator('[data-ticket-slug]')).toHaveCount(0);
    await expect(group(view, SEARCH, 'started').locator('[data-ticket-slug]')).toHaveCount(0);
    await expect(group(view, BOARD, 'started').locator('[data-ticket-slug]')).toHaveCount(2);
    await capture(page, 'columns-collapsed-group-dark-1100');
    await page.emulateMedia({ colorScheme: 'light' });
    // The list view shares the same preference.
    await page.getByLabel('List View').click();
    await expect(group(list(page), SEARCH).locator('[data-action="toggle-ticket-group"]')).toHaveAttribute(
      'aria-expanded',
      'false',
    );
  }, page);
});

test('HS2-TX17CM adds tickets to a new or existing group and removes them', async ({ page }) => {
  test.setTimeout(180_000);
  await withServer(async (server, tickets) => {
    await open(page, server);
    const view = list(page),
      terminal = tickets['Terminal drawer remembers last tab'],
      keyboard = tickets['Group headers collapse with keyboard'],
      onboarding = tickets['Onboarding checklist copy review'];
    const menu = page.locator('[data-context-menu="ticket"]');
    const openGroupSubmenu = async () => {
      const item = menu.getByRole('menuitem', { name: 'Add to Group' });
      await expect(item).toBeVisible();
      await item.hover();
      await expect(menu.getByRole('menuitem', { name: 'New Group…' })).toBeVisible();
    };
    // Multi-select two ungrouped tickets, then New Group… from the context menu.
    await rowOf(view, terminal.slug).click();
    await rowOf(view, keyboard.slug).click({ modifiers: ['ControlOrMeta'] });
    await rowOf(view, keyboard.slug).click({ button: 'right' });
    await openGroupSubmenu();
    // New Group… comes first, then the existing groups.
    const choices = await menu
      .locator('[data-context-group]')
      .evaluateAll((items) => items.map((item) => (item as HTMLElement).dataset.contextGroup));
    // Each existing group is offered by its id, labeled with its name (HS2-9KZSD4).
    expect(choices).toEqual(['', await groupIdOf(server, BOARD), await groupIdOf(server, SEARCH)]);
    await expect(menu.locator('[data-context-group]').filter({ hasText: BOARD })).toBeVisible();
    await expect(menu.getByRole('menuitem', { name: 'Remove from Group' })).toHaveCount(0);
    await capture(page, 'context-add-to-group-light-1440');
    await menu.getByRole('menuitem', { name: 'New Group…' }).click();
    const dialog = page.locator('[data-dialog-component="ticket-group-dialog"]'),
      field = dialog.getByLabel('Group Name'),
      create = dialog.getByRole('button', { name: 'Create Group' });
    await expect(dialog).toBeVisible();
    await expect(field).toBeFocused();
    await expect(create).toBeDisabled();
    await capture(page, 'new-group-dialog-light-1440');
    // A duplicate name, compared case-insensitively, shows an inline error and blocks creation.
    await field.fill('board drag & drop');
    await expect(dialog.getByText('A group named “board drag & drop” already exists.')).toBeVisible();
    await expect(create).toBeDisabled();
    await expect(field).toHaveValue('board drag & drop');
    await capture(page, 'new-group-duplicate-light-1440');
    await field.fill('Terminal polish');
    await expect(create).toBeEnabled();
    await create.click();
    await expect(dialog).toBeHidden();
    const created = group(view, 'Terminal polish');
    await expect(created.locator('[data-ticket-slug]')).toHaveCount(2);
    await expect.poll(() => groupOf(server, terminal.id)).toBe('Terminal polish');
    await expect.poll(() => groupOf(server, keyboard.id)).toBe('Terminal polish');

    // A single ticket joins an existing group from the toolbar's More Actions menu.
    await rowOf(view, onboarding.slug).click();
    await page.getByRole('button', { name: 'More Actions for Selected Tickets' }).click();
    await openGroupSubmenu();
    const searchId = (await groupIdOf(server, SEARCH))!;
    await menu.locator(`[data-context-group="${searchId}"]`).click();
    await expect(group(view, SEARCH).locator('[data-ticket-slug]')).toHaveCount(3);
    await expect.poll(() => groupOf(server, onboarding.id)).toBe(SEARCH);

    // Moving a grouped ticket to another group replaces its membership; its current group is checked.
    await rowOf(view, onboarding.slug).click({ button: 'right' });
    await openGroupSubmenu();
    await expect(menu.locator(`[data-context-group="${searchId}"]`)).toHaveAttribute('aria-checked', 'true');
    await menu.locator(`[data-context-group="${await groupIdOf(server, 'Terminal polish')}"]`).click();
    await expect(created.locator('[data-ticket-slug]')).toHaveCount(3);
    await expect(group(view, SEARCH).locator('[data-ticket-slug]')).toHaveCount(2);
    await expect.poll(() => groupOf(server, onboarding.id)).toBe('Terminal polish');

    // Remove from Group undoes a grouping; removing the last members ends the group.
    await rowOf(view, onboarding.slug).click({ button: 'right' });
    await menu.getByRole('menuitem', { name: 'Remove from Group' }).click();
    await expect(created.locator('[data-ticket-slug]')).toHaveCount(2);
    await expect.poll(() => groupOf(server, onboarding.id)).toBeNull();
    await rowOf(view, terminal.slug).click();
    await rowOf(view, keyboard.slug).click({ modifiers: ['ControlOrMeta'] });
    await rowOf(view, keyboard.slug).click({ button: 'right' });
    await menu.getByRole('menuitem', { name: 'Remove from Group' }).click();
    await expect(created).toHaveCount(0);
    await expect
      .poll(() => groupCounts(server))
      .toEqual([
        [BOARD, 3],
        [SEARCH, 2],
      ]);

    // The standard Undo shortcut restores the removed membership.
    await rowOf(view, keyboard.slug).focus();
    await page.keyboard.press('ControlOrMeta+z');
    await expect(group(view, 'Terminal polish').locator('[data-ticket-slug]')).toHaveCount(2);
  }, page);
});

test('HS2-TX17CM disables grouping with a reason when a provider cannot group', async ({ page }) => {
  test.setTimeout(120_000);
  await withServer(async (server, tickets) => {
    // The same real server, reporting a provider without the groups capability.
    let rewritten = 0;
    await open(page, server, {
      rewrite: (path, body) => {
        if (!path.endsWith('/providers') || !body.includes('"groups":true')) return undefined;
        rewritten += 1;
        return body.replaceAll('"groups":true', '"groups":false');
      },
    });
    expect(rewritten).toBeGreaterThan(0);
    const view = list(page),
      target = tickets['Terminal drawer remembers last tab'],
      menu = page.locator('[data-context-menu="ticket"]');
    await rowOf(view, target.slug).click({ button: 'right' });
    const item = menu.locator('[data-context-submenu="group"]');
    await expect(item).toBeVisible();
    await expect(item).toHaveAttribute('aria-disabled', 'true');
    await expect(item).toHaveAttribute('title', 'One or more selected ticket providers do not support groups.');
    await capture(page, 'context-group-unsupported-light-1440');
  }, page);
});

/** Drag `source` onto `target` with real pointer events, pausing over the target for `over`. */
async function drag(page: Page, source: Locator, target: Locator, over?: () => Promise<void>) {
  // Let the previous change's motion finish, so the rows are at rest when the drag starts.
  await settle(page);
  const from = (await source.boundingBox())!,
    to = (await target.boundingBox())!;
  await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
  await page.mouse.down();
  await page.mouse.move(from.x + from.width / 2 + 12, from.y + from.height / 2 + 12, { steps: 4 });
  await page.mouse.move(to.x + to.width / 2, to.y + Math.min(to.height / 2, 24), { steps: 12 });
  // A real pointer keeps reporting dragover while it rests on the target.
  await page.mouse.move(to.x + to.width / 2 + 2, to.y + Math.min(to.height / 2, 24) + 1, { steps: 2 });
  await over?.();
  await page.mouse.up();
}

test('HS2-PRXD39 drags tickets into and out of groups', async ({ page }) => {
  test.setTimeout(180_000);
  await withServer(async (server, tickets) => {
    await open(page, server);
    const view = list(page),
      terminal = tickets['Terminal drawer remembers last tab'],
      keyboard = tickets['Group headers collapse with keyboard'],
      target = group(view, SEARCH);
    // Dragging over a group outlines the whole group; there is no insertion line.
    await drag(page, rowOf(view, terminal.slug), target.locator('[data-component="list-header"]'), async () => {
      await expect(target).toHaveAttribute('data-drop-target', 'true');
      await expect(target.locator('[data-component="sunken-panel"]')).toHaveAttribute('data-outlined', 'true');
      await expect(group(view, BOARD)).not.toHaveAttribute('data-drop-target', 'true');
      await capture(page, 'drag-into-group-light-1440');
    });
    await expect(target.locator('[data-ticket-slug]')).toHaveCount(3);
    await expect(target).not.toHaveAttribute('data-drop-target', 'true');
    await expect.poll(() => groupOf(server, terminal.id)).toBe(SEARCH);

    // Dropping a grouped ticket on the list outside every group removes its group.
    await drag(page, rowOf(view, terminal.slug), rowOf(view, keyboard.slug));
    await expect(target.locator('[data-ticket-slug]')).toHaveCount(2);
    await expect.poll(() => groupOf(server, terminal.id)).toBeNull();

    // A multi-selection drags together.
    await rowOf(view, terminal.slug).click();
    await rowOf(view, keyboard.slug).click({ modifiers: ['ControlOrMeta'] });
    await drag(page, rowOf(view, keyboard.slug), group(view, BOARD).locator('[data-component="list-header"]'));
    await expect(group(view, BOARD).locator('[data-ticket-slug]')).toHaveCount(5);
    await expect.poll(() => groupOf(server, keyboard.id)).toBe(BOARD);
    await expect.poll(() => groupOf(server, terminal.id)).toBe(BOARD);
    // A group drop moves tickets; it never copies them.
    await expect(view.locator('[data-ticket-slug]')).toHaveCount(SEEDS.length);

    // Columns (HS2-K2QA1J): from another column the whole column is the target, even over a group,
    // so the drop changes only the status.
    await page.getByLabel('Columns View').click();
    const columns = board(page),
      onboarding = tickets['Onboarding checklist copy review'],
      startedColumn = columns.locator('[data-column-id="started"]'),
      startedSearch = group(columns, SEARCH, 'started');
    await drag(
      page,
      rowOf(columns, onboarding.slug),
      startedSearch.locator('[data-component="list-header"]'),
      async () => {
        await expect(startedColumn).toHaveAttribute('data-dragging-ticket', 'true');
        await expect(startedSearch).not.toHaveAttribute('data-drop-target', 'true');
        await expect(startedSearch.locator('[data-component="sunken-panel"]')).not.toHaveAttribute(
          'data-outlined',
          'true',
        );
        await capture(page, 'drag-across-columns-over-group-light-1440');
      },
    );
    await expect.poll(() => statusAndGroupOf(server, onboarding.id)).toEqual(['started', null]);
    await expect(startedSearch.locator(`[data-ticket-slug="${onboarding.slug}"]`)).toHaveCount(0);
    await expect(startedColumn).not.toHaveAttribute('data-dragging-ticket', 'true');

    // Within its own column, dropping on a group sets the group and only the group outlines.
    await drag(
      page,
      rowOf(columns, onboarding.slug),
      startedSearch.locator('[data-component="list-header"]'),
      async () => {
        await expect(startedSearch).toHaveAttribute('data-drop-target', 'true');
        await expect(group(columns, SEARCH, 'not-started')).not.toHaveAttribute('data-drop-target', 'true');
        // The column itself is not also highlighted.
        await expect(startedColumn).not.toHaveAttribute('data-dragging-ticket', 'true');
        await capture(page, 'drag-into-column-group-light-1440');
      },
    );
    await expect(startedSearch.locator(`[data-ticket-slug="${onboarding.slug}"]`)).toBeVisible();
    await expect.poll(() => statusAndGroupOf(server, onboarding.id)).toEqual(['started', SEARCH]);

    // From another column over a different group, a grouped ticket keeps its own group.
    await drag(
      page,
      rowOf(columns, onboarding.slug),
      group(columns, BOARD, 'not-started').locator('[data-component="list-header"]'),
    );
    await expect.poll(() => statusAndGroupOf(server, onboarding.id)).toEqual(['not_started', SEARCH]);
    await drag(page, rowOf(columns, onboarding.slug), startedColumn.locator(':scope > header'));
    await expect.poll(() => statusAndGroupOf(server, onboarding.id)).toEqual(['started', SEARCH]);

    // Dropped in another column outside a group, a grouped ticket keeps its group and changes status.
    await drag(page, rowOf(columns, onboarding.slug), columns.locator('[data-column-id="completed"] > header'));
    await expect.poll(() => statusAndGroupOf(server, onboarding.id)).toEqual(['completed', SEARCH]);
    await expect(group(columns, SEARCH, 'completed').locator('[data-ticket-slug]')).toHaveCount(1);

    // Dropped in its own column outside every group, it leaves the group.
    await drag(page, rowOf(columns, onboarding.slug), columns.locator('[data-column-id="completed"] > header'));
    await expect.poll(() => statusAndGroupOf(server, onboarding.id)).toEqual(['completed', null]);
    await expect(group(columns, SEARCH, 'completed')).toHaveCount(0);
  }, page);
});

test('HS2-8CDFKR renames and ungroups a group from its header menu', async ({ page }) => {
  test.setTimeout(120_000);
  await withServer(async (server, tickets) => {
    await open(page, server);
    const view = list(page),
      boardId = (await groupIdOf(server, BOARD))!,
      menu = page.locator('[data-ticket-group-menu="true"]'),
      dialog = page.locator('[data-dialog-component="ticket-group-dialog"]');
    await group(view, BOARD).locator('[data-component="list-header"]').click({ button: 'right' });
    await expect(menu.getByRole('menuitem', { name: 'Rename Group…' })).toBeVisible();
    await expect(menu.getByRole('menuitem', { name: 'Ungroup' })).toBeVisible();
    // Only a duplicate name offers a merge (HS2-9KZSD4).
    await expect(menu.getByRole('menuitem', { name: 'Merge Duplicate Groups' })).toHaveCount(0);
    await capture(page, 'group-header-menu-light-1440');
    await menu.getByRole('menuitem', { name: 'Rename Group…' }).click();
    const field = dialog.getByLabel('Group Name'),
      rename = dialog.getByRole('button', { name: 'Rename' });
    await expect(field).toHaveValue(BOARD);
    await expect(rename).toBeEnabled();
    // Another group's name is refused; the group's own name in another casing is not.
    await field.fill('search SYNTAX');
    await expect(dialog.getByText('already exists')).toBeVisible();
    await expect(rename).toBeDisabled();
    await capture(page, 'rename-group-duplicate-light-1440');
    await field.fill('Board polish');
    await rename.click();
    await expect(dialog).toBeHidden();
    // A rename is a registry edit: the same id keeps its members and its header (HS2-9KZSD4).
    const polished = group(view, 'Board polish');
    await expect(polished.locator('[data-ticket-slug]')).toHaveCount(3);
    await expect(polished).toHaveAttribute('data-ticket-group', boardId);
    await expect(polished.getByRole('button', { name: 'Board polish, 3 tickets' })).toBeVisible();
    await expect(group(view, BOARD)).toHaveCount(0);
    expect(await groupCounts(server)).toEqual([
      ['Board polish', 3],
      [SEARCH, 2],
    ]);
    expect(await groupIdOf(server, 'Board polish')).toBe(boardId);
    const boardTicket = Object.values(tickets).find((ticket) => ticket.group === boardId)!;
    expect((await server.request<FullTicket>(`/tickets/${boardTicket.id}`)).group).toBe(boardId);

    // The keyboard context-menu request opens the same menu.
    await group(view, SEARCH).locator('[data-action="toggle-ticket-group"]').focus();
    await group(view, SEARCH).locator('[data-action="toggle-ticket-group"]').dispatchEvent('contextmenu', {
      bubbles: true,
      cancelable: true,
      clientX: 0,
      clientY: 0,
    });
    await menu.getByRole('menuitem', { name: 'Ungroup' }).click();
    await expect(group(view, SEARCH)).toHaveCount(0);
    await expect(group(view, 'Board polish')).toBeVisible();
    expect(await groupCounts(server)).toEqual([['Board polish', 3]]);
    await expect(view.locator('[data-ticket-slug]')).toHaveCount(SEEDS.length);
  }, page);
});

test('HS2-EYEGDQ selects all of a group’s tickets in the list or one column from its header menu', async ({ page }) => {
  test.setTimeout(180_000);
  await withServer(async (server) => {
    await open(page, server);
    const view = list(page),
      menu = page.locator('[data-ticket-group-menu="true"]'),
      selectedIn = (scope: Locator) =>
        scope
          .locator('[data-row-selected="true"]')
          .evaluateAll((rows) =>
            rows.map((row) => row.querySelector<HTMLElement>('[data-ticket-slug]')?.dataset.ticketSlug),
          ),
      boardGroup = group(view, BOARD);
    // List view: every member of the group, in drawn order, and nothing outside it.
    await boardGroup.locator('[data-component="list-header"]').click({ button: 'right' });
    const item = menu.getByRole('menuitem', { name: 'Select All in Group' });
    await expect(item).toBeVisible();
    for (const width of [1440, 760]) {
      await page.setViewportSize({ width, height: 900 });
      await settle(page);
      await expect(item).toBeInViewport();
      await capture(page, `select-all-in-group-menu-light-${width}`);
    }
    await page.setViewportSize({ width: 1440, height: 900 });
    await item.click();
    await expect(menu).toHaveCount(0);
    const members = await slugsIn(boardGroup);
    expect(members).toHaveLength(3);
    await expect.poll(() => selectedIn(view)).toEqual(members);
    await capture(page, 'select-all-in-group-list-light-1440');

    // A collapsed group is expanded first, since a selection never holds hidden tickets.
    await group(view, SEARCH).locator('[data-action="toggle-ticket-group"]').click();
    await expect(group(view, SEARCH).locator('[data-ticket-slug]')).toHaveCount(0);
    await group(view, SEARCH).locator('[data-component="list-header"]').click({ button: 'right' });
    await menu.getByRole('menuitem', { name: 'Select All in Group' }).click();
    await expect(group(view, SEARCH).locator('[data-action="toggle-ticket-group"]')).toHaveAttribute(
      'aria-expanded',
      'true',
    );
    const searchMembers = await slugsIn(group(view, SEARCH));
    expect(searchMembers).toHaveLength(2);
    await expect.poll(() => selectedIn(view)).toEqual(searchMembers);

    // Column view: only that column's share of the group.
    await page.getByLabel('Columns View').click();
    const columns = board(page),
      started = group(columns, BOARD, 'started');
    await expect(started.locator('[data-ticket-slug]')).toHaveCount(2);
    await started.locator('[data-component="list-header"]').click({ button: 'right' });
    await menu.getByRole('menuitem', { name: 'Select All in Group' }).click();
    const startedMembers = await slugsIn(started);
    await expect.poll(() => selectedIn(columns)).toEqual(startedMembers);
    await capture(page, 'select-all-in-group-column-light-1440');

    // A group with one ticket in a column is an ordinary single selection.
    const notStarted = group(columns, BOARD, 'not-started');
    await notStarted.locator('[data-component="list-header"]').click({ button: 'right' });
    await menu.getByRole('menuitem', { name: 'Select All in Group' }).click();
    await expect.poll(() => selectedIn(columns)).toEqual(await slugsIn(notStarted));
  }, page);
});

test('HS2-44W3TP searches by group and keeps results grouped', async ({ page }) => {
  test.setTimeout(120_000);
  await withServer(async (server, tickets) => {
    await open(page, server);
    const view = list(page);
    await page.getByRole('button', { name: 'Search tickets' }).click();
    const search = page.getByRole('searchbox', { name: 'Search tickets' });
    // `group:` completes existing group names.
    await search.pressSequentially('group:');
    const suggestion = page.locator('[data-search-field-suggestion]').filter({ hasText: BOARD });
    await expect(suggestion).toBeVisible();
    await suggestion.click();
    const boardId = await groupIdOf(server, BOARD),
      searchId = await groupIdOf(server, SEARCH);
    const boardSlugs = Object.values(tickets)
      .filter((ticket) => ticket.group === boardId)
      .map((ticket) => ticket.slug)
      .sort();
    await expect.poll(async () => (await slugsIn(view)).sort()).toEqual(boardSlugs);
    // The matching tickets stay inside their group container.
    await expect(group(view, BOARD).locator('[data-ticket-slug]')).toHaveCount(3);
    await capture(page, 'search-group-filter-light-1440');

    const run = async (query: string) => {
      await page.getByRole('button', { name: 'Clear search' }).click();
      await search.pressSequentially(query);
      await search.press('Enter');
    };
    const expectSlugs = (expected: FullTicket[]) =>
      expect.poll(async () => (await slugsIn(view)).sort()).toEqual(expected.map((ticket) => ticket.slug).sort());
    const all = Object.values(tickets);
    await run('group:"search syntax"');
    await expectSlugs(all.filter((ticket) => ticket.group === searchId));
    // A group id is also a search term (HS2-9KZSD4).
    await run(`group:${searchId}`);
    await expectSlugs(all.filter((ticket) => ticket.group === searchId));
    await run('has:group');
    await expectSlugs(all.filter((ticket) => ticket.group));
    await run('NOT has:group');
    await expectSlugs(all.filter((ticket) => !ticket.group));
    await expect(view.locator('[data-component="ticket-group"]')).toHaveCount(0);
    await run('group:"Search syntax" OR group:"board drag & drop"');
    await expectSlugs(all.filter((ticket) => ticket.group));
    await run('group:missing');
    await expect(view.locator('[data-ticket-slug]')).toHaveCount(0);
  }, page);
});
