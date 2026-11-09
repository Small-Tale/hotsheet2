import { expect, test } from '@playwright/test';

const row = {
  connection_id: 'git',
  native_id: '1',
  qualified_id: 'git:1',
  id: '1',
  slug: 'HS2-GALLERY',
  title: 'Synthetic image',
  category: 'bug',
  priority: 'default',
  status: 'started',
  up_next: true,
  feedback_needed: false,
  tags: [],
  blocked_by: [],
  claim_count: 0,
  created_at: '2026-09-09T00:00:00Z',
  updated_at: '2026-09-09T00:00:00Z',
};

test('bare annotation shortcuts enter markup through its normal session at every gallery width (HS2-J02JH7)', async ({
  page,
}) => {
  const markupWrites: string[] = [];
  await page.route('**/*', async (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname;
    if (!path.startsWith('/__hotsheet/')) return route.continue();
    if (path === '/__hotsheet/projects/open')
      return route.fulfill({
        status: 201,
        json: {
          id: 'demo',
          root: '/work/demo',
          name: 'Synthetic gallery',
          stores: ['/work/demo.hs2'],
          apiPath: '/__hotsheet/project-api/demo',
          needsTicketSetup: false,
          needsHs1Migration: false,
          hs1ImportCompleted: false,
          hs1CleanupEligible: false,
        },
      });
    if (path.endsWith('/tickets') && request.method() === 'GET')
      return route.fulfill({
        json: url.searchParams.has('page_size')
          ? { items: [row], counts: { all: 1, queue: 1, backlog: 0, archive: 0 } }
          : [row],
      });
    if (path.endsWith('/checkouts/demo/tickets/git%3A1') && request.method() === 'GET')
      return route.fulfill({
        json: {
          store: 'git',
          ...row,
          details: 'Synthetic media fixture',
          blocked_reason: null,
          concurrency_token: 'token',
          notes: [],
          attachments: [{ id: 'IMAGE', filename: 'sample.svg', created_at: '2026-09-09T00:01:00Z' }],
        },
      });
    if (path.endsWith('/providers'))
      return route.fulfill({
        json: [
          {
            connection_id: 'git',
            provider: 'git',
            display_name: 'Hot Sheet Git',
            locator: '/work/demo.hs2',
            default: true,
            capabilities: { create: true, update: true, notes: true, attachments: true, watch: true, query_fields: [] },
          },
        ],
      });
    if (
      path.endsWith('/connections') ||
      path.endsWith('/permissions') ||
      path.endsWith('/commands') ||
      path.endsWith('/terminals') ||
      path.endsWith('/corrupt-tickets')
    )
      return route.fulfill({ json: [] });
    if (path.endsWith('/repository/status'))
      return route.fulfill({
        json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true },
      });
    if (path.endsWith('/terminal-settings')) return route.fulfill({ json: { inherit_global_shell_history: false } });
    if (path.endsWith('/ws/poll')) return route.fulfill({ json: { cursor: 0, events: [], overflow: false } });
    if (path.endsWith('/attachments/IMAGE') && request.method() === 'GET')
      return route.fulfill({
        contentType: 'image/svg+xml',
        body: '<svg xmlns="http://www.w3.org/2000/svg" width="640" height="360"><rect width="640" height="360" fill="#3976b7"/></svg>',
      });
    if (path.endsWith('/attachments/IMAGE/markup') && request.method() === 'PUT') {
      markupWrites.push(request.postData() ?? '');
      return route.fulfill({ json: { revision: 'synthetic-v2' } });
    }
    return route.fulfill({ status: 404, json: { error: 'not mocked' } });
  });

  await page.setViewportSize({ width: 1280, height: 720 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
  await page.locator('[data-ticket-slug="HS2-GALLERY"]').click();
  await page.getByRole('tab', { name: /Attachments/ }).click();
  await page.getByRole('button', { name: 'Open sample.svg in media gallery' }).click();
  const gallery = page.getByRole('dialog', { name: 'Image 1 of 1: sample.svg' }),
    workspace = gallery.locator('.attachment-gallery__workspace'),
    tools = [
      ['v', 'Select annotation'],
      ['r', 'Add rectangle'],
      ['f', 'Draw freehand'],
      ['a', 'Add arrow'],
      ['i', 'Add insertion mark'],
      ['s', 'Add strike-through'],
    ] as const;
  await expect(gallery).toBeVisible();
  for (const [key, name] of tools) {
    await gallery.getByRole('button', { name: 'Annotate media' }).focus();
    await page.keyboard.press(key);
    await expect(workspace).toHaveAttribute('data-markup', 'true');
    await expect(gallery.getByRole('button', { name })).toHaveAttribute('aria-pressed', 'true');
    await gallery.getByRole('button', { name: 'Finish markup' }).click();
    await expect(workspace).toHaveAttribute('data-markup', 'false');
  }
  await gallery.getByRole('button', { name: 'Annotate media' }).focus();
  await page.keyboard.press('Shift+R');
  await expect(workspace).toHaveAttribute('data-markup', 'false');
  for (const [width, height, label] of [
    [1280, 720, 'wide'],
    [940, 720, 'medium'],
    [390, 844, 'phone'],
    [1280, 720, 'returned-wide'],
  ] as const) {
    await page.setViewportSize({ width, height });
    await gallery.getByRole('button', { name: 'Annotate media' }).focus();
    await page.keyboard.press('r');
    await expect(gallery.getByRole('button', { name: 'Add rectangle' })).toHaveAttribute('aria-pressed', 'true');
    await gallery.screenshot({ path: test.info().outputPath(`hs2-j02jh7-${label}.png`), animations: 'disabled' });
    await gallery.getByRole('button', { name: 'Finish markup' }).click();
  }
  expect(markupWrites).toEqual([]);
});
