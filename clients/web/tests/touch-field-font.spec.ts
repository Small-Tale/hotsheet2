import { expect, type Page, test } from '@playwright/test';

import { openDemoProject } from './mobile-project-fixture';

// HS2-8JPRRC: iOS zooms the page into any focused text field set below 16px, so touch screens keep
// every native text field at least that size while desktop keeps the compact type.

/** Computed font size (px) of every element matching `selector`, reading a custom element's inner field. */
const fontSizes = (page: Page, selector: string) =>
  page.evaluate(
    (sel) =>
      [...document.querySelectorAll(sel)].map((element) => {
        const field = (element as HTMLElement).shadowRoot?.querySelector('input, textarea') ?? element;
        return parseFloat(getComputedStyle(field).fontSize);
      }),
    selector,
  );

/** Open every field state the composer, inspector, attachments, and provider form demos expose. */
async function collect(page: Page) {
  const sizes: Record<string, number[]> = {};
  await page.goto('/ux-demo?component=quick-ticket-composer');
  await page.locator('[data-component="quick-ticket-composer-launcher"]').first().click();
  await expect(page.locator('[name="new-ticket-details"]')).toBeVisible();
  sizes.details = await fontSizes(page, '[name="new-ticket-details"]');
  await page.goto('/ux-demo?component=ticket-inspector');
  const inspector = page.locator('[data-component="ticket-inspector"]');
  await inspector.getByRole('button', { name: 'Edit Ticket details' }).getByRole('heading').first().click();
  await expect(page.locator('[name="markdown-source"]')).toBeVisible();
  sizes.markdown = await fontSizes(page, '[name="markdown-source"]');
  await page.goto('/ux-demo?component=ticket-inspector');
  await page.locator('[data-component="ticket-inspector"]').getByRole('button', { name: 'Add tag' }).click();
  await expect(page.locator('[name="ticket-tag-input"]')).toBeVisible();
  sizes.tag = await fontSizes(page, '[name="ticket-tag-input"]');
  await page.goto('/ux-demo?component=ticket-attachments');
  // The purpose pill is a Kerf Select (no text entry); the batch label editor is the text field (HS2-K9HSXZ).
  await page.locator('[data-action="edit-attachment-batch-label"]').first().click();
  await expect(page.locator('wa-input[name="attachment-batch-label"]:not([hidden])').first()).toBeVisible();
  sizes.batchLabel = await fontSizes(page, 'wa-input[name="attachment-batch-label"]');
  await page.goto('/ux-demo?component=provider-setup-form');
  // The repository field is a Kerf Combobox, which keeps its own touch-safe size (HS2-EN1E76).
  await expect(page.locator('input[name="connection-locator"]')).toBeVisible();
  sizes.locator = await fontSizes(page, 'input[name="connection-locator"]');
  return sizes;
}

test.describe('touch phone', () => {
  test.use({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });
  test('keeps native text fields at 16px or larger so iOS never zooms on focus', async ({ page }) => {
    const sizes = await collect(page);
    for (const [field, values] of Object.entries(sizes)) {
      expect(values.length, field).toBeGreaterThan(0);
      for (const value of values) expect(value, field).toBeGreaterThanOrEqual(16);
    }
    // The purpose pill grows with its text instead of clipping it.
    const pill = page.locator('[name="attachment-batch-purpose"]').first();
    await page.goto('/ux-demo?component=ticket-attachments');
    const box = (await pill.boundingBox())!;
    expect(box.height).toBeGreaterThanOrEqual(
      await pill.evaluate((node) => parseFloat(getComputedStyle(node).lineHeight) || 16),
    );
    await page.screenshot({ path: test.info().outputPath('hs2-8jprrc-attachments-390.png') });
  });
});

/**
 * Every editable native text field in the page, including inside shadow roots, whose computed font
 * size is under 16px. Read-only fields never raise the keyboard, and Web Awesome's hidden form-value
 * mirrors (`aria-hidden`, `tabindex=-1`) never take focus, so neither can trigger iOS focus zoom.
 */
