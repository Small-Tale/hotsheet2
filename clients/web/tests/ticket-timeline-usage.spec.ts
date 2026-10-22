import { expect, type Page, test } from '@playwright/test';

// HS2-ZA7W6E / HS2-J14225: the Timeline tab shows the AI tokens and models recorded in each
// phase gap next to the elapsed time, and the ticket's per-model total below the rail.
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
const project = {
  id: 'demo',
  root: '/work/demo',
  name: 'Demo',
  stores: ['/work/demo.hs2'],
  apiPath: '/__hotsheet/project-api/demo',
};
const row = {
  connection_id: 'git-local',
  native_id: '01',
  qualified_id: 'git-local:01',
  id: '01',
  slug: 'HS2-USAGE1',
  title: 'Measure the phases',
  category: 'feature',
  priority: 'default',
  status: 'started',
  up_next: false,
  feedback_needed: false,
  tags: [],
  blocked_by: [],
  claim_count: 0,
  created_at: '2026-10-01T10:00:00Z',
  updated_at: '2026-10-01T12:00:00Z',
};
const activity = (id: string, created_at: string, text: string) => ({
  id,
  kind: 'activity',
  created_at,
  edited_at: created_at,
  text,
});
const full = {
  store: 'git-local',
  ...row,
  details: 'Timeline usage.',
  blocked_reason: null,
  notes: [
    activity('N1', '2026-10-01T10:30:00Z', 'Status changed from Not Started to Started'),
    activity('N2', '2026-10-01T10:30:00Z', 'Started phase changed from Unspecified to Analyzing'),
    activity('N3', '2026-10-01T11:00:00Z', 'Started phase changed from Analyzing to Working'),
  ],
  attachments: [],
  concurrency_token: 'T1',
};
const usage = {
  events: [
    { ts: '2026-10-01T10:40:00Z', tool: 'claude', model: 'opus', tokens_in: 30_000, tokens_out: 8_000, cost_usd: 0.9 },
    {
      ts: '2026-10-01T11:20:00Z',
      tool: 'claude',
      model: 'sonnet',
      tokens_in: 140_000,
      tokens_out: 22_000,
      cost_usd: 0.6,
    },
    { ts: '2026-10-01T11:40:00Z', tool: 'claude', model: 'opus', tokens_in: 10_000, tokens_out: 4_000, cost_usd: 0.34 },
    {
      ts: '2026-10-01T11:50:00Z',
      tool: 'codex',
      model: 'gpt-5-codex',
      tokens_in: 20_000,
      tokens_out: 5_000,
      cost_usd: 0.1,
    },
  ],
  settled_cost_usd: 0,
  settled_through: '',
};

async function mockProject(page: Page, usageResponse: unknown = usage) {
  await page.route('**/*', async (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname.replace(/\/tickets\/git-local%3A(?=[^/]+)/i, '/tickets/');
    if (path === '/__hotsheet/projects/open') return route.fulfill({ status: 201, json: project });
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
    if (path.endsWith('/tickets/01/usage'))
      return route.fulfill({ json: typeof usageResponse === 'function' ? usageResponse() : usageResponse });
    if (path.endsWith('/tickets/01')) return route.fulfill({ json: full });
    if (path.endsWith('/tickets')) {
      if (!url.searchParams.has('page_size')) return route.fulfill({ json: [row] });
      return route.fulfill({
        json: {
          items: [row],
          counts: {
            total: 1,
            open: 1,
            not_started: 0,
            started: 1,
            completed: 0,
            verified: 0,
            backlog: 0,
            archive: 0,
            trash: 0,
            up_next: 0,
            feedback_needed: 0,
            completed_today: 0,
            in_progress: 1,
            completion_trend: [0, 0, 0, 0, 0, 0, 0],
          },
          page: { next_cursor: null, total: 1 },
        },
      });
    }
    if (path.endsWith('/ws/poll'))
      return route.fulfill({
        json: { cursor: Number(url.searchParams.get('since') ?? 0), events: [], overflow: false },
      });
    if (path.endsWith('/repository/status'))
      return route.fulfill({
        json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true },
      });
    if (/\/(connections|permissions|commands|command-runs|views|corrupt-tickets|terminals|ai-tools)$/.test(path))
      return route.fulfill({ json: [] });
    if (path.endsWith('/duplicate-backlinks'))
      return route.fulfill({ json: { backlinks: [], inaccessible_projects: [] } });
    if (path.endsWith('/ai-settings')) return route.fulfill({ json: {} });
    return route.continue();
  });
}

