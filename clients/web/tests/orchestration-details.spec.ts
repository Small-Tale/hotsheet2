import { expect, type Page, test } from '@playwright/test';

import {
  capture,
  openOrchestration,
  orchestrateSelf,
  planTwoSteps,
  realTicketServer,
  scheduler,
  send,
  TARGET,
  visualPass,
} from './orchestration-real-fixtures';

/**
 * On a phone the inspector is a sheet; with no card selected, its toggle opens it. Whether the sheet is
 * open is read from whether the overview takes a click (a closed sheet keeps it laid out but inert).
 */
async function openInspector(page: Page) {
  const overview = page.locator('[data-orchestration-overview]');
  const open = await overview
    .locator('h2')
    .first()
    .click({ trial: true, timeout: 2_000 })
    .then(() => true)
    .catch(() => false);
  if (!open) await page.locator('[data-action="toggle-ticket-inspector"]').first().click();
  await expect(overview).toBeInViewport();
}

/**
 * The orchestration details signed off on HS2-2XJSJK, through the real Orchestration view and its card
 * inspector against a real server. Each test names the build ticket it covers.
 */

test('HS2-15G5T4 the card inspector’s Plan section reads the readiness plan', async ({ page }) => {
  test.setTimeout(150_000);
  const server = await realTicketServer();
  try {
    await orchestrateSelf(server);
    const plan = await planTwoSteps(server);
    await plan.dispatch();
    await page.setViewportSize({ width: 1440, height: 900 });
    await openOrchestration(page, server);
    const view = page.locator('[data-orchestration-view="true"]');
    const inspector = page.locator('[data-orchestration-inspector]');
    const section = inspector.locator('[data-orchestration-section="Plan"]');

    // The running step: in progress, and it unblocks the step that waits on it.
    const working = view.locator(`[data-ticket-slug="${plan.first.slug}"]`);
    await working.click();
    await expect(section).toContainText('In progress');
    await expect(section).toContainText(plan.second.slug);
    // Plan sits between Where It Runs and Run.
    const order = await inspector
      .locator('[data-orchestration-section]')
      .evaluateAll((nodes) => nodes.map((node) => node.getAttribute('data-orchestration-section')));
    expect(order.slice(0, 3)).toEqual(['Where It Runs', 'Plan', 'Run']);

    // The waiting step: blocked, and nothing waits on it.
    const waiting = view.locator(`[data-ticket-slug="${plan.second.slug}"]`);
    await waiting.click();
    await expect(section).toContainText('Blocked');
    await expect(section).toContainText('Nothing waits on it');
    await expect(inspector.locator('[data-orchestration-section="Prerequisites"]')).toContainText(plan.first.slug);

    await working.click();
    await visualPass(page, 'hs2-15g5t4', 'plan-running', {
      check: () => expect(section).toContainText('In progress'),
      atPhone: () => view.locator(`[data-ticket-slug="${plan.first.slug}"]`).click(),
    });
  } finally {
    await server.stop(page);
  }
});

test('HS2-XM692S the Run section shows the last reconciliation beside the run controls', async ({ page }) => {
  test.setTimeout(150_000);
  const server = await realTicketServer();
  try {
    await orchestrateSelf(server);
    const plan = await planTwoSteps(server);
    const run = (await plan.dispatch()) as { run_id: string };
    await page.setViewportSize({ width: 1440, height: 900 });
    await openOrchestration(page, server);
    const view = page.locator('[data-orchestration-view="true"]');
    const section = page.locator('[data-orchestration-inspector] [data-orchestration-section="Run"]');
    await view.locator(`[data-ticket-slug="${plan.first.slug}"]`).click();
    await expect(section).toContainText('Checked');
    // A reconciliation reached through the server reaches the open inspector through the long poll.
    const reconciled = await send(
      server,
      'POST',
      `/checkouts/${server.checkoutId}/orchestration/runs/${run.run_id}/reconcile`,
      { actor: scheduler },
    );
    const disposition = String((reconciled.reconciliation as { disposition?: string } | undefined)?.disposition);
    expect(disposition).not.toBe('undefined');
    await expect(section).toContainText(/, since /, { timeout: 30_000 });
    await expect(section).not.toContainText('Not checked since its last change');
    for (const control of ['pause', 'handoff', 'cancel'])
      await expect(section.locator(`[data-control="${control}"]`)).toBeVisible();
    await expect(section.getByRole('button', { name: 'Correct…' })).toBeVisible();
    await visualPass(page, 'hs2-xm692s', 'run-checked', {
      show: section,
      check: () => expect(section).toContainText(/, since /),
      atPhone: () => view.locator(`[data-ticket-slug="${plan.first.slug}"]`).click(),
    });
  } finally {
    await server.stop(page);
  }
});