const smallEditableFields = (page: Page) =>
  page.evaluate(() => {
    const found: string[] = [];
    const nonText = new Set([
      'checkbox',
      'radio',
      'range',
      'color',
      'file',
      'button',
      'submit',
      'reset',
      'hidden',
      'image',
    ]);
    const walk = (root: Document | ShadowRoot, host: string) => {
      for (const element of root.querySelectorAll<HTMLElement>('*')) {
        if (element.shadowRoot) walk(element.shadowRoot, element.localName);
        const field = element as HTMLInputElement;
        const editable =
          element.isContentEditable ||
          element.localName === 'textarea' ||
          element.localName === 'select' ||
          (element.localName === 'input' && !nonText.has(field.type));
        if (!editable || field.readOnly || field.disabled) continue;
        if (element.getAttribute('aria-hidden') === 'true' && element.tabIndex < 0) continue;
        // xterm's input proxy is offscreen and sized by the terminal, not a typed text field.
        if (element.classList.contains('xterm-helper-textarea')) continue;
        const size = parseFloat(getComputedStyle(element).fontSize);
        if (size < 16) found.push(`${host} ${element.localName}[name=${element.getAttribute('name') ?? ''}] ${size}px`);
      }
    };
    walk(document, 'document');
    return found;
  });

test.describe('touch phone audit (HS2-JDXSND)', () => {
  test.use({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });
  test('every UX demo keeps its editable text fields at 16px or larger', async ({ page }) => {
    test.setTimeout(300_000);
    const small: string[] = [],
      pageErrors: string[] = [];
    page.on('pageerror', (error) => pageErrors.push(error.message));
    await page.goto('/ux-demo');
    // The Vite dev server serves the catalog module, so the walk always covers every listed demo.
    const demos = await page.evaluate(async (module) => {
      const catalog = (await import(module)) as { flattenCatalog: () => Array<{ id: string; name: string }> };
      return catalog.flattenCatalog().map(({ id, name }) => ({ id, name }));
    }, '/src/ux-demo/catalog.ts');
    expect(demos.length).toBeGreaterThan(50);
    // Boot the catalog once and switch entries in the page, as its sidebar does (HS2-Z5EE35): a full
    // document load per entry re-fetched the whole unbundled source graph about 110 times from the
    // shared dev server, and on a busy CI shard one such boot never mounted the catalog.
    for (const { id, name } of demos) {
      await page.evaluate(
        async ([module, demo]) => {
          const state = (await import(module)) as { selectDemo: (id: string) => void };
          state.selectDemo(demo);
        },
        ['/src/ux-demo/components/demo-state.ts', id] as const,
      );
      await expect(
        page.locator(
          `[data-catalog-entry-name=${JSON.stringify(name)}] [aria-current="page"], [data-catalog-entry-name=${JSON.stringify(name)}][aria-current="page"]`,
        ),
        id,
      ).toBeAttached();
      await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
      expect(new URL(page.url()).searchParams.get('component'), id).toBe(id);
      small.push(...(await smallEditableFields(page)).map((field) => `${id}: ${field}`));
    }
    expect(small).toEqual([]);
    expect(pageErrors).toEqual([]);
  });

  test('the Not working note stays at 16px when focused', async ({ page }) => {
    await page.goto('/ux-demo?component=not-working-dialog');
    const note = page.locator('textarea[name="not-working-note"]').first();
    await page.locator('[data-action="open-not-working-demo"]').tap();
    await expect(note).toBeVisible();
    await note.tap();
    expect(await note.evaluate((element) => parseFloat(getComputedStyle(element).fontSize))).toBeGreaterThanOrEqual(16);
    await page.screenshot({ path: test.info().outputPath('hs2-jdxsnd-not-working-390.png') });
  });
});

/**
 * Fields that only render after an interaction in the composed app (HS2-R2H1BA): walk the whole DOM,
 * hidden dialogs included, after each step that opens a surface, and collect every small field.
 */
