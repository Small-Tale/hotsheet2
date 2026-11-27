import { expect, type Locator, type Page, test } from '@playwright/test';

import {
  capture,
  openOrchestration,
  orchestrateSelf,
  realTicketServer,
  resizeTo,
  scheduler,
  send,
  type Server,
  TARGET,
  type Ticket,
  visualPass,
} from './orchestration-real-fixtures';

/**
 * HS2-36XDDX: a template's Wait for Accept through the real shell against a real server. The editor's
 * Template part turns the setting off and on and saves it through `templates/set`; under Auto placement the
 * step a template's goal planned reads as held on its card until a person presses Accept Plan in the
 * inspector, which accepts through `templates/accept`. The project orchestrates itself ("Self"). Synthetic
 * data only.
 */
const person = { role: 'human', id: 'maintainer' };
const WAITING = 'Waiting for someone to accept the template’s plan';
const CARD_WAITING = 'Waits for the plan to be accepted';

const templatesBase = (server: Server) => `/checkouts/${server.checkoutId}/orchestration/templates`;
const listed = async (server: Server) =>
  (await send(server, 'POST', `${templatesBase(server)}/list`, { actor: person })) as {
    templates: Array<Record<string, unknown>>;
  };
const toast = (page: Page, text: string) => page.locator('.app-toast').filter({ hasText: text });
const editorOf = (page: Page) => page.locator('dialog[data-dialog-component="orchestration-template-editor"]');
const fieldOf = (editor: Locator, name: string) => editor.locator(`[data-template-field="${name}"]`);

async function defineTemplate(server: Server, name: string, extra: Record<string, unknown> = {}) {
  const set = await send(server, 'POST', `${templatesBase(server)}/set`, {
    templates: [
      {
        name,
        goal: { title: `${name} goal` },
        steps: [{ key: 'work', target: TARGET, title: `${name} work` }],
        ...extra,
      },
    ],
    actor: person,
  });
  return (set.templates as Array<{ template_id: string; name: string }>).find((known) => known.name === name)!
    .template_id;
}

test('HS2-36XDDX the editor turns Wait for Accept off and on, and saves it', async ({ page }) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    await orchestrateSelf(server);
    const templateId = await defineTemplate(server, 'Repair the release');
    // A template written without the setting waits (the default) and leaves the field out.
    expect((await listed(server)).templates[0]).not.toHaveProperty('wait_for_accept');
    await resizeTo(page, { width: 1440, height: 900 });
    await openOrchestration(page, server);
    await page.getByLabel('Settings View').click();
    await page
      .getByRole('navigation', { name: 'Project Settings' })
      .getByRole('button', { name: 'Orchestration', exact: true })
      .click();
    const section = page.locator('[data-component="orchestration-template-settings"]');
    const row = section.locator(`[data-orchestration-template="${templateId}"]`);
    await row.locator('button[data-item-id]').click();
    const editor = editorOf(page);
    const toggle = fieldOf(editor, 'wait_for_accept').getByRole('checkbox', { name: 'Wait for Accept' });
    await expect(toggle).toBeChecked();
    await expect(fieldOf(editor, 'wait_for_accept')).toContainText('the planned steps wait until someone accepts');
    await expect(editor.getByRole('button', { name: 'Save Template' })).toBeDisabled();
    await capture(page, 'hs2-36xddx', 'editor-on-1440');

    // Off: a changed draft, saved as the next version with the field written false.
    await toggle.uncheck();
    await expect(toggle).not.toBeChecked();
    await expect(editor.getByRole('button', { name: 'Save Template' })).toBeEnabled();
    await capture(page, 'hs2-36xddx', 'editor-off-1440');
    await editor.getByRole('button', { name: 'Save Template' }).click();
    await expect(toast(page, 'Saved Repair the release.')).toBeVisible();
    await expect(editor).toBeHidden();
    expect((await listed(server)).templates[0]).toMatchObject({ version: 2, wait_for_accept: false });

    // Reopened, it reads off; on again leaves the field out once more.
    await row.locator('button[data-item-id]').click();
    await expect(toggle).not.toBeChecked();
    await toggle.check();
    await expect(toggle).toBeChecked();
    await editor.getByRole('button', { name: 'Save Template' }).click();
    await expect(toast(page, 'Saved Repair the release.')).toBeVisible();
    const saved = (await listed(server)).templates[0];
    expect(saved).toMatchObject({ version: 3 });
    expect(saved).not.toHaveProperty('wait_for_accept');

    // Phone and dark: the setting stays readable in the compact drill-down.
    await row.locator('button[data-item-id]').click();
    await resizeTo(page, { width: 390, height: 844 });
    await expect(editor).toBeVisible();
    // The compact drill-down lists the parts first; the Template part holds the setting.
    await editor.locator('[data-template-part]').filter({ hasText: 'Template' }).first().click();
    await page.emulateMedia({ colorScheme: 'dark' });
    await toggle.scrollIntoViewIfNeeded();
    await expect(toggle).toBeVisible();
    await capture(page, 'hs2-36xddx', 'editor-390-dark');
    await page.emulateMedia({ colorScheme: 'light' });
    await resizeTo(page, { width: 1440, height: 900 });
    await expect(toggle).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(editor).toBeHidden();
  } finally {
    await server.stop(page);
  }
});

