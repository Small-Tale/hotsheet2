import { expect, test } from '@playwright/test';

import { openDemoProject } from './mobile-project-fixture';

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
    route.fulfill({ json: { owner: 'rulesync', explicit_selection: false, default_repository: '', sources: [] } }),
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
    await expect(picker.getByText('Rulesync Lock Has Updates')).toBeVisible();
    await expect(picker).toContainText('Rulesync 38.0.0: outdated source locks');
    await expect(picker).toContainText('locked fcb8556bef14');
    await expect(picker).toContainText('observed 7cb9499a655a');
    await expect(picker).not.toContainText('Pinned to');
    expect(await picker.evaluate((element) => element.scrollWidth - element.clientWidth)).toBeLessThanOrEqual(1);
    await picker.screenshot({ path: `target/visual-captures/rulesync-outdated-${width}.png` });
  }
});
