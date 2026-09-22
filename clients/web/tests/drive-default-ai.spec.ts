import { expect, test } from '@playwright/test';

// Regression for HS2-BECC2T: the sidebar Drive control must reflect the *active*
// project's default AI tool immediately — on boot restore and on tab switch —
// without the user first opening the AI settings screen. The bug kept the Drive
// label on the placeholder "Codex" (the first-loaded project's defaults) because
// AI configuration was gated behind a single global "loaded" flag and only
// refreshed for the settings screen.

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

const aiToolCatalog = [
  {
    id: 'codex',
    display_name: 'Codex',
    models: [{ id: 'gpt-6-astra', label: 'GPT-6 Astra', effort_levels: ['low', 'medium', 'high'] }],
    default_model: 'gpt-6-astra',
    default_effort: 'medium',
    actions: ['change_model', 'change_effort'],
  },
  {
    id: 'claude',
    display_name: 'Claude',
    models: [{ id: 'sonnet', label: 'Sonnet', effort_levels: ['low', 'medium', 'high'] }],
    default_model: 'sonnet',
    default_effort: 'medium',
    actions: ['change_model', 'change_effort'],
  },
];

// Two open projects with *different* default AI tools. Alpha loads first (its
// codex default is what the buggy global flag would leave stuck on the label);
// Bravo is the active/restored project and defaults to Claude.
const projects = {
  '/work/alpha': {
    id: 'proj-alpha',
    root: '/work/alpha',
    name: 'Alpha',
    stores: ['/work/alpha.hs2'],
    apiPath: '/__hotsheet/project-api/proj-alpha',
    needsTicketSetup: false,
    needsHs1Migration: false,
    defaultAi: { tool: 'codex', model: 'gpt-6-astra', effort: 'medium' },
  },
  '/work/bravo': {
    id: 'proj-bravo',
    root: '/work/bravo',
    name: 'Bravo',
    stores: ['/work/bravo.hs2'],
    apiPath: '/__hotsheet/project-api/proj-bravo',
    needsTicketSetup: false,
    needsHs1Migration: false,
    defaultAi: { tool: 'claude', model: 'sonnet', effort: 'medium' },
  },
} as const;
type MockProject = (typeof projects)[keyof typeof projects];
const byId = new Map<string, MockProject>(Object.values(projects).map((item) => [item.id, item]));

const emptyCounts = {
  total: 0,
  queued: 0,
  backlog: 0,
  archive: 0,
  trash: 0,
  open: 0,
  up_next: 0,
  active: 0,
  started: 0,
  completed_today: 0,
};

async function mockTwoProjects(page: import('@playwright/test').Page) {
  await page.addInitScript(() => {
    localStorage.setItem('hotsheet.open-projects', JSON.stringify(['/work/alpha', '/work/bravo']));
    localStorage.setItem('hotsheet.workspace.active-project-root.v1', '/work/bravo');
  });
  await page.route('**/*', async (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname;
    if (path === '/__hotsheet/projects/open' && request.method() === 'POST') {
      const root = (request.postDataJSON() as { root: string }).root,
        { defaultAi, ...project } = (projects as Record<string, MockProject>)[root] ?? projects['/work/alpha'];
      void defaultAi;
      return route.fulfill({ status: 201, json: project });
    }
    const api = path.match(/\/__hotsheet\/project-api\/([^/]+)(\/.*)$/);
    if (api) {
      const project = byId.get(api[1]),
        rest = api[2];
      if (project) {
        if (rest.endsWith('/providers'))
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
        if (rest.endsWith('/provider-connections')) return route.fulfill({ json: [] });
        if (rest.endsWith('/ai-tools')) return route.fulfill({ json: aiToolCatalog });
        if (rest.endsWith('/ai-settings')) return route.fulfill({ json: project.defaultAi });
        if (rest.endsWith('/permissions')) return route.fulfill({ json: [] });
        if (rest.endsWith('/connections')) return route.fulfill({ json: [] });
        if (rest.endsWith('/commands')) return route.fulfill({ json: [] });
        if (rest.endsWith('/command-runs')) return route.fulfill({ json: [] });
        if (rest.endsWith('/views')) return route.fulfill({ json: [] });
        if (rest.endsWith('/corrupt-tickets')) return route.fulfill({ json: [] });
        if (rest.endsWith('/repository/status'))
          return route.fulfill({
            json: {
              branch: 'main',
              ahead: 0,
              behind: 0,
              staged: 0,
              unstaged: 0,
              untracked: 0,
              conflicted: 0,
              clean: true,
            },
          });
        if (rest.includes('/tickets') && request.method() === 'GET') {
          const pageSize = url.searchParams.get('page_size');
          return route.fulfill({ json: pageSize ? { items: [], counts: emptyCounts } : [] });
        }
        if (rest.endsWith('/ws/poll') && request.method() === 'GET') {
          const since = url.searchParams.get('since');
          if (since === null) return route.fulfill({ json: { cursor: 0, events: [], overflow: false } });
          await new Promise((resolve) => setTimeout(resolve, 1_000));
          return route.fulfill({ json: { cursor: Number(since), events: [], overflow: false } });
        }
        // Any other project-scoped GET during boot: an empty, well-formed default.
        if (request.method() === 'GET') return route.fulfill({ json: [] });
        return route.fulfill({ status: 204 });
      }
    }
    if (path.startsWith('/__hotsheet/')) return route.fulfill({ json: [] });
    return route.continue();
  });
}

test('the Drive control reflects each project default AI on boot restore and tab switch', async ({ page }) => {
  await mockTwoProjects(page);
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');
  const bravoTab = page.getByRole('tab', { name: /Bravo/ }),
    alphaTab = page.getByRole('tab', { name: /Alpha/ });
  await expect(bravoTab).toBeVisible();
  await expect(alphaTab).toBeVisible();
  const drive = page.locator('.project-sidebar [data-component="drive-control"] [data-action="toggle-drive"]');
  // Boot restore: active project is Bravo (Claude) — never visited settings.
  await expect(drive).toHaveAccessibleName('Drive with Claude');
  if (process.env.HS2_BECC2T_SHOT)
    await page
      .locator('.project-sidebar [data-component="drive-control"]')
      .screenshot({ path: process.env.HS2_BECC2T_SHOT });
  // Tab switch to Alpha must re-resolve to that project's default (Codex).
  await alphaTab.click();
  await expect(alphaTab).toHaveAttribute('aria-selected', 'true');
  await expect(drive).toHaveAccessibleName('Drive with Codex');
  // Switch back to Bravo: Claude again, still without any settings visit.
  await bravoTab.click();
  await expect(bravoTab).toHaveAttribute('aria-selected', 'true');
  await expect(drive).toHaveAccessibleName('Drive with Claude');
});