test('HS2-36XDDX a held template step reads so on its card until Accept Plan releases it', async ({ page }) => {
  test.setTimeout(180_000);
  const server = await realTicketServer();
  try {
    await orchestrateSelf(server);
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    // Auto placement (left unset) for a tool no machine has, so nothing starts wherever this runs.
    await send(server, 'PUT', `${base}/policy`, {
      expected_revision: null,
      policy: {
        tools: [{ tool: 'synthetic-tool', max_sessions: 2 }],
        targets: [{ target_id: TARGET, tools: [{ tool: 'synthetic-tool' }], max_sessions: 2 }],
        placement: { actor: 'scheduler-1' },
      },
      actor: person,
    });
    const templateId = await defineTemplate(server, 'Document a feature');
    // The person applies it; the planner may or may not start here, the goal opens either way.
    await fetch(`${server.url}${templatesBase(server)}/apply`, {
      method: 'POST',
      headers: { 'content-type': 'application/json', 'X-Hotsheet-Secret': server.secret },
      body: JSON.stringify({
        template_id: templateId,
        tool: 'synthetic-tool',
        operation_id: 'tpl-browser-1',
        actor: person,
      }),
    });
    const apply = (await send(server, 'POST', `${templatesBase(server)}/apply_show`, {
      operation_id: 'tpl-browser-1',
    })) as { goal: { native_id: string; created: boolean } };
    expect(apply.goal.created).toBe(true);
    // The planner's step for the goal, as the planner defines it.
    const work = await server.request<Ticket>('/tickets', 'POST', { title: 'Describe the synthetic feature' });
    await send(server, 'POST', `${base}/readiness/define`, {
      ticket: { native_id: apply.goal.native_id },
      target_id: TARGET,
      connection_id: work.connection_id,
      native_id: work.id,
      prerequisites: [],
      actor: scheduler,
    });

    await resizeTo(page, { width: 1440, height: 900 });
    await openOrchestration(page, server);
    const view = page.locator('[data-orchestration-view="true"]');
    const card = view.locator(`[data-ticket-slug="${work.slug}"]`);
    const line = card.locator('[data-orchestration-part="state"]');
    await expect(line).toContainText(CARD_WAITING);
    // The card's line fits: nothing of it is cut off at this width.
    const [lineBox, cardBox] = [await line.boundingBox(), await card.boundingBox()];
    expect(lineBox && cardBox && lineBox.x + lineBox.width <= cardBox.x + cardBox.width).toBe(true);
    expect(await line.evaluate((node) => node.scrollWidth <= node.clientWidth + 1)).toBe(true);
    await card.click();
    const section = page.locator('[data-orchestration-inspector] [data-orchestration-section="Waits for Accept"]');
    await expect(section).toContainText(`${WAITING} (Document a feature)`);
    const accept = section.getByRole('button', { name: 'Accept Plan' });
    await expect(accept).toBeEnabled();
    await visualPass(page, 'hs2-36xddx', 'held-card', {
      show: section,
      check: () => expect(section).toContainText(WAITING),
      atPhone: () => card.click(),
    });

    // Accept Plan: the step is released, the card moves on and the button goes with its section.
    await accept.click();
    await expect(toast(page, 'Plan accepted: 1 step starts within the Orchestration settings.')).toBeVisible();
    await expect(line).not.toContainText(CARD_WAITING);
    await expect(section).toHaveCount(0);
    const shown = (await send(server, 'POST', `${templatesBase(server)}/apply_show`, {
      operation_id: 'tpl-browser-1',
    })) as { accepted_steps: string[] };
    expect(shown.accepted_steps).toHaveLength(1);
    await capture(page, 'hs2-36xddx', 'accepted-1440');
  } finally {
    await server.stop(page);
  }
});