for (const width of [1280, 768, 390]) {
  test(`shows per-phase tokens and the ticket total on the timeline at ${width}px (HS2-ZA7W6E, HS2-J14225)`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    await mockProject(page);
    await page.goto('/');
    await page.getByRole('button', { name: 'Open Project' }).click();
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    await page.getByText('Measure the phases').click();
    await page.getByRole('tab', { name: 'Timeline' }).click();
    const timeline = page.locator('[data-component="ticket-timeline"]');
    // Under the rail: each model's tokens (and cost when known), then the ticket total (HS2-J14225).
    const breakdown = timeline.locator('[data-component="value-table"]');
    await expect(breakdown).toContainText('sonnet');
    await expect(breakdown).toContainText('opus');
    await expect(breakdown).toContainText('gpt-5-codex');
    await expect(breakdown).toContainText('Total');
    await expect(breakdown).toContainText('239K tokens · ≈$1.94');
    // The breakdown sits below the rail, not above it.
    const order = await timeline.evaluate(
      (node) =>
        node.querySelector('.ticket-timeline__list')!.getBoundingClientRect().bottom <=
        node.querySelector('[data-component="value-table"]')!.getBoundingClientRect().top,
    );
    expect(order).toBe(true);
    // Elapsed time follows the time column; token/model text follows step names (HS2-XQTCBA).
    const edges = await timeline.evaluate((node) => {
      const left = (element: Element) => Math.round(element.getBoundingClientRect().left);
      return {
        times: [...node.querySelectorAll('.ticket-timeline__list time')].map(left),
        elapsed: [...node.querySelectorAll('.ticket-timeline__elapsed')].map(left),
        titles: [...node.querySelectorAll('.ticket-timeline__list li > div strong')].map(left),
        usage: [...node.querySelectorAll('.ticket-timeline__usage')].map(left),
      };
    });
    expect(new Set([...edges.times, ...edges.elapsed]).size).toBe(1);
    expect(new Set([...edges.titles, ...edges.usage]).size).toBe(1);
    expect(edges.usage[0]).toBeGreaterThan(edges.elapsed[0]);
    // The time reads in the event title's weight, unlike the quiet elapsed-time labels.
    const weights = await timeline.evaluate((node) => ({
      time: Number(getComputedStyle(node.querySelector('.ticket-timeline__list time')!).fontWeight),
      title: Number(getComputedStyle(node.querySelector('.ticket-timeline__list li > div strong')!).fontWeight),
      duration: Number(getComputedStyle(node.querySelector('.ticket-timeline__duration')!).fontWeight),
    }));
    expect(weights.time).toBe(weights.title);
    expect(weights.duration).toBeLessThan(weights.time);
    // The creation gap (10:00 to 10:30) had no usage; Analyzing (10:30 to 11:00) used opus; the
    // open Working phase (since 11:00) used both Claude models and Codex.
    await expect(timeline.locator('[data-entry-id="01-created"] .ticket-timeline__duration')).toHaveText('30m');
    await expect(timeline.locator('[data-entry-id="N2"] .ticket-timeline__elapsed')).toHaveText('30m');
    await expect(timeline.locator('[data-entry-id="N2"] .ticket-timeline__usage')).toHaveText('38K tokens · opus');
    await expect(timeline.locator('[data-entry-id="N3"] .ticket-timeline__usage')).toHaveText(
      '201K tokens · sonnet, gpt-5-codex +1',
    );
    await expect(timeline.locator('[data-entry-id="N3"] .ticket-timeline__duration')).toHaveAttribute(
      'aria-label',
      'AI usage since this event: 201K tokens · sonnet, gpt-5-codex +1',
    );
    // The label stays inside its row rather than overflowing the panel.
    const overflow = await timeline.evaluate((node) => {
      const panel = node.getBoundingClientRect();
      return [...node.querySelectorAll('.ticket-timeline__usage')].some((label) => {
        const box = label.getBoundingClientRect();
        return box.right > panel.right + 1 || box.left < panel.left - 1;
      });
    });
    expect(overflow).toBe(false);
    await page.screenshot({
      path: `target/visual-captures/hs2-j14225-codex-timeline-usage-${width}.png`,
      fullPage: width === 390,
    });
  });
}

