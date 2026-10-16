import { expect, type Page, test } from '@playwright/test';

import type { Attachment } from '../src/api';
import { ticketsJson } from './ticket-page-fixture';

const projects = {
  source: {
    id: 'source-project',
    root: '/work/source',
    name: 'Kerf',
    stores: ['/work/source.hs2'],
    apiPath: '/__hotsheet/project-api/source-project',
  },
  target: {
    id: 'target-project',
    root: '/work/target',
    name: 'Hot Sheet 2',
    stores: ['/work/target.hs2'],
    apiPath: '/__hotsheet/project-api/target-project',
  },
  deep: {
    id: 'third-project',
    root: '/work/deep',
    name: 'Deep Work',
    stores: ['/work/deep.hs2'],
    apiPath: '/__hotsheet/project-api/third-project',
  },
} as const;
const base = {
  category: 'bug',
  priority: 'default',
  status: 'started',
  up_next: false,
  feedback_needed: false,
  tags: [],
  blocked_by: [],
  claim_count: 0,
  created_at: '2026-09-11T00:00:00Z',
  updated_at: '2026-09-11T00:01:00Z',
};
const source = {
  ...base,
  connection_id: 'git-source',
  native_id: 'source',
  qualified_id: 'git-source:source',
  id: 'source',
  slug: 'KF-ROOT01',
  title: 'Original workspace selection',
  details: 'Open @target-project/HS2-LINK01 without leaving this ticket.',
};
const target = {
  ...base,
  connection_id: 'git-target',
  native_id: 'target',
  qualified_id: 'git-target:target',
  id: 'target',
  slug: 'HS2-LINK01',
  title: 'First linked reader',
  details: 'Continue to @third-project/HS2-LINK01 in another project.',
};
const deep = {
  ...base,
  connection_id: 'git-deep',
  native_id: 'deep',
  qualified_id: 'git-deep:deep',
  id: 'deep',
  slug: 'HS2-LINK01',
  title: 'Same slug, different project',
  details: 'The navigation stack can now unwind safely.',
};
type FixtureTicket = typeof source | typeof target | typeof deep;
const rows: Record<string, FixtureTicket[]> = {
  'source-project': [source],
  'target-project': [target],
  'third-project': [deep],
};
const projectByRoot = new Map<string, (typeof projects)[keyof typeof projects]>(
  Object.values(projects).map((project) => [project.root, project]),
);

// A 1×1 PNG served for fixture attachment media.
const GALLERY_PNG = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==',
  'base64',
);

