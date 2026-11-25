import { execFile } from 'node:child_process';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { promisify } from 'node:util';

import { expect, type Page, test } from '@playwright/test';

import { withoutGitRepositoryEnv } from '../scripts/repository-env.mjs';
import { fakeGithubActions } from './fake-github-actions';
import {
  capture,
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
import { proxyFetch } from './proxy-fetch';

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
    await cardLine.scrollIntoViewIfNeeded();
    await expect(cardLine).toBeInViewport();
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
 * HS2-65AD6J: why integration is or is not running. The real server answers a running step's explanation with
 * its project's integrator (`none` here: nothing is handed off). A browser fixture cannot hand work off to a real
 * pool, so the not-started and unavailable states are the real responses with only the server's own documented
 * `integrator` and `roles_unplaced` shapes (docs/26 §26.30) put in: synthetic codes, no other field changed.
 */
test('HS2-65AD6J a running card says whether its integrator runs and the overview names role sessions not started', async ({
  page,
}) => {
  test.setTimeout(180_000);
  const server = await realTicketServer();
  try {
    await orchestrateSelf(server);
    const plan = await planTwoSteps(server);
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    await send(server, 'PUT', `${base}/policy`, {
      expected_revision: null,
      policy: {
        tools: [{ tool: 'synthetic-tool', max_sessions: 2 }],
        targets: [{ target_id: TARGET, tools: [{ tool: 'synthetic-tool' }], max_sessions: 2 }],
        placement: { automatic: false, actor: 'scheduler-1' },
      },
      actor: { role: 'human', id: 'maintainer' },
    });
    await plan.dispatch();
    await page.setViewportSize({ width: 1440, height: 900 });
    await openOrchestration(page, server);
    const view = page.locator('[data-orchestration-view="true"]');

    // What the server's pass would say once a handoff waits: put into the real responses.
    const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
    let integrator: Record<string, unknown> | undefined;
    let explainedIntegrator: unknown;
    const augment =
      (change: (body: Record<string, unknown>) => Record<string, unknown>) =>
      async (route: Parameters<Parameters<Page['route']>[1]>[0]) => {
        const incoming = new URL(route.request().url());
        const response = await proxyFetch(route, {
          url: `${server.url}${incoming.pathname.slice(apiPath.length)}${incoming.search}`,
          headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
          timeout: 60_000,
        });
        const body = (await response.json()) as Record<string, unknown>;
        await route.fulfill({ response, json: change(body) });
      };
    await page.route(
      '**/orchestration/scheduler/explain',
      augment((body) => {
        explainedIntegrator = body.integrator;
        return integrator && body.integrator ? { ...body, integrator } : body;
      }),
    );
    await page.route(
      '**/orchestration/scheduler/plan',
      augment((body) => ({
        ...body,
        roles_unplaced: [
          {
            target_id: TARGET,
            role: 'integrator',
            reason: 'integrator_unavailable',
            detail: 'replacements_exhausted',
            retry_at_ms: Number(body.now_ms) + 3_600_000,
          },
          {
            target_id: TARGET,
            role: 'dispatcher',
            reason: 'dispatcher_unavailable',
            detail: 'replacements_exhausted',
            retry_at_ms: Number(body.now_ms) + 3_600_000,
          },
        ],
      })),
    );
    // A change to the plan reads the timeline, and with it the overview, again.
    const another = await server.request<{ id: string; slug: string }>('/tickets', 'POST', {
      title: 'Add the synthetic audit log',
    });
    await send(server, 'POST', `${base}/readiness/define`, {
      ticket: { native_id: plan.goal.id },
      target_id: TARGET,
      connection_id: plan.first.connection_id,
      native_id: another.id,
      prerequisites: [],
      actor: scheduler,
    });

    // The overview: each role session the pass does not start, and why.
    const roles = page.locator('[data-orchestration-overview] [data-orchestration-section="Coordination Sessions"]');
    await expect(roles).toContainText('Self Integrator', { timeout: 30_000 });
    await expect(roles).toContainText('Unavailable: the integrators the scheduler started did not take up their role');
    await expect(roles).toContainText('Self Dispatcher');
    await expect(roles).toContainText('Unavailable: the dispatchers the scheduler started did not take up their role');
    await expect(roles).toContainText('looked at again by');
    await expect(roles).not.toContainText('_');
    await visualPass(page, 'hs2-65ad6j', 'overview-roles', {
      show: roles,
      check: () => expect(roles).toContainText('Self Dispatcher'),
      atPhone: () => openInspector(page),
    });

    // The running card, as the real server answers it: no handoff waits, so no integrator runs.
    const running = view.locator(`[data-ticket-slug="${plan.first.slug}"]`);
    const waiting = view.locator(`[data-ticket-slug="${plan.second.slug}"]`);
    const integration = page.locator('[data-orchestration-inspector] [data-orchestration-section="Integration"]');
    await running.click();
    await expect(integration).toContainText('Not running');
    await expect(integration).toContainText('No handoff waits to be integrated');
    expect(explainedIntegrator).toMatchObject({ target_id: TARGET, standing: 'none' });
    const order = await page
      .locator('[data-orchestration-inspector] [data-orchestration-section]')
      .evaluateAll((nodes) => nodes.map((node) => node.getAttribute('data-orchestration-section')));
    expect(order.indexOf('Integration')).toBe(order.indexOf('Run') + 1);
    // A card without a run has no Integration section.
    await waiting.click();
    await expect(page.locator('[data-orchestration-inspector]')).toContainText('Prerequisites');
    await expect(integration).toHaveCount(0);

    // Each standing a fresh pass can give, read again when the card is selected again.
    const reselect = async () => {
      await waiting.click();
      await expect(integration).toHaveCount(0);
      await running.click();
    };
    integrator = {
      target_id: TARGET,
      standing: 'not_started',
      reason: 'integrator_not_started',
      detail: 'policy_opt_out',
    };
    await reselect();
    await expect(integration).toContainText('Not started');
    await expect(integration).toContainText('integrates by hand');
    integrator = {
      target_id: TARGET,
      standing: 'keep',
      reason: 'integrator_kept',
      detail: 'ci_pending',
      session_id: 'sched-role-integrator-synthetic',
    };
    await reselect();
    await expect(integration).toContainText('Running');
    await expect(integration).toContainText('An integrated commit waits on its CI gate');
    await expect(integration).toContainText('sched-role-integrator-synthetic');
    integrator = {
      target_id: TARGET,
      standing: 'not_started',
      reason: 'integrator_unavailable',
      detail: 'replacements_exhausted',
      retry_at_ms: Date.now() + 3_600_000,
    };
    await reselect();
    await expect(integration).toContainText('Unavailable');
    await expect(integration).toContainText('did not take up their role');
    await expect(integration).toContainText('Looked at again by');
    await expect(integration).not.toContainText('_');
    await visualPass(page, 'hs2-65ad6j', 'integration-unavailable', {
      show: integration,
      check: () => expect(integration).toContainText('Unavailable'),
      atPhone: () => running.click(),
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
