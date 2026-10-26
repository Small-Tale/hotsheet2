import { expect, type Locator, type Page, test } from '@playwright/test';

import type { FullTicket } from '../src/api';
import { desktopApp } from './desktop-draft-files-fixture';
import { expectProjectSidebarLayout } from './project-sidebar-layout';

const tools = [
  {
    id: 'fixture',
    display_name: 'Fixture AI',
    models: [{ id: 'model-one', label: 'Model One', effort_levels: ['low', 'high'] }],
    default_model: 'model-one',
    default_effort: 'low',
  },
];

async function openedApp(context: Parameters<typeof desktopApp>[0], baseURL: string) {
  const app = await desktopApp(context, baseURL);
  // Only external AI discovery is synthetic; all ticket, attachment and command storage is real.
  await context.route('**/ai-tools*', (route) => route.fulfill({ json: tools }));
  await context.route('**/ai-settings', (route) =>
    route.fulfill({ json: { tool: 'fixture', model: 'model-one', effort: 'low' } }),
  );
  const page = await app.launch();
  page.on('dialog', (dialog) => {
    throw new Error(`Unexpected native dialog: ${dialog.type()}`);
  });
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
  return { app, page };
}

async function settledDialog(dialog: Locator) {
  await expect(dialog).toBeVisible();
  await expect(dialog).toHaveAttribute('open', '');
  await expect(dialog).not.toHaveAttribute('data-kui-dialog-motion', /.+/);
}

async function captureWidths(page: Page, surface: Locator, name: string) {
  for (const scheme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme: scheme });
    for (const [state, width] of [
      ['wide', 1440],
      ['medium', 1100],
      ['narrow', 390],
      ['wide-return', 1440],
    ] as const) {
      await page.setViewportSize({ width, height: 900 });
      await expectProjectSidebarLayout(page, width >= 1024);
      await expect(surface).toBeVisible();
      await expect
        .poll(async () => {
          const box = await surface.boundingBox();
          return Boolean(box && box.x >= 0 && box.x + box.width <= width && box.width > 0);
        })
        .toBe(true);
      // Layout readiness alone does not settle media-query color transitions. Read the actual
      // controls to flush styles, then await finite motion rather than delaying or forcing state.
      await surface.evaluate(async (owner) => {
        for (const control of owner.querySelectorAll('button, input'))
          getComputedStyle(control).getPropertyValue('color');
        const motion = owner
          .getAnimations({ subtree: true })
          .filter((animation) => Number.isFinite(animation.effect?.getComputedTiming().endTime));
        await Promise.all(motion.map((animation) => animation.finished.catch(() => undefined)));
      });
      await page.screenshot({ path: test.info().outputPath(`hs2-wmz6z5-${name}-${scheme}-${state}.png`) });
    }
  }
  await page.emulateMedia({ colorScheme: 'light' });
}