test('HS2-36XDDX the inspector demo shows a card held for its template’s accept, and the usual wait after reset', async ({
  page,
}) => {
  await page.goto('/ux-demo?component=orchestration-inspector');
  const set = (name: string, value: string) =>
    page
      .locator(`[data-settings="orchestration"] [name="${name}"]`)
      .evaluate((node: HTMLElement & { value: string }, next) => {
        node.value = next;
        node.dispatchEvent(new Event('change', { bubbles: true }));
      }, value);
  await set('selected', 'step-8');
  const held = page.locator('[data-orchestration-section="Waits for Accept"]').first();
  await expect(held).toHaveCount(0);
  await set('doc-wait', 'template_plan_not_accepted');
  await expect(held).toContainText(`${WAITING} (Document a feature)`);
  await expect(held.getByRole('button', { name: 'Accept Plan' })).toBeEnabled();
  // Back to the held resource, then the template's accept again: each change re-reads the card.
  await set('doc-wait', 'resource_held');
  await expect(held).toHaveCount(0);
  await set('doc-wait', 'template_plan_not_accepted');
  await expect(held).toBeVisible();
  await page.locator('[data-settings="orchestration"] [data-action="reset-settings"]').dispatchEvent('click');
  await expect(page.locator('[data-orchestration-section="Waits for Accept"]')).toHaveCount(0);
});

/**
 * HS2-0YHA2S: with several template plans held under Auto placement, the Orchestration header counts the held
 * steps and its one Accept Plan accepts them all through `templates/accept` with `all`; the line and the
 * button go once nothing is held, and every held card moves on.
 */
