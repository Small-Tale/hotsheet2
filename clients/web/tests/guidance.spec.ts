import { expect, test } from '@playwright/test';

import { openDemoProject } from './mobile-project-fixture';

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
  await settings.getByRole('heading', { name: 'Project guidance' }).scrollIntoViewIfNeeded();
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
  await page.getByRole('button', { name: 'Recheck Recommendations' }).click();
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
  await expect(settings.getByText('New Guidance Recommendations Are Available')).toBeVisible();
  await expect(settings.getByText('Evidence: Cargo.toml, Rust rule tags')).toHaveCount(2);
  await settings.locator('[data-guidance-recommendations]').screenshot({
    path: 'target/visual-captures/hs2-qbxrfy-recommendations-wide.png',
  });
  await settings
    .locator('[data-recommendation="playbook:rule:rust-lint"]')
    .getByRole('button', { name: 'Accept Selection' })
    .click();
  await expect(settings.getByText('Suggestion selected. Apply Selected Guidance when ready.')).toBeVisible();
  expect(applyCount).toBe(0);
  await expect(settings.getByText('New Guidance Recommendations Are Available')).toHaveCount(0);
  await expect(settings.getByText('The first accepted selection replaces the bundled default')).toHaveCount(0);
  await settings
    .locator('[data-recommendation="playbook:rule:rust-test"]')
    .getByRole('button', { name: 'Dismiss' })
    .click();
  await expect(settings.getByText('Suggestion dismissed for this evidence.')).toBeVisible();
  expect(decisions).toMatchObject([
    { name: 'rust-lint', decision: 'accepted', fingerprint: 'rust-lint-v1' },
    { name: 'rust-test', decision: 'dismissed', fingerprint: 'rust-test-v1' },
  ]);
  await page.setViewportSize({ width: 390, height: 844 });
  const hideSettings = page.getByRole('button', { name: 'Hide Settings Sidebar' });
  if (await hideSettings.isVisible()) await hideSettings.click();
  await settings.getByRole('heading', { name: 'Project guidance' }).scrollIntoViewIfNeeded();
  const settingsBox = await settings.boundingBox();
  expect(settingsBox).not.toBeNull();
  expect(settingsBox!.x).toBeGreaterThanOrEqual(0);
  expect(settingsBox!.x + settingsBox!.width).toBeLessThanOrEqual(390);
  expect(await settings.evaluate((element) => element.scrollWidth - element.clientWidth)).toBeLessThanOrEqual(1);
  await expect(
    settings.locator('[data-recommendation="playbook:rule:rust-lint"]').getByText('rust-lint guidance'),
  ).toBeVisible();
  await settings.locator('[data-guidance-recommendations]').screenshot({
    path: 'target/visual-captures/hs2-qbxrfy-recommendations-narrow.png',
  });
  failScan = true;
  await settings.getByRole('button', { name: 'Recheck Recommendations' }).click();
  await expect(settings.getByText('Recommendation Status Unknown')).toBeVisible();
  await expect(
    settings.locator('[data-recommendation="playbook:rule:rust-lint"]').getByText('rust-lint guidance'),
  ).toBeVisible();
  expect(applyCount).toBe(0);
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
  await expect(picker.getByText('ai-playbook Has A Different Source Revision')).toBeVisible();
  await expect(picker).not.toContainText('still need Apply Selected Guidance');
  await picker
    .locator('[data-guidance-updates]')
    .screenshot({ path: 'target/visual-captures/guidance-update-available-wide.png' });
  await picker.getByRole('button', { name: 'Review Changes' }).click();
  await expect(picker.locator('[data-guidance-review]')).toContainText('rule example: updated v1 → v2');
  await expect(picker.locator('[data-guidance-review]')).toContainText('rule new-rule: added → v1 · not selected');
  await expect(picker.locator('[data-guidance-review]')).toContainText('skill example-skill: updated');
  await picker
    .locator('[data-guidance-review]')
    .screenshot({ path: 'target/visual-captures/guidance-update-review-wide.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  const hideSettings = page.getByRole('button', { name: 'Hide Settings Sidebar' });
  if (await hideSettings.isVisible()) await hideSettings.click();
  await picker.getByRole('heading', { name: 'Project guidance' }).scrollIntoViewIfNeeded();
  await expect(picker).toBeVisible();
  const pickerBox = await picker.boundingBox();
  expect(pickerBox).not.toBeNull();
  expect(pickerBox!.x).toBeGreaterThanOrEqual(0);
  expect(pickerBox!.x + pickerBox!.width).toBeLessThanOrEqual(390);
  expect(await picker.evaluate((element) => element.scrollWidth - element.clientWidth)).toBeLessThanOrEqual(1);
  await picker
    .locator('[data-guidance-updates]')
    .screenshot({ path: 'target/visual-captures/guidance-update-review-phone.png' });
  await picker
    .locator('[data-guidance-review]')
    .screenshot({ path: 'target/visual-captures/guidance-update-review-detail-phone.png' });
  await picker.getByRole('button', { name: 'Refresh Reviewed Pin' }).click();
  await expect.poll(() => actions.find((action) => action.action === 'register')?.expected_commit).toBe(observed);
  await expect(picker).toContainText('1 selected rule(s) still need Apply Selected Guidance');
  expect(await picker.evaluate((element) => element.scrollWidth - element.clientWidth)).toBeLessThanOrEqual(1);
  await picker
    .locator('[data-guidance-updates]')
    .screenshot({ path: 'target/visual-captures/guidance-update-pending-phone.png' });
  await picker.getByRole('button', { name: 'Apply Selected Guidance' }).click();
  await expect.poll(() => actions.some((action) => action.action === 'apply')).toBe(true);
  await expect(picker).not.toContainText('still need Apply Selected Guidance');
  await picker
    .locator('[data-guidance-updates]')
    .screenshot({ path: 'target/visual-captures/guidance-update-applied-phone.png' });
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
  await expect(picker.locator('[data-guidance-review]')).toContainText('The source is current.');
  await picker.getByRole('button', { name: 'Check Now' }).click();
  await expect(picker).toContainText('Source Check Failed');
  await expect(picker).toContainText('Previous results may be stale');
  await expect(picker).toContainText('ai-playbook: source is current');
  expect(updateChecks).toBe(checksBeforeReload + 3);
  await picker
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
  await picker.locator('input[name="guidance-search"]').fill('rust');
  await expect(picker.locator('[data-guidance-rule="rust-lint-and-format"]')).toBeVisible();
  await expect(picker.locator('[data-guidance-rule="hotsheet-visual-qa"]')).toBeHidden();
  await picker.locator('input[name="guidance-search"]').fill('');
  const ownerFilter = picker.locator('kui-select[name="guidance-owner-filter"]');
  await ownerFilter.locator('.kui-select__trigger').click();
  await ownerFilter.locator('.kui-select__option[data-value="rust"]').click();
  await expect(picker.locator('[data-guidance-rule="hotsheet-visual-qa"]')).toBeHidden();
  await ownerFilter.locator('.kui-select__trigger').click();
  await ownerFilter.locator('.kui-select__option[data-value=""]').click();
  await picker.locator('[data-guidance-rule="rust-lint-and-format"] input[type="checkbox"]').check();
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
  const overflow = await picker.evaluate((element) => element.scrollWidth - element.clientWidth);
  expect(overflow).toBeLessThanOrEqual(1);
  await picker.getByRole('heading', { name: 'Project guidance' }).scrollIntoViewIfNeeded();
  const heading = await picker.getByRole('heading', { name: 'Project guidance' }).boundingBox();
  expect(heading?.x).toBeGreaterThanOrEqual(0);
  expect((heading?.x ?? 0) + (heading?.width ?? 0)).toBeLessThanOrEqual(390);
  await page.screenshot({ path: 'target/visual-captures/guidance-phone-top.png', fullPage: true });
  await picker.getByRole('button', { name: 'Apply Selected Guidance' }).scrollIntoViewIfNeeded();
  await page.screenshot({ path: 'target/visual-captures/guidance-phone-page.png', fullPage: true });
  await picker.getByRole('heading', { name: 'Project guidance' }).scrollIntoViewIfNeeded();
  await page.screenshot({ path: 'target/visual-captures/guidance-phone-top-settled.png', fullPage: true });
  await visualRule.scrollIntoViewIfNeeded();
  await page.screenshot({ path: 'target/visual-captures/guidance-rules-phone.png' });
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
