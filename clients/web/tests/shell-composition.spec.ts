import { expect, type Page, test } from '@playwright/test';

// The app shell, terminal grid, terminal drawer, and project strip configure one another through
// props rather than cross-component CSS (HS2-DR549A). These flows drive the real client against the
// server's wire shapes and assert each composed presentation, both ways, through the shipped shell.

const capabilities = {
  create: true,
  update: true,
  close: true,
  notes: true,
  note_edit: true,
  note_delete: true,
  attachments: true,
  assignment: true,
  review_requests: true,
  dependencies: true,
  up_next: true,
  close_reasons: true,
  claims: true,
  atomic_batch: true,
  not_working_report: true,
  offline_mutation: true,
  history: true,
  watch: true,
  provider_idempotency: true,
  query_fields: [],
};
const ticket = (slug: string) => ({
  connection_id: 'git-local',
  native_id: slug,
  qualified_id: `git-local:${slug}`,
  id: slug,
  slug,
  title: slug,
  status: 'started',
  up_next: false,
  feedback_needed: false,
  tags: [],
  blocked_by: [],
  claim_count: 0,
});

async function openProject(page: Page) {
  await page.route('**/*', async (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname;
    if (path === '/__hotsheet/projects/open') {
      const root = request.postDataJSON().root as string;
      return route.fulfill({
        status: 201,
        json: {
          id: 'demo-checkout',
          root,
          name: root.split('/').at(-1),
          stores: [`${root}.hs2`],
          apiPath: '/__hotsheet/project-api/demo-checkout',
        },
      });
    }
    if (path.endsWith('/providers'))
      return route.fulfill({
        json: [
          {
            connection_id: 'git-local',
            provider: 'git',
            display_name: 'Hot Sheet git',
            locator: '/tickets',
            default: true,
            capabilities,
          },
        ],
      });
    if (path.endsWith('/tickets'))
      return route.fulfill({
        json: {
          items: [ticket('HS2-C1'), ticket('HS2-C2')],
          counts: {
            total: 2,
            queued: 2,
            backlog: 0,
            archive: 0,
            open: 2,
            up_next: 0,
            active: 0,
            started: 2,
            completed_today: 0,
            completion_trend: [0, 0, 0, 0, 0, 0, 0],
          },
        },
      });
    if (path.endsWith('/repository/status'))
      return route.fulfill({
        json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true },
      });
    if (path.endsWith('/ws/poll'))
      return route.fulfill({
        json: { cursor: Number(url.searchParams.get('since') ?? 0), events: [], overflow: false },
      });
    if (path.endsWith('/terminals'))
      return route.fulfill({ json: [{ id: 'codex-main', alive: true, busy: false, cwd: '/work/demo' }] });
    if (
      path.endsWith('/connections') ||
      path.endsWith('/commands') ||
      path.endsWith('/command-runs') ||
      path.endsWith('/views') ||
      path.endsWith('/corrupt-tickets')
    )
      return route.fulfill({ json: [] });
    return route.continue();
  });
  await page.addInitScript(() => {
    localStorage.setItem('hotsheet.terminals.drawer-open', 'true');
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('textbox', { name: /^Project folder/ }).fill('/work/demo');
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await expect(page.locator('.app-shell__workspace')).toBeVisible();
}

test('composes the desktop shell, project strip, grid zoom, and magnified focus ring through props (HS2-DR549A)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await openProject(page);
  const shell = page.locator('.app-shell'),
    strip = page.locator('.app-shell__main > .project-tab-bar'),
    workArea = page.locator('.app-shell__work-area'),
    workspace = page.locator('.app-shell__workspace');

  // The application root renders the edge-to-edge viewport presentation, not the framed demo shell.
  await expect(shell).toHaveAttribute('data-presentation', 'viewport');
  await expect(shell).toHaveCSS('border-top-width', '0px');
  expect((await shell.boundingBox())!.width).toBe(1280);

  // Project mode: the strip shares the column surface and the page header draws the separator.
  await expect(strip).toHaveAttribute('data-surface', 'default');
  await expect(strip).toHaveAttribute('data-divider', 'false');
  await expect(strip).toHaveCSS('border-bottom-width', '0px');
  const surface = await shell.evaluate((node) => getComputedStyle(node).backgroundColor);
  await expect(strip).toHaveCSS('background-color', surface);
  await expect(workspace).toHaveAttribute('data-bottom-edge', 'false');

  // Terminals mode: the strip draws its own bottom rule (HS2-WH6CCR).
  await page.getByRole('button', { name: 'Workspace grid' }).click();
  await expect(shell).toHaveAttribute('data-mode', 'terminals');
  await expect(strip).toHaveAttribute('data-divider', 'true');
  await expect(strip).toHaveCSS('border-bottom-width', '1px');

  // The visibility Select is configured as a borderless toolbar trigger inside its control group.
  const visibility = page.locator('.terminal-dashboard-controls__visibility-select [data-component="select"]');
  await expect(visibility).toHaveAttribute('data-presentation', 'toolbar-borderless');
  await expect(visibility).toHaveAttribute('data-focus-ring-owner', 'group');

  // Kerf's zoom group owns its buttons' disabled state (KF-FTADQT, HS2-T44PFW): a disabled zoom button
  // reads as unavailable, dims, and paints no hover chrome.
  const zoomIn = page.getByRole('button', { name: /^Zoom in/ });
  for (let step = 0; step < 8 && (await zoomIn.isEnabled()); step += 1) await zoomIn.click();
  await expect(zoomIn).toBeDisabled();
  await expect(zoomIn).toHaveCSS('cursor', 'not-allowed');
  await expect(zoomIn).toHaveCSS('opacity', '0.5');
  await zoomIn.hover({ force: true });
  await expect(zoomIn).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');

  // Magnifying a terminal turns the work-area ring off; dismissing it restores the ring.
  await expect(workArea).toHaveAttribute('data-focus-ring', 'true');
  await page.locator('.terminal-tile[data-preview-only="true"]').first().click();
  await expect(page.locator('.terminal-dashboard__magnified')).toBeVisible();
  await expect(workArea).toHaveAttribute('data-focus-ring', 'false');
  await expect(workArea).toHaveCSS('outline-color', 'rgba(0, 0, 0, 0)');
  await page.keyboard.press('Escape');
  await expect(page.locator('.terminal-dashboard__magnified')).toHaveCount(0);
  await expect(workArea).toHaveAttribute('data-focus-ring', 'true');

  // Back in project mode the strip returns to its divider-free presentation.
  await strip.getByRole('tab', { name: 'demo' }).click();
  await expect(shell).toHaveAttribute('data-mode', 'project');
  await expect(strip).toHaveAttribute('data-divider', 'false');
  await expect(strip).toHaveCSS('border-bottom-width', '0px');
});