test('HS2-9XJEZC Run Validation asks through the permission prompt and shows each step’s evidence', async ({
  page,
}) => {
  test.setTimeout(180_000);
  const server = await realTicketServer();
  try {
    await orchestrateSelf(server);
    const plan = await planTwoSteps(server);
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    // A command step the orchestration project chose: it asks a person before each run (§26.7).
    await send(server, 'POST', `${base}/validations/define`, {
      ticket: { native_id: plan.goal.id },
      step: { step_id: 'smoke', target_id: TARGET, check: { kind: 'command', program: 'true', args: [] } },
      actor: scheduler,
    });
    await plan.dispatch();
    await page.setViewportSize({ width: 1440, height: 900 });
    await openOrchestration(page, server);
    const view = page.locator('[data-orchestration-view="true"]');
    const section = page.locator('[data-orchestration-inspector] [data-orchestration-section="Validation"]');
    await view.locator(`[data-ticket-slug="${plan.first.slug}"]`).click();
    await expect(section).toContainText('smoke (command)');
    await expect(section).toContainText('Not confirmed');
    await expect(section).toContainText('Output is never shown here.');

    await section.getByRole('button', { name: 'Run Validation' }).click();
    // The ordinary permission prompt asks, headed by the orchestrator.
    const popup = page.locator('[data-component="permission-request-popup"]');
    await expect(popup).toContainText('Orchestrator', { timeout: 30_000 });
    await expect(section).toContainText('Waits for permission');
    await expect(section.getByRole('button', { name: 'Show Request' })).toBeVisible();
    await capture(page, 'hs2-9xjezc', 'validation-asks-1440');
    // A command is outside any remembered rule, so the card offers Allow (once) and Deny only.
    await popup.getByRole('button', { name: 'Allow', exact: true }).click();

    // The answer changes the timeline; the same operation is sent again and the step runs once.
    await expect(section).toContainText(/Passed · .* · exit 0/, { timeout: 60_000 });
    await expect(section.getByRole('button', { name: 'Run Validation' })).toBeDisabled();
    await expect(section.getByRole('button', { name: 'Show Request' })).toHaveCount(0);
    const evidence = (await send(server, 'POST', `${base}/validations/list`, {
      ticket: { native_id: plan.goal.id },
    })) as unknown as Array<{ history?: unknown[] }>;
    expect(evidence[0].history).toHaveLength(1);

    await visualPass(page, 'hs2-9xjezc', 'validation-passed', {
      show: section,
      check: () => expect(section).toContainText('Passed'),
      atPhone: () => view.locator(`[data-ticket-slug="${plan.first.slug}"]`).click(),
    });
  } finally {
    await server.stop(page);
  }
});