async function auditAppInteractions(page: Page, phone: boolean) {
  const small: string[] = [],
    audit = async (step: string) => {
      small.push(...(await smallEditableFields(page)).map((field) => `${step}: ${field}`));
    },
    dismiss = () => page.keyboard.press('Escape');
  await page.goto('/');
  await page.getByRole('button', { name: 'Open Project' }).click();
  await expect(page.getByRole('textbox', { name: /^Project Folder/ })).toBeVisible();
  await audit('project dialog');
  await openDemoProject(page);
  await expect(page.locator('[data-component="app-shell"]')).toBeVisible();
  await audit('project');
  if (phone) {
    await page.getByRole('button', { name: 'Show Project Sidebar' }).click();
    await audit('phone sidebar');
    await dismiss();
    await page.getByRole('button', { name: 'Show Ticket Inspector' }).click();
    await audit('phone inspector');
    return small;
  }
  await page.getByRole('button', { name: 'Search tickets' }).click();
  await audit('search');
  await dismiss();
  await page.getByRole('button', { name: 'New ticket…' }).click();
  await audit('new ticket');
  await dismiss();
  await page.getByRole('button', { name: 'Add view' }).click();
  await audit('add view');
  await dismiss();
  await page.getByLabel('HS2-M1: HS2-M1').first().click();
  const rail = page.locator('#app-right-rail');
  for (const action of ['Add Note', 'Add tag', 'Block ticket']) {
    await rail.getByRole('button', { name: action }).first().click();
    await audit(`inspector ${action}`);
    await dismiss();
  }
  await page.getByRole('button', { name: 'Show Terminal Drawer' }).click();
  await audit('terminal drawer');
  await page.getByLabel('Settings View').click();
  await expect(page.locator('[data-component="settings-workspace"]')).toBeVisible();
  await audit('settings');
  return small;
}

test.describe('touch app interactions (HS2-R2H1BA)', () => {
  for (const [label, viewport] of [
    ['phone', { width: 390, height: 844 }],
    ['tablet', { width: 1180, height: 820 }],
  ] as const)
    test.describe(label, () => {
      test.use({ viewport, hasTouch: true, isMobile: true });
      test(`keeps fields the real app opens on interaction at 16px or larger on a touch ${label}`, async ({ page }) => {
        expect(await auditAppInteractions(page, label === 'phone')).toEqual([]);
        await page.screenshot({ path: test.info().outputPath(`hs2-r2h1ba-${label}.png`) });
      });
    });
});

/**
 * Saved commands, a custom view, and two installed AI tools, in the `GET /commands`, `GET /views`,
 * `GET /ai-tools`, and `GET /ai-settings` response shapes.
 */
const POPULATED = {
  commands: [
    { id: 'cmd-tests', title: 'Run tests', kind: 'shell', command: 'npm test', group: 'Build' },
    { id: 'cmd-review', title: 'Review diff', kind: 'ai', prompt: 'Review the staged diff.', group: 'AI' },
  ],
  views: [{ id: 'view-mine', name: 'Mine', query: 'status:started' }],
  aiTools: [
    {
      id: 'codex',
      display_name: 'Codex',
      models: [
        { id: 'gpt-6-astra', label: 'GPT-6 Astra', effort_levels: ['low', 'medium', 'high'] },
        { id: 'gpt-5.5', label: 'GPT-5.5', effort_levels: ['low', 'medium', 'high'] },
      ],
      default_model: 'gpt-6-astra',
      default_effort: 'medium',
      actions: ['change_model', 'change_effort'],
    },
    {
      id: 'claude',
      display_name: 'Claude',
      models: [
        { id: 'opus', label: 'Opus', effort_levels: ['low', 'medium', 'high'] },
        { id: 'sonnet', label: 'Sonnet', effort_levels: ['low', 'medium', 'high'] },
      ],
      default_model: 'sonnet',
      default_effort: 'medium',
      actions: ['change_model', 'change_effort'],
    },
  ],
  aiSettings: { tool: 'codex', model: 'gpt-6-astra', effort: 'medium' },
};

/** Close the open dialog and wait until no dialog in the page is still open. */
async function closeDialog(page: Page) {
  await page.keyboard.press('Escape');
  await expect.poll(() => page.locator('dialog[open]').count()).toBe(0);
}

/**
 * Populated surfaces (HS2-NYGR2Q, HS2-ME1XBC): every settings category with saved commands and
 * installed AI tools, the AI tools "Other…" model field, each non-git provider connection form, the
 * saved-view and terminal renames, and the repository setup remote field, each walked with the
 * hidden-dialog-inclusive field audit. `shot` names the screenshot of each loaded surface.
 */