async function mockLayeredProjects(
  page: Page,
  mutations: Array<{ projectId: string; ticketId: string; patch: Record<string, unknown> }> = [],
  rejectMutations = false,
  attachmentsFor: Record<string, Attachment[]> = {},
  notesFor: Record<string, unknown[]> = {},
  codeReviewRequests: Array<{ projectId: string; ticketId: string; method: string }> = [],
) {
  const chooser = ['/work/target', '/work/deep'];
  await page.route('**/*', async (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = decodeURIComponent(url.pathname);
    if (path === '/__hotsheet/projects/open') {
      const root = (request.postDataJSON() as { root: string }).root;
      return route.fulfill({ status: 201, json: projectByRoot.get(root) ?? projects.source });
    }
    if (path === '/__hotsheet/folders/choose') return route.fulfill({ json: { path: chooser.shift() } });
    const projectId = path.match(/project-api\/([^/]+)/)?.[1],
      project = projectId ? Object.values(projects).find((item) => item.id === projectId) : undefined,
      tickets = projectId ? (rows[projectId] ?? []) : [];
    if (path.endsWith('/providers'))
      return route.fulfill({
        json: [
          {
            connection_id: tickets[0]?.connection_id ?? 'git-source',
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
      await new Promise((resolve) => setTimeout(resolve, 500));
      return route.fulfill({ json: { cursor: 0, events: [], overflow: false } });
    }
    if (path.endsWith('/tickets') && request.method() === 'GET') {
      const query = url.searchParams.get('text')?.toLocaleLowerCase();
      return route.fulfill({
        json: ticketsJson(
          request,
          query
            ? tickets.filter(
                (ticket) =>
                  ticket.slug.toLocaleLowerCase() === query || ticket.title.toLocaleLowerCase().includes(query),
              )
            : tickets,
        ),
      });
    }
    const reviewTicket = path.match(/\/tickets\/([^/]+)\/code-review$/)?.[1];
    if (reviewTicket) {
      codeReviewRequests.push({ projectId: projectId!, ticketId: reviewTicket, method: request.method() });
      if (request.method() === 'POST') return route.fulfill({ status: 204, body: '' });
      if (projectId === 'target-project')
        return route.fulfill({ status: 500, json: { error: 'Code review unavailable for this checkout' } });
      return route.fulfill({
        json: {
          difftool: 'Glassbox',
          truncated: false,
          ranges: [],
          commits: [
            {
              sha: `${projectId}-sha`,
              short_sha: `${projectId}`.slice(0, 7),
              subject: `${reviewTicket}: commit in ${projectId}`,
              committed_at: '2026-09-02T10:00:00Z',
            },
          ],
        },
      });
    }
    const ticketId = path.match(/\/tickets\/([^/]+)$/)?.[1];
    if (ticketId && request.method() === 'PATCH') {
      if (rejectMutations) return route.fulfill({ status: 500, json: { error: 'Draft persistence failed' } });
      const ticket = tickets.find((item) =>
        [item.id, item.native_id, item.qualified_id].some((identity) => identity === ticketId),
      );
      const patch = request.postDataJSON() as Record<string, unknown>;
      if (!ticket) return route.fulfill({ status: 404, json: { error: `No ${ticketId}` } });
      mutations.push({ projectId: projectId!, ticketId, patch });
      return route.fulfill({
        json: {
          store: ticket.connection_id,
          ...ticket,
          ...patch,
          blocked_reason: patch.blocked_reason ?? null,
          concurrency_token: `token-${ticket.id}-${mutations.length}`,
          notes: [],
          attachments: [],
        },
      });
    }
    if (/\/attachments\/[^/]+$/.test(path) && request.method() === 'GET')
      return route.fulfill({ contentType: 'image/png', body: GALLERY_PNG });
    if (ticketId && request.method() === 'GET') {
      const ticket = tickets.find((item) =>
        [item.id, item.native_id, item.qualified_id].some((identity) => identity === ticketId),
      );
      return ticket
        ? route.fulfill({
            json: {
              store: ticket.connection_id,
              ...ticket,
              blocked_reason: null,
              concurrency_token: `token-${ticket.id}`,
              notes: notesFor[ticket.id] ?? [],
              attachments: attachmentsFor[ticket.id] ?? [],
            },
          })
        : route.fulfill({ status: 404, json: { error: `No ${ticketId}` } });
    }
    if (!path.startsWith('/__hotsheet/')) return route.continue();
    return route.fulfill({ status: 404, json: { error: `Unhandled ${request.method()} ${path}` } });
  });
}

test('uses native modality for focus, nested Escape, backdrop policy, and responsive containment', async ({ page }) => {
  await mockLayeredProjects(page);
  await page.setViewportSize({ width: 1200, height: 900 });
  await openProjects(page);
  const row = page.locator('[data-component="ticket-list-row"][data-ticket-slug="KF-ROOT01"]');
  await row.dblclick();
  const reader = page.getByRole('dialog', { name: 'Read and edit KF-ROOT01 in Kerf' }),
    close = reader.getByRole('button', { name: 'Close Ticket Reader' });
  await expect(reader).toBeVisible();
  await expect(close).toBeFocused();
  expect(
    await reader.evaluate((host) => (host.shadowRoot!.querySelector('dialog') as HTMLDialogElement).matches(':modal')),
  ).toBe(true);
  await page.getByRole('button', { name: 'Add Project' }).evaluate((node) => {
    (node as HTMLElement).focus();
  });
  expect(await reader.evaluate((host) => host.contains(document.activeElement))).toBe(true);
  for (const key of ['Tab', 'Shift+Tab', 'Tab']) {
    await page.keyboard.press(key);
    expect(await reader.evaluate((host) => host.contains(document.activeElement))).toBe(true);
  }
  const status = reader.locator('[data-action="open-inspector-status-menu"]');
  const statusMenu = reader.locator('[data-inspector-status-menu]');
  await status.click();
  await expect(statusMenu).toHaveJSProperty('open', true);
  await page.keyboard.press('Escape');
  await expect(statusMenu).toHaveJSProperty('open', false);
  await page.waitForTimeout(200);
  await expect(reader).toBeVisible();
  await page.mouse.click(4, 4);
  await expect(reader).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hs2-x1wgx5-reader-modal-wide.png', fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  let bounds = await reader.evaluate((host) => {
    const { x, y, width, height } = (
      host.shadowRoot!.querySelector('dialog') as HTMLDialogElement
    ).getBoundingClientRect();
    return { x, y, width, height };
  });
  expect(bounds.x).toBeGreaterThanOrEqual(0);
  expect(bounds.y).toBeGreaterThanOrEqual(0);
  expect(bounds.x + bounds.width).toBeLessThanOrEqual(390);
  expect(bounds.y + bounds.height).toBeLessThanOrEqual(844);
  await page.screenshot({ path: 'target/visual-captures/hs2-x1wgx5-reader-modal-narrow.png', fullPage: true });
  await page.setViewportSize({ width: 600, height: 450 });
  bounds = await reader.evaluate((host) => {
    const { x, y, width, height } = (
      host.shadowRoot!.querySelector('dialog') as HTMLDialogElement
    ).getBoundingClientRect();
    return { x, y, width, height };
  });
  expect(bounds.x + bounds.width).toBeLessThanOrEqual(600);
  expect(bounds.y + bounds.height).toBeLessThanOrEqual(450);
  await page.screenshot({ path: 'target/visual-captures/hs2-x1wgx5-reader-modal-200-percent.png', fullPage: true });
  await page.keyboard.press('Escape');
  await expect(reader).toBeHidden();
  await expect(row).toBeFocused();
});

test('vetoes reader dismissal when an asynchronous draft flush fails', async ({ page }) => {
  await mockLayeredProjects(page, [], true);
  await openProjects(page);
  await page.locator('[data-component="ticket-list-row"][data-ticket-slug="KF-ROOT01"]').dblclick();
  const reader = page.getByRole('dialog', { name: 'Read and edit KF-ROOT01 in Kerf' });
  await reader.locator('[data-action="edit-markdown"]').click({ position: { x: 4, y: 4 } });
  await reader.getByRole('textbox', { name: 'Ticket details' }).fill('Unsaved reader draft');
  await page.keyboard.press('Escape');
  await expect(page.locator('.app-error')).toContainText('Draft persistence failed');
  await expect(reader).toBeVisible();
  await expect(reader).toHaveAttribute('open', '');
});

async function openProjects(page: Page) {
  await page.goto('/');
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Add Project' }).click();
  await expect(page.getByRole('tab', { name: 'Hot Sheet 2' })).toHaveAttribute('aria-selected', 'true');
  await page.getByRole('button', { name: 'Add Project' }).click();
  await expect(page.getByRole('tab', { name: 'Deep Work' })).toHaveAttribute('aria-selected', 'true');
  await page.getByRole('tab', { name: 'Kerf' }).click();
  await page.locator('[data-ticket-slug="KF-ROOT01"]').click();
}

test('layers exact cross-project ticket readers and unwinds focus without changing workspace selection', async ({
  page,
}) => {
  await mockLayeredProjects(page);
  await page.setViewportSize({ width: 1200, height: 900 });
  await openProjects(page);
  const workspaceInspector = page.locator('#app-right-rail');
  const inspectorLink = workspaceInspector.getByRole('link', { name: '@target-project/HS2-LINK01' });
  await inspectorLink.click();
  const first = page.getByRole('dialog', { name: 'Read and edit HS2-LINK01 in Hot Sheet 2' });
  await expect(first).toBeVisible();
  await expect(page.getByRole('tab', { name: 'Kerf' })).toHaveAttribute('aria-selected', 'true');
  await expect(workspaceInspector.locator('[data-component="ticket-inspector-header"]')).toHaveAttribute(
    'data-ticket-slug',
    'KF-ROOT01',
  );
  await first.getByRole('link', { name: '@third-project/HS2-LINK01' }).click();
  const third = page.getByRole('dialog', { name: 'Read and edit HS2-LINK01 in Deep Work' });
  await expect(third).toBeVisible();
  await expect(third.getByText('Reader 2 of 2')).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hs2-t5vnj8-layered-readers-wide.png', fullPage: true });
  await page.setViewportSize({ width: 720, height: 760 });
  await expect(third).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hs2-t5vnj8-layered-readers-narrow.png', fullPage: true });
  // In the mobile layout the workspace inspector is a closed overlay whose content Kerf makes inert, so
  // unwind at the wide layout where the opening link is reachable.
  await page.setViewportSize({ width: 1200, height: 900 });
  await page.keyboard.press('Escape');
  await expect(third).toHaveCount(0);
  await expect(first.getByRole('link', { name: '@third-project/HS2-LINK01' })).toBeFocused();
  await first.getByRole('button', { name: 'Close Ticket Reader' }).click();
  await expect(first).toHaveCount(0);
  await expect(inspectorLink).toBeFocused();
  await expect(page.getByRole('tab', { name: projects.source.name })).toHaveAttribute('aria-selected', 'true');
  await expect(workspaceInspector.locator('[data-component="ticket-inspector-header"]')).toHaveAttribute(
    'data-ticket-slug',
    'KF-ROOT01',
  );
});

test('layers links above an existing reader and closes one level at a time', async ({ page }) => {
  await mockLayeredProjects(page);
  await openProjects(page);
  await page.locator('[data-component="ticket-list-row"][data-ticket-slug="KF-ROOT01"]').dblclick();
  const baseReader = page.getByRole('dialog', { name: 'Read and edit KF-ROOT01 in Kerf' });
  await baseReader.getByRole('link', { name: '@target-project/HS2-LINK01' }).click();
  const linked = page.getByRole('dialog', { name: 'Read and edit HS2-LINK01 in Hot Sheet 2' });
  await expect(linked.getByText('Reader 2 of 2')).toBeVisible();
  await expect(baseReader).not.toBeVisible();
  await linked.getByRole('button', { name: 'Close Ticket Reader' }).click();
  await expect(linked).toHaveCount(0);
  await expect(baseReader).toBeVisible();
  await expect(baseReader.getByRole('link', { name: '@target-project/HS2-LINK01' })).toBeFocused();
});

test('edits same-slug linked readers through their owning project and flushes before unwind', async ({ page }) => {
  const mutations: Array<{ projectId: string; ticketId: string; patch: Record<string, unknown> }> = [];
  await mockLayeredProjects(page, mutations);
  await page.setViewportSize({ width: 1200, height: 900 });
  await openProjects(page);
  await page.locator('#app-right-rail').getByRole('link', { name: '@target-project/HS2-LINK01' }).click();
  const targetReader = page.getByRole('dialog', { name: 'Read and edit HS2-LINK01 in Hot Sheet 2' });
  await targetReader.getByRole('link', { name: '@third-project/HS2-LINK01' }).click();
  const deepReader = page.getByRole('dialog', { name: 'Read and edit HS2-LINK01 in Deep Work' });
  await deepReader.locator('[data-action="edit-markdown"]').click({ position: { x: 4, y: 4 } });
  await deepReader.getByRole('textbox', { name: 'Ticket details' }).fill('Deep project draft stays with git-deep.');
  await page.screenshot({ path: 'target/visual-captures/hs2-1xqb2k-linked-edit-wide.png', fullPage: true });
  await page.setViewportSize({ width: 720, height: 760 });
  await page.screenshot({ path: 'target/visual-captures/hs2-1xqb2k-linked-edit-narrow.png', fullPage: true });
  await deepReader.getByRole('button', { name: 'Close Ticket Reader' }).click();
  await expect(deepReader).toHaveCount(0);
  await targetReader.locator('[data-action="edit-markdown"]').click({ position: { x: 4, y: 4 } });
  await targetReader
    .getByRole('textbox', { name: 'Ticket details' })
    .fill('Target project draft stays with git-target.');
  await targetReader.getByRole('button', { name: 'Close Ticket Reader' }).click();
  await expect(targetReader).toHaveCount(0);
  await expect
    .poll(() => mutations.map((item) => [item.projectId, item.ticketId, item.patch.details]))
    .toEqual([
      ['third-project', 'git-deep:deep', 'Deep project draft stays with git-deep.'],
      ['target-project', 'git-target:target', 'Target project draft stays with git-target.'],
    ]);
  await expect(page.locator('#app-right-rail [data-component="ticket-inspector-header"]')).toHaveAttribute(
    'data-ticket-slug',
    'KF-ROOT01',
  );
});

test("shows each stacked linked reader's own Code Review tab (HS2-9Q4AFB)", async ({ page }) => {
  const codeReviewRequests: Array<{ projectId: string; ticketId: string; method: string }> = [];
  await mockLayeredProjects(page, [], false, {}, {}, codeReviewRequests);
  await page.setViewportSize({ width: 1280, height: 900 });
  await openProjects(page);
  await page.locator('#app-right-rail').getByRole('link', { name: '@target-project/HS2-LINK01' }).click();
  const targetReader = page.getByRole('dialog', { name: 'Read and edit HS2-LINK01 in Hot Sheet 2' });
  // A failed load explains itself instead of leaving only the title.
  await targetReader.getByRole('tab', { name: 'Code Review' }).click();
  await expect(targetReader.getByText('Code review unavailable for this checkout')).toBeVisible();
  await targetReader.getByRole('tab', { name: 'Info' }).click();
  await targetReader.getByRole('link', { name: '@third-project/HS2-LINK01' }).click();
  const deepReader = page.getByRole('dialog', { name: 'Read and edit HS2-LINK01 in Deep Work' });
  await expect(deepReader).toBeVisible();
  await deepReader.getByRole('tab', { name: 'Code Review' }).click();
  // The second reader loads its own ticket's commits from its own project, not the workspace selection.
  await expect(deepReader.getByText('deep: commit in third-project')).toBeVisible();
  expect(codeReviewRequests).toContainEqual({ projectId: 'third-project', ticketId: 'git-deep:deep', method: 'GET' });
  await page.screenshot({ path: 'target/visual-captures/hs2-9q4afb-stacked-code-review-wide.png' });
  await deepReader.getByRole('button', { name: /^Open Commit .* in Glassbox$/ }).click();
  await expect
    .poll(() => codeReviewRequests.filter((item) => item.method === 'POST'))
    .toEqual([{ projectId: 'third-project', ticketId: 'git-deep:deep', method: 'POST' }]);
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(deepReader.getByText('deep: commit in third-project')).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hs2-9q4afb-stacked-code-review-narrow.png' });
});

test('keeps every key typed into a linked reader through an immediate close (HS2-WV0VPE)', async ({ page }) => {
  const mutations: Array<{ projectId: string; ticketId: string; patch: Record<string, unknown> }> = [];
  await mockLayeredProjects(page, mutations);
  await page.setViewportSize({ width: 1200, height: 900 });
  await openProjects(page);
  await page.locator('#app-right-rail').getByRole('link', { name: '@target-project/HS2-LINK01' }).click();
  const reader = page.getByRole('dialog', { name: 'Read and edit HS2-LINK01 in Hot Sheet 2' });
  await reader.locator('[data-action="edit-markdown"]').click({ position: { x: 4, y: 4 } });
  const details = reader.getByRole('textbox', { name: 'Ticket details' });
  await details.fill('');
  // Keystrokes reach the frame at typing pauses; the textarea keeps the live text meanwhile.
  await details.pressSequentially('Typed key by key', { delay: 15 });
  await expect(details).toHaveValue('Typed key by key');
  // Closing within the typing pause still lands (and saves) the last keys.
  await reader.getByRole('button', { name: 'Close Ticket Reader' }).click();
  await expect(reader).toHaveCount(0);
  await expect
    .poll(() => mutations.map((item) => [item.projectId, item.ticketId, item.patch.details]))
    .toEqual([['target-project', 'git-target:target', 'Typed key by key']]);
});

test('edits a link-opened reader title and up-next like any other reader (HS2-KRVWKF)', async ({ page }) => {
  const mutations: Array<{ projectId: string; ticketId: string; patch: Record<string, unknown> }> = [];
  await mockLayeredProjects(page, mutations);
  await page.setViewportSize({ width: 1200, height: 900 });
  await openProjects(page);
  await page.locator('#app-right-rail').getByRole('link', { name: '@target-project/HS2-LINK01' }).click();
  const reader = page.getByRole('dialog', { name: 'Read and edit HS2-LINK01 in Hot Sheet 2' });
  await expect(reader).toBeVisible();
  const heading = reader.locator('[data-action="edit-ticket-title"]');
  await expect(heading).toHaveAttribute('data-editable', 'true');
  await heading.dblclick();
  const title = reader.getByRole('textbox', { name: 'Ticket Title' });
  await expect(title).toHaveValue('First linked reader');
  await title.fill('Renamed from a link');
  // Typing alone never writes; the single save happens when focus leaves the title.
  await page.waitForTimeout(300);
  expect(mutations).toEqual([]);
  await title.blur();
  await expect
    .poll(() => mutations.map((item) => [item.projectId, item.ticketId, item.patch.title]))
    .toEqual([['target-project', 'git-target:target', 'Renamed from a link']]);
  await expect(heading).toContainText('Renamed from a link');
  await page.screenshot({ path: 'target/visual-captures/hs2-krvwkf-linked-title-wide.png', fullPage: true });
  await reader.locator('[data-action="toggle-inspector-up-next"]').click();
  await expect.poll(() => mutations.at(-1)?.patch).toMatchObject({ up_next: true });
  await page.setViewportSize({ width: 720, height: 760 });
  await expect(reader).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hs2-krvwkf-linked-title-narrow.png', fullPage: true });
  await page.setViewportSize({ width: 1200, height: 900 });
  await reader.getByRole('button', { name: 'Close Ticket Reader' }).click();
  await expect(reader).toHaveCount(0);
  expect(mutations.every((item) => item.projectId === 'target-project')).toBe(true);
  // The workspace selection was never edited and shows no leaked title editor.
  const inspector = page.locator('#app-right-rail');
  await expect(inspector.locator('[data-component="ticket-inspector-header"]')).toHaveAttribute(
    'data-ticket-slug',
    'KF-ROOT01',
  );
  await expect(inspector.getByRole('textbox', { name: 'Ticket Title' })).toHaveCount(0);
});

test('keeps linked reader attachment batches read-only even with an editable provider (HS2-QZFZA8)', async ({
  page,
}) => {
  const attachmentWrites: string[] = [];
  page.on('request', (request) => {
    if (request.url().includes('/attachments') && request.method() !== 'GET') attachmentWrites.push(request.url());
  });
  await mockLayeredProjects(page, [], false, {
    source: [
      {
        id: 'SOURCE',
        filename: 'source.png',
        created_at: '2026-09-11T00:02:00Z',
        batch_id: 'source-batch',
        batch_label: 'Workspace evidence',
        purpose: 'reference',
      },
    ],
    target: [
      {
        id: 'LINKED',
        filename: 'linked.png',
        created_at: '2026-09-11T00:02:00Z',
        batch_id: 'linked-batch',
        batch_label: 'Linked evidence',
        purpose: 'problem_evidence',
      },
    ],
  });
  await page.setViewportSize({ width: 1200, height: 900 });
  await openProjects(page);
  await page.locator('[data-component="ticket-list-row"][data-ticket-slug="KF-ROOT01"]').dblclick();
  const workspaceReader = page.getByRole('dialog', { name: 'Read and edit KF-ROOT01 in Kerf' });
  await workspaceReader.getByRole('link', { name: '@target-project/HS2-LINK01' }).click();
  const linked = page.getByRole('dialog', { name: 'Read and edit HS2-LINK01 in Hot Sheet 2' });
  await linked.getByRole('tab', { name: /Attachments/ }).click();
  const batch = linked.locator('[data-attachment-batch="linked-batch"]');
  await expect(linked.getByText('Attachments are view-only in linked ticket readers.')).toBeVisible();
  await expect(linked.getByText('This provider does not support attachment actions.')).toHaveCount(0);
  await expect(batch.getByRole('heading', { name: 'Linked evidence' })).toBeVisible();
  await expect(batch.locator('[name="attachment-batch-label"]')).toBeDisabled();
  await expect(batch.locator('[name="attachment-batch-purpose"]')).toBeDisabled();
  await expect(batch.locator('[name="attachment-batch-purpose"]')).toHaveValue('problem_evidence');
  await expect(linked.locator('[data-action="edit-attachment-batch-label"]')).toHaveCount(0);
  await expect(
    linked.locator(
      '[data-attachment-group-drop-target], [data-attachment-new-group-drop-target], [data-drag-attachment-id]',
    ),
  ).toHaveCount(0);
  await batch.getByRole('heading', { name: 'Linked evidence' }).dblclick();
  await expect(batch.locator('[name="attachment-batch-label"]')).not.toBeVisible();
  await page.screenshot({ path: test.info().outputPath('linked-batch-readonly-wide.png') });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(batch.getByRole('heading', { name: 'Linked evidence' })).toBeVisible();
  await page.screenshot({ path: test.info().outputPath('linked-batch-readonly-phone.png') });
  await linked.getByRole('button', { name: 'Close Ticket Reader' }).click();
  await workspaceReader.getByRole('tab', { name: /Attachments/ }).click();
  await expect(workspaceReader.getByRole('button', { name: 'Edit batch label Workspace evidence' })).toBeVisible();
  await expect(workspaceReader.locator('[name="attachment-batch-purpose"]')).toBeEnabled();
  await expect(workspaceReader.locator('[name="attachment-batch-purpose"]')).toHaveValue('reference');
  await workspaceReader.getByRole('button', { name: 'Close Ticket Reader' }).click();
  expect(attachmentWrites).toEqual([]);
  expect(
    await page.evaluate(() => Object.keys(localStorage).filter((key) => key.includes('attachment_label'))),
  ).toEqual([]);
});

test('opens media from a stacked linked reader in the gallery for that reader ticket (HS2-97E0QR)', async ({
  page,
}) => {
  await mockLayeredProjects(page, [], false, {
    target: [{ id: 'LINKED-IMG', filename: 'linked-proof.png', created_at: '2026-09-11T00:02:00Z' }],
  });
  await openProjects(page);
  await page.locator('[data-component="ticket-list-row"][data-ticket-slug="KF-ROOT01"]').dblclick();
  const baseReader = page.getByRole('dialog', { name: 'Read and edit KF-ROOT01 in Kerf' });
  await baseReader.getByRole('link', { name: '@target-project/HS2-LINK01' }).click();
  const linked = page.getByRole('dialog', { name: 'Read and edit HS2-LINK01 in Hot Sheet 2' });
  await expect(linked.getByText('Reader 2 of 2')).toBeVisible();
  await linked.getByRole('tab', { name: /Attachments/ }).click();
  const opener = linked.locator('[data-action="open-attachment-gallery"]').first();
  await expect(opener).toBeVisible();
  await opener.click();
  // The gallery shows the linked reader's own media (from its project), above both readers.
  const gallery = page.locator('[data-component="attachment-gallery"]');
  await expect(gallery).toBeVisible();
  await expect(gallery).toContainText('linked-proof.png');
  await expect(gallery.locator('img').first()).toHaveAttribute(
    'src',
    // Same checkout-scoped URL the linked reader itself uses: its owning checkout and ticket.
    /\/checkouts\/target-project\/tickets\/git-target%3Atarget\/attachments\/LINKED-IMG$/,
  );
  const galleryOnTop = await gallery.evaluate((node) => {
    const box = node.getBoundingClientRect(),
      hit = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
    return Boolean(hit && node.contains(hit));
  });
  expect(galleryOnTop).toBe(true);
  // The linked reader is view-only for attachments, so its gallery offers no markup either.
  await expect(gallery.getByRole('button', { name: 'Annotate media' })).toBeDisabled();
  await page.screenshot({ path: test.info().outputPath('hs2-97e0qr-linked-reader-gallery.png') });
  // Closing the gallery returns to the linked reader, still stacked above the base reader.
  await page.keyboard.press('Escape');
  await expect(gallery).toHaveCount(0);
  await expect(linked).toBeVisible();
  await expect(linked.getByText('Reader 2 of 2')).toBeVisible();
});

test('merges a concurrent remote edit into a linked reader draft instead of discarding it (HS2-A4XCXE)', async ({
  page,
}) => {
  await mockLayeredProjects(page);
  let details = 'First line\nSecond line',
    token = 'token-target';
  const patches: Array<Record<string, unknown>> = [];
  const ticketJson = () => ({
    store: target.connection_id,
    ...target,
    details,
    blocked_reason: null,
    concurrency_token: token,
    notes: [],
    attachments: [],
  });
  await page.route('**/project-api/target-project/**/tickets/git-target%3Atarget', async (route) => {
    const request = route.request();
    if (request.method() === 'GET') return route.fulfill({ json: ticketJson() });
    if (request.method() !== 'PATCH') return route.fallback();
    const patch = request.postDataJSON() as Record<string, unknown>;
    patches.push(patch);
    if (patch.expected_token !== token)
      return route.fulfill({ status: 409, json: { error: 'ticket changed since it was read' } });
    if (typeof patch.details === 'string') details = patch.details;
    token = `committed-${patches.length}`;
    return route.fulfill({ json: ticketJson() });
  });
  await page.setViewportSize({ width: 1200, height: 900 });
  await openProjects(page);
  await page.locator('#app-right-rail').getByRole('link', { name: '@target-project/HS2-LINK01' }).click();
  const reader = page.getByRole('dialog', { name: 'Read and edit HS2-LINK01 in Hot Sheet 2' });
  await reader.locator('[data-action="edit-markdown"]').click({ position: { x: 4, y: 4 } });
  const editor = reader.getByRole('textbox', { name: 'Ticket details' });
  await expect(editor).toHaveValue('First line\nSecond line');
  // Another writer appends a line the reader has not seen.
  details = 'First line\nSecond line\nAppended elsewhere';
  token = 'remote-append';
  await editor.fill('First line, edited in the reader\nSecond line');
  // Text edits save when focus leaves the editor, never while typing (HS2-RE1PS6).
  await editor.blur();
  await expect.poll(() => details).toBe('First line, edited in the reader\nSecond line\nAppended elsewhere');
  // Leaving the editor closes it; the rendered details show the merged text.
  const rendered = reader.locator('[data-action="edit-markdown"]');
  await expect(rendered).toContainText('First line, edited in the reader');
  await expect(rendered).toContainText('Appended elsewhere');
  await expect(page.locator('.app-error')).toHaveCount(0);
  await rendered.click({ position: { x: 4, y: 4 } });
  await expect(editor).toHaveValue('First line, edited in the reader\nSecond line\nAppended elsewhere');
  // A genuine overlap keeps the draft and says so instead of silently replacing it.
  details = 'First line, rewritten elsewhere\nSecond line\nAppended elsewhere';
  token = 'remote-overlap';
  await editor.fill('First line, rewritten here\nSecond line\nAppended elsewhere');
  await editor.blur();
  await expect(page.locator('.app-error')).toContainText('changed remotely in the same place you edited');
  await expect(editor).toHaveValue('First line, rewritten here\nSecond line\nAppended elsewhere');
});

for (const action of ['Respond', 'No Response Needed'] as const) {
  test(`answers a linked reader frame feedback prompt with ${action} on the frame ticket (HS2-04XTV5)`, async ({
    page,
  }) => {
    const mutations: Array<{ projectId: string; ticketId: string; patch: Record<string, unknown> }> = [];
    const prompt = {
      id: 'FB1',
      kind: 'feedback_needed',
      created_at: '2026-09-11T00:02:00Z',
      edited_at: '2026-09-11T00:02:00Z',
      text: 'Should the linked frame keep its draft?',
    };
    await mockLayeredProjects(page, mutations, false, {}, { target: [prompt] });
    await page.setViewportSize({ width: 1200, height: 900 });
    await openProjects(page);
    await page.locator('#app-right-rail').getByRole('link', { name: '@target-project/HS2-LINK01' }).click();
    const reader = page.getByRole('dialog', { name: 'Read and edit HS2-LINK01 in Hot Sheet 2' }),
      card = reader.locator('article[data-note-id="FB1"]'),
      response = card.getByRole('textbox', { name: 'Feedback response' });
    await expect(response).toBeVisible();
    await response.fill('Keep it, please.');
    // A frame reply stays a local draft until an explicit action (HS2-MB2Q10).
    await response.blur();
    await page.waitForTimeout(300);
    expect(mutations).toEqual([]);
    await card.getByRole('button', { name: action, exact: true }).click();
    await expect
      .poll(() => mutations.map((item) => [item.projectId, item.ticketId, item.patch.note, item.patch.note_kind]))
      .toEqual([
        [
          'target-project',
          'git-target:target',
          action === 'Respond' ? 'Keep it, please.' : 'No response needed',
          'regular',
        ],
      ]);
    expect(mutations.some((item) => item.patch.note_id === 'FB1')).toBe(false);
    // The workspace selection is untouched by a frame answer.
    await expect(page.locator('#app-right-rail [data-component="ticket-inspector-header"]')).toHaveAttribute(
      'data-ticket-slug',
      'KF-ROOT01',
    );
  });
}

/** A stateful target-ticket server: note adds, deletes, and started phases persist (HS2-ANRXAE). */
async function statefulTargetTicket(page: Page, initialNotes: Array<Record<string, unknown>> = []) {
  const state = {
    notes: [...initialNotes],
    status: target.status,
    started_phase: null as string | null,
    token: 1,
  };
  const requests: Array<{ method: string; path: string; body?: Record<string, unknown> }> = [];
  const ticketJson = () => ({
    store: target.connection_id,
    ...target,
    status: state.status,
    started_phase: state.started_phase,
    blocked_reason: null,
    concurrency_token: `token-target-${state.token}`,
    notes: state.notes,
    attachments: [],
  });
  await page.route('**/project-api/target-project/**/tickets/git-target%3Atarget**', async (route) => {
    const request = route.request(),
      path = decodeURIComponent(new URL(request.url()).pathname),
      method = request.method();
    if (path.endsWith('/code-review') || path.includes('/attachments')) return route.fallback();
    const noteId = path.match(/\/notes\/([^/]+)$/)?.[1];
    if (noteId && method === 'DELETE') {
      requests.push({ method, path });
      state.notes = state.notes.filter((note) => note.id !== noteId);
      state.token += 1;
      return route.fulfill({ json: ticketJson() });
    }
    if (method === 'GET') return route.fulfill({ json: ticketJson() });
    if (method !== 'PATCH') return route.fallback();
    const body = request.postDataJSON() as Record<string, unknown>;
    requests.push({ method, path, body });
    if (typeof body.note === 'string' && typeof body.note_id !== 'string')
      state.notes = [
        ...state.notes,
        {
          id: `ADDED${state.notes.length + 1}`,
          kind: body.note_kind ?? 'regular',
          created_at: '2026-09-11T00:03:00Z',
          edited_at: '2026-09-11T00:03:00Z',
          text: body.note,
        },
      ];
    if (typeof body.status === 'string') state.status = body.status;
    if (Object.hasOwn(body, 'started_phase')) state.started_phase = (body.started_phase as string | null) ?? null;
    state.token += 1;
    return route.fulfill({ json: ticketJson() });
  });
  return requests;
}

test('adds and deletes notes and records the started phase in a link-opened reader (HS2-ANRXAE)', async ({ page }) => {
  const workspaceWrites: string[] = [];
  page.on('request', (request) => {
    if (request.method() !== 'GET' && request.url().includes('/project-api/source-project/'))
      workspaceWrites.push(`${request.method()} ${request.url()}`);
  });
  await mockLayeredProjects(page);
  const requests = await statefulTargetTicket(page);
  await page.setViewportSize({ width: 1280, height: 900 });
  await openProjects(page);
  await page.locator('#app-right-rail').getByRole('link', { name: '@target-project/HS2-LINK01' }).click();
  const reader = page.getByRole('dialog', { name: 'Read and edit HS2-LINK01 in Hot Sheet 2' });
  await expect(reader).toBeVisible();
  // The frame owns its composer: opening it never opens the workspace inspector's composer.
  await reader.locator('[data-action="add-ticket-note"]').first().click();
  const composer = reader.getByRole('textbox', { name: 'New note' });
  await expect(composer).toBeFocused();
  await expect(page.locator('#app-right-rail [name="new-note-body"]')).toHaveCount(0);
  await composer.pressSequentially('Noted from a linked reader', { delay: 10 });
  // A new note is a submission, never an autosave: typing alone writes nothing.
  await page.waitForTimeout(300);
  expect(requests).toEqual([]);
  await page.screenshot({ path: 'target/visual-captures/hs2-anrxae-linked-composer-wide.png' });
  await reader.getByRole('button', { name: 'Add Note' }).click();
  await expect
    .poll(() => requests.map((item) => [item.method, item.body?.note, item.body?.note_kind]))
    .toEqual([['PATCH', 'Noted from a linked reader', 'regular']]);
  const added = reader.locator('article[data-note-id="ADDED1"]');
  await expect(added).toContainText('Noted from a linked reader');
  await expect(reader.locator('[name="new-note-body"]')).toHaveCount(0);
  // Delete the note through the frame's own checkout and qualified identity.
  await added.hover();
  await added.getByRole('button', { name: 'Delete Note' }).click();
  await expect(added).toHaveCount(0);
  expect(requests.at(-1)).toMatchObject({
    method: 'DELETE',
    path: expect.stringMatching(/checkouts\/target-project\/tickets\/git-target:target\/notes\/ADDED1$/),
  });
  // Record a started phase from the reader's status menu, routed to the owning project.
  const status = reader.locator('.ticket-status-menu');
  await status.locator('[data-action="open-inspector-status-menu"]').click();
  await status.locator('[data-ticket-status="started"]').hover();
  const working = status.locator('[data-started-phase="working"]');
  await expect(working).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hs2-anrxae-linked-phase-menu-wide.png' });
  await working.click();
  await expect.poll(() => requests.at(-1)?.body).toMatchObject({ started_phase: 'working' });
  await expect(status.locator('[data-component="status-badge"]')).toHaveText('Working');
  await page.setViewportSize({ width: 390, height: 844 });
  await reader.locator('[data-action="add-ticket-note"]').first().click();
  await expect(reader.getByRole('textbox', { name: 'New note' })).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hs2-anrxae-linked-composer-narrow.png' });
  // Cancelling drops only this frame's draft.
  await reader.getByRole('button', { name: 'Cancel' }).click();
  await expect(reader.locator('[name="new-note-body"]')).toHaveCount(0);
  await page.setViewportSize({ width: 1280, height: 900 });
  await reader.getByRole('button', { name: 'Close Ticket Reader' }).click();
  await expect(reader).toHaveCount(0);
  expect(requests.every((item) => item.path.includes('/checkouts/target-project/'))).toBe(true);
  expect(workspaceWrites).toEqual([]);
  await expect(page.locator('#app-right-rail [data-component="ticket-inspector-header"]')).toHaveAttribute(
    'data-ticket-slug',
    'KF-ROOT01',
  );
});

test('records the started phase from the workspace reader through the selected ticket (HS2-ANRXAE)', async ({
  page,
}) => {
  const patches: Array<Record<string, unknown>> = [];
  await mockLayeredProjects(page);
  await page.route('**/project-api/source-project/**/tickets/git-source%3Asource', async (route) => {
    if (route.request().method() !== 'PATCH') return route.fallback();
    const patch = route.request().postDataJSON() as Record<string, unknown>;
    patches.push(patch);
    return route.fulfill({
      json: { store: source.connection_id, ...source, ...patch, blocked_reason: null, notes: [], attachments: [] },
    });
  });
  await openProjects(page);
  await page.locator('[data-component="ticket-list-row"][data-ticket-slug="KF-ROOT01"]').dblclick();
  const reader = page.getByRole('dialog', { name: 'Read and edit KF-ROOT01 in Kerf' }),
    status = reader.locator('.ticket-status-menu');
  await status.locator('[data-action="open-inspector-status-menu"]').click();
  await status.locator('[data-ticket-status="started"]').hover();
  await status.locator('[data-started-phase="planning"]').click();
  await expect.poll(() => patches.at(-1)).toMatchObject({ started_phase: 'planning' });
  await expect(status.locator('[data-component="status-badge"]')).toHaveText('Planning');
});

test('keeps inline feedback replies and choices per linked reader frame (HS2-ANRXAE)', async ({ page }) => {
  const prompt = {
      id: 'FB1',
      kind: 'feedback_needed',
      created_at: '2026-09-11T00:02:00Z',
      edited_at: '2026-09-11T00:02:00Z',
      text: 'FEEDBACK NEEDED\n\nTwo questions\n\n1. Something\n2. Another thing',
    },
    choices = {
      id: 'FB2',
      kind: 'feedback_needed',
      created_at: '2026-09-11T00:02:30Z',
      edited_at: '2026-09-11T00:02:30Z',
      text: 'Which direction?\n\nCHOICE:\n- Keep it\n- Change it',
    };
  // The workspace ticket shows prompts with the same note ids, so shared state would leak between readers.
  await mockLayeredProjects(page, [], false, {}, { source: [prompt, choices] });
  const requests = await statefulTargetTicket(page, [prompt, choices]);
  await page.setViewportSize({ width: 1280, height: 900 });
  await openProjects(page);
  await page.locator('[data-component="ticket-list-row"][data-ticket-slug="KF-ROOT01"]').dblclick();
  const workspaceReader = page.getByRole('dialog', { name: 'Read and edit KF-ROOT01 in Kerf' });
  const clickAfter = async (scope: typeof workspaceReader, text: string) => {
    const block = scope.locator('article[data-note-id="FB1"] .note-card__feedback-block').filter({ hasText: text });
    await block.scrollIntoViewIfNeeded();
    const point = await block.evaluate((element, needle) => {
      const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
      let node: Node | null;
      while ((node = walker.nextNode())) {
        const index = node.textContent?.indexOf(needle) ?? -1;
        if (index < 0) continue;
        const range = document.createRange();
        range.setStart(node, index + needle.length - 1);
        range.setEnd(node, index + needle.length);
        const rect = range.getBoundingClientRect();
        return { x: rect.right - 0.5, y: rect.y + rect.height / 2 };
      }
      throw new Error(`Text not found: ${needle}`);
    }, text);
    await page.mouse.click(point.x, point.y);
  };
  const choiceItems = (scope: typeof workspaceReader) =>
    scope
      .locator('article[data-note-id="FB2"]')
      .getByRole('grid', { name: 'Feedback Choices' })
      .locator('[data-component="content-item"]');
  await clickAfter(workspaceReader, 'Something');
  await workspaceReader.getByRole('textbox', { name: /Response at character/ }).fill('Workspace inline reply');
  await choiceItems(workspaceReader).nth(0).click();
  await expect(choiceItems(workspaceReader).nth(0)).toHaveAttribute('aria-selected', 'true');
  await workspaceReader.getByRole('link', { name: '@target-project/HS2-LINK01' }).click();
  const linked = page.getByRole('dialog', { name: 'Read and edit HS2-LINK01 in Hot Sheet 2' });
  await expect(linked.getByText('Reader 2 of 2')).toBeVisible();
  // The linked frame starts with none of the workspace reader's reply state for the same note id.
  await expect(linked.getByRole('textbox', { name: /Response at character/ })).toHaveCount(0);
  await clickAfter(linked, 'Another thing');
  const inline = linked.getByRole('textbox', { name: /Response at character/ });
  await expect(inline).toBeFocused();
  await inline.pressSequentially('Linked inline reply', { delay: 10 });
  await expect(choiceItems(linked).nth(0)).toHaveAttribute('aria-selected', 'false');
  await choiceItems(linked).nth(1).click();
  await expect(choiceItems(linked).nth(1)).toHaveAttribute('aria-selected', 'true');
  await page.screenshot({ path: 'target/visual-captures/hs2-anrxae-linked-feedback-wide.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(inline).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hs2-anrxae-linked-feedback-narrow.png' });
  await page.setViewportSize({ width: 1280, height: 900 });
  // Respond while the last keys may still be coalescing: the frame's own inline reply is answered.
  await linked
    .locator('article[data-note-id="FB1"]')
    .getByRole('button', { name: 'Respond', exact: true })
    .evaluate((button) => {
      (button as HTMLElement).click();
    });
  await expect
    .poll(() => requests.find((item) => item.body?.note_kind === 'regular')?.body?.note)
    .toBe('> FEEDBACK NEEDED\n>\n> Two questions\n>\n> 1. Something\n> 2. Another thing\n\nLinked inline reply');
  await linked.getByRole('button', { name: 'Close Ticket Reader' }).click();
  await expect(linked).toHaveCount(0);
  // The workspace reader's own inline draft survived the linked frame's answer.
  await expect(workspaceReader.getByRole('textbox', { name: /Response at character/ })).toHaveValue(
    'Workspace inline reply',
  );
  await expect(choiceItems(workspaceReader).nth(0)).toHaveAttribute('aria-selected', 'true');
  await expect(choiceItems(workspaceReader).nth(1)).toHaveAttribute('aria-selected', 'false');
});