test('HS2-0D60T8 the Built Ahead section shows work at risk and a person reopens it with a reason', async ({
  page,
}) => {
  test.setTimeout(150_000);
  const server = await realTicketServer();
  try {
    await orchestrateSelf(server);
    const plan = await planTwoSteps(server);
    await plan.dispatch();
    await page.setViewportSize({ width: 1440, height: 900 });
    await openOrchestration(page, server);
    // A red CI gate needs a provider, so the ledger's record is added to the real readiness answer: the
    // same shape `readiness show` gives for work started ahead of a gate that then failed (§26.22).
    const reopened: Array<Record<string, unknown>> = [];
    await page.route('**/orchestration/readiness/show', async (route) => {
      const real = await send(
        server,
        'POST',
        `/checkouts/${server.checkoutId}/orchestration/readiness/show`,
        route.request().postDataJSON(),
      );
      await route.fulfill({
        json: {
          ...real,
          provisional: {
            assignment: 'assignment-1',
            step_id: real.step_id,
            ticket: { connection_id: plan.goal.connection_id, native_id: plan.goal.id },
            target: { target_id: TARGET, connection_id: plan.first.connection_id, native_id: plan.first.id },
            operation_id: 'op-details',
            state: reopened.length > 0 ? 'reopened' : 'at_risk',
            depth: 1,
            explain: `provisional on ${plan.second.slug} CI run-7`,
            gates: [
              {
                target_id: TARGET,
                connection_id: plan.second.connection_id,
                native_id: plan.second.id,
                commit: '3f9c2a1d00aa3f9c2a1d00aa3f9c2a1d00aa3f9c',
                gate_ids: ['gate-1'],
                result: 'red',
                run: 'run-7',
              },
            ],
            recovery: 'rebase_onto_fix',
            reopen_reason: reopened.length > 0 ? 'fix_changed_contract' : undefined,
            recorded_at_ms: Date.now(),
            integration: 'ready',
            held: true,
          },
        },
      });
    });
    await page.route('**/orchestration/readiness/reopen', async (route) => {
      reopened.push(route.request().postDataJSON() as Record<string, unknown>);
      await route.fulfill({ json: { step_id: reopened[0].step_id, state: 'reopened' } });
    });
    const view = page.locator('[data-orchestration-view="true"]');
    const section = page.locator('[data-orchestration-inspector] [data-orchestration-section="Built Ahead"]');
    await view.locator(`[data-ticket-slug="${plan.first.slug}"]`).click();
    await expect(section).toContainText('At Risk');
    await expect(section).toContainText(`Repair ${plan.second.slug} first`);
    await expect(section).toContainText(`${plan.second.slug} · CI failed on 3f9c2a1`);
    await expect(section).toContainText('Held off the shipping branch for now');
    const order = await page
      .locator('[data-orchestration-inspector] [data-orchestration-section]')
      .evaluateAll((nodes) => nodes.map((node) => node.getAttribute('data-orchestration-section')));
    expect(order.slice(0, 4)).toEqual(['Where It Runs', 'Plan', 'Built Ahead', 'Run']);
    await visualPass(page, 'hs2-0d60t8', 'at-risk', {
      show: section,
      check: () => expect(section).toContainText('At Risk'),
      atPhone: () => view.locator(`[data-ticket-slug="${plan.first.slug}"]`).click(),
    });

    // Reopen… asks which reason, then confirms, as the browser's person.
    await section.getByRole('button', { name: 'Reopen…' }).click();
    await page.getByRole('menuitem', { name: 'The Fix Changed the Contract…' }).click();
    const dialog = page.getByRole('alertdialog');
    await expect(dialog).toContainText(`Reopen the work on ${plan.first.slug}?`);
    await capture(page, 'hs2-0d60t8', 'reopen-confirm-1440');
    await dialog.getByRole('button', { name: 'Reopen', exact: true }).click();
    await expect.poll(() => reopened.length).toBe(1);
    expect(reopened[0]).toMatchObject({ reason: 'fix_changed_contract', actor: { role: 'human' } });
    await expect(section).toContainText('Reopened (fix changed contract)');
    await expect(section.getByRole('button', { name: 'Reopen…' })).toHaveCount(0);
    await expect(section).not.toContainText('At Risk');
  } finally {
    await server.stop(page);
  }
});

