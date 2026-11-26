import { mkdir, mkdtemp, rename, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { expect, test } from '@playwright/test';

test('HS2-7V2EEV closes a missing remembered project on a phone and keeps it forgotten after reload', async ({
  page,
}) => {
  const root = '/work/synthetic-missing',
    captureDir = process.env.HS2_7V2EEV_CAPTURE_DIR;
  let opens = 0;
  await page.emulateMedia({ colorScheme: 'dark' });
  await page.addInitScript((remembered) => {
    if (localStorage.getItem('hs2-7v2eev-seeded')) return;
    localStorage.setItem('hs2-7v2eev-seeded', 'true');
    localStorage.setItem('hotsheet.open-projects', JSON.stringify([remembered]));
    localStorage.setItem('hotsheet.workspace.active-project-root.v1', remembered);
  }, root);
  await page.route('**/__hotsheet/projects/open', async (route) => {
    opens += 1;
    await route.fulfill({ status: 404, json: { error: 'Synthetic project folder is missing' } });
  });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/');
  const failure = page.locator('.project-restore-error'),
    mobileBar = page.locator('.project-tab-bar--mobile'),
    close = mobileBar.getByRole('button', { name: 'Close synthetic-missing Project' });
  await expect(failure).toContainText('Hot Sheet could not open synthetic-missing.');
  await expect(close).toBeVisible();

  for (const [name, width] of [
    ['phone', 390],
    ['medium', 940],
    ['wide', 1280],
    ['returned-wide', 1280],
  ] as const) {
    if (name === 'returned-wide') await page.setViewportSize({ width: 390, height: 844 });
    await page.setViewportSize({ width, height: 844 });
    await expect(failure).toBeVisible();
    if (width < 1024) await expect(close).toBeVisible();
    else await expect(mobileBar).toBeHidden();
    if (captureDir) {
      // The shell changes layout at this breakpoint; capture after its finite transition settles.
      await page.waitForTimeout(250);
      await mkdir(captureDir, { recursive: true });
      await page.screenshot({ path: `${captureDir}/hs2-7v2eev-${name}.png` });
    }
  }

  await page.setViewportSize({ width: 390, height: 844 });
  await close.click();
  const dialog = page.getByRole('dialog', { name: 'Close synthetic-missing?' });
  await expect(dialog).toBeVisible();
  await dialog.getByRole('button', { name: 'Cancel' }).click();
  await expect(failure).toBeVisible();
  await close.click();
  await dialog.getByRole('button', { name: 'Close Project' }).click();
  await expect(failure).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => localStorage.getItem('hotsheet.open-projects'))).toBe('[]');
  const attempts = opens;
  await page.reload();
  await expect(page.getByRole('button', { name: 'Open Project' })).toBeVisible();
  await expect(failure).toHaveCount(0);
  expect(opens).toBe(attempts);
});

test('HS2-TGYKHQ right-click removes an unavailable desktop project tab', async ({ page }) => {
  const root = '/work/synthetic-unavailable',
    captureDir = process.env.HS2_TGYKHQ_CAPTURE_DIR;
  let opens = 0;
  await page.addInitScript((remembered) => {
    if (localStorage.getItem('hs2-tgykhq-seeded')) return;
    localStorage.setItem('hs2-tgykhq-seeded', 'true');
    localStorage.setItem('hotsheet.open-projects', JSON.stringify([remembered]));
    localStorage.setItem('hotsheet.workspace.active-project-root.v1', remembered);
  }, root);
  await page.route('**/__hotsheet/projects/open', async (route) => {
    opens += 1;
    await route.fulfill({ status: 404, json: { error: 'Synthetic project folder is missing' } });
  });
  await page.setViewportSize({ width: 1280, height: 844 });
  await page.goto('/');

  const failure = page.locator('.project-restore-error'),
    tab = page.locator('[data-tab-kind="project"][data-restore-failure="true"]');
  await expect(failure).toContainText('Hot Sheet could not open synthetic-unavailable.');
  await expect(tab).toBeVisible();

  for (const [name, width] of [
    ['wide', 1280],
    ['medium', 940],
    ['narrow', 390],
    ['returned-wide', 1280],
  ] as const) {
    await page.setViewportSize({ width, height: 844 });
    await expect(failure).toBeVisible();
    if (width >= 1024) await expect(tab).toBeVisible();
    else await expect(page.getByRole('button', { name: 'Close synthetic-unavailable Project' })).toBeVisible();
    if (captureDir) {
      await mkdir(captureDir, { recursive: true });
      await page.screenshot({ path: `${captureDir}/hs2-tgykhq-${name}.png` });
    }
  }

  if (captureDir) {
    await page.emulateMedia({ colorScheme: 'dark' });
    await page.screenshot({ path: `${captureDir}/hs2-tgykhq-dark-wide.png` });
    await page.emulateMedia({ colorScheme: 'light' });
  }

  await tab.click({ button: 'right' });
  const menu = page.getByRole('menu', { name: 'Project Tab Actions' });
  await expect(menu).toBeVisible();
  await expect(menu.getByRole('menuitem')).toHaveText(['Close Tab']);
  if (captureDir) await page.screenshot({ path: `${captureDir}/hs2-tgykhq-context-menu-wide.png` });
  await menu.getByRole('menuitem', { name: 'Close Tab', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: 'Close synthetic-unavailable?' });
  await expect(dialog).toBeVisible();
  await dialog.getByRole('button', { name: 'Cancel' }).click();
  await expect(failure).toBeVisible();
  await tab.hover();
  await tab.getByRole('button', { name: 'Close synthetic-unavailable' }).click();
  await expect(dialog).toBeVisible();
  await dialog.getByRole('button', { name: 'Close Project' }).click();
  await expect(tab).toHaveCount(0);
  await expect(failure).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => localStorage.getItem('hotsheet.open-projects'))).toBe('[]');
  const attempts = opens;
  await page.reload();
  await expect(page.getByRole('button', { name: 'Open Project' })).toBeVisible();
  expect(opens).toBe(attempts);
});