test.describe('production prompt replacements (HS2-WMZ6Z5)', () => {
  test.setTimeout(120_000);

  test('renames inline in the real ticket reader with cancel, blank, failed-save retry and later edit @ci-smoke @ci-webkit', async ({
    context,
    baseURL,
  }) => {
    const { app, page } = await openedApp(context, baseURL!);
    try {
      const ticket = await app.server.request<FullTicket>(`/checkouts/${app.server.checkoutId}/tickets`, 'POST', {
        title: 'Synthetic attachment rename',
        category: 'task',
      });
      await page.reload();
      await page.locator(`[data-ticket-slug="${ticket.slug}"]`).first().dblclick();
      const reader = page.locator('[data-component="ticket-reader"][data-reader-active="true"]');
      await expect(reader).toBeVisible();
      await reader.getByRole('tab', { name: 'Attachments', exact: true }).click();
      await reader
        .getByLabel('Browse and add attachments', { exact: true })
        .setInputFiles({ name: 'original.txt', mimeType: 'text/plain', buffer: Buffer.from('Synthetic proof') });
      const row = reader.locator('[data-component="ticket-attachment-item"]').first();
      await expect(row).toContainText('original.txt');
      const rename = async () => {
        await row.getByRole('button', { name: /^More Actions/ }).click();
        await reader.getByRole('menuitem', { name: 'Rename', exact: true }).click();
        const field = row.getByRole('textbox', { name: /^Filename/ });
        await expect(field).toBeVisible();
        await expect(field).toBeFocused();
        return field;
      };
      let field = await rename();
      await expect(field).toHaveValue('original.txt');
      await field.fill('discard.txt');
      await field.press('Escape');
      await expect(field).toBeHidden();
      await expect(row.getByRole('button', { name: /^More Actions/ })).toBeFocused();
      await expect(row).toContainText('original.txt');
      field = await rename();
      await field.fill('   ');
      await field.press('Enter');
      await expect(field).toBeHidden();
      await expect(row).toContainText('original.txt');
      app.failApiWhen((method, path) => method === 'PATCH' && path.includes('/attachments/'));
      field = await rename();
      await field.fill('renamed-proof.txt');
      await field.press('Enter');
      await expect(row).toContainText('Synthetic operation rejected.');
      await expect(field).toHaveValue('renamed-proof.txt');
      app.failApiWhen(() => false);
      await field.focus();
      await field.press('Enter');
      await expect(field).toBeHidden();
      await expect(row).toContainText('renamed-proof.txt');
      field = await rename();
      await expect(field).toHaveValue('renamed-proof.txt');
      await field.fill('later-edit.txt');
      await field.press('Tab');
      await expect(row).toContainText('later-edit.txt');
      field = await rename();
      await field.fill('wide-and-narrow-draft.txt');
      await captureWidths(page, reader, 'filename');
      await expect(field).toHaveValue('wide-and-narrow-draft.txt');
      await field.press('Escape');
      const stored = await app.server.request<FullTicket>(
        `/checkouts/${app.server.checkoutId}/tickets/${encodeURIComponent(ticket.qualified_id)}`,
      );
      expect(stored.attachments[0].filename).toBe('later-edit.txt');
      // The same editor also ships through the sidebar inspector, outside the reader's modality.
      await reader.getByRole('button', { name: 'Close Ticket Reader', exact: true }).click();
      await expect(reader).toHaveCount(0);
      const inspector = page.locator('#app-right-rail');
      await inspector.getByRole('tab', { name: 'Attachments', exact: true }).click();
      const inspectorRow = inspector.locator('[data-component="ticket-attachment-item"]').first();
      await inspectorRow.getByRole('button', { name: /^More Actions/ }).click();
      await page.getByRole('menuitem', { name: 'Rename', exact: true }).click();
      const inspectorField = inspectorRow.getByRole('textbox', { name: /^Filename/ });
      await expect(inspectorField).toBeFocused();
      await expect(inspectorField).toHaveValue('later-edit.txt');
      await inspectorField.fill('inspector-edit.txt');
      await inspectorField.press('Enter');
      await expect(inspectorField).toBeHidden();
      await expect(inspectorRow).toContainText('inspector-edit.txt');
    } finally {
      await page.close();
      await app.stop();
    }
  });

  test('creates groups through real command settings, retains validation, resets on cancel and recovers failed writes @ci-smoke @ci-webkit', async ({
    context,
    baseURL,
  }) => {
    const { app, page } = await openedApp(context, baseURL!);
    try {
      await page.getByLabel('Settings View').click();
      await page.getByRole('button', { name: 'Commands', exact: true }).click();
      const open = () => page.getByRole('button', { name: 'Add group…', exact: true }).click();
      const dialog = page.getByRole('dialog', { name: 'New Command Group' });
      const name = dialog.getByRole('textbox', { name: 'Group Name' });
      await open();
      await settledDialog(dialog);
      await expect(name).toBeFocused();
      await name.press('Enter');
      await expect(dialog).toContainText('Enter a group name.');
      await name.fill('Discarded');
      await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
      await expect(dialog).toHaveCount(0);
      await expect(page.getByRole('button', { name: 'Add group…', exact: true })).toBeFocused();
      await open();
      await expect(name).toHaveValue('');
      await name.fill('Ideas');
      await name.press('Enter');
      const groups = () => app.server.request<string[]>(`/checkouts/${app.server.checkoutId}/command-groups`);
      await expect.poll(groups).toEqual(['Ideas']);
      await open();
      await name.fill('Ideas');
      await name.press('Enter');
      await expect(dialog).toContainText('already exists');
      await name.fill('New Group');
      await settledDialog(dialog);
      await expect(dialog.getByRole('button', { name: 'Create Group', exact: true })).toBeEnabled();
      await captureWidths(page, dialog, 'command-group');
      await name.press('Escape');
      await expect(dialog).toHaveCount(0);
      app.failApiWhen((method, path) => method === 'PUT' && path.endsWith('/command-groups'));
      await open();
      await name.fill('Pending');
      await name.press('Enter');
      await expect(page.locator('[data-component="command-settings-editor"]')).toContainText(
        'Synthetic operation rejected.',
      );
      app.failApiWhen(() => false);
      await open();
      await name.fill('Later');
      await name.press('Enter');
      await expect.poll(groups).toEqual(['Ideas', 'Pending', 'Later']);
      await expect(page.locator('[data-component="command-settings-editor"]')).not.toContainText(
        'Synthetic operation rejected.',
      );
    } finally {
      await page.close();
      await app.stop();
    }
  });

  test('customizes a real drawer launch with whole-dialog cancel, defaults, validation and a subsequent launch @ci-smoke @ci-webkit', async ({
    context,
    baseURL,
  }) => {
    const { app, page } = await openedApp(context, baseURL!);
    const created: unknown[] = [];
    try {
      // Never launch a real AI tool: the real drawer reaches its existing terminal API boundary.
      await context.route('**/terminals', async (route) => {
        if (route.request().method() !== 'POST') return route.fallback();
        created.push(route.request().postDataJSON());
        await route.fulfill({ status: 500, json: { error: 'Synthetic launch refused.' } });
      });
      await page.getByRole('button', { name: 'Show Terminal Drawer' }).click();
      const drawer = page.locator('[data-component="terminal-drawer"]');
      const open = async () => {
        await drawer.getByRole('button', { name: 'New Drawer Item' }).click();
        await drawer
          .locator('[data-action="create-terminal-drawer-item"][data-item-id="ai-shell"]')
          .click({ modifiers: ['Alt'] });
      };
      const dialog = page.getByRole('dialog', { name: 'New AI Terminal' });
      const provider = dialog.getByRole('textbox', { name: 'AI Provider' });
      const model = dialog.getByRole('textbox', { name: 'Model', exact: true });
      await open();
      await settledDialog(dialog);
      await expect(provider).toBeFocused();
      await expect(model).toHaveValue('model-one');
      const effort = dialog.locator('[name="launch-effort"]');
      await expect(effort).toHaveJSProperty('value', 'low');
      await model.fill('discarded');
      await expect(dialog.getByRole('combobox', { name: 'Effort Level' })).toBeDisabled();
      await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
      expect(created).toHaveLength(0);
      await expect(drawer.getByRole('button', { name: 'New Drawer Item' })).toBeFocused();
      await open();
      await expect(model).toHaveValue('model-one');
      await provider.fill('unknown');
      await dialog.getByRole('button', { name: 'Create Terminal', exact: true }).click();
      await expect(dialog).toContainText('Unknown AI provider: unknown');
      expect(created).toHaveLength(0);
      await provider.fill('Fixture AI');
      await expect(effort).toHaveJSProperty('value', 'low');
      await model.fill('custom-temporary');
      await expect(effort).toHaveJSProperty('value', '__provider-default__');
      await expect(dialog.getByRole('combobox', { name: 'Effort Level' })).toBeDisabled();
      await model.fill('model-one');
      await expect(dialog.getByRole('combobox', { name: 'Effort Level' })).toBeEnabled();
      await dialog.getByRole('combobox', { name: 'Effort Level' }).click();
      await page.getByRole('option', { name: 'high', exact: true }).click();
      await expect(effort).toHaveJSProperty('value', 'high');
      await model.fill('');
      await dialog.getByRole('combobox', { name: 'Effort Level' }).click();
      await page.getByRole('option', { name: 'Provider Default', exact: true }).click();
      await expect(effort).toHaveJSProperty('value', '__provider-default__');
      await settledDialog(dialog);
      await captureWidths(page, dialog, 'ai-launch');
      await dialog.getByRole('button', { name: 'Create Terminal', exact: true }).click();
      await expect.poll(() => created).toHaveLength(1);
      expect(created[0]).toMatchObject({ connect: 'fixture', model: 'model-one', effort: 'low' });
      await expect(drawer).toContainText('Synthetic launch refused.');
      await open();
      await model.fill('custom-exact-id');
      await dialog.getByRole('button', { name: 'Create Terminal', exact: true }).click();
      await expect.poll(() => created).toHaveLength(2);
      expect(created[1]).toMatchObject({ connect: 'fixture', model: 'custom-exact-id' });
      expect(created[1]).not.toHaveProperty('effort');
    } finally {
      await page.close();
      await app.stop();
    }
  });
});