test('HS2-77SRHC a ready card says why it is not running and the overview lists the scheduler’s decisions', async ({
  page,
}) => {
  test.setTimeout(180_000);
  const server = await realTicketServer();
  try {
    await orchestrateSelf(server);
    const plan = await planTwoSteps(server);
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    // A third step with nothing before it is ready and not running.
    const ready = await server.request<{ id: string; slug: string }>('/tickets', 'POST', {
      title: 'Add the synthetic audit log',
    });
    await send(server, 'POST', `${base}/readiness/define`, {
      ticket: { native_id: plan.goal.id },
      target_id: TARGET,
      connection_id: plan.first.connection_id,
      native_id: ready.id,
      prerequisites: [],
      actor: scheduler,
    });
    await plan.dispatch();
    // A limits policy for a tool no machine has, so every pass is the same wherever this runs.
    await send(server, 'PUT', `${base}/policy`, {
      expected_revision: null,
      policy: {
        tools: [{ tool: 'synthetic-tool', max_sessions: 2 }],
        targets: [{ target_id: TARGET, tools: [{ tool: 'synthetic-tool' }], max_sessions: 2 }],
        placement: { automatic: false, actor: 'scheduler-1' },
      },
      actor: { role: 'human', id: 'maintainer' },
    });
    // Enough logged passes that the dialog has an earlier page (50 per page).
    for (let pass = 0; pass < 30; pass += 1)
      await send(server, 'POST', `${base}/scheduler/tick`, { dry_run: true, actor: scheduler });
    await page.setViewportSize({ width: 1440, height: 900 });
    await openOrchestration(page, server);
    const view = page.locator('[data-orchestration-view="true"]');

    // Nothing selected: the overview, with the scheduler and its latest three decisions.
    const overview = page.locator('[data-orchestration-overview]');
    const scheduling = overview.locator('[data-orchestration-section="Scheduler"]');
    await expect(scheduling).toContainText('Last pass');
    await expect(scheduling).toContainText('(dry run)');
    const latest = overview.locator('[data-orchestration-section="Latest Decisions"]');
    await expect(latest).toContainText(/Would start|Deferred/);
    await visualPass(page, 'hs2-77srhc', 'overview', {
      show: latest,
      check: () => expect(latest).toContainText(/Would start|Deferred/),
      atPhone: () => openInspector(page),
    });

    // Show All Decisions…: newest first, Load Earlier reads the page before, Close and Escape dismiss.
    await latest.getByRole('button', { name: 'Show All Decisions…' }).click();
    const dialog = page.getByRole('dialog', { name: 'Scheduler Decisions' });
    await expect(dialog).toBeVisible();
    const before = await dialog.innerText();
    await capture(page, 'hs2-77srhc', 'decisions-1440');
    await dialog.getByRole('button', { name: 'Load Earlier' }).click();
    await expect.poll(async () => (await dialog.innerText()).length).toBeGreaterThan(before.length);
    // The dialog's own close control dismisses it.
    await dialog.getByRole('button', { name: 'Close' }).click();
    await expect(dialog).toHaveCount(0);
    await latest.getByRole('button', { name: 'Show All Decisions…' }).click();
    await expect(page.getByRole('dialog', { name: 'Scheduler Decisions' })).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(page.getByRole('dialog', { name: 'Scheduler Decisions' })).toHaveCount(0);
    await page.setViewportSize({ width: 390, height: 844 });
    await openInspector(page);
    await latest.getByRole('button', { name: 'Show All Decisions…' }).click();
    await expect(page.getByRole('dialog', { name: 'Scheduler Decisions' })).toBeVisible();
    await capture(page, 'hs2-77srhc', 'decisions-390');
    await page.keyboard.press('Escape');
    await page.setViewportSize({ width: 1440, height: 900 });

    // The ready card: the scheduler's own explanation.
    await view.locator(`[data-ticket-slug="${ready.slug}"]`).click();
    const why = page.locator('[data-orchestration-inspector] [data-orchestration-section="Why It Is Not Running"]');
    await expect(why).toContainText('Scheduler');
    await visualPass(page, 'hs2-77srhc', 'why-not-running', {
      show: why,
      check: () => expect(why).toContainText('Scheduler'),
      atPhone: () => view.locator(`[data-ticket-slug="${ready.slug}"]`).click(),
    });
  } finally {
    await server.stop(page);
  }
});