test('HS2-125F0X closes real unavailable desktop project tabs by right-click and hover close', async ({ page }) => {
  // Real bridge state, no route mocks: one remembered folder was deleted and the other path is a
  // file, so the real `/__hotsheet/projects/open` refuses both and the app keeps them as failed tabs.
  const parent = await mkdtemp(join(tmpdir(), 'hs2-125f0x-')),
    missing = join(parent, 'deleted-project'),
    notFolder = join(parent, 'file-project'),
    captureDir = process.env.HS2_125F0X_CAPTURE_DIR;
  await writeFile(notFolder, 'not a project folder\n');
  let opens = 0;
  page.on('request', (request) => {
    if (new URL(request.url()).pathname === '/__hotsheet/projects/open') opens += 1;
  });
  await page.addInitScript(
    (remembered) => {
      if (localStorage.getItem('hs2-125f0x-seeded')) return;
      localStorage.setItem('hs2-125f0x-seeded', 'true');
      localStorage.setItem('hotsheet.open-projects', JSON.stringify(remembered));
      localStorage.setItem('hotsheet.workspace.active-project-root.v1', remembered[0]);
    },
    [missing, notFolder],
  );
  try {
    await page.setViewportSize({ width: 1280, height: 844 });
    await page.goto('/');
    const failure = page.locator('.project-restore-error'),
      tabs = page.locator('[data-tab-kind="project"][data-restore-failure="true"]'),
      deleted = tabs.filter({ hasText: 'deleted-project' }),
      file = tabs.filter({ hasText: 'file-project' });
    await expect(failure).toContainText('Project Folder Not Found');
    await expect(tabs).toHaveCount(2);

    for (const [name, width] of [
      ['wide', 1280],
      ['medium', 940],
      ['narrow', 390],
      ['returned-wide', 1280],
    ] as const) {
      await page.setViewportSize({ width, height: 844 });
      await expect(failure).toBeVisible();
      if (width >= 1024) {
        await expect(tabs).toHaveCount(2);
        // The shell keeps its regions for the unavailable tab, empty, with controls disabled (HS2-H0QGK7).
        await expect(
          page.getByRole('complementary', { name: 'Sidebar rail' }).locator('[data-project-unavailable-region]'),
        ).toContainText('Project Unavailable');
        await expect(page.locator('[data-component="toolbar"][aria-label="Workspace Toolbar"]')).toBeVisible();
        await expect(page.getByRole('button', { name: 'Show Terminal Drawer' })).toBeDisabled();
      }
      if (captureDir) {
        // The shell changes layout at this breakpoint; capture after its finite transition settles.
        await page.waitForTimeout(250);
        await mkdir(captureDir, { recursive: true });
        await page.screenshot({ path: `${captureDir}/hs2-125f0x-${name}.png` });
      }
    }

    // Right-click the unselected unavailable tab: the menu offers Close Tab alone.
    await file.click({ button: 'right' });
    const menu = page.getByRole('menu', { name: 'Project Tab Actions' });
    await expect(menu).toBeVisible();
    await expect(menu.getByRole('menuitem')).toHaveText(['Close Tab']);
    if (captureDir) {
      await page.screenshot({ path: `${captureDir}/hs2-125f0x-context-menu-wide.png` });
      await page.emulateMedia({ colorScheme: 'dark' });
      await page.screenshot({ path: `${captureDir}/hs2-125f0x-context-menu-dark-wide.png` });
      await page.emulateMedia({ colorScheme: 'light' });
    }
    await menu.getByRole('menuitem', { name: 'Close Tab', exact: true }).click();
    await page
      .getByRole('dialog', { name: 'Close file-project?' })
      .getByRole('button', { name: 'Close Project' })
      .click();
    await expect(file).toHaveCount(0);
    await expect(deleted).toBeVisible();

    // The selected unavailable tab reveals its own close control on hover, like any project tab.
    await deleted.hover();
    const close = deleted.getByRole('button', { name: 'Close deleted-project' });
    await expect(close).toHaveCSS('opacity', '1');
    await close.click();
    await page
      .getByRole('dialog', { name: 'Close deleted-project?' })
      .getByRole('button', { name: 'Close Project' })
      .click();
    await expect(tabs).toHaveCount(0);
    await expect(failure).toHaveCount(0);
    await expect.poll(() => page.evaluate(() => localStorage.getItem('hotsheet.open-projects'))).toBe('[]');

    const attempts = opens;
    await page.reload();
    await expect(page.getByRole('button', { name: 'Open Project' })).toBeVisible();
    await expect(tabs).toHaveCount(0);
    expect(opens).toBe(attempts);
  } finally {
    await rm(parent, { recursive: true, force: true });
  }
});