async function auditPopulatedSurfaces(page: Page, label: 'phone' | 'tablet') {
  const small: string[] = [],
    phone = label === 'phone',
    audit = async (step: string) => {
      small.push(...(await smallEditableFields(page)).map((field) => `${step}: ${field}`));
    },
    shot = (name: string) => page.screenshot({ path: test.info().outputPath(`hs2-me1xbc-${name}-${label}.png`) }),
    sidebar = async () => {
      const show = page.getByRole('button', { name: 'Show Project Sidebar' });
      if (phone && (await show.isVisible())) await show.click();
    };
  await openDemoProject(page, true, 1, undefined, undefined, { ...POPULATED, uninitializedRepository: true });
  await expect(page.locator('[data-component="app-shell"]')).toBeVisible();
  await audit('populated project');

  // Repository setup: the not-a-repository state, then the remote URL field after initializing.
  await sidebar();
  await page.locator('[data-action="open-repository-status"]').first().click();
  const repository = page.locator('[data-repository-surface="repository-status"]');
  await expect(repository).toHaveAttribute('data-state', 'uninitialized');
  await repository.getByRole('button', { name: 'Initialize Git repository' }).click();
  await expect(repository.getByRole('textbox', { name: 'Remote URL' })).toBeVisible();
  await audit('repository remote');
  await repository.getByRole('textbox', { name: 'Remote URL' }).fill('git@example.com:small-tale/demo.git');
  await shot('repository-remote');
  await page.keyboard.press('Escape');
  await expect(repository).toBeHidden();

  await sidebar();
  await page.getByRole('button', { name: 'More Actions for Mine' }).click();
  await page.locator('[data-context-menu="saved-view"]').getByText('Edit View…').click();
  await expect(page.getByRole('textbox', { name: 'View Name' })).toHaveValue('Mine');
  await audit('saved view rename');
  await page.keyboard.press('Escape');
  await expect(page.getByRole('textbox', { name: 'View Name' })).toBeHidden();
  if (phone) await page.getByRole('button', { name: 'Hide Project Sidebar' }).click();
  await page.getByRole('button', { name: 'Show Terminal Drawer' }).click();
  const drawer = page.locator('[data-component="terminal-drawer"]');
  await drawer.getByRole('tab', { name: /Codex Main/ }).click({ button: 'right' });
  await page.locator('[data-context-menu="app-tab"]').getByText('Rename…').click();
  const rename = page.locator('[data-terminal-rename-dialog]');
  await expect(rename).toHaveJSProperty('open', true);
  await audit('terminal rename');
  await page.keyboard.press('Escape');
  await expect(rename).toHaveJSProperty('open', false);
  if (phone) await page.getByRole('button', { name: 'Hide Terminal Drawer' }).click();
  await page.getByLabel('Settings View').click();
  const workspace = page.locator('[data-component="settings-workspace"]');
  await expect(workspace).toBeVisible();
  for (const category of [
    'Ticket sources',
    'AI tools',
    'Commands',
    'Lifecycle',
    'Terminals',
    'Permissions',
    'Column view',
    'General',
    'Accounts',
    'Keyboard',
  ]) {
    // The phone keeps the settings categories in a sidebar overlay.
    const settingsSidebar = page.getByRole('button', { name: 'Show Settings Sidebar' });
    if (phone && (await settingsSidebar.isVisible())) await settingsSidebar.click();
    await page.getByRole('button', { name: category, exact: true }).first().click();
    if (phone) await page.getByRole('button', { name: 'Hide Settings Sidebar' }).click();
    await audit(`settings ${category}`);
    if (category === 'Ticket sources') {
      await page.getByRole('button', { name: 'Edit Hot Sheet Git' }).click();
      await expect(page.getByRole('textbox', { name: 'Location' })).toHaveValue('/tickets');
      await audit('ticket source form');
      await page.keyboard.press('Escape');
      for (const provider of ['GitHub Issues', 'GitLab Issues', 'Jira Cloud']) {
        await page.getByRole('button', { name: 'Data Source…', exact: true }).click();
        await page.getByRole('button', { name: `Connect ${provider}` }).click();
        const form = page.locator('[data-component="provider-setup-form"]').last();
        await expect(form).toBeVisible();
        await audit(`${provider} connection form`);
        const locator = form.locator('input[name="connection-locator"]');
        if (await locator.isVisible()) {
          await locator.fill(provider === 'Jira Cloud' ? 'HS' : 'small-tale/demo');
          await locator.tap();
        }
        // Let the form's slide-in transition settle so the screenshot shows the finished state.
        await page.waitForTimeout(400);
        await shot(`${provider.split(' ')[0].toLowerCase()}-form`);
        await closeDialog(page);
      }
    }
    if (category === 'AI tools') {
      const settings = page.locator('[data-component="ai-tool-settings"]'),
        model = settings.locator('wa-select[name="ai-provider-model-codex"]');
      await expect(model).toHaveAttribute('value', 'gpt-6-astra');
      await shot('ai-tools');
      await model.click();
      await model.getByText('Other…').click();
      const field = page.getByRole('textbox', { name: /Model Identifier/ });
      await expect(field).toBeVisible();
      await field.fill('codex-preview');
      await audit('AI tools other model');
      // Let the dialog's open transition settle so the screenshot shows the finished state.
      await page.waitForTimeout(400);
      await shot('ai-other-model');
      await page.locator('[data-component="manual-model-dialog"]').getByRole('button', { name: 'Cancel' }).click();
      await expect.poll(() => page.locator('dialog[open]').count()).toBe(0);
    }
    if (category === 'Commands') {
      await expect(page.getByRole('list', { name: 'Custom commands' })).toContainText('Run tests');
      const row = page.locator('[data-component="command-settings-editor"] .command-settings-editor__row').first();
      await row.locator('[data-component="popup-menu"] [slot="trigger"]').click();
      await row.locator('[data-action="edit-command-setting"]').dispatchEvent('click');
      await expect(page.locator('#command-editor-dialog').getByLabel('Button Label')).toHaveValue('Run tests');
      await audit('command editor');
      await page.screenshot({ path: test.info().outputPath(`hs2-nygr2q-command-editor-${label}.png`) });
      await page.keyboard.press('Escape');
    }
  }
  return small;
}

