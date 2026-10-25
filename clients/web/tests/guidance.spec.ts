import { expect, test } from '@playwright/test';

import { openDemoProject } from './mobile-project-fixture';

test('HS2-1YXCCW reviews multiple sources and full commit pins before registration', async ({ page }) => {
  await openDemoProject(page);
  const requests: unknown[] = [];
  const actions: unknown[] = [];
  const review = {
    requests: [
      { id: 'one', repository: 'https://example.test/one', refs: ['main', 'v1.*'] },
      { id: 'two', repository: 'https://example.test/two', refs: ['release'] },
    ],
    pins: [
      {
        id: 'one-a',
        group_id: 'one',
        repository: 'https://example.test/one',
        requested_ref: 'main',
        commit: 'a'.repeat(40),
        rules: 2,
        skills: 1,
      },
      {
        id: 'one-b',
        group_id: 'one',
        repository: 'https://example.test/one',
        requested_ref: 'v1.0',
        commit: 'b'.repeat(40),
        rules: 1,
        skills: 0,
      },
      {
        id: 'two',
        group_id: 'two',
        repository: 'https://example.test/two',
        requested_ref: 'release',
        commit: 'c'.repeat(40),
        rules: 3,
        skills: 0,
      },
    ],
  };
  let registered = false;
  await page.route('**/checkouts/demo-checkout/guidance/batch-review', async (route) => {
    requests.push(route.request().postDataJSON());
    await route.fulfill({ json: review });
  });
  await page.route('**/checkouts/demo-checkout/guidance', async (route) => {
    if (route.request().method() === 'POST') {
      actions.push(route.request().postDataJSON());
      registered = true;
    }
    await route.fulfill({
      json: {
        owner: 'builtin',
        explicit_selection: true,
        default_repository: 'https://example.test/one',
        sources: registered
          ? review.pins.map((pin) => ({
              id: pin.id,
              group_id: pin.group_id,
              repository: pin.repository,
              requested_ref: pin.requested_ref,
              commit: pin.commit,
              rules: [],
              skills: [],
            }))
          : [],
      },
    });
  });
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.getByLabel('Settings View').click();
  await page.getByRole('button', { name: 'Project guidance' }).click();
  await page.locator('input[name="guidance-source-id"]').fill('one');
  await page.locator('input[name="guidance-repository"]').fill('https://example.test/one');
  await page.locator('input[name="guidance-ref"]').fill('main, v1.*');
  await page.getByRole('button', { name: 'Add Another Source' }).click();
  const second = page.locator('[data-guidance-batch-row="0"]');
  await second.locator('input[name="guidance-batch-id"]').fill('two');
  await second.locator('input[name="guidance-batch-repository"]').fill('https://example.test/two');
  await second.locator('input[name="guidance-batch-refs"]').fill('release');
  await second.scrollIntoViewIfNeeded();
  await page.screenshot({ path: 'target/visual-captures/hs2-1yxccw-form-wide.png' });
  await expect(page.getByRole('button', { name: 'Register Reviewed Pins' })).toHaveCount(0);
  await page.getByRole('button', { name: 'Review Exact Pins' }).click();
  const pins = page.getByRole('region', { name: 'Reviewed Source Pins' });
  await expect(pins.getByText('a'.repeat(40))).toBeVisible();
  await expect(pins.getByText('b'.repeat(40))).toBeVisible();
  await expect(pins.getByText('c'.repeat(40))).toBeVisible();
  expect(actions).toEqual([]);
  await pins.scrollIntoViewIfNeeded();
  await page.screenshot({ path: 'target/visual-captures/hs2-1yxccw-guidance-wide.png' });
  await page.setViewportSize({ width: 768, height: 900 });
  const hideSettings = page.getByRole('button', { name: 'Hide Settings Sidebar' });
  if (await hideSettings.isVisible()) await hideSettings.click();
  await pins.scrollIntoViewIfNeeded();
  await pins.screenshot({ path: 'target/visual-captures/hs2-1yxccw-pins-medium.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  if (await hideSettings.isVisible()) await hideSettings.click();
  await pins.scrollIntoViewIfNeeded();
  expect(await pins.evaluate((element) => element.scrollWidth - element.clientWidth)).toBeLessThanOrEqual(1);
  await page.screenshot({ path: 'target/visual-captures/hs2-1yxccw-guidance-phone.png' });
  await second.scrollIntoViewIfNeeded();
  await page.screenshot({ path: 'target/visual-captures/hs2-1yxccw-form-phone.png' });
  await page.setViewportSize({ width: 1280, height: 800 });
  await expect(pins.getByText('a'.repeat(40))).toBeVisible();
  await pins.getByRole('button', { name: 'Register Reviewed Pins' }).click();
  expect(requests).toEqual([review.requests]);
  expect(actions).toEqual([{ action: 'register_batch', review }]);
  await expect(page.getByText('Reviewed source revisions pinned.')).toBeVisible();
});

test('HS2-63D6F9 chooses an older rule without moving the source pin', async ({ page }) => {
  await openDemoProject(page);
  const commit = 'a'.repeat(40);
  const olderDigest = 'd'.repeat(64);
  const pinnedDigest = 'c'.repeat(64);
  const actions: unknown[] = [];
  let cached = false;
  let chosen = pinnedDigest;
  const overview = () => ({
    owner: 'builtin',
    explicit_selection: true,
    default_repository: '',
    sources: [
      {
        id: 'playbook',
        repository: 'https://example.test/playbook',
        requested_ref: 'main',
        commit: 'b'.repeat(40),
        skills: [],
        rules: [
          {
            name: 'example',
            description: 'Example rule',
            version: 3,
            pinned_sha256: pinnedDigest,
            chosen_version: chosen === olderDigest ? 1 : 3,
            chosen_sha256: chosen,
            chosen_commit: chosen === olderDigest ? commit : 'b'.repeat(40),
            compatible: true,
            minimum_hotsheet_version: null,
            installed_version: 3,
            owner: 'Hot Sheet',
            tags: [],
            selected: true,
            historical_versions: cached
              ? [
                  { version: 1, sha256: olderDigest, commit, minimum_hotsheet_version: null, compatible: true },
                  {
                    version: 2,
                    sha256: 'e'.repeat(64),
                    commit: 'f'.repeat(40),
                    minimum_hotsheet_version: '999.0.0',
                    compatible: false,
                  },
                ]
              : [],
          },
        ],
      },
    ],
  });
  await page.route('**/checkouts/demo-checkout/guidance', async (route) => {
    if (route.request().method() === 'POST') {
      const action = route.request().postDataJSON();
      actions.push(action);
      if (action.action === 'cache_rule_version') cached = true;
      if (action.action === 'choose_rule_version') chosen = action.sha256;
    }
    await route.fulfill({ json: overview() });
  });
  await page.route('**/checkouts/demo-checkout/guidance/rules/playbook/example/preview', (route) =>
    route.fulfill({
      json: { owner: 'builtin', current: 'Installed v3', pinned: 'Older v1', after: 'Older v1', outcome: 'replace' },
    }),
  );
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.getByLabel('Settings View').click();
  await page.getByRole('button', { name: 'Project guidance' }).click();
  const row = page.locator('[data-guidance-rule="example"]');
  await row.getByText('Versions', { exact: true }).click();
  await row.getByLabel('Older Version Commit').fill(commit);
  await row.getByRole('button', { name: 'Cache Older Version' }).click();
  const picker = row.locator('kui-select[name="guidance-rule-version"]');
  await picker.locator('.kui-select__trigger').click();
  await expect(picker.locator(`.kui-select__option[data-value="${olderDigest}"]`)).toBeVisible();
  await picker.locator(`.kui-select__option[data-value="${olderDigest}"]`).click();
  await expect(row.getByText(/chosen v1/)).toBeVisible();
  await row.getByRole('button', { name: 'Preview Rule and Changes' }).click();
  await expect(row.getByLabel('After Apply')).toHaveValue('Older v1');
  await row.getByRole('button', { name: 'Close Preview' }).click();
  await row.scrollIntoViewIfNeeded();
  await page.screenshot({ path: 'target/visual-captures/hs2-63d6f9-guidance-wide.png' });
  await row.screenshot({ path: 'target/visual-captures/hs2-63d6f9-rule-wide.png' });
  await page.setViewportSize({ width: 768, height: 900 });
  const hideSettings = page.getByRole('button', { name: 'Hide Settings Sidebar' });
  if (await hideSettings.isVisible()) await hideSettings.click();
  await row.scrollIntoViewIfNeeded();
  await page.screenshot({ path: 'target/visual-captures/hs2-63d6f9-guidance-medium.png' });
  await row.screenshot({ path: 'target/visual-captures/hs2-63d6f9-rule-medium.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  if (await hideSettings.isVisible()) await hideSettings.click();
  await row.scrollIntoViewIfNeeded();
  expect(await row.evaluate((element) => element.scrollWidth - element.clientWidth)).toBeLessThanOrEqual(1);
  await page.screenshot({ path: 'target/visual-captures/hs2-63d6f9-guidance-phone.png' });
  await row.screenshot({ path: 'target/visual-captures/hs2-63d6f9-rule-phone.png' });
  await page.setViewportSize({ width: 1280, height: 800 });
  const showSettings = page.getByRole('button', { name: 'Show Settings Sidebar' });
  if (await showSettings.isVisible()) await showSettings.click();
  await expect(row.getByText(/chosen v1/)).toBeVisible();
  await row.scrollIntoViewIfNeeded();
  await page.screenshot({ path: 'target/visual-captures/hs2-63d6f9-guidance-wide-return.png' });
  expect(actions).toEqual([
    {
      action: 'cache_rule_version',
      source: 'playbook',
      rule: 'example',
      requested_ref: commit,
      expected_commit: commit,
    },
    { action: 'choose_rule_version', source: 'playbook', rule: 'example', sha256: olderDigest },
  ]);
});

test('HS2-6ZA2RN searches guidance from the main toolbar with owner and tag chips', async ({ page }) => {
  await openDemoProject(page);
  await page.route('**/checkouts/demo-checkout/guidance', (route) =>
    route.fulfill({
      json: {
        owner: 'builtin',
        explicit_selection: true,
        default_repository: '',
        sources: [
          {
            id: 'playbook',
            repository: 'https://example.test/playbook',
            requested_ref: 'main',
            commit: 'a'.repeat(40),
            skills: [],
            rules: [
              {
                name: 'rust-cli',
                description: 'Command line guidance',
                version: 1,
                installed_version: null,
                owner: 'Hot Sheet',
                tags: ['rust'],
                selected: true,
              },
              {
                name: 'web-ui',
                description: 'Browser guidance',
                version: 1,
                installed_version: null,
                owner: 'Kerf',
                tags: ['web'],
                selected: false,
              },
            ],
          },
        ],
      },
    }),
  );
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.getByLabel('Settings View').click();
  await page.getByRole('button', { name: 'Project guidance' }).click();
  const settings = page.locator('[data-component="guidance-settings"]');
  await expect(settings.getByLabel('Search Rules')).toHaveCount(0);
  await page.getByRole('button', { name: 'Search guidance rules' }).click();
  const search = page.getByRole('searchbox', { name: 'Search guidance rules' });
  await search.fill('owner:"Hot Sheet" tag:rust ');
  await expect(settings.locator('[data-guidance-rule="rust-cli"]')).toBeVisible();
  await expect(settings.locator('[data-guidance-rule="web-ui"]')).toHaveCount(0);
  await expect(page.locator('[data-search-field-editor="guidance-search"] .kui-search-field__token')).toHaveCount(2);
  await settings.locator('[data-guidance-rule="rust-cli"]').scrollIntoViewIfNeeded();
  await page.screenshot({ path: 'target/visual-captures/hs2-6za2rn-guidance-filtered-wide.png' });
  await page.keyboard.press('Escape');
  await expect(search).toBeVisible();
  await page.getByRole('button', { name: 'Clear search' }).click();
  await expect(settings.locator('[data-guidance-rule]')).toHaveCount(2);
  await search.pressSequentially('browser', { delay: 10 });
  await expect(settings.locator('[data-guidance-rule="web-ui"]')).toBeVisible();
  await expect(settings.locator('[data-guidance-rule="rust-cli"]')).toHaveCount(0);
  await page.getByRole('button', { name: 'Clear search' }).click();
  await expect(settings.locator('[data-guidance-rule]')).toHaveCount(2);
  await settings.locator('[data-guidance-rule="rust-cli"]').scrollIntoViewIfNeeded();
  await page.screenshot({ path: 'target/visual-captures/hs2-6za2rn-guidance-wide.png' });
  await page.setViewportSize({ width: 1024, height: 900 });
  await page.waitForTimeout(400);
  await settings.locator('[data-guidance-rule="rust-cli"]').scrollIntoViewIfNeeded();
  await page.screenshot({ path: 'target/visual-captures/hs2-6za2rn-guidance-medium.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.waitForTimeout(400);
  const hideSettings = page.getByRole('button', { name: 'Hide Settings Sidebar' });
  if (await hideSettings.isVisible()) await hideSettings.click();
  await settings.locator('[data-guidance-rule="rust-cli"]').scrollIntoViewIfNeeded();
  expect(await settings.evaluate((element) => element.scrollWidth - element.clientWidth)).toBeLessThanOrEqual(1);
  await page.screenshot({ path: 'target/visual-captures/hs2-6za2rn-guidance-phone.png' });
  await page.setViewportSize({ width: 1280, height: 800 });
  const showSettings = page.getByRole('button', { name: 'Show Settings Sidebar' });
  if (await showSettings.isVisible()) await showSettings.click();
  await expect(settings.locator('[data-guidance-rule]')).toHaveCount(2);
});

test('HS2-019VVA browses grouped rules and previews changes at wide and phone widths', async ({ page }) => {
  await openDemoProject(page);
  let writes = 0;
  const rules = Array.from({ length: 32 }, (_, index) => ({
    name: `rule-${index}`,
    version: 2,
    installed_version: index === 0 ? 1 : null,
    description: `Synthetic guidance ${index}`,
    owner: index < 16 ? 'Hot Sheet' : 'Kerf',
    tags: index === 0 ? ['rust', 'testing'] : ['testing'],
    selected: index === 0,
  }));
  await page.route('**/checkouts/demo-checkout/guidance', (route) => {
    if (route.request().method() === 'POST') writes += 1;
    return route.fulfill({
      json: {
        owner: 'builtin',
        explicit_selection: true,
        default_repository: '',
        sources: [
          {
            id: 'playbook',
            repository: 'https://example.test/playbook',
            requested_ref: 'main',
            commit: 'a'.repeat(40),
            rules,
            skills: [],
          },
        ],
      },
    });
  });
  await page.route('**/checkouts/demo-checkout/guidance/rules/playbook/rule-0/preview', (route) =>
    route.fulfill({
      json: {
        owner: 'builtin',
        current: 'Installed v1',
        pinned: 'Pinned v2',
        after: 'Pinned v2',
        outcome: 'replace',
      },
    }),
  );
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.getByLabel('Settings View').click();
  await page.getByRole('button', { name: 'Project guidance' }).click();
  const settings = page.locator('[data-component="guidance-settings"]');
  await expect(settings.getByText('Hot Sheet · 16 rules')).toBeVisible();
  await expect(settings.getByText('Kerf · 16 rules')).toBeVisible();
  const first = settings.locator('[data-guidance-rule="rule-0"]');
  await expect(first.getByRole('checkbox')).toBeChecked();
  await expect(first.getByText('rust')).toBeVisible();
  await first.getByRole('button', { name: 'Preview Rule and Changes' }).click();
  await expect(first.getByText('Applying this selection replaces the installed rule')).toBeVisible();
  await expect(first.getByLabel('Currently Installed')).toHaveValue('Installed v1');
  await expect(first.getByLabel('After Apply')).toHaveValue('Pinned v2');
  expect(writes).toBe(0);
  await page.screenshot({ path: 'target/visual-captures/hs2-019vva-guidance-wide.png' });
  await first.screenshot({ path: 'target/visual-captures/hs2-019vva-rule-wide.png' });
  await page.setViewportSize({ width: 768, height: 900 });
  await first.screenshot({ path: 'target/visual-captures/hs2-019vva-rule-medium.png' });
  await page.setViewportSize({ width: 390, height: 1600 });
  const hideSettings = page.getByRole('button', { name: 'Hide Settings Sidebar' });
  if (await hideSettings.isVisible()) await hideSettings.click();
  await expect(page.getByRole('button', { name: 'Show Settings Sidebar' })).toBeVisible();
  await first.scrollIntoViewIfNeeded();
  const settingsBox = await settings.boundingBox();
  expect(settingsBox).not.toBeNull();
  expect(settingsBox!.x).toBeGreaterThanOrEqual(0);
  expect(settingsBox!.x + settingsBox!.width).toBeLessThanOrEqual(390);
  expect(await settings.evaluate((element) => element.scrollWidth - element.clientWidth)).toBeLessThanOrEqual(1);
  await page.screenshot({ path: 'target/visual-captures/hs2-019vva-guidance-phone.png' });
  await first.screenshot({ path: 'target/visual-captures/hs2-019vva-rule-phone.png' });
  await page.setViewportSize({ width: 1280, height: 800 });
  const showSettings = page.getByRole('button', { name: 'Show Settings Sidebar' });
  if (await showSettings.isVisible()) await showSettings.click();
  await expect(first.getByLabel('After Apply')).toHaveValue('Pinned v2');
  await first.getByRole('button', { name: 'Close Preview' }).click();
  await expect(first.getByLabel('After Apply')).toHaveCount(0);
  expect(writes).toBe(0);
});

test('optional guidance interview previews context, requires consent, and records advice without applying', async ({
  page,
}) => {
  await openDemoProject(page);
  await page.context().grantPermissions(['clipboard-read', 'clipboard-write']);
  const interviewPath = '**/checkouts/demo-checkout/guidance/recommendations/interview';
  let state: 'not_started' | 'awaiting_answer' | 'answered' = 'not_started';
  let answer: unknown = null;
  let opens = 0;
  let submits = 0;
  let guidanceWrites = 0;
  let offline = false;
  let changed = false;
  const review = () => ({
    state: changed ? 'not_started' : state,
    preview: {
      fingerprint: changed ? 'preview-2' : 'preview-1',
      context: {
        signals: ['react', 'vue'],
        candidates: [
          {
            id: 'candidate-1',
            source: 'playbook',
            kind: 'rule',
            name: 'react-review',
            version: 1,
            pinned_commit: 'a'.repeat(40),
            selected: false,
            evidence: ['package.json: react'],
          },
        ],
      },
      questions: [{ id: 'active-framework', prompt: 'Which UI framework is active?', choices: ['react', 'vue'] }],
      outbound_prompt: 'Exact outbound context: react, vue, candidate-1.',
    },
    answer: changed ? null : answer,
  });
  await page.route('**/checkouts/demo-checkout/guidance', (route) => {
    if (route.request().method() === 'POST') guidanceWrites += 1;
    return route.fulfill({
      json: { owner: 'builtin', explicit_selection: false, default_repository: '', sources: [] },
    });
  });
  await page.route('**/checkouts/demo-checkout/guidance/recommendations', (route) =>
    route.fulfill({
      json: {
        fingerprint: changed ? 'scan-2' : 'scan-1',
        signals: [],
        candidates: [],
        changes: [],
        source_registration_suggested: false,
        first_accept_replaces_bundled_default: false,
      },
    }),
  );
  await page.route(interviewPath, (route) =>
    offline
      ? route.fulfill({ status: 503, json: { error: 'Local interview unavailable' } })
      : route.fulfill({ json: review() }),
  );
  await page.route(`${interviewPath}/open`, (route) => {
    opens += 1;
    expect(route.request().postDataJSON()).toEqual({ fingerprint: 'preview-1', consent: true });
    state = 'awaiting_answer';
    return route.fulfill({ json: review() });
  });
  await page.route(`${interviewPath}/answer`, (route) => {
    submits += 1;
    const body = route.request().postDataJSON() as { fingerprint: string; answer: unknown };
    expect(body.fingerprint).toBe('preview-1');
    answer = body.answer;
    state = 'answered';
    return route.fulfill({ json: review() });
  });
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.getByLabel('Settings View').click();
  await page.getByRole('button', { name: 'Project guidance' }).click();
  const settings = page.locator('[data-component="guidance-settings"]');
  const interview = settings.locator('[data-guidance-interview]');
  await expect(interview.getByText('Exact outbound context: react, vue, candidate-1.')).toBeVisible();
  await expect(interview.getByRole('button', { name: 'Copy Prompt for AI' })).toHaveCount(0);
  expect(opens).toBe(0);
  await interview.screenshot({ path: 'target/visual-captures/hs2-ygv9rn-preview-wide.png' });
  await interview.getByRole('button', { name: 'I Choose to Share This Context' }).click();
  await expect(interview.getByRole('button', { name: 'Copy Prompt for AI' })).toBeVisible();
  expect(opens).toBe(1);
  await interview.getByRole('button', { name: 'Copy Prompt for AI' }).click();
  await expect(settings.getByText('Interview prompt copied. Share it with an AI tool you choose.')).toBeVisible();
  await interview.getByLabel('AI Advisory JSON').fill('not json');
  await interview.getByRole('button', { name: 'Review Advisory Answer' }).click();
  await expect(interview.getByText('Interview Status Unknown')).toBeVisible();
  expect(submits).toBe(0);
  await interview.getByLabel('AI Advisory JSON').fill(
    JSON.stringify({
      advice: [
        { candidate_id: 'candidate-1', stance: 'uncertain', explanation: 'Ask which framework runs in production.' },
      ],
    }),
  );
  await interview.getByRole('button', { name: 'Review Advisory Answer' }).click();
  await expect(interview.getByText('Ask which framework runs in production.')).toBeVisible();
  expect(submits).toBe(1);
  expect(guidanceWrites).toBe(0);
  await page.setViewportSize({ width: 390, height: 1600 });
  const hideSettings = page.getByRole('button', { name: 'Hide Settings Sidebar' });
  if (await hideSettings.isVisible()) await hideSettings.click();
  await settings.locator('[name="guidance-owner"]').scrollIntoViewIfNeeded();
  await interview.scrollIntoViewIfNeeded();
  const settingsBox = await settings.boundingBox();
  expect(settingsBox).not.toBeNull();
  expect(settingsBox!.x).toBeGreaterThanOrEqual(0);
  expect(settingsBox!.x + settingsBox!.width).toBeLessThanOrEqual(390);
  expect(await interview.evaluate((element) => element.scrollWidth - element.clientWidth)).toBeLessThanOrEqual(1);
  await interview.screenshot({ path: 'target/visual-captures/hs2-ygv9rn-advice-narrow.png' });
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.reload();
  await page.getByLabel('Settings View').click();
  const showSettings = page.getByRole('button', { name: 'Show Settings Sidebar' });
  if (await showSettings.isVisible()) await showSettings.click();
  await page.getByRole('button', { name: 'Project guidance' }).click();
  await expect(
    page.locator('[data-guidance-interview]').getByText('Ask which framework runs in production.'),
  ).toBeVisible();
  changed = true;
  await page.getByRole('button', { name: 'AI Recommendations', exact: true }).click();
  await page.getByRole('button', { name: 'Recheck Recommendations' }).click();
  await page
    .getByRole('dialog', { name: 'AI Guidance Recommendations' })
    .getByRole('button', { name: 'Close' })
    .click();
  await expect(
    page.locator('[data-guidance-interview]').getByRole('button', { name: 'I Choose to Share This Context' }),
  ).toBeVisible();
  await expect(
    page.locator('[data-guidance-interview]').getByText('Ask which framework runs in production.'),
  ).toHaveCount(0);
  offline = true;
  await page.reload();
  await page.getByLabel('Settings View').click();
  if (await showSettings.isVisible()) await showSettings.click();
  await page.getByRole('button', { name: 'Project guidance' }).click();
  const unknown = page.locator('[data-component="guidance-settings"]').getByText('Interview Status Unknown');
  await expect(unknown).toBeVisible();
  await unknown.scrollIntoViewIfNeeded();
  await page.screenshot({ path: 'target/visual-captures/hs2-ygv9rn-offline-wide.png' });
  expect(guidanceWrites).toBe(0);
});

test('Project guidance reviews local recommendations and keeps accept separate from apply', async ({ page }) => {
  await openDemoProject(page);
  const commit = '1111111111111111111111111111111111111111';
  const selected = new Set<string>();
  const dismissed = new Set<string>();
  const decisions: unknown[] = [];
  let failScan = false;
  let applyCount = 0;
  const overview = () => ({
    owner: 'builtin',
    explicit_selection: selected.size > 0,
    default_repository: '',
    sources: [
      {
        id: 'playbook',
        repository: 'https://example.test/playbook.git',
        requested_ref: 'main',
        commit,
        rules: ['rust-lint', 'rust-test'].map((name) => ({
          name,
          version: 1,
          installed_version: null,
          description: `${name} guidance`,
          owner: 'rust',
          tags: ['rust'],
          selected: selected.has(name),
        })),
        skills: [],
      },
    ],
  });
  const report = () => ({
    fingerprint: 'manifest-rust-v1',
    signals: [{ key: 'manifest', evidence: 'Cargo.toml' }],
    candidates: ['rust-lint', 'rust-test'].map((name) => ({
      source: 'playbook',
      kind: 'rule',
      name,
      version: 1,
      description: `${name} guidance`,
      pinned_commit: commit,
      evidence: ['Cargo.toml', 'Rust rule tags'],
      fingerprint: `${name}-v1`,
      selected: selected.has(name),
      installed_version: null,
      state: selected.has(name) ? 'accepted' : dismissed.has(name) ? 'dismissed' : 'new',
    })),
    changes:
      selected.size || dismissed.size
        ? []
        : ['rust-lint', 'rust-test'].map((name) => ({
            source: 'playbook',
            kind: 'rule',
            name,
            change: 'new',
          })),
    source_registration_suggested: false,
    first_accept_replaces_bundled_default: selected.size === 0,
  });
  await page.route('**/checkouts/demo-checkout/guidance', (route) => {
    if (route.request().method() === 'POST') applyCount += 1;
    return route.fulfill({ json: overview() });
  });
  await page.route('**/checkouts/demo-checkout/guidance/recommendations', (route) => {
    if (failScan) return route.fulfill({ status: 503, json: { error: 'Local scan unavailable' } });
    if (route.request().method() === 'POST') {
      const decision = route.request().postDataJSON() as { name: string; decision: string; fingerprint: string };
      decisions.push(decision);
      if (decision.fingerprint !== `${decision.name}-v1`)
        return route.fulfill({ status: 409, json: { error: 'Stale recommendation' } });
      if (decision.decision === 'accepted') selected.add(decision.name);
      else dismissed.add(decision.name);
    }
    return route.fulfill({ json: report() });
  });
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.getByLabel('Settings View').click();
  await page.getByRole('button', { name: 'Project guidance' }).click();
  const settings = page.locator('[data-component="guidance-settings"]');
  await expect(settings.locator('[data-guidance-recommendations]')).not.toBeVisible();
  await page.getByRole('button', { name: 'AI Recommendations', exact: true }).click();
  const recommendationDialog = page.getByRole('dialog', { name: 'AI Guidance Recommendations' });
  await expect(recommendationDialog.getByText('New Guidance Recommendations Are Available')).toBeVisible();
  await expect(recommendationDialog.getByText('Evidence: Cargo.toml, Rust rule tags')).toHaveCount(2);
  await page.waitForTimeout(400); // HS2-WEKNRD: capture after the dialog's entrance motion.
  await page.screenshot({ path: 'target/visual-captures/hs2-weknrd-dialog-wide.png' });
  await recommendationDialog.locator('[data-guidance-recommendations]').screenshot({
    path: 'target/visual-captures/hs2-qbxrfy-recommendations-wide.png',
  });
  await recommendationDialog
    .locator('[data-recommendation="playbook:rule:rust-lint"]')
    .getByRole('button', { name: 'Accept Selection' })
    .click();
  await expect(settings.getByText('Suggestion selected. Apply Selected Guidance when ready.')).toHaveCount(1);
  expect(applyCount).toBe(0);
  await expect(recommendationDialog.getByText('New Guidance Recommendations Are Available')).toHaveCount(0);
  await expect(recommendationDialog.getByText('The first accepted selection replaces the bundled default')).toHaveCount(
    0,
  );
  await recommendationDialog
    .locator('[data-recommendation="playbook:rule:rust-test"]')
    .getByRole('button', { name: 'Dismiss' })
    .click();
  await expect(settings.getByText('Suggestion dismissed for this evidence.')).toHaveCount(1);
  expect(decisions).toMatchObject([
    { name: 'rust-lint', decision: 'accepted', fingerprint: 'rust-lint-v1' },
    { name: 'rust-test', decision: 'dismissed', fingerprint: 'rust-test-v1' },
  ]);
  await page.setViewportSize({ width: 768, height: 900 });
  await expect(recommendationDialog).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hs2-weknrd-dialog-medium.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  const hideSettings = page.getByRole('button', { name: 'Hide Settings Sidebar' });
  if (await hideSettings.isVisible()) await hideSettings.click();
  await recommendationDialog.locator('[data-guidance-recommendations]').scrollIntoViewIfNeeded();
  const settingsBox = await settings.boundingBox();
  expect(settingsBox).not.toBeNull();
  expect(settingsBox!.x).toBeGreaterThanOrEqual(0);
  expect(settingsBox!.x + settingsBox!.width).toBeLessThanOrEqual(390);
  expect(await settings.evaluate((element) => element.scrollWidth - element.clientWidth)).toBeLessThanOrEqual(1);
  await expect(
    recommendationDialog.locator('[data-recommendation="playbook:rule:rust-lint"]').getByText('rust-lint guidance'),
  ).toBeVisible();
  await recommendationDialog.locator('[data-guidance-recommendations]').screenshot({
    path: 'target/visual-captures/hs2-qbxrfy-recommendations-narrow.png',
  });
  await page.screenshot({ path: 'target/visual-captures/hs2-weknrd-dialog-phone.png' });
  failScan = true;
  await recommendationDialog.getByRole('button', { name: 'Recheck Recommendations' }).click();
  await expect(recommendationDialog.getByText('Recommendation Status Unknown')).toBeVisible();
  await expect(
    recommendationDialog.locator('[data-recommendation="playbook:rule:rust-lint"]').getByText('rust-lint guidance'),
  ).toBeVisible();
  await recommendationDialog.getByRole('button', { name: 'Close' }).click();
  await expect(recommendationDialog).not.toBeVisible();
  await page.getByRole('button', { name: 'More Workspace Controls' }).click();
  await page.locator('[data-workspace-overflow-action="open-guidance-recommendations"]').click();
  await expect(recommendationDialog).toBeVisible();
  await page.setViewportSize({ width: 1280, height: 800 });
  await expect(recommendationDialog).toBeVisible();
  expect(applyCount).toBe(0);
});

test('HS2-WEKNRD shows loading and empty recommendation dialog states', async ({ page }) => {
  await openDemoProject(page);
  let finishScan: (() => void) | undefined;
  const scanGate = new Promise<void>((resolve) => {
    finishScan = resolve;
  });
  await page.route('**/checkouts/demo-checkout/guidance/recommendations', async (route) => {
    await scanGate;
    await route.fulfill({
      json: {
        fingerprint: 'empty-v1',
        signals: [],
        candidates: [],
        changes: [],
        source_registration_suggested: false,
        first_accept_replaces_bundled_default: false,
      },
    });
  });
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.getByLabel('Settings View').click();
  await page.getByRole('button', { name: 'Project guidance' }).click();
  await page.getByRole('button', { name: 'AI Recommendations', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: 'AI Guidance Recommendations' });
  await expect(dialog.getByText('Checking local project signals…')).toBeVisible();
  finishScan?.();
  await expect(dialog.getByText('No guidance suggestions match the current project signals.')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(dialog).not.toBeVisible();
  await page.getByRole('button', { name: 'AI Recommendations', exact: true }).click();
  await expect(dialog.getByText('No guidance suggestions match the current project signals.')).toBeVisible();
});

test('HS2-B8M0QB opens an empty source-check dialog from the toolbar at wide and phone widths', async ({ page }) => {
  await openDemoProject(page);
  await page.route('**/checkouts/demo-checkout/guidance', (route) =>
    route.fulfill({ json: { owner: 'builtin', explicit_selection: false, default_repository: '', sources: [] } }),
  );
  await page.route('**/checkouts/demo-checkout/guidance/updates', (route) => route.fulfill({ json: [] }));
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.getByLabel('Settings View').click();
  await page.getByRole('button', { name: 'Project guidance' }).click();
  await page.getByRole('button', { name: 'Check Guidance Sources' }).click();
  const dialog = page.getByRole('dialog', { name: 'Guidance Source Updates' });
  await expect(dialog.getByText('Register a source to check for updates.')).toBeVisible();
  await dialog.screenshot({ path: 'target/visual-captures/guidance-update-empty-wide.png' });
  await page.setViewportSize({ width: 768, height: 900 });
  await expect(dialog).toBeVisible();
  await dialog.screenshot({ path: 'target/visual-captures/guidance-update-empty-medium.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(dialog).toBeVisible();
  await dialog.screenshot({ path: 'target/visual-captures/guidance-update-empty-phone.png' });
  await dialog.getByRole('button', { name: 'Close' }).click();
  await page.getByRole('button', { name: 'More Workspace Controls' }).click();
  await page.locator('[data-workspace-overflow-action="open-guidance-updates"]').click();
  await expect(dialog).toBeVisible();
  await page.setViewportSize({ width: 1280, height: 800 });
  await expect(dialog).toBeVisible();
});

test('Project guidance reviews a changed source revision before refreshing its pin', async ({ page }) => {
  let buildRevision = 'build-a';
  let hostRevision = 'host-a';
  await openDemoProject(page, false, 1, undefined, undefined, {
    buildRevision: () => buildRevision,
    hostRevision: () => hostRevision,
  });
  const pinned = '1111111111111111111111111111111111111111';
  const observed = '2222222222222222222222222222222222222222';
  const actions: Array<Record<string, unknown>> = [];
  let updateChecks = 0;
  let failNextCheck = false;
  const overview = {
    owner: 'builtin',
    explicit_selection: true,
    default_repository: '',
    sources: [
      {
        id: 'ai-playbook',
        repository: 'https://example.test/playbook.git',
        requested_ref: 'main',
        commit: pinned,
        rules: [
          {
            name: 'example',
            version: 1,
            installed_version: 1,
            description: 'Example rule',
            owner: null,
            tags: [],
            selected: true,
          },
        ],
        skills: [{ name: 'example-skill', selected: true }],
      },
    ],
  };
  await page.route('**/checkouts/demo-checkout/guidance', (route) => {
    if (route.request().method() === 'POST') {
      const action = route.request().postDataJSON() as Record<string, unknown>;
      actions.push(action);
      if (action.action === 'register') {
        overview.sources[0].commit = observed;
        overview.sources[0].rules[0].version = 2;
      }
      if (action.action === 'apply') overview.sources[0].rules[0].installed_version = 2;
    }
    return route.fulfill({ json: overview });
  });
  await page.route('**/checkouts/demo-checkout/guidance/updates', (route) => {
    updateChecks += 1;
    if (failNextCheck) return route.fulfill({ status: 503, json: { error: 'Source check unavailable' } });
    return route.fulfill({
      json: [
        {
          id: 'ai-playbook',
          requested_ref: 'main',
          pinned_commit: overview.sources[0].commit,
          observed_commit: observed,
          state: overview.sources[0].commit === pinned ? 'revision_changed' : 'current',
          other_refs: [{ reference: 'refs/tags/v2', commit: observed }],
          other_refs_truncated: false,
        },
      ],
    });
  });
  await page.route('**/checkouts/demo-checkout/guidance/updates/ai-playbook/review', (route) =>
    route.fulfill({
      json: {
        update: {
          id: 'ai-playbook',
          requested_ref: 'main',
          pinned_commit: overview.sources[0].commit,
          observed_commit: observed,
          state: overview.sources[0].commit === pinned ? 'revision_changed' : 'current',
          other_refs: [{ reference: 'refs/tags/v2', commit: observed }],
          other_refs_truncated: false,
        },
        changes:
          overview.sources[0].commit === pinned
            ? [
                {
                  name: 'example',
                  kind: 'rule',
                  change: 'updated',
                  selected: true,
                  before_version: 1,
                  after_version: 2,
                },
                {
                  name: 'new-rule',
                  kind: 'rule',
                  change: 'added',
                  selected: false,
                  before_version: null,
                  after_version: 1,
                },
                {
                  name: 'example-skill',
                  kind: 'skill',
                  change: 'updated',
                  selected: true,
                  before_version: null,
                  after_version: null,
                },
              ]
            : [],
      },
    }),
  );
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.getByLabel('Settings View').click();
  await page.getByRole('button', { name: 'Project guidance' }).click();
  const picker = page.locator('[data-component="guidance-settings"]');
  await page.locator('.workspace-header__updates-group').screenshot({
    path: 'target/visual-captures/hs2-b8m0qb-toolbar-refresh-wide.png',
  });
  await page.getByRole('button', { name: 'Check Guidance Sources' }).click();
  const updates = page.getByRole('dialog', { name: 'Guidance Source Updates' });
  await expect(updates.getByText('ai-playbook Has A Different Source Revision')).toBeVisible();
  await expect(updates).toContainText('refs/tags/v2');
  await expect(updates).not.toContainText('still need Apply Selected Guidance');
  await page.screenshot({ path: 'target/visual-captures/hs2-b8m0qb-source-updates-wide.png' });
  await page.setViewportSize({ width: 2048, height: 1128 });
  await page.screenshot({ path: 'target/visual-captures/hs2-b8m0qb-source-updates-reference-2048.png' });
  await page.setViewportSize({ width: 1280, height: 800 });
  await updates
    .locator('[data-guidance-updates]')
    .screenshot({ path: 'target/visual-captures/guidance-update-available-wide.png' });
  await updates.getByRole('button', { name: 'Review Changes' }).click();
  await expect(updates.locator('[data-guidance-review]')).toContainText('rule example: updated v1 → v2');
  await expect(updates.locator('[data-guidance-review]')).toContainText('rule new-rule: added → v1 · not selected');
  await expect(updates.locator('[data-guidance-review]')).toContainText('skill example-skill: updated');
  await updates
    .locator('[data-guidance-review]')
    .screenshot({ path: 'target/visual-captures/guidance-update-review-wide.png' });
  await page.setViewportSize({ width: 768, height: 900 });
  await updates.screenshot({ path: 'target/visual-captures/guidance-update-review-medium.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  const hideSettings = page.getByRole('button', { name: 'Hide Settings Sidebar' });
  if (await hideSettings.isVisible()) await hideSettings.click();
  await picker.locator('[name="guidance-owner"]').scrollIntoViewIfNeeded();
  await expect(picker).toBeVisible();
  const pickerBox = await picker.boundingBox();
  expect(pickerBox).not.toBeNull();
  expect(pickerBox!.x).toBeGreaterThanOrEqual(0);
  expect(pickerBox!.x + pickerBox!.width).toBeLessThanOrEqual(390);
  expect(await picker.evaluate((element) => element.scrollWidth - element.clientWidth)).toBeLessThanOrEqual(1);
  await updates
    .locator('[data-guidance-updates]')
    .screenshot({ path: 'target/visual-captures/guidance-update-review-phone.png' });
  await page.screenshot({ path: 'target/visual-captures/hs2-b8m0qb-source-updates-phone.png' });
  await updates
    .locator('[data-guidance-review]')
    .screenshot({ path: 'target/visual-captures/guidance-update-review-detail-phone.png' });
  await updates.getByRole('button', { name: 'Refresh Reviewed Pin' }).click();
  await expect.poll(() => actions.find((action) => action.action === 'register')?.expected_commit).toBe(observed);
  await expect(picker).toContainText('1 selected rule(s) still need Apply Selected Guidance');
  expect(await picker.evaluate((element) => element.scrollWidth - element.clientWidth)).toBeLessThanOrEqual(1);
  await updates
    .locator('[data-guidance-updates]')
    .screenshot({ path: 'target/visual-captures/guidance-update-pending-phone.png' });
  await page.keyboard.press('Escape');
  await expect(updates).not.toBeVisible();
  await picker.getByRole('button', { name: 'Apply Selected Guidance' }).click();
  await expect.poll(() => actions.some((action) => action.action === 'apply')).toBe(true);
  await expect(picker).not.toContainText('still need Apply Selected Guidance');
  await picker.getByRole('button', { name: 'Review Pinned Source' }).click();
  await expect(updates).toBeVisible();
  await updates
    .locator('[data-guidance-updates]')
    .screenshot({ path: 'target/visual-captures/guidance-update-applied-phone.png' });
  await page.setViewportSize({ width: 1280, height: 800 });
  await updates.screenshot({ path: 'target/visual-captures/guidance-update-applied-wide-return.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(picker).toContainText('ai-playbook: source is current');
  const checksBeforeReload = updateChecks;
  await page.reload();
  await page.getByLabel('Settings View').click();
  const showSettings = page.getByRole('button', { name: 'Show Settings Sidebar' });
  if (await showSettings.isVisible()) await showSettings.click();
  await page.getByRole('button', { name: 'Project guidance' }).click();
  const hideAgain = page.getByRole('button', { name: 'Hide Settings Sidebar' });
  if (await hideAgain.isVisible()) await hideAgain.click();
  await expect(picker).toContainText('ai-playbook: source is current');
  expect(updateChecks).toBe(checksBeforeReload);
  hostRevision = 'host-b';
  await page.reload();
  await page.getByLabel('Settings View').click();
  const showAfterUpgrade = page.getByRole('button', { name: 'Show Settings Sidebar' });
  if (await showAfterUpgrade.isVisible()) await showAfterUpgrade.click();
  await page.getByRole('button', { name: 'Project guidance' }).click();
  const hideAfterUpgrade = page.getByRole('button', { name: 'Hide Settings Sidebar' });
  if (await hideAfterUpgrade.isVisible()) await hideAfterUpgrade.click();
  await expect.poll(() => updateChecks).toBe(checksBeforeReload + 1);
  await expect(picker).toContainText('ai-playbook: source is current');
  failNextCheck = true;
  buildRevision = 'build-b';
  await page.reload();
  await page.getByLabel('Settings View').click();
  const showAfterFailedUpgrade = page.getByRole('button', { name: 'Show Settings Sidebar' });
  if (await showAfterFailedUpgrade.isVisible()) await showAfterFailedUpgrade.click();
  await page.getByRole('button', { name: 'Project guidance' }).click();
  const hideAfterFailedUpgrade = page.getByRole('button', { name: 'Hide Settings Sidebar' });
  if (await hideAfterFailedUpgrade.isVisible()) await hideAfterFailedUpgrade.click();
  await expect(picker).toContainText('Source Check Failed');
  await expect(picker).toContainText('Previous results may be stale');
  await expect(picker).toContainText('ai-playbook: source is current');
  expect(updateChecks).toBe(checksBeforeReload + 2);
  await picker.getByRole('button', { name: 'Review Pinned Source' }).click();
  await expect(updates.locator('[data-guidance-review]')).toContainText('The source is current.');
  await updates.getByRole('button', { name: 'Check again' }).click();
  await expect(picker).toContainText('Source Check Failed');
  await expect(picker).toContainText('Previous results may be stale');
  await expect(picker).toContainText('ai-playbook: source is current');
  expect(updateChecks).toBe(checksBeforeReload + 3);
  await updates
    .locator('[data-guidance-updates]')
    .screenshot({ path: 'target/visual-captures/guidance-update-stale-phone.png' });
});

test('project guidance picker searches, selects, applies, and reports versions at wide and narrow sizes', async ({
  page,
}) => {
  await openDemoProject(page);
  const actions: Array<Record<string, unknown>> = [];
  const overview = {
    owner: 'builtin',
    explicit_selection: false,
    default_repository: 'https://github.com/Small-Tale/ai-playbook.git',
    sources: [
      {
        id: 'ai-playbook',
        repository: 'https://github.com/Small-Tale/ai-playbook.git',
        requested_ref: 'v1.2.0',
        commit: '7cb9499a655a15213666d684a6659db43f840acf',
        rules: [
          {
            name: 'rust-lint-and-format',
            version: 1,
            installed_version: null,
            description: 'Rust lint and formatting defaults',
            owner: 'rust',
            tags: ['rust', 'lint'],
            selected: false,
          },
          {
            name: 'hotsheet-visual-qa',
            version: 2,
            installed_version: 1,
            description: 'Visual review of rendered changes',
            owner: 'hotsheet',
            tags: ['ui', 'testing'],
            selected: true,
          },
        ],
        skills: [{ name: 'example-skill', selected: false }],
      },
    ],
  };
  await page.route('**/checkouts/demo-checkout/guidance', async (route) => {
    if (route.request().method() === 'POST') {
      const action = route.request().postDataJSON() as Record<string, unknown>;
      actions.push(action);
      if (action.action === 'select') {
        overview.explicit_selection = true;
        overview.sources[0].rules.forEach((rule) => {
          rule.selected = (action.rules as string[]).includes(rule.name);
        });
        overview.sources[0].skills.forEach((skill) => {
          skill.selected = (action.skills as string[]).includes(skill.name);
        });
      }
      if (action.action === 'owner') overview.owner = action.owner as string;
      if (action.action === 'apply') overview.sources[0].rules[0].installed_version = 1;
    }
    await route.fulfill({ json: overview });
  });
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.getByLabel('Settings View').click();
  await page.getByRole('button', { name: /AI tools/ }).click();
  await expect(page.locator('[data-component="guidance-settings"]')).toHaveCount(0);
  await page.getByRole('button', { name: 'Project guidance' }).click();
  const picker = page.locator('[data-component="guidance-settings"]');
  const guidanceContent = picker.locator('#guidance-content');
  const guidanceFooter = picker.locator('.guidance-settings__footer');
  await expect(guidanceFooter.getByText('1 rule or skill selected')).toBeVisible();
  expect(await guidanceContent.evaluate((element) => element.scrollHeight > element.clientHeight)).toBe(true);
  const footerAtTop = await guidanceFooter.boundingBox();
  await guidanceContent.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });
  const footerAtEnd = await guidanceFooter.boundingBox();
  expect(footerAtEnd?.y).toBeCloseTo(footerAtTop?.y ?? 0, 0);
  await guidanceContent.evaluate((element) => {
    element.scrollTop = 0;
  });
  const visualRule = picker.locator('[data-guidance-rule="hotsheet-visual-qa"]');
  await expect(visualRule.locator('label')).toContainText('Visual review of rendered changes');
  await expect(visualRule.locator('label')).not.toContainText('hotsheet-visual-qa');
  await expect(visualRule).toContainText('hotsheet-visual-qa · available v2');
  await expect(picker).toContainText('v1 installed');
  await expect(picker).toContainText('not installed');
  await expect(picker).toContainText('available v2');
  await visualRule.scrollIntoViewIfNeeded();
  await page.screenshot({ path: 'target/visual-captures/guidance-rules-wide.png' });
  await picker.screenshot({ path: 'target/visual-captures/guidance-wide.png' });
  await page.screenshot({ path: 'target/visual-captures/guidance-wide-page.png', fullPage: true });
  await page.getByRole('button', { name: 'Search guidance rules' }).click();
  const toolbarSearch = page.getByRole('searchbox', { name: 'Search guidance rules' });
  await toolbarSearch.fill('rust');
  await expect(picker.locator('[data-guidance-rule="rust-lint-and-format"]')).toBeVisible();
  await expect(picker.locator('[data-guidance-rule="hotsheet-visual-qa"]')).toBeHidden();
  await toolbarSearch.fill('');
  const ownerFilter = picker.locator('kui-select[name="guidance-owner-filter"]');
  await ownerFilter.locator('.kui-select__trigger').click();
  await ownerFilter.locator('.kui-select__option[data-value="rust"]').click();
  await expect(picker.locator('[data-guidance-rule="hotsheet-visual-qa"]')).toBeHidden();
  await ownerFilter.locator('.kui-select__trigger').click();
  await ownerFilter.locator('.kui-select__option[data-value=""]').click();
  await picker.locator('[data-guidance-rule="rust-lint-and-format"] input[type="checkbox"]').check();
  await expect(guidanceFooter.getByText('2 rules and skills selected')).toBeVisible();
  await expect
    .poll(() =>
      actions.some(
        (action) => action.action === 'select' && (action.rules as string[]).includes('rust-lint-and-format'),
      ),
    )
    .toBe(true);
  await picker.getByRole('button', { name: 'Apply Selected Guidance' }).click();
  await expect.poll(() => actions.some((action) => action.action === 'apply')).toBe(true);
  await expect(picker).toContainText('Selected guidance applied.');
  await picker.locator('input[name="guidance-source-id"]').fill('team-rules');
  await picker.getByRole('button', { name: 'Register source' }).click();
  await expect
    .poll(() => actions.some((action) => action.action === 'register' && action.id === 'team-rules'))
    .toBe(true);
  const writer = picker.locator('kui-select[name="guidance-owner"]');
  await writer.locator('.kui-select__trigger').click();
  await writer.locator('.kui-select__option[data-value="rulesync"]').click();
  await expect
    .poll(() => actions.some((action) => action.action === 'owner' && action.owner === 'rulesync'))
    .toBe(true);
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(picker).toBeVisible();
  const phoneFooter = await guidanceFooter.boundingBox();
  expect(phoneFooter?.x).toBeGreaterThanOrEqual(0);
  expect((phoneFooter?.x ?? 0) + (phoneFooter?.width ?? 0)).toBeLessThanOrEqual(390);
  const overflow = await picker.evaluate((element) => element.scrollWidth - element.clientWidth);
  expect(overflow).toBeLessThanOrEqual(1);
  await writer.scrollIntoViewIfNeeded();
  const writerBox = await writer.boundingBox();
  expect(writerBox?.x).toBeGreaterThanOrEqual(0);
  expect((writerBox?.x ?? 0) + (writerBox?.width ?? 0)).toBeLessThanOrEqual(390);
  await page.screenshot({ path: 'target/visual-captures/guidance-phone-top.png', fullPage: true });
  await picker.getByRole('button', { name: 'Apply Selected Guidance' }).scrollIntoViewIfNeeded();
  await page.screenshot({ path: 'target/visual-captures/guidance-phone-page.png', fullPage: true });
  await writer.scrollIntoViewIfNeeded();
  await page.screenshot({ path: 'target/visual-captures/guidance-phone-top-settled.png', fullPage: true });
  await visualRule.scrollIntoViewIfNeeded();
  await page.screenshot({ path: 'target/visual-captures/guidance-rules-phone.png' });
  await page.setViewportSize({ width: 1024, height: 768 });
  await expect(guidanceFooter).toBeVisible();
  await page.waitForTimeout(300);
  await page.screenshot({ path: 'target/visual-captures/guidance-footer-medium.png' });
  await page.setViewportSize({ width: 1280, height: 800 });
  await expect(guidanceFooter).toBeVisible();
  await page.waitForTimeout(300);
  await page.screenshot({ path: 'target/visual-captures/guidance-footer-wide-return.png' });
});

test('HS2-T0GHX5 puts Instruction Writer first in the shipped Settings view', async ({ page }) => {
  await openDemoProject(page);
  await page.route('**/checkouts/demo-checkout/guidance', (route) =>
    route.fulfill({
      json: {
        owner: 'rulesync',
        explicit_selection: true,
        default_repository: '',
        sources: [
          {
            id: 'sample-source',
            repository: 'https://example.test/guidance.git',
            requested_ref: 'v1',
            commit: '1'.repeat(40),
            rules: [
              {
                name: 'sample-rule',
                description: 'Synthetic guidance rule',
                version: 1,
                installed_version: 1,
                owner: null,
                tags: [],
                selected: true,
              },
            ],
            skills: [],
          },
        ],
      },
    }),
  );
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.getByLabel('Settings View').click();
  await page.getByRole('button', { name: 'Project guidance' }).click();
  const picker = page.locator('[data-component="guidance-settings"]');
  const writer = picker.locator('[name="guidance-owner"]');
  await expect(writer).toBeVisible();
  await expect(picker.getByRole('heading', { name: 'Project guidance' })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Project guidance' })).toBeVisible();
  expect(
    await writer.evaluate((element) => {
      const recommendations = document.querySelector('[data-guidance-recommendations]');
      return recommendations !== null && Boolean(element.compareDocumentPosition(recommendations) & 4);
    }),
  ).toBe(true);
  for (const [width, height, label] of [
    [1280, 800, 'wide'],
    [1024, 768, 'medium'],
    [390, 844, 'phone'],
    [1280, 800, 'wide-return'],
  ] as const) {
    await page.setViewportSize({ width, height });
    if (label === 'phone' || label === 'wide-return') {
      await page.reload();
      await page.getByLabel('Settings View').click();
      const showSidebar = page.getByRole('button', { name: 'Show Settings Sidebar' });
      if (await showSidebar.isVisible()) await showSidebar.click();
      await page.getByRole('button', { name: 'Project guidance' }).click();
    }
    const hideSettings = page.getByRole('button', { name: 'Hide Settings Sidebar' });
    if (width === 390 && (await hideSettings.isVisible())) await hideSettings.click();
    const showSettings = page.getByRole('button', { name: 'Show Settings Sidebar' });
    if (width === 390) await expect(showSettings).toBeVisible();
    if (width !== 390 && (await showSettings.isVisible())) await showSettings.click();
    await page.evaluate(() => {
      window.scrollTo(0, 0);
    });
    await expect(writer).toBeVisible();
    await expect
      .poll(async () => {
        const box = await picker.boundingBox();
        return box !== null && box.x >= 0 && box.x + box.width <= width;
      })
      .toBe(true);
    const writerBox = await writer.boundingBox();
    expect(writerBox?.x).toBeGreaterThanOrEqual(0);
    expect((writerBox?.x ?? 0) + (writerBox?.width ?? 0)).toBeLessThanOrEqual(width);
    expect(await picker.evaluate((element) => element.scrollWidth - element.clientWidth)).toBeLessThanOrEqual(1);
    await page.screenshot({ path: `target/visual-captures/hs2-t0ghx5-${label}.png`, animations: 'disabled' });
  }
});

test('Project guidance has its own Settings tab with an empty source state at wide and phone sizes', async ({
  page,
}) => {
  await openDemoProject(page);
  await page.route('**/checkouts/demo-checkout/guidance', async (route) => {
    await route.fulfill({
      json: {
        owner: 'builtin',
        explicit_selection: false,
        default_repository: 'https://github.com/Small-Tale/ai-playbook.git',
        sources: [],
      },
    });
  });
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.getByLabel('Settings View').click();
  await page.getByRole('button', { name: 'Project guidance' }).click();
  const picker = page.locator('[data-component="guidance-settings"]');
  await expect(picker.getByText('Register a source to browse its rules and skills.')).toBeVisible();
  await expect(page.locator('[data-component="ai-tool-settings"]')).toHaveCount(0);
  await picker.getByText('Register a source to browse its rules and skills.').scrollIntoViewIfNeeded();
  await page.screenshot({ path: 'target/visual-captures/guidance-empty-tab-wide.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.reload();
  await page.getByLabel('Settings View').click();
  const showSettings = page.getByRole('button', { name: 'Show Settings Sidebar' });
  if (await showSettings.isVisible()) await showSettings.click();
  await page.getByRole('button', { name: 'Project guidance' }).click();
  const hideSettings = page.getByRole('button', { name: 'Hide Settings Sidebar' });
  if (await hideSettings.isVisible()) await hideSettings.click();
  await picker.getByText('Register a source to browse its rules and skills.').scrollIntoViewIfNeeded();
  await page.waitForTimeout(400);
  await expect(picker).toBeVisible();
  expect(await picker.evaluate((element) => element.scrollWidth - element.clientWidth)).toBeLessThanOrEqual(1);
  await page.screenshot({ path: 'target/visual-captures/guidance-empty-tab-phone.png' });
});

test('Project guidance reports a blocked bundled section at wide and phone sizes', async ({ page }) => {
  await openDemoProject(page, false, 1, undefined, undefined, {
    bundledGuidanceBlocked: [
      { path: 'AGENTS.md', section: 'ticket-driven-work', installedVersion: 1, bundledVersion: 6 },
    ],
  });
  await page.route('**/checkouts/demo-checkout/guidance', (route) =>
    route.fulfill({ json: { owner: 'builtin', explicit_selection: false, default_repository: '', sources: [] } }),
  );
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 844 });
    if (width === 390) await page.reload();
    await page.getByLabel('Settings View').click();
    const showSettings = page.getByRole('button', { name: 'Show Settings Sidebar' });
    if (await showSettings.isVisible()) await showSettings.click();
    await page.getByRole('button', { name: 'Project guidance' }).click();
    const hideSettings = page.getByRole('button', { name: 'Hide Settings Sidebar' });
    if (width === 390) await page.waitForTimeout(250);
    if (await hideSettings.isVisible()) await hideSettings.click();
    if (width === 390) await page.waitForTimeout(500);
    const picker = page.locator('[data-component="guidance-settings"]');
    await expect(picker.getByText('Bundled Guidance Needs Review')).toBeVisible();
    await expect(picker).toContainText('AGENTS.md: ticket-driven-work (local v1, bundled v6)');
    if (width === 390) {
      await expect(page.getByRole('button', { name: 'Show Settings Sidebar' })).toBeVisible();
      await page.evaluate(() => {
        window.scrollTo(0, 0);
      });
      const box = await picker.boundingBox();
      expect(box).not.toBeNull();
      expect(box!.x).toBeGreaterThanOrEqual(0);
      expect(box!.x + box!.width).toBeLessThanOrEqual(width);
      await picker.screenshot({ path: 'target/visual-captures/bundled-guidance-blocked-content-390.png' });
    }
    expect(await picker.evaluate((element) => element.scrollWidth - element.clientWidth)).toBeLessThanOrEqual(1);
    await page.screenshot({ path: `target/visual-captures/bundled-guidance-blocked-${width}.png` });
  }
});

test('Project guidance keeps standalone Rulesync lock status separate from pinned sources', async ({ page }) => {
  await openDemoProject(page);
  await page.route('**/checkouts/demo-checkout/guidance', (route) =>
    route.fulfill({
      json: {
        owner: 'rulesync',
        explicit_selection: false,
        default_repository: '',
        sources: [],
        local_rules: ['project-local'],
      },
    }),
  );
  await page.route('**/checkouts/demo-checkout/guidance/rulesync-updates', (route) =>
    route.fulfill({
      json: {
        state: 'outdated',
        executable_version: '38.0.0',
        sources: [
          {
            source: 'https://github.com/Small-Tale/ai-playbook.git',
            state: 'outdated',
            requested_ref: 'main',
            locked_ref: 'fcb8556bef14a9dfdaf8cc53f8b36dfb13d9a36a',
            latest_ref: '7cb9499a655a15213666d684a6659db43f840acf',
          },
        ],
      },
    }),
  );
  for (const [index, width] of [1280, 768, 390, 1280].entries()) {
    await page.setViewportSize({ width, height: 844 });
    if (index > 0) await page.reload();
    await page.getByLabel('Settings View').click();
    const showSettings = page.getByRole('button', { name: 'Show Settings Sidebar' });
    if (await showSettings.isVisible()) await showSettings.click();
    await page.getByRole('button', { name: 'Project guidance' }).click();
    const hideSettings = page.getByRole('button', { name: 'Hide Settings Sidebar' });
    if (width === 390) await page.waitForTimeout(250);
    if (await hideSettings.isVisible()) await hideSettings.click();
    if (width === 390) await page.waitForTimeout(500);
    const picker = page.locator('[data-component="guidance-settings"]');
    await expect(picker.getByText('Rulesync Lock Has Updates')).toBeVisible();
    await expect(picker).toContainText('Rulesync 38.0.0: outdated source locks');
    await expect(picker).toContainText('locked fcb8556bef14');
    await expect(picker).toContainText('observed 7cb9499a655a');
    await expect(picker).not.toContainText('Pinned to');
    expect(await picker.evaluate((element) => element.scrollWidth - element.clientWidth)).toBeLessThanOrEqual(1);
    await picker.screenshot({ path: `target/visual-captures/rulesync-outdated-${width}.png` });
    const localGuidance = picker.getByText('Hot Sheet saves unmarked local guidance');
    await expect(localGuidance).toBeVisible();
    await expect(picker.getByRole('region', { name: 'Project-Local Rulesync Rules' })).toContainText(
      'project-local · .rulesync/rules/project-local.md',
    );
    await localGuidance.scrollIntoViewIfNeeded();
    await page.screenshot({ path: `target/visual-captures/rulesync-local-guidance-${width}.png` });
  }
});

test('HS2-C9FC2K shows pending Rulesync review until a ticket source is linked', async ({ page }) => {
  await openDemoProject(page);
  let pending = true;
  await page.route('**/checkouts/demo-checkout/guidance', (route) =>
    route.fulfill({
      json: {
        owner: 'rulesync',
        pending_consolidation_review: pending,
        explicit_selection: false,
        default_repository: '',
        local_rules: ['project-local'],
        sources: [],
      },
    }),
  );
  for (const [index, width] of [1280, 768, 390, 1280].entries()) {
    await page.setViewportSize({ width, height: 844 });
    if (index > 0) await page.reload();
    await page.getByLabel('Settings View').click();
    const showSettings = page.getByRole('button', { name: 'Show Settings Sidebar' });
    if (await showSettings.isVisible()) await showSettings.click();
    await page.getByRole('button', { name: 'Project guidance' }).click();
    const hideSettings = page.getByRole('button', { name: 'Hide Settings Sidebar' });
    if (width === 390) await page.waitForTimeout(250);
    if (await hideSettings.isVisible()) await hideSettings.click();
    if (width === 390) await page.waitForTimeout(500);
    const picker = page.locator('[data-component="guidance-settings"]');
    const notice = picker.getByText('Rulesync Guidance Review Pending');
    await expect(notice).toBeVisible();
    const box = await picker.boundingBox();
    expect(box).not.toBeNull();
    expect(box!.x).toBeGreaterThanOrEqual(0);
    expect(box!.x + box!.width).toBeLessThanOrEqual(width);
    await expect(picker).toContainText('Ticket sources');
    expect(await picker.evaluate((element) => element.scrollWidth - element.clientWidth)).toBeLessThanOrEqual(1);
    await page.screenshot({ path: `target/visual-captures/hs2-c9fc2k-pending-${width}.png` });
  }
  pending = false;
  await page.reload();
  await page.getByLabel('Settings View').click();
  const showSettings = page.getByRole('button', { name: 'Show Settings Sidebar' });
  if (await showSettings.isVisible()) await showSettings.click();
  await page.getByRole('button', { name: 'Project guidance' }).click();
  await expect(page.getByText('Rulesync Guidance Review Pending')).toHaveCount(0);
});