test('tracks the phone bottom edge and dedicated-terminal clipping through the drawer state (HS2-DR549A)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openProject(page);
  const workspace = page.locator('.app-shell__workspace'),
    drawer = page.locator('[data-component="terminal-drawer"]');
  await page.addStyleTag({ content: ':root { --hotsheet-safe-area-bottom: 34px !important; }' });

  // The expanded drawer owns the bottom edge: the workspace keeps its ordinary padding.
  await expect(drawer).toBeVisible();
  await expect(workspace).toHaveAttribute('data-bottom-edge', 'false');
  const ordinary = await workspace.evaluate((node) => parseFloat(getComputedStyle(node).paddingBottom));

  // The drawer grid anchors its zoom toolbar at the drawer's already-padded edge.
  await expect(page.locator('[data-layout-mode="drawer"] .terminal-dashboard__zoom')).toHaveCSS(
    'inset-block-end',
    '0px',
  );

  // A dedicated drawer terminal clips its scaled xterm on a phone.
  await drawer.getByRole('tab', { name: /Codex Main/ }).click();
  await expect(drawer.locator('.terminal-session[data-mobile="true"] .terminal-viewport')).toHaveCSS(
    'overflow-x',
    'clip',
  );

  // Collapsing the drawer hands the edge to the workspace, which then pads by the inset.
  await drawer.getByRole('button', { name: 'Hide terminal drawer' }).click();
  await expect(workspace).toHaveAttribute('data-bottom-edge', 'true');
  await expect
    .poll(() => workspace.evaluate((node) => parseFloat(getComputedStyle(node).paddingBottom)))
    .toBe(ordinary + 34);
  await expect(workspace).toHaveCSS('scroll-padding-bottom', '34px');

  // Reopening the drawer returns the edge to it.
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  await expect(workspace).toHaveAttribute('data-bottom-edge', 'false');
  await expect
    .poll(() => workspace.evaluate((node) => parseFloat(getComputedStyle(node).paddingBottom)))
    .toBe(ordinary);
});
