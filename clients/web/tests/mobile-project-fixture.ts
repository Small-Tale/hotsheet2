/**
 * A mocked single-project phone/desktop app fixture shared by the mobile layout and touch field
 * audits: provider capabilities, one checkout, `count` started tickets, and optional terminals.
 */
export const capabilities = {
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
export const project = (id: string, root: string) => ({
  id,
  root,
  name: root.split('/').at(-1),
  stores: [`${root}.hs2`],
  apiPath: `/__hotsheet/project-api/${id}`,
});
export const ticket = (slug: string, status: string, title = slug) => ({
  connection_id: 'git-local',
  native_id: slug,
  qualified_id: `git-local:${slug}`,
  id: slug,
  slug,
  title,
  status,
  up_next: false,
  feedback_needed: false,
  tags: [],
  blocked_by: [],
  claim_count: 0,
});

const CLEAN_REPOSITORY = {
  branch: 'main',
  ahead: 0,
  behind: 0,
  staged: 0,
  unstaged: 0,
  untracked: 0,
  conflicted: 0,
  clean: true,
};

export const DEFAULT_TERMINALS = [{ id: 'codex-main', alive: true, busy: false, cwd: '/work/demo' }];

export async function openDemoProject(
  page: import('@playwright/test').Page,
  withTerminal = false,
  ticketCount = 1,
  terminals: Array<{ id: string; alive: boolean; busy: boolean; cwd: string }> = DEFAULT_TERMINALS,
  firstTicketTitle?: string,
  /** Saved commands and custom views (HS2-NYGR2Q), shaped like `GET /commands` and `GET /views`. */
  populated: {
    commands?: unknown[];
    views?: unknown[];
    /** Installed AI tools and saved defaults (HS2-ME1XBC), shaped like `GET /ai-tools` and `GET /ai-settings`. */
    aiTools?: unknown[];
    aiSettings?: unknown;
    /** Start in a folder without a repository, so the setup and remote steps render (HS2-ME1XBC). */
    uninitializedRepository?: boolean;
  } = {},
) {
  let repositoryInitialized = false;
  await page.route('**/*', async (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname;
    if (path === '/__hotsheet/projects/open')
      return route.fulfill({ status: 201, json: project('demo-checkout', request.postDataJSON().root as string) });
    if (path === '/__hotsheet/folders/choose') return route.fulfill({ json: { path: '/work/demo' } });
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
    // Flattened ticket + store, matching GET /checkouts/{ref}/tickets/{id} (the client wraps it itself).
    if (path.endsWith('/checkouts/demo-checkout/tickets/git-local%3AHS2-M1'))
      return route.fulfill({
        json: {
          store: 'git-local',
          ...ticket('HS2-M1', 'started', firstTicketTitle),
          category: 'bug',
          priority: 'default',
          details: '',
          blocked_reason: null,
          notes: [],
          attachments: [],
          created_at: '2026-09-01T00:00:00Z',
          updated_at: '2026-09-14T00:00:00Z',
          concurrency_token: 't1',
        },
      });
    if (path.endsWith('/tickets'))
      return route.fulfill({
        json: {
          items: Array.from({ length: ticketCount }, (_, index) =>
            ticket(`HS2-M${index + 1}`, 'started', index === 0 ? firstTicketTitle : undefined),
          ),
          counts: {
            total: ticketCount,
            queued: ticketCount,
            backlog: 0,
            archive: 0,
            open: ticketCount,
            up_next: 0,
            active: 0,
            started: ticketCount,
            completed_today: 0,
            completion_trend: [0, 0, 0, 0, 0, 0, 0],
          },
        },
      });
    if (path.endsWith('/repository/init') && request.method() === 'POST') {
      repositoryInitialized = true;
      return route.fulfill({ json: { ...CLEAN_REPOSITORY, initialized: true } });
    }
    if (path.endsWith('/repository/status'))
      return route.fulfill({
        json: populated.uninitializedRepository && !repositoryInitialized ? { initialized: false } : CLEAN_REPOSITORY,
      });
    if (path.endsWith('/repository/files') || path.endsWith('/repository/commits'))
      return route.fulfill({ json: { items: [], next_cursor: null } });
    if (path.endsWith('/ai-tools') && populated.aiTools) return route.fulfill({ json: populated.aiTools });
    if (path.endsWith('/ai-settings') && populated.aiSettings) return route.fulfill({ json: populated.aiSettings });
    if (path.endsWith('/ws/poll'))
      return route.fulfill({
        json: { cursor: Number(url.searchParams.get('since') ?? 0), events: [], overflow: false },
      });
    if (path.endsWith('/terminals'))
      return route.fulfill({
        json: withTerminal ? terminals : [],
      });
    if (path.endsWith('/commands') && populated.commands) return route.fulfill({ json: populated.commands });
    if (path.endsWith('/views') && populated.views) return route.fulfill({ json: populated.views });
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
  await page.goto('/');
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('textbox', { name: /^Project Folder/ }).fill('/work/demo');
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
}
