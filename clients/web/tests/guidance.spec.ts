import { expect, test } from '@playwright/test';

import { openDemoProject } from './mobile-project-fixture';

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
  const picker = page.locator('[data-component="guidance-settings"]');
  await expect(picker).toContainText('installed v1');
  await expect(picker).toContainText('available v2');
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
});