test('shows priced Fable terminal usage in the narrow ticket timeline (HS2-TTYZGX)', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 900 });
  await mockProject(page, {
    events: [
      {
        ts: '2026-10-01T11:20:00Z',
        tool: 'claude',
        model: 'claude-fable-5-1',
        tokens_in: 1037,
        tokens_out: 2,
        cost_usd: 0.00087,
      },
    ],
    settled_cost_usd: 0,
    settled_through: '',
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
  await page.getByText('Measure the phases').click();
  await page.getByRole('tab', { name: 'Timeline' }).click();
  const breakdown = page.locator('[data-component="ticket-timeline"] [data-component="value-table"]');
  await expect(breakdown).toContainText('claude-fable-5-1');
  await expect(breakdown).toContainText('≈$0.0009');
  await page.screenshot({ path: 'target/visual-captures/hs2-ttyzgx-fable-timeline-390.png', fullPage: true });
});

test('keeps the timeline unchanged when a ticket has no recorded usage', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await mockProject(page, { events: [], settled_cost_usd: 0, settled_through: '' });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
  await page.getByText('Measure the phases').click();
  await page.getByRole('tab', { name: 'Timeline' }).click();
  const timeline = page.locator('[data-component="ticket-timeline"]');
  await expect(timeline.getByText('Working', { exact: true })).toBeVisible();
  await expect(timeline.locator('[data-component="value-table"]')).toHaveCount(0);
  await expect(timeline.locator('[data-entry-id="N2"] .ticket-timeline__duration')).toHaveText('30m');
  await expect(timeline.locator('[data-entry-id="N3"] .ticket-timeline__duration')).toHaveCount(0);
});

for (const width of [1280, 390])
  test(`refreshes active usage when the timeline opens and retains it after reload at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    let currentUsage = { events: [] as typeof usage.events, settled_cost_usd: 0, settled_through: '' };
    await mockProject(page, () => currentUsage);
    await page.goto('/');
    await page.getByRole('button', { name: 'Open Project' }).click();
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    await page.getByText('Measure the phases').click();
    await page.getByRole('tab', { name: 'Timeline' }).click();
    const timeline = page.locator('[data-component="ticket-timeline"]');
    await expect(timeline.locator('[data-component="value-table"]')).toHaveCount(0);

    currentUsage = { ...currentUsage, events: [usage.events[3]] };
    await page.getByRole('tab', { name: 'Info' }).click();
    await page.getByRole('tab', { name: 'Timeline' }).click();
    await expect(timeline.locator('[data-component="value-table"]')).toContainText('25K tokens');
    await page.screenshot({
      path: `target/visual-captures/hs2-nfq1nk-active-usage-${width}.png`,
      fullPage: width === 390,
    });

    currentUsage = { ...currentUsage, events: [usage.events[3], usage.events[0]] };
    await page.getByRole('tab', { name: 'Info' }).click();
    await page.getByRole('tab', { name: 'Timeline' }).click();
    await expect(timeline.locator('[data-component="value-table"]')).toContainText('63K tokens');
    await page.screenshot({
      path: `target/visual-captures/hs2-nfq1nk-refreshed-usage-${width}.png`,
      fullPage: width === 390,
    });
    await page.reload();
    await expect(timeline.locator('[data-component="value-table"]')).toContainText('63K tokens');
  });