test('HS2-5GRXFN a held resource shows on the deferred card and in the overview with who waits', async ({ page }) => {
  test.setTimeout(150_000);
  const server = await realTicketServer();
  try {
    await orchestrateSelf(server);
    const plan = await planTwoSteps(server);
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    // A ready step that needs the one VM, which someone holds through the claim service.
    const ready = await server.request<{ id: string; slug: string }>('/tickets', 'POST', {
      title: 'Run the synthetic VM smoke test',
    });
    await send(server, 'POST', `${base}/readiness/define`, {
      ticket: { native_id: plan.goal.id },
      target_id: TARGET,
      connection_id: plan.first.connection_id,
      native_id: ready.id,
      prerequisites: [],
      resources: [{ namespace: 'vm', resource: 'macos-vm' }],
      actor: scheduler,
    });
    await send(server, 'PUT', `${base}/policy`, {
      expected_revision: null,
      policy: {
        tools: [{ tool: 'synthetic-tool', max_sessions: 2 }],
        targets: [{ target_id: TARGET, tools: [{ tool: 'synthetic-tool' }], max_sessions: 2 }],
        placement: { automatic: false, actor: 'scheduler-1' },
      },
      actor: { role: 'human', id: 'maintainer' },
    });
    const grant = await send(server, 'POST', '/resource-claims/vm/macos-vm/acquire', { duration_ms: 600_000 });
    expect(grant.acquired).toBe(true);
    await page.setViewportSize({ width: 1440, height: 900 });
    await openOrchestration(page, server);
    const view = page.locator('[data-orchestration-view="true"]');

    // The overview: the resource, until when it is held, and the step that waits for it.
    const resources = page.locator('[data-orchestration-overview] [data-orchestration-section="Resources"]');
    await expect(resources).toContainText('vm/macos-vm', { timeout: 30_000 });
    await expect(resources).toContainText(/Held until about .* unless renewed/);
    await expect(resources).toContainText(`${ready.slug} waits`);
    await visualPass(page, 'hs2-5grxfn', 'overview-resources', {
      show: resources,
      check: () => expect(resources).toContainText('vm/macos-vm'),
      atPhone: () => openInspector(page),
    });

    // The deferred card: what it needs and how that resource stands.
    await view.locator(`[data-ticket-slug="${ready.slug}"]`).click();
    const why = page.locator('[data-orchestration-inspector] [data-orchestration-section="Why It Is Not Running"]');
    await expect(why).toContainText('Needs');
    await expect(why).toContainText('vm/macos-vm');
    await expect(why).toContainText(/held until about/);
    await visualPass(page, 'hs2-5grxfn', 'why-resource', {
      show: why,
      check: () => expect(why).toContainText('vm/macos-vm'),
      atPhone: () => view.locator(`[data-ticket-slug="${ready.slug}"]`).click(),
    });
  } finally {
    await server.stop(page);
  }
});

test('HS2-XJEH08 the overview shows progress and cost, with an unknown cost never read as zero', async ({ page }) => {
  test.setTimeout(150_000);
  const server = await realTicketServer();
  try {
    await orchestrateSelf(server);
    const plan = await planTwoSteps(server);
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    const run = (await plan.dispatch()) as { run_id: string };
    // The first step finishes: its run completes, so its lead time is known; no usage was recorded.
    await server.request(`/tickets/${plan.first.id}`, 'PATCH', { status: 'completed' });
    await send(server, 'POST', `${base}/runs/${run.run_id}/sync`, { actor: scheduler });
    await page.setViewportSize({ width: 1440, height: 900 });
    await openOrchestration(page, server);
    const progress = page.locator('[data-orchestration-overview] [data-orchestration-section="Progress"]');
    await expect(progress).toContainText('1 of 2', { timeout: 30_000 });
    await expect(progress).toContainText(/on average over 1 run/);
    await expect(progress).not.toContainText('About under');
    await expect(progress).toContainText('Unknown (no usage recorded)');
    await expect(progress).not.toContainText('$0.00');
    const projects = page.locator('[data-orchestration-overview] [data-orchestration-section="Projects"]');
    await expect(projects).toContainText('Self');
    await expect(projects).toContainText('1 done');
    await expect(projects).toContainText('cost unknown (no usage recorded)');
    await visualPass(page, 'hs2-xjeh08', 'overview-progress', {
      show: progress,
      check: () => expect(progress).toContainText('1 of 2'),
      atPhone: () => openInspector(page),
    });
  } finally {
    await server.stop(page);
  }
});