test('HS2-ZKAVZZ explains a moved folder, a file path, and a broken ticket repository link in plain language', async ({
  page,
}) => {
  // Real bridge state, no route mocks: a remembered folder moved away, a path that is now a file, and a
  // folder whose `.hotsheet2/store` link names a deleted ticket repository.
  const parent = await mkdtemp(join(tmpdir(), 'hs2-zkavzz-')),
    moved = join(parent, 'moved-project'),
    notFolder = join(parent, 'file-project'),
    linked = join(parent, 'linked-project'),
    captureDir = process.env.HS2_ZKAVZZ_CAPTURE_DIR;
  await mkdir(moved);
  await rename(moved, join(parent, 'moved-project-renamed'));
  await writeFile(notFolder, 'not a project folder\n');
  await mkdir(join(linked, '.hotsheet2'), { recursive: true });
  await writeFile(join(linked, '.hotsheet2', 'store'), '../deleted-tickets.hs2\n');
  let opens = 0;
  page.on('request', (request) => {
    if (new URL(request.url()).pathname === '/__hotsheet/projects/open') opens += 1;
  });
  await page.addInitScript(
    (remembered) => {
      if (localStorage.getItem('hs2-zkavzz-seeded')) return;
      localStorage.setItem('hs2-zkavzz-seeded', 'true');
      localStorage.setItem('hotsheet.open-projects', JSON.stringify(remembered));
      localStorage.setItem('hotsheet.workspace.active-project-root.v1', remembered[0]);
    },
    [moved, notFolder, linked],
  );
  try {
    await page.setViewportSize({ width: 1280, height: 844 });
    await page.goto('/');
    const failure = page.locator('.project-restore-error'),
      tabs = page.locator('[data-tab-kind="project"][data-restore-failure="true"]'),
      retry = failure.getByRole('button', { name: 'Retry', exact: true }),
      closeTab = failure.getByRole('button', { name: 'Close Tab', exact: true }),
      details = failure.locator('[data-project-unavailable-details]');
    await expect(tabs).toHaveCount(3);

    // The moved folder: a plain title and explanation, no error code, path, or server advice.
    await expect(failure).toHaveAttribute('data-project-unavailable-cause', 'folder-missing');
    await expect(failure.getByText('Project Folder Not Found', { exact: true })).toBeVisible();
    await expect(failure.getByText(/The folder for moved-project was moved, renamed, or deleted\./)).toBeVisible();
    await expect(retry).toBeEnabled();
    await expect(closeTab).toHaveAttribute('data-emphasis', 'primary');
    await expect(retry).toHaveAttribute('data-emphasis', 'secondary');
    await expect(failure).not.toContainText('remains remembered');
    // The raw error stays for diagnosis behind Technical Details, collapsed until asked for.
    const raw = details.getByText(/^ENOENT: no such file or directory, realpath '.*moved-project'$/);
    await expect(raw).toBeHidden();
    if (captureDir) {
      await mkdir(captureDir, { recursive: true });
      await page.screenshot({ path: `${captureDir}/hs2-zkavzz-moved-collapsed-wide.png` });
    }
    await details.getByText('Technical Details', { exact: true }).click();
    await expect(raw).toBeVisible();
    await expect(details.getByText(`Project folder: ${moved}`, { exact: true })).toBeVisible();
    // HS2-1ZKMS9: Technical Details sits in the EmptyState's start-aligned secondary region (Kerf
    // KF-GZDZXH), so the open disclosure keeps its chevron beside its title and its lines start on the
    // title's column, while Retry and Close Tab stay centered, at every width.
    const disclosureGeometry = () =>
      details.evaluate((node) => {
        const arrow = node.querySelector('.kui-disclosure-arrow')!.getBoundingClientRect(),
          title = node.querySelector('.kui-disclosure__title')!.getBoundingClientRect(),
          // The first glyph of the first detail line, past the Text's own inset.
          glyphs = document.createRange();
        glyphs.selectNodeContents(node.querySelector('.kui-disclosure__content .kui-text')!);
        const line = glyphs.getClientRects()[0],
          panel = node.closest('.kui-empty-state')!.getBoundingClientRect(),
          actions = node
            .closest('.kui-empty-state')!
            .querySelector('.kui-empty-state__action')!
            .getBoundingClientRect();
        return {
          inSecondary: Boolean(node.closest('.kui-empty-state__secondary')),
          chevronGap: Math.round(title.left - arrow.right),
          lineOffset: Math.round(Math.abs(line.left - title.left)),
          actionsCentered: Math.abs((actions.left + actions.right) / 2 - (panel.left + panel.right) / 2) <= 2,
        };
      });
    for (const width of [1280, 940, 390, 1280]) {
      await page.setViewportSize({ width, height: 844 });
      await expect(raw).toBeVisible();
      const geometry = await disclosureGeometry();
      expect(geometry.inSecondary).toBe(true);
      expect(geometry.chevronGap).toBeGreaterThanOrEqual(0);
      expect(geometry.chevronGap).toBeLessThanOrEqual(12);
      expect(geometry.lineOffset).toBeLessThanOrEqual(2);
      expect(geometry.actionsCentered).toBe(true);
    }

    if (captureDir) {
      await mkdir(captureDir, { recursive: true });
      for (const scheme of ['light', 'dark'] as const) {
        await page.emulateMedia({ colorScheme: scheme });
        for (const [name, width] of [
          ['wide', 1280],
          ['medium', 940],
          ['narrow', 390],
          ['returned-wide', 1280],
        ] as const) {
          await page.setViewportSize({ width, height: 844 });
          await expect(failure).toBeVisible();
          // The shell changes layout at this breakpoint; capture after its finite transition settles.
          await page.waitForTimeout(250);
          await page.screenshot({ path: `${captureDir}/hs2-zkavzz-moved-${scheme}-${name}.png` });
        }
      }
      await page.emulateMedia({ colorScheme: 'light' });
    }

    // Retry while the folder is still gone asks the bridge again and keeps the same explanation.
    const attempts = opens;
    await retry.click();
    await expect.poll(() => opens).toBeGreaterThan(attempts);
    await expect(retry).toBeEnabled();
    await expect(failure).toHaveAttribute('data-project-unavailable-cause', 'folder-missing');

    // A path that is now a file.
    await tabs.filter({ hasText: 'file-project' }).getByRole('tab').click();
    await expect(failure).toHaveAttribute('data-project-unavailable-cause', 'not-a-folder');
    await expect(failure.getByText('Project Path Is Not a Folder', { exact: true })).toBeVisible();
    await expect(closeTab).toHaveAttribute('data-emphasis', 'primary');

    // A folder whose ticket repository link names a deleted repository.
    await tabs.filter({ hasText: 'linked-project' }).getByRole('tab').click();
    await expect(failure).toHaveAttribute('data-project-unavailable-cause', 'store-link-broken');
    await expect(failure.getByText('Ticket Repository Not Found', { exact: true })).toBeVisible();
    await expect(retry).toHaveAttribute('data-emphasis', 'primary');
    await expect(retry).toBeEnabled();
    if (captureDir) {
      await page.screenshot({ path: `${captureDir}/hs2-zkavzz-store-link-wide.png` });
      await tabs.filter({ hasText: 'file-project' }).getByRole('tab').click();
      await page.screenshot({ path: `${captureDir}/hs2-zkavzz-not-a-folder-wide.png` });
    }

    // Close Tab in the panel enters the same close flow as the tab's own close control.
    await tabs.filter({ hasText: 'moved-project' }).getByRole('tab').click();
    await expect(failure).toHaveAttribute('data-project-unavailable-cause', 'folder-missing');
    await closeTab.click();
    await page
      .getByRole('dialog', { name: 'Close moved-project?' })
      .getByRole('button', { name: 'Close Project' })
      .click();
    await expect(tabs).toHaveCount(2);
    await expect(tabs.filter({ hasText: 'moved-project' })).toHaveCount(0);
    await expect
      .poll(() => page.evaluate(() => JSON.parse(localStorage.getItem('hotsheet.open-projects') ?? '[]') as string[]))
      .toEqual([notFolder, linked]);
  } finally {
    await rm(parent, { recursive: true, force: true });
  }
});
