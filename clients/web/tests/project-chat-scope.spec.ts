import { expect, test } from '@playwright/test';

import { ticketsJson } from './ticket-page-fixture';

/**
 * AI chat tabs belong to one project (HS2-6GHSCY, HS2-5AGPWE). The machine server's
 * `/connections` lists every connection on the machine, so a project must keep only the
 * connections whose `project` is its own root (or one of its stores) and never show
 * another checkout's chats in its drawer or its close dialog.
 */
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
const project = (id: string, root: string) => ({
  id,
  root,
  name: root.split('/').at(-1),
  stores: [`${root}.hs2`],
  apiPath: `/__hotsheet/project-api/${id}`,
});
const demo = project('demo-checkout', '/work/demo'),
  other = project('other-checkout', '/work/other');
/** Every connection the machine server knows, regardless of which project asks. */
const machineConnections = [
  {
    id: 'hotsheet-drawer-chat-other',
    tool: 'codex',
    project: other.root,
    role: 'main',
    busy: true,
    actions: ['send_turn', 'interrupt'],
    session_id: 'other-thread',
    model: 'gpt-6-astra',
    effort: 'medium',
  },
  {
    id: 'hotsheet-saved-chat-other',
    tool: 'codex',
    project: other.root,
    role: 'main',
    busy: false,
    actions: ['send_turn'],
    session_id: 'other-saved',
  },
  {
    id: 'hotsheet-drawer-chat-demo-store',
    tool: 'codex',
    project: demo.stores[0],
    role: 'main',
    busy: false,
    actions: ['send_turn', 'interrupt'],
    session_id: 'demo-thread',
  },
];

async function installFixture(page: import('@playwright/test').Page) {
  await page.route('**/*', (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname;
    if (path === '/__hotsheet/projects/open') {
      const root = request.postDataJSON().root as string;
      return route.fulfill({ status: 201, json: root === other.root ? other : demo });
    }
    if (path === '/__hotsheet/folders/choose') return route.fulfill({ json: { path: other.root } });
    if (path.endsWith('/providers'))
      return route.fulfill({
        json: [
          {
            connection_id: 'git-local',
            provider: 'git',
            display_name: 'Hot Sheet Git',
            locator: '/tickets',
            default: true,
            capabilities,
          },
        ],
      });
    if (path.endsWith('/ai-tools'))
      return route.fulfill({
        json: [
          {
            id: 'codex',
            display_name: 'Codex',
            models: [{ id: 'gpt-6-astra', label: 'GPT-6 Astra', effort_levels: ['medium'] }],
            default_model: 'gpt-6-astra',
            default_effort: 'medium',
            actions: ['change_model', 'change_effort'],
          },
        ],
      });
    if (path.endsWith('/ai-settings'))
      return route.fulfill({ json: { tool: 'codex', model: 'gpt-6-astra', effort: 'medium' } });
    if (path.endsWith('/connections') && request.method() === 'GET') return route.fulfill({ json: machineConnections });
    if (path.endsWith('/drive/sessions') && request.method() === 'GET') return route.fulfill({ json: [] });
    if (path.endsWith('/tickets')) return route.fulfill({ json: ticketsJson(request, []) });
    if (path.endsWith('/terminals') && request.method() === 'GET') return route.fulfill({ json: [] });
    if (
      path.endsWith('/permissions') ||
      path.endsWith('/commands') ||
      path.endsWith('/command-runs') ||
      path.endsWith('/views') ||
      path.endsWith('/corrupt-tickets')
    )
      return route.fulfill({ json: [] });
    if (path.endsWith('/repository/status'))
      return route.fulfill({
        json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true },
      });
    if (path.endsWith('/ws/poll'))
      return route.fulfill({
        json: { cursor: Number(url.searchParams.get('since') ?? 0), events: [], overflow: false },
      });
    return route.continue();
  });
}

test('a project restores only its own AI chats from the machine-wide connection list', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await installFixture(page);
  await page.addInitScript(() => {
    localStorage.setItem('hotsheet.terminals.drawer-open', 'true');
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
  const drawer = page.locator('[data-component="terminal-drawer"]');
  await expect(drawer).toBeVisible();
  const chats = drawer.locator('[data-tab-kind="ai-chat"]');
  // The demo project owns the connection registered under its store path, and nothing else.
  await expect(chats).toHaveCount(1);
  await expect(drawer.getByRole('tab', { name: 'Codex chat' })).toBeVisible();
  await expect(
    drawer.locator('[data-tab-kind="ai-chat"][data-tab-id="ai-chat:hotsheet-drawer-chat-other"]'),
  ).toHaveCount(0);
  await expect(
    drawer.locator('[data-tab-kind="ai-chat"][data-tab-id="ai-chat:hotsheet-saved-chat-other"]'),
  ).toHaveCount(0);

  await page.getByRole('button', { name: 'Add Project' }).click();
  await expect(page.getByRole('tab', { name: 'other' })).toHaveAttribute('aria-selected', 'true');
  await expect(
    drawer.locator('[data-tab-kind="ai-chat"][data-tab-id="ai-chat:hotsheet-drawer-chat-other"]'),
  ).toBeVisible();
  await expect(
    drawer.locator('[data-tab-kind="ai-chat"][data-tab-id="ai-chat:hotsheet-saved-chat-other"]'),
  ).toBeVisible();
  await expect(
    drawer.locator('[data-tab-kind="ai-chat"][data-tab-id="ai-chat:hotsheet-drawer-chat-demo-store"]'),
  ).toHaveCount(0);

  await page.getByRole('tab', { name: 'demo' }).click();
  await expect(chats).toHaveCount(1);
  await expect(
    drawer.locator('[data-tab-kind="ai-chat"][data-tab-id="ai-chat:hotsheet-drawer-chat-other"]'),
  ).toHaveCount(0);

  // The close dialog for the demo project lists only its own chat, never the other project's running one.
  await page.getByRole('tab', { name: 'demo' }).hover();
  await page.getByRole('button', { name: 'Close demo' }).click();
  const dialog = page.locator('[data-component="project-close-dialog"]');
  await expect(dialog).toHaveJSProperty('open', true);
  await expect(dialog).toContainText('1 AI chat');
  await expect(dialog).not.toContainText('2 AI chats');
  await expect(dialog.getByRole('button', { name: /Codex Main/ })).toHaveCount(0);
  await page.screenshot({ path: 'target/visual-captures/hs2-6ghscy-close-dialog-own-chat-wide.png', fullPage: true });
  await dialog.getByRole('button', { name: 'Cancel' }).click();
});