test.describe('touch populated surfaces (HS2-NYGR2Q, HS2-ME1XBC)', () => {
  for (const [label, viewport] of [
    ['phone', { width: 390, height: 844 }],
    ['tablet', { width: 1180, height: 820 }],
  ] as const)
    test.describe(label, () => {
      test.use({ viewport, hasTouch: true, isMobile: true });
      test(`keeps populated settings editors, provider forms, and rename fields at 16px or larger on a touch ${label}`, async ({
        page,
      }) => {
        test.setTimeout(120_000);
        expect(await auditPopulatedSurfaces(page, label)).toEqual([]);
        await page.screenshot({ path: test.info().outputPath(`hs2-nygr2q-${label}.png`) });
      });
    });
});

test.describe('desktop', () => {
  test.use({ viewport: { width: 1280, height: 900 }, hasTouch: false, isMobile: false });
  test('keeps the compact field type with a mouse', async ({ page }) => {
    const sizes = await collect(page);
    expect(sizes.details).toEqual([14]);
    expect(sizes.markdown).toEqual([14]);
    expect(sizes.tag).toEqual([12]);
    // The purpose pill is a Kerf Select now; the batch label editor is the attachment text field (HS2-K9HSXZ).
    expect(new Set(sizes.batchLabel)).toEqual(new Set([14]));
    // The repository field is a Kerf Combobox at its compact 14px desktop size (HS2-EN1E76).
    expect(new Set(sizes.locator)).toEqual(new Set([14]));
  });
  test('keeps the compact Not working note with a mouse (HS2-JDXSND)', async ({ page }) => {
    await page.goto('/ux-demo?component=not-working-dialog');
    await page.locator('[data-action="open-not-working-demo"]').click();
    const note = page.locator('textarea[name="not-working-note"]').first();
    await expect(note).toBeVisible();
    expect(await note.evaluate((element) => parseFloat(getComputedStyle(element).fontSize))).toBe(14);
    await page.screenshot({ path: test.info().outputPath('hs2-jdxsnd-not-working-1280.png') });
  });
});