test('HS2-0YHA2S the header counts held template steps and one Accept Plan releases them all', async ({ page }) => {
  test.setTimeout(180_000);
  const server = await realTicketServer();
  try {
    await orchestrateSelf(server);
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    // Auto placement (left unset) for a tool no machine has, so nothing starts wherever this runs.
    await send(server, 'PUT', `${base}/policy`, {
      expected_revision: null,
      policy: {
        tools: [{ tool: 'synthetic-tool', max_sessions: 2 }],
        targets: [{ target_id: TARGET, tools: [{ tool: 'synthetic-tool' }], max_sessions: 2 }],
        placement: { actor: 'scheduler-1' },
      },
      actor: person,
    });
    const cards: Ticket[] = [];
    for (const [name, operation, titles] of [
      ['Document a feature', 'tpl-header-1', ['Describe the synthetic feature', 'Add the synthetic example']],
      ['Repair the release', 'tpl-header-2', ['Repair the synthetic release']],
    ] as const) {
      const templateId = await defineTemplate(server, name);
      await fetch(`${server.url}${templatesBase(server)}/apply`, {
        method: 'POST',
        headers: { 'content-type': 'application/json', 'X-Hotsheet-Secret': server.secret },
        body: JSON.stringify({
          template_id: templateId,
          tool: 'synthetic-tool',
          operation_id: operation,
          actor: person,
        }),
      });
      const apply = (await send(server, 'POST', `${templatesBase(server)}/apply_show`, {
        operation_id: operation,
      })) as { goal: { native_id: string; created: boolean } };
      expect(apply.goal.created).toBe(true);
      for (const title of titles) {
        const work = await server.request<Ticket>('/tickets', 'POST', { title });
        await send(server, 'POST', `${base}/readiness/define`, {
          ticket: { native_id: apply.goal.native_id },
          target_id: TARGET,
          connection_id: work.connection_id,
          native_id: work.id,
          prerequisites: [],
          actor: scheduler,
        });
        cards.push(work);
      }
    }

    await resizeTo(page, { width: 1440, height: 900 });
    await openOrchestration(page, server);
    const view = page.locator('[data-orchestration-view="true"]');
    const header = page.locator('[data-orchestration-toolbar]');
    const accept = header.getByRole('button', { name: 'Accept Plan' });
    await expect(header).toContainText('3 steps wait for an accept');
    await expect(accept).toBeEnabled();
    await expect(accept).toHaveAttribute('title', /Document a feature and Repair the release \(3 steps\)/);
    for (const work of cards)
      await expect(view.locator(`[data-ticket-slug="${work.slug}"] [data-orchestration-part="state"]`)).toContainText(
        CARD_WAITING,
      );
    await visualPass(page, 'hs2-0yha2s', 'held-header', {
      show: header,
      check: async () => {
        await expect(header).toContainText('3 steps wait for an accept');
        await expect(accept).toBeVisible();
        // The header's Accept Plan stays whole at every width.
        expect(await accept.evaluate((node) => node.scrollWidth <= node.clientWidth + 1)).toBe(true);
      },
    });

    // One Accept Plan: every held step is released, the line and the button go, and each card moves on.
    await accept.click();
    await expect(toast(page, 'Plan accepted: 3 steps start within the Orchestration settings.')).toBeVisible();
    await expect(accept).toHaveCount(0);
    await expect(header).not.toContainText('wait for an accept');
    for (const work of cards)
      await expect(
        view.locator(`[data-ticket-slug="${work.slug}"] [data-orchestration-part="state"]`),
      ).not.toContainText(CARD_WAITING);
    for (const operation of ['tpl-header-1', 'tpl-header-2']) {
      const shown = (await send(server, 'POST', `${templatesBase(server)}/apply_show`, {
        operation_id: operation,
      })) as { accepted_steps: string[] };
      expect(shown.accepted_steps.length).toBeGreaterThan(0);
    }
    await capture(page, 'hs2-0yha2s', 'accepted-1440');
  } finally {
    await server.stop(page);
  }
});

test('HS2-0YHA2S the toolbar demo counts a held step and offers Accept Plan, and neither after reset', async ({
  page,
}) => {
  await page.goto('/ux-demo?component=orchestration-toolbar');
  const set = (name: string, value: string) =>
    page
      .locator(`[data-settings="orchestration"] [name="${name}"]`)
      .evaluate((node: HTMLElement & { value: string }, next) => {
        node.value = next;
        node.dispatchEvent(new Event('change', { bubbles: true }));
      }, value);
  const accept = page.locator('[data-action="accept-orchestration-template-plans"]');
  await expect(accept).toHaveCount(0);
  await set('doc-wait', 'template_plan_not_accepted');
  await expect(accept).toHaveCount(1);
  await expect(page.locator('[data-orchestration-toolbar="running"]')).toContainText('1 step waits for an accept');
  await set('doc-wait', 'resource_held');
  await expect(accept).toHaveCount(0);
  await set('doc-wait', 'template_plan_not_accepted');
  await expect(accept).toHaveCount(1);
  await page.locator('[data-settings="orchestration"] [data-action="reset-settings"]').dispatchEvent('click');
  await expect(accept).toHaveCount(0);
});
