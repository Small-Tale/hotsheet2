import { execFile } from 'node:child_process';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { promisify } from 'node:util';

import { expect, type Page, test } from '@playwright/test';

import { withoutGitRepositoryEnv } from '../scripts/repository-env.mjs';
import { fakeGithubActions } from './fake-github-actions';
import {
  capture,
  gitCheckout,
  handOffInPool,
  openOrchestration,
  orchestrateSelf,
  planTwoSteps,
  realTicketServer,
  scheduler,
  send,
  SYNTHETIC_TOOL,
  syntheticTool,
  TARGET,
  type Ticket,
  visualPass,
} from './orchestration-real-fixtures';

const execute = promisify(execFile);
const REPO = 'acme/synthetic-widgets';
/** HS2-QDARNW: a second site of the Self project. */
const SITE = '01ARZ3NDEKTSV4RRFFQ69G5FJ9';

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

test('HS2-ESVYQH a released run’s Next says it may be in review or back for corrections, at every width', async ({
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
    const view = page.locator('[data-orchestration-view="true"]');
    const card = view.locator(`[data-ticket-slug="${plan.first.slug}"]`);
    const section = page.locator('[data-orchestration-inspector] [data-orchestration-section="Run"]');
    await card.click();
    // The person hands the run off through the shipped control: its worker no longer holds the ticket.
    await section.locator('[data-control="handoff"]').click();
    await page.getByRole('alertdialog').getByRole('button', { name: 'Hand Off', exact: true }).click();
    const next = 'Its handoff may be in review or back for corrections';
    await expect(section).toContainText(next, { timeout: 30_000 });
    await expect(section).toContainText('No worker holds it');
    await expect(section).toContainText('it is not done until its ticket is completed');
    await visualPass(page, 'hs2-esvyqh', 'released-run-next', {
      show: section,
      check: () => expect(section).toContainText(next),
      atPhone: () => card.click(),
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
    // HS2-ZYM6ZT: the audit names the orchestrator as the actor and this browser's person as requested_by.
    const person = await page.evaluate(() => localStorage.getItem('hotsheet.orchestrationPersonId'));
    expect(person).toMatch(/^person-[0-9a-f]+$/);
    const audit = (await send(server, 'GET', `${base}/audit?limit=500`)) as unknown as {
      entries: Array<{ action: string; actor_id?: string; requested_by?: string; outcome: string }>;
    };
    const runs = audit.entries.filter((entry) => entry.action === 'validation_run');
    expect(runs.length).toBeGreaterThan(0);
    for (const entry of runs) {
      expect(entry.actor_id).toBe(`orchestrator.${server.checkoutId}`);
      expect(entry.requested_by).toBe(person);
    }
    expect(runs.map((entry) => entry.outcome)).toEqual(expect.arrayContaining(['allowed', 'succeeded']));

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
  test.setTimeout(180_000);
  // HS2-8KV14Y: real provisional work and a real red CI gate, end to end. The prerequisite's gate is
  // watched by the server's own Job monitor through the real GitHub Actions client against a loopback
  // fake provider; nothing is added to the server's answers.
  const provider = await fakeGithubActions();
  const server = await realTicketServer({
    env: { ...provider.env(), HOTSHEET_JOB_MONITOR_AUTOSTART: 'on' },
    seed: async ({ store, root, env: inherited, cli }) => {
      const env = withoutGitRepositoryEnv(inherited);
      await execute('git', ['init', '-q', '-b', 'main'], { cwd: root, env });
      await execute('git', ['commit', '-q', '--allow-empty', '-m', 'Synthetic base'], { cwd: root, env });
      // The target project opts in to building ahead of a pending gate (docs/05 §5.9.1).
      await execute(
        cli,
        ['-C', store, 'coordination', '--project', root, 'speculation-policy', '--enable', '--base-branch', 'main'],
        { cwd: root, env },
      );
    },
  });
  try {
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    const human = { role: 'human', id: 'maintainer' };
    await orchestrateSelf(server);
    // Observing a gate never asks; building ahead is its own remembered decision.
    await send(server, 'POST', `${base}/permission-rules`, {
      target_id: TARGET,
      action: 'provisional_dispatch',
      actor: human,
    });
    // HS2-78HGC8: dispatch both runs explicitly. Automatic placement can raise a session permission
    // prompt that covers the Reopen confirmation when the host has a supported AI tool installed.
    await send(server, 'PUT', `${base}/policy`, {
      expected_revision: null,
      actor: human,
      policy: { speculation: { allowed: true }, placement: { automatic: false } },
    });
    const goal = await server.request<Ticket>('/tickets', 'POST', { title: 'Ship the synthetic sign-on' });
    const prerequisite = await server.request<Ticket>('/tickets', 'POST', {
      title: 'Add the synthetic token endpoint',
    });
    const built = await server.request<Ticket>('/tickets', 'POST', { title: 'Add the synthetic login page' });
    const define = (native: string, prerequisites: object[]) =>
      send(server, 'POST', `${base}/readiness/define`, {
        ticket: { native_id: goal.id },
        target_id: TARGET,
        connection_id: prerequisite.connection_id,
        native_id: native,
        prerequisites,
        actor: scheduler,
      });
    const dispatch = (native: string, operation: string) =>
      send(server, 'POST', `${base}/runs`, {
        ticket: { native_id: goal.id },
        target_id: TARGET,
        connection_id: prerequisite.connection_id,
        native_id: native,
        operation_id: operation,
        worker_id: `worker-${operation}`,
        actor: scheduler,
      });
    // The prerequisite runs and is integrated at a commit whose CI run starts.
    await define(prerequisite.id, []);
    const run = await dispatch(prerequisite.id, 'op-prerequisite');
    const commit = '3f9c2a1d00aa3f9c2a1d00aa3f9c2a1d00aa3f9c';
    provider.run(REPO, { id: 7, sha: commit, status: 'in_progress' });
    const linked = await send(server, 'POST', `${base}/runs/${String(run.run_id)}/gates`, {
      commit,
      repository: REPO,
      actor: scheduler,
    });
    expect((linked.gate as { state: string }).state).toBe('pending');
    // The dependent needs the prerequisite with CI: provisional, and dispatched ahead of the gate.
    const step = await define(built.id, [
      {
        kind: 'ticket',
        target_id: TARGET,
        connection_id: prerequisite.connection_id,
        native_id: prerequisite.id,
        gates: 'ci',
      },
    ]);
    expect(step.level).toBe('provisional');
    expect((await dispatch(built.id, 'op-built')).state).toBe('dispatched');

    await page.setViewportSize({ width: 1440, height: 900 });
    await openOrchestration(page, server);
    const view = page.locator('[data-orchestration-view="true"]');
    const section = page.locator('[data-orchestration-inspector] [data-orchestration-section="Built Ahead"]');
    // HS2-H5H9QP: the timeline card's orchestration line words the ledger record as a worker's card does.
    const cardLine = view.locator(`[data-ticket-slug="${built.slug}"] [data-orchestration-part="state"]`);
    await expect(cardLine).toHaveText(`Provisional on ${prerequisite.slug} · CI running`, { timeout: 30_000 });
    await expect(cardLine).toHaveAttribute('data-tone', 'info');
    await view.locator(`[data-ticket-slug="${built.slug}"]`).click();
    await expect(section).toContainText(`${prerequisite.slug} · CI running on 3f9c2a1`);
    await expect(section).not.toContainText('At Risk');

    // The prerequisite's run fails. The server's monitor records the red verdict and its gate follower puts
    // the work at risk; the inspector follows the run events to it.
    provider.run(REPO, { id: 7, sha: commit, status: 'completed', conclusion: 'failure' });
    await expect(section).toContainText('At Risk', { timeout: 60_000 });
    await expect(section).toContainText(`Repair ${prerequisite.slug} first`);
    await expect(section).toContainText(`${prerequisite.slug} · CI failed on 3f9c2a1`);
    await expect(cardLine).toHaveText(`At risk · repair ${prerequisite.slug} first`, { timeout: 30_000 });
    await expect(cardLine).toHaveAttribute('data-tone', 'warning');
    const order = await page
      .locator('[data-orchestration-inspector] [data-orchestration-section]')
      .evaluateAll((nodes) => nodes.map((node) => node.getAttribute('data-orchestration-section')));
    expect(order.slice(0, 4)).toEqual(['Where It Runs', 'Plan', 'Built Ahead', 'Run']);
    await visualPass(page, 'hs2-h5h9qp', 'at-risk', {
      show: section,
      check: () => expect(section).toContainText('At Risk'),
      atPhone: () => view.locator(`[data-ticket-slug="${built.slug}"]`).click(),
    });

    // Reopen… asks which reason, then confirms; the real `readiness reopen` records it as the person.
    await section.getByRole('button', { name: 'Reopen…' }).click();
    await page.getByRole('menuitem', { name: 'The Fix Changed the Contract…' }).click();
    const dialog = page.getByRole('alertdialog');
    await expect(dialog).toContainText(`Reopen the work on ${built.slug}?`);
    await capture(page, 'hs2-8kv14y', 'reopen-confirm-1440');
    await dialog.getByRole('button', { name: 'Reopen', exact: true }).click();
    await expect(section).toContainText('Reopened (fix changed contract)');
    await expect(section.getByRole('button', { name: 'Reopen…' })).toHaveCount(0);
    await expect(section).not.toContainText('At Risk');
    await expect(cardLine).toHaveText('Reopened · the fix invalidated it', { timeout: 30_000 });
    await expect(cardLine).toHaveAttribute('data-tone', 'warning');
    // The card itself on a phone, with the inspector sheet closed.
    await page.setViewportSize({ width: 390, height: 844 });
    await page.keyboard.press('Escape');
    // HS2-3KHE7R: the narrower board re-renders its cards, so a scroll begun on the old card can find it
    // detached; retry the scroll until the re-rendered card is in view.
    await expect(async () => {
      await cardLine.scrollIntoViewIfNeeded({ timeout: 2_000 });
      await expect(cardLine).toBeInViewport({ timeout: 1_000 });
    }).toPass({ timeout: 20_000 });
    await capture(page, 'hs2-h5h9qp', 'timeline-card-reopened-390');
    await page.setViewportSize({ width: 1440, height: 900 });
    await capture(page, 'hs2-h5h9qp', 'timeline-card-reopened-1440');
    const listed = await send(server, 'POST', `${base}/readiness/speculation`, { actor: scheduler });
    expect(listed.work).toEqual([
      expect.objectContaining({ state: 'reopened', reopen_reason: 'fix_changed_contract' }),
    ]);
    // The monitor only read the provider.
    expect(provider.requests.every((request) => request.startsWith('GET '))).toBe(true);
  } finally {
    await execute(server.cli, ['job', 'watch', 'stop'], { cwd: server.root, env: server.env }).catch(() => undefined);
    await server.stop(page);
    await provider.close();
  }
});

test('HS2-A7YCB2 Link Again watches a CI gate the provider kept ending, and words why it is not watched', async ({
  page,
}) => {
  test.setTimeout(180_000);
  // A real gate on a real server: the server's Job monitor watches it through the real GitHub Actions client
  // against a loopback fake that cannot read the repository (404), so the provider ends each watch. The
  // gate follower links it again on its back-off (shortened by the test-build switch) until the gate
  // reads `watch_gone`. Link Again in the inspector then goes through the real gate link route.
  const provider = await fakeGithubActions();
  provider.readable(REPO, false);
  const server = await realTicketServer({
    env: { ...provider.env(), HOTSHEET_JOB_MONITOR_AUTOSTART: 'on', HOTSHEET_TEST_GATE_RELINK_RETRY_MS: '150' },
    seed: async ({ root, env: inherited }) => {
      const env = withoutGitRepositoryEnv(inherited);
      await execute('git', ['init', '-q', '-b', 'main'], { cwd: root, env });
      await execute('git', ['commit', '-q', '--allow-empty', '-m', 'Synthetic base'], { cwd: root, env });
    },
  });
  try {
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    await orchestrateSelf(server);
    const plan = await planTwoSteps(server);
    const run = (await plan.dispatch()) as { run_id: string };
    const commit = '7a1e5c3b00aa7a1e5c3b00aa7a1e5c3b00aa7a1e';
    const linked = await send(server, 'POST', `${base}/runs/${run.run_id}/gates`, {
      commit,
      repository: REPO,
      actor: scheduler,
    });
    const gateId = (linked.gate as { gate_id: string }).gate_id;

    await page.setViewportSize({ width: 1440, height: 900 });
    await openOrchestration(page, server);
    const view = page.locator('[data-orchestration-view="true"]');
    const card = view.locator(`[data-ticket-slug="${plan.first.slug}"]`);
    const gates = page.locator('[data-orchestration-inspector] [data-orchestration-section="Gates"]');
    const runSection = page.locator('[data-orchestration-inspector] [data-orchestration-section="Run"]');
    const linkAgain = gates.getByRole('button', { name: 'Link Again' });
    await card.click();
    // The follower's three re-links end too; the gate is given up and the inspector follows the events.
    await expect(gates).toContainText('kept ending the watch', { timeout: 90_000 });
    await expect(gates).toContainText(
      'Unknown: the CI provider kept ending the watch; check the provider’s credentials and repository, then link the gate again',
    );
    await expect(linkAgain).toBeVisible();
    await expect(linkAgain).toHaveAttribute('data-gate-id', gateId);
    // Reconciliation holds the run for attention and says the same in its Not verified row.
    await send(server, 'POST', `${base}/runs/${run.run_id}/reconcile`, { actor: scheduler });
    await expect(runSection).toContainText('Not verified', { timeout: 30_000 });
    await expect(runSection).toContainText('CI (the CI provider kept ending the watch');
    // HS2-N9BFHW: the Events row words the reconciliation as the Checked row does, never its code.
    const events = page.locator('[data-orchestration-inspector] [data-orchestration-section="Events"]');
    await expect(events).toContainText('Checked · Held: something it depends on could not be verified');
    await expect(events).not.toContainText('hold unverified');
    // HS2-7NJN7S: a Gate event says which gate and the state it entered.
    await expect(events).toContainText('CI gate: unknown (the CI provider kept ending its watch)');
    await visualPass(page, 'hs2-a7ycb2', 'watch-gone', {
      show: gates,
      check: () => expect(linkAgain).toBeVisible(),
      atPhone: () => card.click(),
    });

    // The credentials are fixed and the run is in flight: Link Again watches the same gate again.
    provider.readable(REPO, true);
    provider.run(REPO, { id: 11, sha: commit, status: 'in_progress' });
    await linkAgain.click();
    await expect(gates).toContainText('Pending', { timeout: 30_000 });
    await expect(gates).not.toContainText('kept ending the watch');
    await expect(linkAgain).toHaveCount(0);
    // The same gate, watched again: Link Again made no second gate.
    const read = await send(server, 'GET', `${base}/gates?run=${run.run_id}`);
    expect((read.gates as Array<{ gate_id: string; state: string }>).map((gate) => [gate.gate_id, gate.state])).toEqual(
      [[gateId, 'pending']],
    );
    // The watch reaches its verdict with nobody linking again.
    provider.run(REPO, { id: 11, sha: commit, status: 'completed', conclusion: 'success' });
    await expect(gates).toContainText('Passed', { timeout: 60_000 });
    // HS2-N9BFHW: the verdict reconciles the run the CI gate held, so its Run section no longer reads CI as not
    // verified, and the Events say it continues.
    await expect(runSection).not.toContainText('Not verified', { timeout: 30_000 });
    await expect(runSection).toContainText('The run’s worker holds its ticket');
    await expect(events).toContainText('Checked · The run’s worker holds its ticket');
    await expect(events).toContainText('CI gate: passed');
    await expect(events).not.toContainText(/Gate( ·|$)/);
    await capture(page, 'hs2-a7ycb2', 'linked-again-passed-1440');
    await visualPass(page, 'hs2-7njn7s', 'gate-events', {
      show: events,
      check: () => expect(events).toContainText('CI gate: passed'),
      atPhone: () => card.click(),
    });
    await visualPass(page, 'hs2-n9bfhw', 'gate-settled', {
      show: events,
      check: () => expect(runSection).not.toContainText('Not verified'),
      atPhone: () => card.click(),
    });
    expect(provider.requests.every((request) => request.startsWith('GET '))).toBe(true);
  } finally {
    await execute(server.cli, ['job', 'watch', 'stop'], { cwd: server.root, env: server.env }).catch(() => undefined);
    await server.stop(page);
    await provider.close();
  }
});

test('HS2-A7YCB2 the inspector demo shows each CI gate state, with Link Again only where a link watches again', async ({
  page,
}) => {
  await page.goto('/ux-demo?component=orchestration-inspector');
  const gates = page.locator('[data-orchestration-section="Gates"]').first();
  const choose = (value: string) =>
    page
      .locator('[data-settings="orchestration"] [name="ci-gate"]')
      .evaluate((node: HTMLElement & { value: string }, next) => {
        node.value = next;
        node.dispatchEvent(new Event('change', { bubbles: true }));
      }, value);
  await expect(gates).toContainText('Pending');
  await expect(gates.getByRole('button', { name: 'Link Again' })).toHaveCount(0);
  for (const [reason, words] of [
    ['not_watched', 'nothing watches it yet'],
    ['watch_ended', 'it is linked again automatically'],
    ['watch_gone', 'then link the gate again'],
  ] as const) {
    await choose(reason);
    await expect(gates).toContainText(words);
    await expect(gates.getByRole('button', { name: 'Link Again' })).toBeEnabled();
  }
  // HS2-2751D6: a failed gate says what to do and offers no Link Again.
  await choose('failed');
  await expect(gates).toContainText('Fix the work and push the fix');
  await expect(gates.getByRole('button', { name: 'Link Again' })).toHaveCount(0);
  // Reset returns the pending gate.
  await page.locator('[data-settings="orchestration"] [data-action="reset-settings"]').dispatchEvent('click');
  await expect(gates.getByRole('button', { name: 'Link Again' })).toHaveCount(0);
  await expect(gates).not.toContainText('Fix the work');
});

test('HS2-JGNCAR the inspector names no session here once another worker took the ticket, and its own after reset', async ({
  page,
}) => {
  await page.goto('/ux-demo?component=orchestration-inspector');
  const where = page.locator('[data-orchestration-section="Where It Runs"]').first();
  const run = page.locator('[data-orchestration-section="Run"]').first();
  const choose = (value: string) =>
    page
      .locator('[data-settings="orchestration"] [name="run-condition"]')
      .evaluate((node: HTMLElement & { value: string }, next) => {
        node.value = next;
        node.dispatchEvent(new Event('change', { bubbles: true }));
      }, value);
  await expect(where).toContainText('session, running');
  await choose('elsewhere');
  await expect(where).toContainText('None here: another worker took the ticket');
  await expect(where).not.toContainText('session, running');
  await expect(where).toContainText('Not known here');
  await expect(run).toContainText('home-worker-1');
  // Back to a worker of this machine at work, then the condition again: each change re-reads the row.
  await choose('');
  await expect(where).toContainText('session, running');
  await choose('elsewhere');
  await expect(where).toContainText('None here: another worker took the ticket');
  await page.locator('[data-settings="orchestration"] [data-action="reset-settings"]').dispatchEvent('click');
  await expect(where).toContainText('session, running');
  await expect(where).not.toContainText('another worker took');
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

/**
 * HS2-QDARNW (HS2-9FKTB3): a project with two sites, the second one able to run the step and the first one
 * turned off in the policy (Sessions 0), so the scheduler chooses the second. The card's Why It Is Not Running names the
 * chosen site and why the first was passed over, through the real Orchestration view and a real server.
 */
test('HS2-QDARNW Why It Is Not Running names the chosen site and each site passed over', async ({
  page,
  browserName,
}) => {
  test.setTimeout(180_000);
  const server = await realTicketServer({ seed: ({ env }) => syntheticTool(env.HOTSHEET_HOME!) });
  try {
    await orchestrateSelf(server);
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    const person = { role: 'human', id: 'maintainer' };
    // A second checkout of the same ticket store, declared as another site of Self and bound here.
    const root = join(dirname(server.root), 'second-site');
    await mkdir(root, { recursive: true });
    const second = await server.request<{ id: string }>('/checkouts', 'POST', { root, stores: [server.store] });
    const declared = await send(server, 'GET', `${base}/targets`);
    const added = await send(server, 'POST', `${base}/targets`, {
      target_id: SITE,
      label: 'Self second site',
      expected_revision: declared.revision,
      site_of: TARGET,
    });
    const candidate = await send(server, 'POST', `${base}/inspect`, {
      destination: { kind: 'local', checkout_id: second.id },
    });
    await send(server, 'PUT', `${base}/targets/${SITE}/binding`, {
      target: { owner_checkout_id: server.checkoutId, target_id: SITE },
      expected_manifest_revision: added.revision,
      expected_binding_revision: 0,
      previous_destination: null,
      destination: candidate.destination,
    });
    for (const target of [TARGET, SITE]) {
      await send(server, 'POST', `${base}/permission-rules`, {
        target_id: target,
        action: 'work_start',
        actor: person,
      });
      await send(server, 'POST', `${base}/permission-rules`, {
        target_id: target,
        action: 'session_start',
        tool: SYNTHETIC_TOOL,
        actor: person,
      });
    }
    const plan = await planTwoSteps(server);
    // Self's Sessions is 0, so no session may start there; the second site may run the tool. Self must be
    // listed with 0: an unlisted project is Auto (HS2-N5BDE4) and would win on site order.
    // The machine limits never refuse on a busy test host.
    await send(server, 'PUT', `${base}/policy`, {
      expected_revision: null,
      policy: {
        machine: { max_sessions: 64, min_free_ram_mb: 0, max_cpu_percent: 100, min_free_disk_mb: 0 },
        tools: [{ tool: SYNTHETIC_TOOL, max_sessions: 4 }],
        targets: [
          { target_id: TARGET, max_sessions: 0 },
          { target_id: SITE, tools: [{ tool: SYNTHETIC_TOOL }], max_sessions: 2 },
        ],
        placement: { automatic: false, actor: 'scheduler-1' },
      },
      actor: person,
    });
    await send(server, 'POST', `${base}/scheduler/tick`, { dry_run: true, actor: scheduler });
    // The scheduler's own explanation: placed on the second site, Self passed over with its reason.
    const explained = (await send(server, 'POST', `${base}/scheduler/explain`, {
      target_id: TARGET,
      connection_id: plan.first.connection_id,
      native_id: plan.first.id,
      actor: scheduler,
    })) as { standing: string; placement?: { target_id: string }; sites?: { target_id: string; reason: string }[] };
    expect(explained, JSON.stringify(explained)).toMatchObject({ standing: 'start', placement: { target_id: SITE } });
    expect(explained.sites?.map((pass) => pass.target_id)).toEqual([TARGET]);
    // Refused for a stated reason, not merely passed over on site order.
    expect(explained.sites?.[0]?.reason).not.toBe('not_chosen');

    await page.setViewportSize({ width: 1440, height: 900 });
    await openOrchestration(page, server);
    const view = page.locator('[data-orchestration-view="true"]');
    const card = view.locator(`[data-ticket-slug="${plan.first.slug}"]`);
    await card.click();
    const why = page.locator('[data-orchestration-inspector] [data-orchestration-section="Why It Is Not Running"]');
    const rows = why.locator('dl > div');
    await expect(why).toContainText('It would start on the next pass');
    await expect(why).toContainText('Self second site');
    await expect(why).toContainText('Not on Self');
    const labels = await why.locator('dt').allInnerTexts();
    expect(labels.map((label) => label.trim())).toEqual(expect.arrayContaining(['Scheduler', 'Site', 'Not on Self']));
    // The chosen site comes before the sites passed over.
    expect(labels.findIndex((label) => label.trim() === 'Site')).toBeLessThan(
      labels.findIndex((label) => label.trim() === 'Not on Self'),
    );
    await expect(rows.filter({ hasText: 'Site' }).first()).toContainText('Self second site');
    // HS2-PR984R: Where It Runs agrees: the chosen site and its machine lead, the step's own site follows.
    const where = page.locator('[data-orchestration-inspector] [data-orchestration-section="Where It Runs"]');
    const whereRow = (label: string) =>
      where.locator('dl > div').filter({ has: page.locator('dt', { hasText: label }) });
    await expect(whereRow('Project').locator('dd')).toHaveText('Self second site (local)');
    await expect(whereRow("Step's site").locator('dd')).toHaveText('Self (local)');
    await expect(whereRow('Machine').locator('dd')).toHaveText('This machine');
    // HS2-MFS7FY: the passed-over site's reason is words, never the scheduler's detail code.
    const passedOver = rows.filter({ has: page.locator('dt', { hasText: 'Not on Self' }) });
    await expect(passedOver).toHaveCount(1);
    expect(await passedOver.locator('dd').innerText()).not.toMatch(/[a-z]+_[a-z_]+/);

    // Visual QA: 1440, 1100, 390 (the card's inspector sheet), wide again, in both color schemes.
    const directory = join('target', 'visual-captures', 'hs2-qdarnw');
    await mkdir(directory, { recursive: true });
    const settle = () =>
      page.waitForFunction(() =>
        document
          .getAnimations()
          .every(
            (animation) => animation.playState !== 'running' || animation.effect?.getTiming().iterations === Infinity,
          ),
      );
    for (const scheme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      for (const [index, [width, height]] of (
        [
          [1440, 900],
          [1100, 820],
          [390, 844],
          [1440, 900],
        ] as const
      ).entries()) {
        await page.setViewportSize({ width, height });
        if (width === 390) await card.click();
        await expect(why).toContainText('Not on Self');
        await why.scrollIntoViewIfNeeded();
        await settle();
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
        await page.screenshot({
          path: join(
            directory,
            `hs2-qdarnw-why-${String(width)}${index === 3 ? '-after-390' : ''}-${scheme}-${browserName}.png`,
          ),
        });
      }
    }

    // HS2-MFS7FY: with Self turned off in the policy, its pass carries the policy's detail code, which the
    // row words instead of printing.
    const current = (await send(server, 'GET', `${base}/policy`)) as { revision: string };
    await send(server, 'PUT', `${base}/policy`, {
      expected_revision: current.revision,
      policy: {
        machine: { max_sessions: 64, min_free_ram_mb: 0, max_cpu_percent: 100, min_free_disk_mb: 0 },
        tools: [{ tool: SYNTHETIC_TOOL, max_sessions: 4 }],
        targets: [
          { target_id: TARGET, enabled: false },
          { target_id: SITE, tools: [{ tool: SYNTHETIC_TOOL }], max_sessions: 2 },
        ],
        placement: { automatic: false, actor: 'scheduler-1' },
      },
      actor: person,
    });
    await send(server, 'POST', `${base}/scheduler/tick`, { dry_run: true, actor: scheduler });
    const disabled = (await send(server, 'POST', `${base}/scheduler/explain`, {
      target_id: TARGET,
      connection_id: plan.first.connection_id,
      native_id: plan.first.id,
      actor: scheduler,
    })) as { sites?: { target_id: string; reason: string; detail?: string }[] };
    expect(disabled.sites, JSON.stringify(disabled)).toEqual([
      { target_id: TARGET, reason: 'policy_denied', detail: 'policy_target_disabled' },
    ]);
    await page.emulateMedia({ colorScheme: 'light' });
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.reload();
    await card.click();
    const worded = why.locator('dl > div').filter({ has: page.locator('dt', { hasText: 'Not on Self' }) });
    await expect(worded.locator('dd')).toHaveText(
      'The policy does not allow it (the site is turned off in the policy)',
    );
    await why.scrollIntoViewIfNeeded();
    await settle();
    await page.screenshot({ path: join(directory, `hs2-mfs7fy-why-1440-light-${browserName}.png`) });
  } finally {
    await server.stop(page);
  }
});

/**
 * HS2-747WQA (HS2-0VTQFF): a project whose tool lists only a model pattern no reported model matches starts
 * nothing. The card's Why It Is Not Running says why in words, never the scheduler's model code.
 */
test('HS2-747WQA Why It Is Not Running words a Models pattern that resolves to no model', async ({ page }) => {
  test.setTimeout(180_000);
  const server = await realTicketServer({ seed: ({ env }) => syntheticTool(env.HOTSHEET_HOME!) });
  try {
    await orchestrateSelf(server);
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    const person = { role: 'human', id: 'maintainer' };
    await send(server, 'POST', `${base}/permission-rules`, { target_id: TARGET, action: 'work_start', actor: person });
    await send(server, 'POST', `${base}/permission-rules`, {
      target_id: TARGET,
      action: 'session_start',
      tool: SYNTHETIC_TOOL,
      actor: person,
    });
    const plan = await planTwoSteps(server);
    // Only a pattern, which no model the machine reports for the synthetic tool matches.
    await send(server, 'PUT', `${base}/policy`, {
      expected_revision: null,
      policy: {
        machine: { max_sessions: 64, min_free_ram_mb: 0, max_cpu_percent: 100, min_free_disk_mb: 0 },
        tools: [{ tool: SYNTHETIC_TOOL, max_sessions: 4 }],
        targets: [{ target_id: TARGET, tools: [{ tool: SYNTHETIC_TOOL, models: ['syn-none-*'] }], max_sessions: 2 }],
        placement: { automatic: false, actor: 'scheduler-1' },
      },
      actor: person,
    });
    await send(server, 'POST', `${base}/scheduler/tick`, { dry_run: true, actor: scheduler });
    const explained = (await send(server, 'POST', `${base}/scheduler/explain`, {
      target_id: TARGET,
      connection_id: plan.first.connection_id,
      native_id: plan.first.id,
      actor: scheduler,
    })) as { standing: string; reason: string; detail?: string };
    expect(explained, JSON.stringify(explained)).toMatchObject({
      standing: 'not_placed',
      reason: 'no_tool_available',
      detail: 'no_matching_model',
    });

    await page.setViewportSize({ width: 1440, height: 900 });
    await openOrchestration(page, server);
    const view = page.locator('[data-orchestration-view="true"]');
    const card = view.locator(`[data-ticket-slug="${plan.first.slug}"]`);
    await card.click();
    const why = page.locator('[data-orchestration-inspector] [data-orchestration-section="Why It Is Not Running"]');
    const scheduled = why.locator('dl > div').filter({ has: page.locator('dt', { hasText: 'Scheduler' }) });
    const words = 'no model its machine reports is an allowed model';
    await expect(scheduled.locator('dd')).toContainText(`Waits: ${words}`);
    expect(await why.innerText()).not.toMatch(/model_unresolved|no_matching_model/);
    await visualPass(page, 'hs2-747wqa', 'why-not-running', {
      show: why,
      check: () => expect(scheduled.locator('dd')).toContainText(words),
      atPhone: () => card.click(),
    });
  } finally {
    await server.stop(page);
  }
});

/**
 * HS2-ZRPZJJ: a project whose only AI tool its machine reports signed out starts nothing. Its card says so in
 * plain words, and Why It Is Not Running names the tool, the machine and what to do, never the scheduler's code.
 * The fixture host never probes a real tool (HS2-F6R2HQ), so its roomy test machine reports the synthetic tool
 * signed out.
 */
test('HS2-ZRPZJJ a card whose AI tool is signed out on its machine says so in plain words', async ({ page }) => {
  test.setTimeout(180_000);
  const server = await realTicketServer({
    env: { HOTSHEET_TEST_ROOMY_CAPACITY: '1', HOTSHEET_TEST_SIGNED_OUT_TOOLS: SYNTHETIC_TOOL },
    seed: ({ env }) => syntheticTool(env.HOTSHEET_HOME!),
  });
  try {
    await orchestrateSelf(server);
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    const person = { role: 'human', id: 'maintainer' };
    await send(server, 'POST', `${base}/permission-rules`, { target_id: TARGET, action: 'work_start', actor: person });
    await send(server, 'POST', `${base}/permission-rules`, {
      target_id: TARGET,
      action: 'session_start',
      tool: SYNTHETIC_TOOL,
      actor: person,
    });
    const plan = await planTwoSteps(server);
    await send(server, 'PUT', `${base}/policy`, {
      expected_revision: null,
      policy: {
        tools: [{ tool: SYNTHETIC_TOOL, max_sessions: 4 }],
        targets: [{ target_id: TARGET, tools: [{ tool: SYNTHETIC_TOOL }], max_sessions: 2 }],
        placement: { automatic: true, actor: 'scheduler-1', cooldown_minutes: 0, role_sessions: [] },
      },
      actor: person,
    });
    const detail = `tool_signed_out:${SYNTHETIC_TOOL}`;
    await expect
      .poll(
        async () => {
          const explained = (await send(server, 'POST', `${base}/scheduler/explain`, {
            target_id: TARGET,
            connection_id: plan.first.connection_id,
            native_id: plan.first.id,
            actor: person,
          })) as { standing: string; reason: string; detail?: string };
          return [explained.standing, explained.reason, explained.detail ?? null];
        },
        { timeout: 60_000 },
      )
      .toEqual(['not_placed', 'no_tool_available', detail]);
    // Nothing started on the machine where the tool is signed out.
    expect(await send(server, 'GET', `${base}/sessions`)).toEqual([]);
    // The machine's own admission names the tool.
    const admission = (await send(server, 'POST', `${base}/targets/${TARGET}/admission`, {
      tool: SYNTHETIC_TOOL,
      actor: scheduler,
    })) as { outcome: string; reason: string };
    expect([admission.outcome, admission.reason]).toEqual(['refused', 'tool_signed_out']);

    await page.setViewportSize({ width: 1440, height: 900 });
    await openOrchestration(page, server);
    const view = page.locator('[data-orchestration-view="true"]');
    const card = view.locator(`[data-ticket-slug="${plan.first.slug}"]`);
    const words = 'Hs-synthetic-agent is signed out on this machine';
    // The card says it beside its machine badge; Why It Is Not Running says it in full.
    const state = card.locator('[data-orchestration-part="state"]');
    await expect(state).toHaveText('Hs-synthetic-agent is signed out', { timeout: 30_000 });
    await expect(card.locator('[data-orchestration-part="placement"]')).toHaveText('This machine');
    await card.click();
    const why = page.locator('[data-orchestration-inspector] [data-orchestration-section="Why It Is Not Running"]');
    const row = (label: string) => why.locator('dl > div').filter({ has: page.locator('dt', { hasText: label }) });
    await expect(row('Scheduler').locator('dd')).toHaveText(new RegExp(`^Waits: ${words}`), { timeout: 30_000 });
    await expect(row('What to do').locator('dd')).toHaveText(
      'Sign in to Hs-synthetic-agent on this machine, or allow another AI tool for its project',
    );
    await expect(row('Next').locator('dd')).toHaveText('Looked at again when something it waits on changes');
    await expect(why).not.toContainText('_');
    await visualPass(page, 'hs2-zrpzjj', 'why-signed-out', {
      show: why,
      check: () => expect(row('What to do')).toHaveCount(1),
      atPhone: () => card.click(),
    });
    await visualPass(page, 'hs2-zrpzjj', 'card-signed-out', {
      check: () => expect(state).toHaveText('Hs-synthetic-agent is signed out'),
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

/**
 * HS2-65AD6J, HS2-91AF8K: integration and the project's coordination sessions when the scheduler's role sessions do
 * not take up their role, with nothing put into the server's answers. The synthetic AI tool refuses every role
 * prompt through its session push (`refusesRolePrompt`), so each integrator and dispatcher the real scheduler starts
 * has a failed role prompt, is wound down at once and replaced, and after its replacements are used up the project
 * reports `integrator_unavailable` and `dispatcher_unavailable` (docs/26 §26.30, "Failure handling"). A handoff in
 * the project's real pool wants the integrator; a ready step wants the dispatcher. HS2-6ES76F drives none,
 * not_started, start and keep from a real handoff below.
 */
test('HS2-65AD6J HS2-91AF8K role sessions that never take up their role leave the integrator and dispatcher unavailable', async ({
  page,
}) => {
  test.setTimeout(300_000);
  const server = await realTicketServer({
    // The scheduler's sessions launch in the project's resolved directory (HS2-KTHJN2).
    canonicalRoot: true,
    seed: async (fixture) => {
      await gitCheckout(fixture);
      await syntheticTool(fixture.env.HOTSHEET_HOME!, { refusesRolePrompt: true });
    },
  });
  try {
    await orchestrateSelf(server);
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    const person = { role: 'human', id: 'maintainer' };
    await send(server, 'POST', `${base}/permission-rules`, {
      target_id: TARGET,
      action: 'session_start',
      tool: SYNTHETIC_TOOL,
      actor: person,
    });
    const plan = await planTwoSteps(server);
    // A step with nothing before it is ready, so the project wants a dispatcher.
    const ready = await server.request<Ticket>('/tickets', 'POST', { title: 'Add the synthetic audit log' });
    await send(server, 'POST', `${base}/readiness/define`, {
      ticket: { native_id: plan.goal.id },
      target_id: TARGET,
      connection_id: plan.first.connection_id,
      native_id: ready.id,
      prerequisites: [],
      actor: scheduler,
    });
    await plan.dispatch();
    // A worker hands a ticket off in the project's pool, so the project wants an integrator.
    const handed = await server.request<Ticket>('/tickets', 'POST', { title: 'Add the synthetic export' });
    await handOffInPool(server, handed);
    const policy = async (automatic: boolean) => {
      const current = (await send(server, 'GET', `${base}/policy`)) as { revision?: string | null };
      await send(server, 'PUT', `${base}/policy`, {
        expected_revision: current.revision ?? null,
        policy: {
          machine: { max_sessions: 64, min_free_ram_mb: 0, max_cpu_percent: 100, min_free_disk_mb: 0 },
          tools: [{ tool: SYNTHETIC_TOOL, max_sessions: 8 }],
          targets: [{ target_id: TARGET, tools: [{ tool: SYNTHETIC_TOOL }], max_sessions: 8 }],
          placement: {
            automatic,
            actor: 'scheduler-1',
            cooldown_minutes: 0,
            role_sessions: [{ target_id: TARGET, roles: ['integrator', 'dispatcher'] }],
          },
        },
        actor: person,
      });
    };
    // HS2-KH41E3: the main checkout keeps uncommitted changes throughout (the policy file a person changes, the AI
    // tool's setup files a launch writes). An integrator that never took up its role owns no work there, so it is
    // still wound down and replaced.
    const git = (...args: string[]) =>
      execute('git', args, { cwd: server.root, env: withoutGitRepositoryEnv(server.env) });
    type Decision = {
      kind: string;
      outcome?: string;
      reason?: string;
      detail?: string;
      operation_id?: string;
      session_id?: string;
    };
    // The whole decision log, newest first, paging back.
    const decisions = async () => {
      const entries: Decision[] = [];
      let before: number | undefined;
      for (;;) {
        const page = (await send(server, 'POST', `${base}/scheduler/decisions`, {
          newest: true,
          limit: 200,
          ...(before === undefined ? {} : { before }),
        })) as { entries?: Decision[]; cursor?: number; more?: boolean };
        entries.push(...(page.entries ?? []));
        if (!page.more || page.cursor === undefined) return entries;
        before = page.cursor;
      }
    };
    type Unplaced = { target_id: string; role: string; reason: string; detail?: string; retry_at_ms?: number };
    const unplaced = async () =>
      (
        (await send(server, 'POST', `${base}/scheduler/plan`, { actor: person })) as {
          roles_unplaced?: Unplaced[];
        }
      ).roles_unplaced ?? [];

    // Automatic placement: the scheduler starts each role, learns its prompt failed, winds it down and starts the
    // next, until the third of each in a row failed. It is turned off again once both are reported.
    await policy(true);
    type Session = { session_id: string; role: string; state: string; owner: string };
    const roleSessions = async () =>
      ((await send(server, 'GET', `${base}/sessions`)) as unknown as Session[]).filter(
        (session) => session.role !== 'worker',
      );
    await expect
      .poll(
        async () => ({
          reported: (await unplaced()).map((entry) => entry.reason).sort(),
          running: (await roleSessions()).filter((session) => session.state !== 'stopped').length,
        }),
        { timeout: 60_000 },
      )
      .toEqual({ reported: ['dispatcher_unavailable', 'integrator_unavailable'], running: 0 });
    await policy(false);
    expect((await git('status', '--porcelain')).stdout, 'the main checkout has uncommitted changes').not.toBe('');
    // Three sessions of each role, each stopped by a wind-down because its role prompt failed; the last had used up
    // its replacements.
    const sessions = await roleSessions();
    // HS2-X3Z0HA: a pass logs its decisions after it applied them, so the log can trail the last wind-downs the
    // sessions already show. Wait until it has one for every stopped role session.
    let log: Decision[] = [];
    await expect
      .poll(
        async () => {
          log = await decisions();
          return sessions
            .filter(
              (session) => !log.some((entry) => entry.kind === 'wind_down' && entry.session_id === session.session_id),
            )
            .map((session) => session.session_id);
        },
        { timeout: 30_000 },
      )
      .toEqual([]);
    for (const role of ['integrator', 'dispatcher']) {
      const ofRole = sessions.filter((session) => session.role === role);
      expect(
        ofRole.map((session) => [session.owner, session.state]),
        role,
      ).toEqual(Array.from({ length: 3 }, () => ['orchestrator', 'stopped']));
      // HS2-2YCHRC: each start the log records carries its own role's operation and the session it began, also when
      // one pass started both roles.
      const started = log.filter(
        (entry) =>
          entry.kind === 'start' &&
          entry.outcome === 'started' &&
          entry.operation_id === `sched-role-${role}-${TARGET}`,
      );
      expect(started.map((entry) => entry.session_id).sort(), role).toEqual(
        ofRole.map((session) => session.session_id).sort(),
      );
      // The decision of each session's wind-down; a pass that decides the same again is not logged again, so the one
      // logged may be an earlier try that found the session not yet at its safe point.
      const windDown = (session: Session) =>
        log.find((entry) => entry.kind === 'wind_down' && entry.session_id === session.session_id);
      expect(ofRole.map((session) => [windDown(session)?.reason, windDown(session)?.detail]).sort(), role).toEqual([
        ['role_prompt_failed', role],
        ['role_prompt_failed', role],
        ['role_prompt_failed', 'replacements_exhausted'],
      ]);
    }
    const reported = await unplaced();
    for (const entry of reported) {
      expect(entry).toMatchObject({ target_id: TARGET, detail: 'replacements_exhausted' });
      expect(entry.retry_at_ms).toBeGreaterThan(Date.now());
    }

    await page.setViewportSize({ width: 1440, height: 900 });
    await openOrchestration(page, server);
    const view = page.locator('[data-orchestration-view="true"]');

    // The overview: each role session the scheduler does not start, why, and what a person can do meanwhile.
    const roles = page.locator('[data-orchestration-overview] [data-orchestration-section="Coordination Sessions"]');
    const row = (section: typeof roles, label: string) =>
      section.locator('dl > div').filter({ has: page.locator('dt', { hasText: label }) });
    await expect(roles.locator('dt')).toHaveText(['Self Integrator', 'Self Dispatcher'], { timeout: 30_000 });
    await expect(row(roles, 'Self Integrator').locator('dd')).toHaveText(
      new RegExp(
        '^Unavailable: the integrators the scheduler started did not take up their role, so none starts for a while\\. ' +
          'Open the AI tool in a terminal of this project to check that it starts and is signed in, or integrate handoffs by hand ' +
          'until then; looked at again by .+$',
      ),
    );
    await expect(row(roles, 'Self Dispatcher').locator('dd')).toHaveText(
      new RegExp(
        '^Unavailable: the dispatchers the scheduler started did not take up their role, so none starts for a while\\. ' +
          'Open the AI tool in a terminal of this project to check that it starts and is signed in, or assign ready work by hand ' +
          'until then; looked at again by .+$',
      ),
    );
    await expect(roles).not.toContainText('_');
    await visualPass(page, 'hs2-91af8k', 'overview-roles', {
      show: roles,
      check: () => expect(roles).toContainText('Self Dispatcher'),
      atPhone: () => openInspector(page),
    });

    // The running card, as the real server explains it: its project's integrator is unavailable.
    const running = view.locator(`[data-ticket-slug="${plan.first.slug}"]`);
    const waiting = view.locator(`[data-ticket-slug="${plan.second.slug}"]`);
    const integration = page.locator('[data-orchestration-inspector] [data-orchestration-section="Integration"]');
    await running.click();
    // The card reads the scheduler's explanation of its step, a full pass, when it is selected.
    await expect(row(integration, 'Integrator').locator('dd')).toHaveText('Unavailable', { timeout: 30_000 });
    await expect(row(integration, 'Why').locator('dd')).toHaveText(
      'The integrators the scheduler started did not take up their role, so none starts for a while. ' +
        'Open the AI tool in a terminal of this project to check that it starts and is signed in, or integrate handoffs by hand ' +
        'until then',
    );
    await expect(row(integration, 'Next').locator('dd')).toHaveText(/^Looked at again by .+/);
    await expect(row(integration, 'Session')).toHaveCount(0);
    await expect(integration).not.toContainText('_');
    const order = await page
      .locator('[data-orchestration-inspector] [data-orchestration-section]')
      .evaluateAll((nodes) => nodes.map((node) => node.getAttribute('data-orchestration-section')));
    expect(order.indexOf('Integration')).toBe(order.indexOf('Run') + 1);
    // A card without a run has no Integration section; selecting the running card again reads it again.
    await waiting.click();
    await expect(page.locator('[data-orchestration-inspector]')).toContainText('Prerequisites');
    await expect(integration).toHaveCount(0);
    await running.click();
    await expect(row(integration, 'Integrator').locator('dd')).toHaveText('Unavailable', { timeout: 30_000 });
    await visualPass(page, 'hs2-91af8k', 'integration-unavailable', {
      show: integration,
      check: () => expect(row(integration, 'Integrator').locator('dd')).toHaveText('Unavailable'),
      atPhone: () => running.click(),
    });
  } finally {
    await server.stop(page);
  }
});

/**
 * HS2-D1W59D: an integrator whose AI tool crashes at launch, before it ever joins the pool, is a failed attempt like
 * one that never takes up its role. The synthetic tool exits as soon as it starts (`crashesAtLaunch`), so each
 * integrator the real scheduler starts for a handoff in the project's pool is lost; after three in a row the project
 * reports `integrator_unavailable` and no fourth starts (docs/26 §26.30, "Failure handling"). Before the fix the
 * scheduler started a new one on every pass.
 */
test('HS2-D1W59D an integrator that crashes at launch is not restarted without bound', async ({ page }) => {
  test.setTimeout(180_000);
  const server = await realTicketServer({
    canonicalRoot: true,
    seed: async (fixture) => {
      await gitCheckout(fixture);
      await syntheticTool(fixture.env.HOTSHEET_HOME!, { crashesAtLaunch: true });
    },
  });
  try {
    await orchestrateSelf(server);
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    const person = { role: 'human', id: 'maintainer' };
    await send(server, 'POST', `${base}/permission-rules`, {
      target_id: TARGET,
      action: 'session_start',
      tool: SYNTHETIC_TOOL,
      actor: person,
    });
    const plan = await planTwoSteps(server);
    await plan.dispatch();
    const handed = await server.request<Ticket>('/tickets', 'POST', { title: 'Add the synthetic export' });
    await handOffInPool(server, handed);
    const policy = async (automatic: boolean) => {
      const current = (await send(server, 'GET', `${base}/policy`)) as { revision?: string | null };
      await send(server, 'PUT', `${base}/policy`, {
        expected_revision: current.revision ?? null,
        policy: {
          machine: { max_sessions: 64, min_free_ram_mb: 0, max_cpu_percent: 100, min_free_disk_mb: 0 },
          tools: [{ tool: SYNTHETIC_TOOL, max_sessions: 8 }],
          targets: [{ target_id: TARGET, tools: [{ tool: SYNTHETIC_TOOL }], max_sessions: 8 }],
          placement: {
            automatic,
            actor: 'scheduler-1',
            cooldown_minutes: 0,
            role_sessions: [{ target_id: TARGET, roles: ['integrator'] }],
          },
        },
        actor: person,
      });
    };
    type Session = { role: string; state: string; owner: string; joined_at_ms?: number | null };
    const integrators = async () =>
      ((await send(server, 'GET', `${base}/sessions`)) as unknown as Session[]).filter(
        (session) => session.role === 'integrator',
      );
    const unavailable = async () =>
      (
        (
          (await send(server, 'POST', `${base}/scheduler/plan`, { actor: person })) as {
            roles_unplaced?: { role: string; reason: string; detail?: string }[];
          }
        ).roles_unplaced ?? []
      ).filter((entry) => entry.reason === 'integrator_unavailable');

    await policy(true);
    await expect
      .poll(
        async () => ({
          reported: (await unavailable()).map((entry) => entry.detail),
          live: (await integrators()).filter((session) => ['requested', 'running'].includes(session.state)).length,
        }),
        { timeout: 90_000 },
      )
      .toEqual({ reported: ['replacements_exhausted'], live: 0 });
    // Further passes start no fourth: the next one runs on the policy change that stops automatic placement.
    await send(server, 'POST', `${base}/scheduler/tick`, { actor: scheduler });
    await policy(false);
    const sessions = await integrators();
    expect(sessions.map((session) => [session.owner, session.state, session.joined_at_ms ?? null])).toEqual(
      Array.from({ length: 3 }, () => ['orchestrator', 'lost', null]),
    );

    await page.setViewportSize({ width: 1440, height: 900 });
    await openOrchestration(page, server);
    const roles = page.locator('[data-orchestration-overview] [data-orchestration-section="Coordination Sessions"]');
    await expect(
      roles
        .locator('dl > div')
        .filter({ has: page.locator('dt', { hasText: 'Self Integrator' }) })
        .locator('dd'),
    ).toHaveText(/^Unavailable: the integrators the scheduler started did not take up their role/, {
      timeout: 30_000,
    });
  } finally {
    await server.stop(page);
  }
});

/**
 * HS2-ADASY5: a worker whose AI tool crashes at launch, before it ever joins the pool, is a failed attempt of its
 * step's run. The synthetic tool exits as soon as it starts (`crashesAtLaunch`), so each worker the real scheduler
 * starts for the ready step is lost; after three in a row the step is not started again for an hour, and its
 * running card says why and what a person can do (docs/26 §26.24, "Bounded"). Before the fix the scheduler started
 * a new worker on every pass.
 */
test('HS2-ADASY5 a worker that crashes at launch is not restarted without bound and its card says why', async ({
  page,
}) => {
  test.setTimeout(180_000);
  const server = await realTicketServer({
    canonicalRoot: true,
    seed: async (fixture) => {
      await gitCheckout(fixture);
      await syntheticTool(fixture.env.HOTSHEET_HOME!, { crashesAtLaunch: true });
    },
  });
  try {
    await orchestrateSelf(server);
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    const person = { role: 'human', id: 'maintainer' };
    await send(server, 'POST', `${base}/permission-rules`, {
      target_id: TARGET,
      action: 'session_start',
      tool: SYNTHETIC_TOOL,
      actor: person,
    });
    const plan = await planTwoSteps(server);
    const policy = async (automatic: boolean) => {
      const current = (await send(server, 'GET', `${base}/policy`)) as { revision?: string | null };
      await send(server, 'PUT', `${base}/policy`, {
        expected_revision: current.revision ?? null,
        policy: {
          machine: { max_sessions: 64, min_free_ram_mb: 0, max_cpu_percent: 100, min_free_disk_mb: 0 },
          tools: [{ tool: SYNTHETIC_TOOL, max_sessions: 8 }],
          targets: [{ target_id: TARGET, tools: [{ tool: SYNTHETIC_TOOL }], max_sessions: 8 }],
          placement: { automatic, actor: 'scheduler-1', cooldown_minutes: 0, role_sessions: [] },
        },
        actor: person,
      });
    };
    type Session = { role: string; state: string; owner: string; joined_at_ms?: number | null };
    const workers = async () =>
      ((await send(server, 'GET', `${base}/sessions`)) as unknown as Session[]).filter(
        (session) => session.role === 'worker',
      );
    const explain = async () =>
      (await send(server, 'POST', `${base}/scheduler/explain`, {
        target_id: TARGET,
        connection_id: plan.first.connection_id,
        native_id: plan.first.id,
        actor: person,
      })) as { standing: string; reason: string; detail?: string; retry_at_ms?: number };

    await policy(true);
    await expect
      .poll(
        async () => {
          const explained = await explain();
          return {
            held: [explained.standing, explained.reason, explained.detail ?? null],
            live: (await workers()).filter((session) => ['requested', 'running'].includes(session.state)).length,
          };
        },
        { timeout: 90_000 },
      )
      .toEqual({ held: ['not_placed', 'never_joined', 'replacements_exhausted'], live: 0 });
    // Further passes start no fourth: the next one runs on the policy change that stops automatic placement.
    await send(server, 'POST', `${base}/scheduler/tick`, { actor: scheduler });
    await policy(false);
    const sessions = await workers();
    expect(sessions.map((session) => [session.owner, session.state, session.joined_at_ms ?? null])).toEqual(
      Array.from({ length: 3 }, () => ['orchestrator', 'lost', null]),
    );
    expect((await explain()).retry_at_ms).toEqual(expect.any(Number));

    await page.setViewportSize({ width: 1440, height: 900 });
    await openOrchestration(page, server);
    const view = page.locator('[data-orchestration-view="true"]');
    const card = view.locator(`[data-ticket-slug="${plan.first.slug}"]`);
    const why = page.locator('[data-orchestration-inspector] [data-orchestration-section="Why It Is Not Running"]');
    const row = (label: string) => why.locator('dl > div').filter({ has: page.locator('dt', { hasText: label }) });
    // HS2-9FS7YX: the card reads held with when it starts again, not Working with the busy spinner, and so do the
    // inspector's badge and its Run section.
    await expect(card.locator('[data-orchestration-part="state"]')).toHaveText(/^Held until .+/, { timeout: 30_000 });
    await expect(card).toHaveAttribute('data-busy', 'false');
    await card.click();
    await expect(page.locator('[data-orchestration-inspector] [data-component="badge"]').first()).toHaveText('Held');
    await expect(page.locator('[data-orchestration-inspector] [data-orchestration-section="Run"]')).toContainText(
      /No session works it now; the scheduler starts one again by .+/,
    );
    await expect(row('Scheduler').locator('dd')).toHaveText(
      /^Waits: the sessions started for it ended before they took up the work, so none starts for a while/,
      { timeout: 30_000 },
    );
    await expect(row('What to do').locator('dd')).toHaveText(
      'Open the AI tool in a terminal of its project to check that it starts and is signed in, or work the ticket by hand',
    );
    await expect(row('Next').locator('dd')).toHaveText(/^Looked at again by .+/);
    await expect(why).not.toContainText('_');
    await visualPass(page, 'hs2-adasy5', 'worker-held', {
      show: why,
      check: () => expect(row('What to do')).toHaveCount(1),
      atPhone: () => card.click(),
    });
    // The board re-renders its cards at each width, so the pass scrolls nothing: the held card is in Working Now,
    // the column every width opens on (HS2-3KHE7R).
    await visualPass(page, 'hs2-9fs7yx', 'card-held', {
      check: () => expect(card.locator('[data-orchestration-part="state"]')).toHaveText(/^Held until .+/),
    });
  } finally {
    await server.stop(page);
  }
});

/**
 * HS2-6ES76F: the integrator's standing from a real handoff. The project is a git checkout whose coordination pool
 * has work handed off through the headless coordination CLI (`handOffInPool`), and every state is the real
 * scheduler's pass over it: none before the handoff, not started while the policy's Coordination Sessions leave the
 * integrator out, a dry-run start once they include it, then a real integrator session the scheduler started and
 * keeps, first for the handoff's queue and then for the commit's CI gate. Nothing is put into the server's answers.
 */
test('HS2-6ES76F the integrator states come from a real handoff in the project’s coordination pool', async ({
  page,
}) => {
  test.setTimeout(240_000);
  const server = await realTicketServer({
    // The scheduler's integrator session launches in the project's resolved directory (HS2-KTHJN2).
    canonicalRoot: true,
    seed: async (fixture) => {
      await gitCheckout(fixture);
      await syntheticTool(fixture.env.HOTSHEET_HOME!);
    },
  });
  try {
    await orchestrateSelf(server);
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    const person = { role: 'human', id: 'maintainer' };
    await send(server, 'POST', `${base}/permission-rules`, {
      target_id: TARGET,
      action: 'session_start',
      tool: SYNTHETIC_TOOL,
      actor: person,
    });
    const plan = await planTwoSteps(server);
    const handed = await server.request<Ticket>('/tickets', 'POST', { title: 'Add the synthetic audit log' });
    // The policy's Coordination Sessions for Self; the machine limits never refuse on a busy test host.
    const policy = async (roles: string[], automatic = false) => {
      const current = (await send(server, 'GET', `${base}/policy`)) as { revision?: string | null };
      await send(server, 'PUT', `${base}/policy`, {
        expected_revision: current.revision ?? null,
        policy: {
          machine: { max_sessions: 64, min_free_ram_mb: 0, max_cpu_percent: 100, min_free_disk_mb: 0 },
          tools: [{ tool: SYNTHETIC_TOOL, max_sessions: 2 }],
          targets: [{ target_id: TARGET, tools: [{ tool: SYNTHETIC_TOOL }], max_sessions: 2 }],
          placement: {
            automatic,
            actor: 'scheduler-1',
            cooldown_minutes: 0,
            role_sessions: [{ target_id: TARGET, roles }],
          },
        },
        actor: person,
      });
    };
    const explain = async () =>
      (
        (await send(server, 'POST', `${base}/scheduler/explain`, {
          target_id: TARGET,
          connection_id: plan.first.connection_id,
          native_id: plan.first.id,
          actor: scheduler,
        })) as { integrator?: Record<string, unknown> }
      ).integrator;
    await policy(['dispatcher']);
    await plan.dispatch();
    await page.setViewportSize({ width: 1440, height: 900 });
    await openOrchestration(page, server);
    const view = page.locator('[data-orchestration-view="true"]');
    const running = view.locator(`[data-ticket-slug="${plan.first.slug}"]`);
    const waiting = view.locator(`[data-ticket-slug="${plan.second.slug}"]`);
    const integration = page.locator('[data-orchestration-inspector] [data-orchestration-section="Integration"]');
    const roles = page.locator('[data-orchestration-overview] [data-orchestration-section="Coordination Sessions"]');
    const row = (label: string) =>
      integration.locator('dl > div').filter({ has: page.locator('dt', { hasText: label }) });
    const reselect = async () => {
      await waiting.click();
      await expect(integration).toHaveCount(0);
      await running.click();
    };
    // The overview reads the plan again when the view opens afresh with nothing selected.
    const overviewAfresh = async () => {
      await page.reload();
      await expect(view).toBeVisible();
    };

    // Before any handoff: nothing to integrate, and no coordination session waits to start.
    await expect(roles).toContainText('No integrator runs now');
    await running.click();
    await expect(row('Integrator').locator('dd')).toHaveText('Not running');
    await expect(row('Why').locator('dd')).toHaveText('No handoff waits to be integrated');

    // A worker hands its ticket off in the pool. Self's Coordination Sessions leave the integrator out.
    const handoff = await handOffInPool(server, handed);
    expect(await explain()).toEqual({
      target_id: TARGET,
      standing: 'not_started',
      reason: 'integrator_not_started',
      detail: 'policy_opt_out',
    });
    await reselect();
    await expect(row('Integrator').locator('dd')).toHaveText('Not started');
    await expect(row('Why').locator('dd')).toHaveText(
      'The project’s Coordination Sessions leave the integrator out, so it integrates by hand',
    );
    await expect(row('Session')).toHaveCount(0);
    await overviewAfresh();
    await expect(roles.locator('dt')).toHaveText(['Self Integrator']);
    await expect(roles.locator('dd')).toHaveText([
      'Not started: the project’s Coordination Sessions leave the integrator out, so it integrates by hand',
    ]);
    await visualPass(page, 'hs2-6es76f', 'overview-opt-out', {
      show: roles,
      check: () => expect(roles).toContainText('integrates by hand'),
      atPhone: () => openInspector(page),
    });

    // The person adds the integrator back: a dry-run pass would start one for the handoff.
    await policy(['integrator']);
    expect(await explain()).toMatchObject({
      standing: 'start',
      reason: 'integrator_started',
      detail: 'handoff_pending',
    });
    await running.click();
    await reselect();
    await expect(row('Integrator').locator('dd')).toHaveText('Would start on the next pass (dry run)');
    await expect(row('Why').locator('dd')).toHaveText('A handoff waits to be integrated');

    // Automatic placement starts a real integrator session for the handoff; it is turned off again once the
    // scheduler logged the start, so nothing else starts.
    await policy(['integrator'], true);
    type Decision = { kind: string; outcome: string; operation_id?: string; session_id?: string; reason?: string };
    const integratorStart = async () => {
      const page = (await send(server, 'POST', `${base}/scheduler/decisions`, { newest: true, limit: 50 })) as {
        entries?: Decision[];
      };
      return page.entries?.find(
        (entry) => entry.kind === 'start' && entry.operation_id === `sched-role-integrator-${TARGET}`,
      );
    };
    await expect.poll(async () => (await integratorStart())?.outcome, { timeout: 60_000 }).toBe('started');
    await policy(['integrator']);
    const started = await integratorStart();
    expect(started).toMatchObject({ reason: 'integrator_started' });
    const session = started!.session_id!;
    await expect.poll(async () => (await explain())?.standing, { timeout: 30_000 }).toBe('keep');
    expect(await explain()).toMatchObject({ reason: 'integrator_kept', detail: 'queue', session_id: session });
    await reselect();
    await expect(row('Integrator').locator('dd')).toHaveText('Running');
    await expect(row('Why').locator('dd')).toHaveText('Handoffs wait to be integrated');
    await expect(row('Session').locator('dd')).toHaveText(session);

    // The integrator pushes the handoff: it is kept while the commit waits on its CI gate.
    await handoff.integrate();
    expect(await explain()).toMatchObject({ standing: 'keep', reason: 'integrator_kept', detail: 'ci_pending' });
    await reselect();
    await expect(row('Why').locator('dd')).toHaveText('An integrated commit waits on its CI gate');
    await expect(row('Session').locator('dd')).toHaveText(session);
    await expect(integration).not.toContainText('_');
    await visualPass(page, 'hs2-6es76f', 'integration-kept', {
      show: integration,
      check: () => expect(row('Why').locator('dd')).toHaveText('An integrated commit waits on its CI gate'),
      atPhone: () => running.click(),
    });
    await overviewAfresh();
    await expect(roles.locator('dt')).toHaveText(['Self Integrator']);
    await expect(roles.locator('dd')).toHaveText(['Running: an integrated commit waits on its CI gate']);
    // The scheduler's session counts include the integrator it started.
    const scheduling = page.locator('[data-orchestration-overview] [data-orchestration-section="Scheduler"]');
    await expect(scheduling.locator('dl > div').filter({ hasText: 'Sessions' }).locator('dd')).toHaveText('1 running');
    await expect(page.getByText('Paused · 1 Session', { exact: true })).toBeVisible();
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
    await page.setViewportSize({ width: 1440, height: 900 });
    await openOrchestration(page, server);
    const progress = page.locator('[data-orchestration-overview] [data-orchestration-section="Progress"]');
    // HS2-RNFEYR: while the only run works, its lead time is unknown, and the reason reads in plain words.
    await expect(progress).toContainText('Unknown (run not finished yet)', { timeout: 30_000 });
    await expect(progress).not.toContainText('run unfinished');
    await visualPass(page, 'hs2-rnfeyr', 'overview-run-unfinished', {
      show: progress,
      check: () => expect(progress).toContainText('Unknown (run not finished yet)'),
      atPhone: () => openInspector(page),
    });
    // The first step finishes: its run completes, so its lead time is known; no usage was recorded.
    await server.request(`/tickets/${plan.first.id}`, 'PATCH', { status: 'completed' });
    await send(server, 'POST', `${base}/runs/${run.run_id}/sync`, { actor: scheduler });
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

test('HS2-F1BKQ0 a plan conflict and a damaged checked-in plan say what is wrong and how to resolve it', async ({
  page,
}) => {
  test.setTimeout(180_000);
  const server = await realTicketServer();
  try {
    await orchestrateSelf(server);
    const plan = await planTwoSteps(server);
    const sharedPath = resolve(server.root, '.hotsheet2/orchestration-plan.json');
    const localPath = resolve(server.home, 'orchestration-plan.json');
    const shared = JSON.parse(await readFile(sharedPath, 'utf8')) as { steps: Array<Record<string, unknown>> };
    // This machine kept the first step from before the plan was checked in, with another preferred tool.
    const first = shared.steps.find((step) => (step.target as { native_id: string }).native_id === plan.first.id)!;
    const local = JSON.parse(await readFile(localPath, 'utf8')) as Record<string, unknown>;
    await writeFile(
      localPath,
      JSON.stringify({
        ...local,
        steps: [{ step: { ...first, owner_checkout_id: server.checkoutId, tool: 'codex' } }],
      }),
    );
    await page.setViewportSize({ width: 1440, height: 900 });
    await openOrchestration(page, server);
    const view = page.locator('[data-orchestration-view="true"]');
    const problem = view.locator('[data-orchestration-plan-problem]');
    await expect(problem).toHaveAttribute('data-orchestration-plan-problem', 'conflict');
    await expect(problem).toContainText('This Machine’s Plan Differs From the Checked-In Plan');
    await expect(problem).toContainText(`step ${String(first.step_id).slice(-6)}`);
    await expect(view).not.toContainText('The Orchestration Timeline Could Not Be Read');
    await visualPass(page, 'hs2-f1bkq0', 'conflict', {
      show: problem,
      check: () => expect(problem).toBeVisible(),
    });

    // The person keeps the checked-in plan through the app's confirmation; the timeline reads again.
    await problem.getByRole('button', { name: 'Keep the Checked-In Plan…' }).click();
    const dialog = page.getByRole('alertdialog');
    await expect(dialog).toContainText('Keep the checked-in plan?');
    await capture(page, 'hs2-f1bkq0', 'confirm-1440');
    await dialog.getByRole('button', { name: 'Keep the Checked-In Plan', exact: true }).click();
    await expect(problem).toHaveCount(0, { timeout: 30_000 });
    await expect(view.locator(`[data-ticket-slug="${plan.first.slug}"]`)).toBeVisible();
    const kept = JSON.parse(await readFile(localPath, 'utf8')) as { steps?: unknown[] };
    expect(kept.steps ?? []).toEqual([]);

    // A Git merge leaves markers in the checked-in plan: the view says which file and what to do.
    const good = await readFile(sharedPath, 'utf8');
    await writeFile(sharedPath, `<<<<<<< ours\n${good}=======\n${good}>>>>>>> theirs\n`);
    // Any change wakes the view's long poll; a ticket edit is one.
    await server.request(`/tickets/${plan.second.id}`, 'PATCH', { title: 'Add the synthetic login page again' });
    await expect(problem).toHaveAttribute('data-orchestration-plan-problem', 'damaged', { timeout: 40_000 });
    await expect(problem).toContainText('The Checked-In Plan Cannot Be Read');
    await expect(problem.getByRole('button')).toHaveCount(0);
    await capture(page, 'hs2-f1bkq0', 'damaged-1440');
    await page.emulateMedia({ colorScheme: 'dark' });
    await capture(page, 'hs2-f1bkq0', 'damaged-1440-dark');
    await page.emulateMedia({ colorScheme: 'light' });
    // Resolving the merge brings the plan back without a reload.
    await writeFile(sharedPath, good);
    await expect(problem).toHaveCount(0, { timeout: 60_000 });
    await expect(view.locator(`[data-ticket-slug="${plan.first.slug}"]`)).toBeVisible();
  } finally {
    await server.stop(page);
  }
});

test('HS2-NY0MRJ a Run Validation that waits for permission is kept across a reload and runs once', async ({
  page,
}) => {
  test.setTimeout(180_000);
  const server = await realTicketServer();
  try {
    await orchestrateSelf(server);
    const plan = await planTwoSteps(server);
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    await send(server, 'POST', `${base}/validations/define`, {
      ticket: { native_id: plan.goal.id },
      step: { step_id: 'smoke-check', target_id: TARGET, check: { kind: 'command', program: 'true', args: [] } },
      actor: scheduler,
    });
    await plan.dispatch();
    await page.setViewportSize({ width: 1440, height: 900 });
    await openOrchestration(page, server);
    const view = page.locator('[data-orchestration-view="true"]');
    const section = page.locator('[data-orchestration-inspector] [data-orchestration-section="Validation"]');
    await view.locator(`[data-ticket-slug="${plan.first.slug}"]`).click();
    await section.getByRole('button', { name: 'Run Validation' }).click();
    await expect(section).toContainText('Waits for permission', { timeout: 30_000 });

    // Reload: the waiting request is read back from the server's pending permission requests.
    await page.reload();
    await page.locator('[data-segment-value="orchestration"]').first().click();
    await view.locator(`[data-ticket-slug="${plan.first.slug}"]`).click();
    await expect(section).toContainText('smoke-check (command)');
    await expect(section).toContainText('Waits for permission', { timeout: 30_000 });
    await expect(section.getByRole('button', { name: 'Show Request' })).toBeVisible();
    await section.scrollIntoViewIfNeeded();
    await capture(page, 'hs2-ny0mrj', 'recovered-after-reload-1440');
    // Run Validation does not ask again for the step that already waits.
    const permissions = () =>
      send(server, 'GET', '/permissions') as unknown as Promise<Array<{ orchestrator?: { operation_id: string } }>>;
    expect((await permissions()).filter((item) => item.orchestrator)).toHaveLength(1);
    await section.getByRole('button', { name: 'Run Validation' }).click();
    expect((await permissions()).filter((item) => item.orchestrator)).toHaveLength(1);

    // The answer runs the original operation, once.
    const popup = page.locator('[data-component="permission-request-popup"]');
    await expect(popup).toContainText('Orchestrator', { timeout: 30_000 });
    await popup.getByRole('button', { name: 'Allow', exact: true }).click();
    await expect(section).toContainText(/Passed · .* · exit 0/, { timeout: 60_000 });
    const evidence = (await send(server, 'POST', `${base}/validations/list`, {
      ticket: { native_id: plan.goal.id },
    })) as unknown as Array<{ history?: unknown[] }>;
    expect(evidence[0].history).toHaveLength(1);
    await capture(page, 'hs2-ny0mrj', 'passed-after-reload-1440');
    await page.emulateMedia({ colorScheme: 'dark' });
    await capture(page, 'hs2-ny0mrj', 'passed-after-reload-1440-dark');
    await page.emulateMedia({ colorScheme: 'light' });
  } finally {
    await server.stop(page);
  }
});

/**
 * HS2-GPABYY: a step whose ticket was completed reads plainly Done until its review asks for changes, then its
 * card says Changes requested (warning), its inspector badge Done · Changes requested and its Run section's Next
 * what to do; requesting the review again and approving it read plainly Done again, and requesting and sending
 * it back once more repeats the warning. Through the real server's review route, run sync and the timeline long poll.
 */
test('HS2-GPABYY a Done card whose review asks for changes reads as needing repair', async ({ page }) => {
  test.setTimeout(150_000);
  const server = await realTicketServer();
  try {
    await orchestrateSelf(server);
    const plan = await planTwoSteps(server);
    const run = (await plan.dispatch()) as { run_id: string };
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    const commit = 'c'.repeat(40);
    const review = (mark: string) =>
      send(server, 'POST', `${base}/runs/${run.run_id}/reviews`, {
        review: mark,
        commit: mark === 'requested' ? commit : undefined,
        actor: scheduler,
      });
    await review('requested');
    await server.request(`/tickets/${plan.first.id}`, 'PATCH', { status: 'completed' });
    await send(server, 'POST', `${base}/runs/${run.run_id}/sync`, { actor: scheduler });
    await page.setViewportSize({ width: 1440, height: 900 });
    await openOrchestration(page, server);
    const view = page.locator('[data-orchestration-view="true"]');
    const card = view.locator(`[data-ticket-slug="${plan.first.slug}"]`);
    const line = card.locator('[data-orchestration-part="state"]');
    const inspector = page.locator('[data-orchestration-inspector]');
    const section = inspector.locator('[data-orchestration-section="Run"]');
    const next = 'Its ticket is completed, but its review asks for changes: make them, then request the review again.';
    await expect(view.locator('[aria-label="Done Column"]')).toContainText(plan.first.slug, { timeout: 30_000 });
    await card.click();
    // Requested and not yet answered: plainly Done.
    await expect(inspector).toContainText('Done');
    await expect(line).toHaveCount(0);
    await expect(inspector).not.toContainText('Changes requested');
    // Sent back after completion: the card needs repair, with what to do next.
    await review('changes_requested');
    await expect(line).toHaveText('Changes requested', { timeout: 30_000 });
    await expect(line).toHaveAttribute('data-tone', 'warning');
    await expect(inspector).toContainText('Done · Changes requested');
    await expect(section).toContainText(next);
    await visualPass(page, 'hs2-gpabyy', 'changes-requested', {
      show: section,
      check: () => expect(line).toHaveText('Changes requested'),
      atPhone: () => card.click(),
    });
    // Requested again, then approved: plainly Done. Requested once more and sent back: the warning returns.
    await review('requested');
    await expect(line).toHaveCount(0, { timeout: 30_000 });
    await review('approved');
    await expect(inspector).toContainText('Done');
    await expect(line).toHaveCount(0);
    await expect(inspector).not.toContainText('Changes requested');
    await review('requested');
    await review('changes_requested');
    await expect(line).toHaveText('Changes requested', { timeout: 30_000 });
    await expect(section).toContainText(next);
  } finally {
    await server.stop(page);
  }
});

/**
 * HS2-V7C7MM: HS2-GPABYY's deployment path through a real server. The project is a git checkout that tracks a
 * synthetic deploy script, its commit's CI passes through the real Job monitor against the loopback GitHub
 * Actions fake, and the ticket is completed: plainly Done. A production deployment asks a person, who denies it,
 * so the card says Deploy failed (warning), its badge Done · Deploy failed and its Run section's Next what to do.
 * A newer commit's CI fails while the failed deployment stands: CI is named first. A third commit's CI passes and
 * Deploy failed returns; a new request a person allows runs the tracked script, passes, and the card reads
 * plainly Done again. Synthetic data only.
 */
test('HS2-V7C7MM a Done card whose deployment failed reads as needing repair, CI first', async ({ page }) => {
  test.setTimeout(240_000);
  const provider = await fakeGithubActions();
  const commits: string[] = [];
  const server = await realTicketServer({
    env: { ...provider.env(), HOTSHEET_JOB_MONITOR_AUTOSTART: 'on' },
    seed: async ({ root, env: inherited }) => {
      const env = withoutGitRepositoryEnv(inherited);
      const git = async (...args: string[]) => (await execute('git', args, { cwd: root, env })).stdout.trim();
      await git('init', '-q', '-b', 'main');
      await mkdir(join(root, 'scripts'), { recursive: true });
      await writeFile(join(root, 'scripts', 'deploy.sh'), '#!/bin/sh\nexit 0\n', { mode: 0o755 });
      await git('add', 'scripts/deploy.sh');
      for (const message of ['Synthetic release', 'Synthetic change', 'Synthetic fix']) {
        await git('commit', '-q', '--allow-empty', '-m', message);
        commits.push(await git('rev-parse', 'HEAD'));
      }
    },
  });
  const [released, change, fix] = commits;
  const maintainer = { role: 'human', id: 'maintainer' };
  try {
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    await orchestrateSelf(server);
    const plan = await planTwoSteps(server);
    const run = (await plan.dispatch()) as { run_id: string };
    const ci = async (id: number, sha: string, conclusion: 'success' | 'failure') => {
      provider.run(REPO, { id, sha, status: 'in_progress' });
      await send(server, 'POST', `${base}/runs/${run.run_id}/gates`, {
        commit: sha,
        repository: REPO,
        actor: scheduler,
      });
      provider.run(REPO, { id, sha, status: 'completed', conclusion });
    };
    const deploy = (operation: string) =>
      send(server, 'POST', `${base}/runs/${run.run_id}/deployments`, {
        environment: 'production',
        script: 'scripts/deploy.sh',
        production: true,
        operation_id: operation,
        actor: scheduler,
      }) as Promise<{ gate: { gate_id: string; state: string; reason?: string }; request_id?: string }>;
    const answer = (requestId: string, reply: 'deny' | 'allow_once') =>
      send(server, 'POST', `${base}/permissions/${requestId}`, { answer: reply, actor: maintainer });

    await page.setViewportSize({ width: 1440, height: 900 });
    await openOrchestration(page, server);
    const view = page.locator('[data-orchestration-view="true"]');
    const card = view.locator(`[data-ticket-slug="${plan.first.slug}"]`);
    const line = card.locator('[data-orchestration-part="state"]');
    const inspector = page.locator('[data-orchestration-inspector]');
    const section = inspector.locator('[data-orchestration-section="Run"]');
    const gates = inspector.locator('[data-orchestration-section="Gates"]');
    const events = inspector.locator('[data-orchestration-section="Events"]');
    const row = (within: typeof section, label: string) =>
      within
        .locator('dl > div')
        .filter({ has: page.locator('dt', { hasText: label }) })
        .locator('dd');
    const nextRow = row(section, 'Next');
    const deployNext =
      'Its ticket is completed, but its deployment failed: fix the deployment and request it again as the Gates section says.';
    const ciNext =
      'Its ticket is completed, but its CI failed, so the work needs a fix: repair it as the Gates section says.';

    // 1. The released commit's CI passes and the ticket is completed: plainly Done.
    await ci(31, released, 'success');
    await server.request(`/tickets/${plan.first.id}`, 'PATCH', { status: 'completed' });
    await send(server, 'POST', `${base}/runs/${run.run_id}/sync`, { actor: scheduler });
    await expect(view.locator('[aria-label="Done Column"]')).toContainText(plan.first.slug, { timeout: 30_000 });
    await card.click();
    await expect(row(gates, `CI · ${released.slice(0, 7)}`)).toHaveText('Passed', { timeout: 60_000 });
    await expect(inspector).toContainText('Done');
    await expect(line).toHaveCount(0);

    // 2. A production deployment asks a person; while it waits the card is still plainly Done.
    const waiting = await deploy('deploy-prod-1');
    expect(waiting.gate.state).toBe('awaiting_approval');
    expect(waiting.request_id).toBeTruthy();
    await expect(events).toContainText('Deployment gate: awaiting approval', { timeout: 30_000 });
    await expect(line).toHaveCount(0);
    await expect(inspector).not.toContainText('Deploy failed');

    // 3. The person denies it and the request is repeated: the deployment fails and the Done card needs repair.
    await answer(waiting.request_id ?? '', 'deny');
    const denied = await deploy('deploy-prod-1');
    expect([denied.gate.state, denied.gate.reason]).toEqual(['failed', 'permission_denied']);
    await expect(events).toContainText('Deployment gate: failed', { timeout: 30_000 });
    await expect(line).toHaveText('Deploy failed', { timeout: 30_000 });
    await expect(line).toHaveAttribute('data-tone', 'warning');
    await expect(inspector).toContainText('Done · Deploy failed');
    await expect(nextRow).toHaveText(deployNext);
    await visualPass(page, 'hs2-v7c7mm', 'deploy-failed', {
      show: section,
      check: () => expect(line).toHaveText('Deploy failed'),
      atPhone: () => card.click(),
    });

    // 4. A newer commit's CI fails while the failed deployment stands: CI is named first.
    await ci(32, change, 'failure');
    await expect(row(gates, `CI · ${change.slice(0, 7)}`)).toHaveText('Failed', { timeout: 60_000 });
    await expect(line).toHaveText('CI failed', { timeout: 30_000 });
    await expect(line).toHaveAttribute('data-tone', 'warning');
    await expect(inspector).toContainText('Done · CI failed');
    await expect(inspector).not.toContainText('Deploy failed');
    await expect(nextRow).toHaveText(ciNext);
    await visualPass(page, 'hs2-v7c7mm', 'ci-and-deploy-failed', {
      show: section,
      check: () => expect(line).toHaveText('CI failed'),
      atPhone: () => card.click(),
    });

    // 5. The fix's CI passes: the CI warning goes and the failed deployment is named again.
    await ci(33, fix, 'success');
    await expect(row(gates, `CI · ${fix.slice(0, 7)}`)).toHaveText('Passed', { timeout: 60_000 });
    await expect(row(gates, `CI · ${change.slice(0, 7)}`)).toHaveCount(0);
    await expect(line).toHaveText('Deploy failed', { timeout: 30_000 });
    await expect(inspector).toContainText('Done · Deploy failed');
    await expect(nextRow).toHaveText(deployNext);

    // 6. A new request in the same environment, which a person allows, runs the tracked script and passes:
    // it replaces the failed deployment and the card reads plainly Done.
    const again = await deploy('deploy-prod-2');
    expect(again.gate.state).toBe('awaiting_approval');
    await answer(again.request_id ?? '', 'allow_once');
    await deploy('deploy-prod-2');
    await expect
      .poll(
        async () => {
          await send(server, 'POST', `${base}/runs/${run.run_id}/gate-sync`, { actor: scheduler, wait_ms: 5_000 });
          const read = await send(server, 'GET', `${base}/gates/${again.gate.gate_id}`);
          return read.state;
        },
        { timeout: 60_000, intervals: [0] },
      )
      .toBe('passed');
    await expect(events).toContainText('Deployment gate: passed', { timeout: 30_000 });
    await expect(line).toHaveCount(0, { timeout: 30_000 });
    await expect(inspector).not.toContainText('Deploy failed');
    await expect(inspector).not.toContainText('CI failed');
    await expect(view.locator('[aria-label="Done Column"]')).toContainText(plan.first.slug);
    expect(provider.requests.every((request) => request.startsWith('GET '))).toBe(true);
  } finally {
    await execute(server.cli, ['job', 'watch', 'stop'], { cwd: server.root, env: server.env }).catch(() => undefined);
    await server.stop(page);
    await provider.close();
  }
});
