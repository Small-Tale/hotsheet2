import { execFile } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync } from 'node:fs';
import { mkdir, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';

import { expect, test } from '@playwright/test';
import Ajv2020 from 'ajv/dist/2020.js';
import WebSocket from 'ws';
import YAML from 'yaml';

import { withoutGitRepositoryEnv } from '../scripts/repository-env.mjs';
import { routeMethods } from '../scripts/sync-openapi-routes.mjs';
import { realTicketServer } from './real-ticket-server';

function contract(name: string): Record<string, unknown> {
  return JSON.parse(
    readFileSync(fileURLToPath(new URL(`../../../docs/contracts/v1/${name}`, import.meta.url)), 'utf8'),
  ) as Record<string, unknown>;
}

test('all HTTP routes have concrete, compilable OpenAPI schemas', () => {
  const openapi = YAML.parse(
    readFileSync(fileURLToPath(new URL('../../../docs/contracts/v1/openapi.yaml', import.meta.url)), 'utf8'),
  ) as {
    paths: Record<
      string,
      Record<string, { operationId: string; responses: object; 'x-hotsheet-contract-depth'?: string }>
    >;
    components: { schemas: Record<string, object> };
  };
  const router = routeMethods(
    readFileSync(fileURLToPath(new URL('../../../crates/hotsheet-server/src/lib.rs', import.meta.url)), 'utf8'),
  );
  expect(Object.keys(openapi.paths).sort()).toEqual([...router.keys()].sort());
  // HS2-RX1ET8: the exact path comparison above pins the inventory; no hand-bumped count. The
  // README route count is checked against the contract by scripts/check-openapi-routes.mjs (HS2-ZVB932).
  expect(router.size).toBeGreaterThan(0);
  for (const [path, methods] of router)
    for (const [method] of Object.entries(methods)) {
      const operation = openapi.paths[path][method];
      expect(operation.operationId, `${method.toUpperCase()} ${path}`).toBeTruthy();
      expect(operation.responses, `${method.toUpperCase()} ${path}`).toBeDefined();
      expect(operation['x-hotsheet-contract-depth'], `${method.toUpperCase()} ${path}`).toBeUndefined();
    }
  const ajv = new Ajv2020({ strict: false });
  for (const [name, value] of Object.entries(openapi.components.schemas))
    expect(() => ajv.compile({ ...value, components: openapi.components }), name).not.toThrow();
});

test('client contracts accept real ticket and replay responses', async () => {
  const openapi = YAML.parse(
    readFileSync(fileURLToPath(new URL('../../../docs/contracts/v1/openapi.yaml', import.meta.url)), 'utf8'),
  ) as {
    openapi: string;
    paths: Record<string, unknown>;
    components: { schemas: Record<string, object>; parameters: Record<string, { name: string } | undefined> };
  };
  expect(openapi.openapi).toBe('3.1.0');
  const router = routeMethods(
    readFileSync(fileURLToPath(new URL('../../../crates/hotsheet-server/src/lib.rs', import.meta.url)), 'utf8'),
  );
  expect(Object.keys(openapi.paths).sort()).toEqual([...router.keys()].sort());
  // HS2-RX1ET8: the exact path comparison above pins the inventory; no hand-bumped count. The
  // README route count is checked against the contract by scripts/check-openapi-routes.mjs (HS2-ZVB932).
  expect(router.size).toBeGreaterThan(0);
  for (const [path, methods] of router) {
    const operations = openapi.paths[path] as Record<string, unknown>;
    const parameterNames = new Set(
      ((operations.parameters ?? []) as Array<{ name?: string; $ref?: string }>).map(
        (parameter) => parameter.name ?? openapi.components.parameters[parameter.$ref?.split('/').at(-1) ?? '']?.name,
      ),
    );
    for (const [, name] of path.matchAll(/\{([^}]+)\}/g))
      expect(parameterNames.has(name), `${path}: ${name}`).toBe(true);
    for (const [method] of Object.entries(methods)) {
      expect(operations[method], `${method.toUpperCase()} ${path}`).toBeDefined();
      const operation = operations[method] as Record<string, unknown>;
      expect(operation.operationId, `${method.toUpperCase()} ${path}`).toBeTruthy();
      expect(operation['x-hotsheet-contract-depth'], `${method.toUpperCase()} ${path}`).toBeUndefined();
      expect(operation.responses, `${method.toUpperCase()} ${path}`).toBeDefined();
    }
  }
  const ids = Object.values(openapi.paths).flatMap((value) =>
    Object.entries(value as Record<string, unknown>)
      .filter(([method]) => ['get', 'post', 'put', 'patch', 'delete'].includes(method))
      .map(([, operation]) => (operation as { operationId: string }).operationId),
  );
  expect(new Set(ids).size).toBe(ids.length);

  const ajv = new Ajv2020({ strict: false });
  for (const [name, value] of Object.entries(openapi.components.schemas))
    expect(() => ajv.compile({ ...value, components: openapi.components }), name).not.toThrow();
  ajv.addSchema(contract('change-event.schema.json'));
  const change = ajv.getSchema(
    'https://raw.githubusercontent.com/Small-Tale/hotsheet2/main/docs/contracts/v1/change-event.schema.json',
  );
  expect(change).toBeDefined();
  const poll = ajv.compile(contract('poll-envelope.schema.json'));
  const schema = (name: string) => ({ ...openapi.components.schemas[name], components: openapi.components });
  const validateResponse = (path: string, value: unknown, status = '200') => {
    const operation = (
      openapi.paths[path] as Record<
        string,
        { responses: Record<string, { content: Record<string, { schema: object }> }> }
      >
    ).get;
    const responseSchema = operation.responses[status].content['application/json'].schema;
    const validate = ajv.compile({ ...responseSchema, components: openapi.components });
    expect(validate(value), `${path}: ${JSON.stringify(validate.errors)}`).toBe(true);
  };
  const compatibility = ajv.compile(schema('Compatibility'));
  const ticket = ajv.compile(schema('Ticket'));
  const row = ajv.compile(schema('TicketRow'));
  const createRequest = ajv.compile(schema('CreateTicket'));
  const updateRequest = ajv.compile(schema('UpdateTicket'));
  const claimRequest = ajv.compile(schema('ClaimTicket'));
  const apiError = ajv.compile(schema('ApiError'));
  const typedTicketRoutes = [
    ['/tickets', 'get'],
    ['/tickets', 'post'],
    ['/tickets/{id}', 'get'],
    ['/tickets/{id}', 'patch'],
    ['/stores/{store_id}/tickets', 'get'],
    ['/stores/{store_id}/tickets', 'post'],
    ['/stores/{store_id}/tickets/{id}', 'get'],
    ['/stores/{store_id}/tickets/{id}', 'patch'],
    ['/checkouts/{reference}/tickets', 'post'],
    ['/tickets/{id}/notes/{note_id}', 'delete'],
    ['/claim-next', 'post'],
    ['/tickets/{id}/claim', 'post'],
    ['/tickets/{id}/release', 'post'],
    ['/tickets/{id}/renew', 'post'],
    ['/providers/{connection_id}/tickets/{id}/not-working', 'post'],
    ['/providers/{connection_id}/tickets/{id}/not-working-json', 'post'],
    ['/terminals/{id}/attach', 'get'],
  ] as const;
  for (const [path, method] of typedTicketRoutes) {
    const operation = (openapi.paths[path] as Record<string, Record<string, unknown>>)[method];
    expect(operation['x-hotsheet-contract-depth'], `${method.toUpperCase()} ${path}`).toBeUndefined();
    expect(operation.responses, `${method.toUpperCase()} ${path}`).toBeDefined();
  }
  const server = await realTicketServer();
  let socket: WebSocket | undefined;
  try {
    const ruleBody =
      '<!-- hotsheet:begin section=contract-rule v=1 -->\n\nSynthetic contract guidance.\n\n<!-- hotsheet:end section=contract-rule -->\n';
    await mkdir(join(server.root, '.hotsheet2'), { recursive: true });
    await writeFile(
      join(server.root, '.hotsheet2', 'guidance-sources.json'),
      JSON.stringify({
        schema_version: 1,
        explicit_selection: false,
        sources: [
          {
            id: 'fixture',
            repository: 'fixture',
            requested_ref: 'v1',
            commit: 'a'.repeat(40),
            selected_rules: [],
            selected_skills: [],
            skills: [],
            rules: [
              {
                name: 'contract-rule',
                version: 1,
                sha256: createHash('sha256').update(ruleBody).digest('hex'),
                description: 'Synthetic preview',
                owner: null,
                tags: [],
                body: ruleBody,
                known_bodies: [],
              },
            ],
          },
        ],
      }),
    );
    const metadata = await server.request('/compatibility');
    expect(compatibility(metadata), JSON.stringify(compatibility.errors)).toBe(true);
    for (const path of [
      '/providers',
      '/commands',
      '/mcp-presence',
      '/analytics/tickets',
      '/analytics/usage',
      '/live-state',
      '/notifications',
      '/host-health',
      '/host-health/cpu',
      '/host-health/thresholds',
      '/standby-settings',
      '/feature-settings',
      `/checkouts/${server.checkoutId}/providers`,
      `/checkouts/${server.checkoutId}/repository/status`,
      `/checkouts/${server.checkoutId}/guidance`,
      `/checkouts/${server.checkoutId}/guidance/updates`,
    ]) {
      const documentedPath = path.replace(server.checkoutId, '{reference}');
      validateResponse(documentedPath, await server.request(path));
    }
    // HS2-M8HHEA: a CI report through the real server is stored and shows in the live state.
    const ciReport = {
      status: 'success',
      branch: 'main',
      sha: 'abcdef1',
      workflow: 'CI',
      started_at_ms: 1_000,
      finished_at_ms: 2_000,
    };
    const ciRequest = ajv.compile(schema('CiReport'));
    expect(ciRequest(ciReport), JSON.stringify(ciRequest.errors)).toBe(true);
    const ciResponse = ajv.compile(schema('CiReportResponse'));
    const reported = await server.request(`/checkouts/${server.checkoutId}/ci-status`, 'POST', ciReport);
    expect(ciResponse(reported), JSON.stringify(ciResponse.errors)).toBe(true);
    expect(reported).toMatchObject({ applied: true, ci: { status: 'success', checkout: server.checkoutId } });
    // HS2-D3JJ49: a run in flight carries its verification and trust bound; the live state lists each run.
    const running = await server.request(`/checkouts/${server.checkoutId}/ci-status`, 'POST', {
      ...ciReport,
      status: 'in_progress',
      workflow: 'Platforms',
      finished_at_ms: null,
    });
    expect(ciResponse(running), JSON.stringify(ciResponse.errors)).toBe(true);
    expect(running).toMatchObject({ ci: { verification: 'verified', workflow: 'Platforms' } });
    const live = await server.request<{ stores: { ci_runs: unknown[] }[] }>('/live-state');
    validateResponse('/live-state', live);
    expect(live.stores[0]?.ci_runs).toHaveLength(2);
    const cleared = await server.request(
      `/checkouts/${server.checkoutId}/ci-status?reported_at_ms=${String((running as { ci: { reported_at_ms: number } }).ci.reported_at_ms)}&workflow=Platforms`,
      'DELETE',
    );
    const clearResponse = ajv.compile(schema('CiClearResponse'));
    expect(clearResponse(cleared), JSON.stringify(clearResponse.errors)).toBe(true);
    expect(cleared).toMatchObject({ cleared: true, runs: [{ workflow: 'CI' }] });
    const rules = await fetch(`${server.url}/permissions/rules`, {
      headers: { 'X-Hotsheet-Secret': server.secret },
    });
    const rulesBody: unknown = await rules.json();
    expect([200, 503]).toContain(rules.status);
    validateResponse('/permissions/rules', rulesBody, rules.status === 200 ? '200' : '503');
    const previewPath = `/checkouts/${server.checkoutId}/guidance/rules/fixture/contract-rule/preview`;
    const preview = await server.request(previewPath);
    validateResponse('/checkouts/{reference}/guidance/rules/{source}/{rule}/preview', preview);
    expect(preview).toMatchObject({ owner: 'builtin', current: null, pinned: ruleBody, outcome: 'add' });
    const missingRule = await fetch(
      `${server.url}/checkouts/${server.checkoutId}/guidance/rules/fixture/unknown/preview`,
      { headers: { 'X-Hotsheet-Secret': server.secret } },
    );
    expect(missingRule.status).toBe(409);
    expect(apiError(await missingRule.json()), JSON.stringify(apiError.errors)).toBe(true);
    const handshake = await server.request<{ cursor: number }>(`/ws/poll?timeout_ms=0`);
    expect(poll(handshake), JSON.stringify(poll.errors)).toBe(true);
    socket = new WebSocket(`${server.url.replace(/^http/, 'ws')}/ws/sync?secret=${server.secret}`);
    await new Promise<void>((resolve, reject) => {
      socket?.once('open', resolve);
      socket?.once('error', reject);
    });
    const frame = new Promise<unknown>((resolve, reject) => {
      socket?.once('message', (message) => {
        const text = Array.isArray(message)
          ? Buffer.concat(message).toString('utf8')
          : message instanceof ArrayBuffer
            ? Buffer.from(new Uint8Array(message)).toString('utf8')
            : message.toString('utf8');
        resolve(JSON.parse(text) as unknown);
      });
      socket?.once('error', reject);
    });

    const create = { title: 'Contract response' };
    expect(createRequest(create), JSON.stringify(createRequest.errors)).toBe(true);
    const created = await server.request<{ id: string }>('/tickets', 'POST', create);
    expect(ticket(created), JSON.stringify(ticket.errors)).toBe(true);
    const patch = { details: 'Contract body' };
    expect(updateRequest(patch), JSON.stringify(updateRequest.errors)).toBe(true);
    const updated = await server.request(`/tickets/${created.id}`, 'PATCH', patch);
    expect(ticket(updated), JSON.stringify(ticket.errors)).toBe(true);
    const claim = { worker: 'contract-test', lease_minutes: 5 };
    expect(claimRequest(claim), JSON.stringify(claimRequest.errors)).toBe(true);
    const claimed = await server.request(`/tickets/${created.id}/claim`, 'POST', claim);
    expect(ticket(claimed), JSON.stringify(ticket.errors)).toBe(true);
    const missing = await fetch(`${server.url}/tickets/no-such-ticket`, {
      headers: { 'X-Hotsheet-Secret': server.secret },
    });
    expect(missing.status).toBe(404);
    expect(apiError(await missing.json()), JSON.stringify(apiError.errors)).toBe(true);
    for (const path of [
      '/notifications/no-such-notification/ack',
      `/checkouts/${server.checkoutId}/command-runs/no-such-run`,
      `/checkouts/${server.checkoutId}/tickets/no-such-ticket`,
    ]) {
      const response = await fetch(`${server.url}${path}`, {
        method: path.includes('/ack') ? 'POST' : 'GET',
        headers: { 'X-Hotsheet-Secret': server.secret },
      });
      expect(response.status, path).toBe(404);
      expect(apiError(await response.json()), `${path}: ${JSON.stringify(apiError.errors)}`).toBe(true);
    }
    const pushed = await frame;
    expect(change?.(pushed), JSON.stringify(change?.errors)).toBe(true);
    const fetched = await server.request(`/tickets/${created.id}`);
    expect(ticket(fetched), JSON.stringify(ticket.errors)).toBe(true);
    const rows = await server.request<unknown[]>(`/checkouts/${server.checkoutId}/tickets`);
    expect(rows.length).toBe(1);
    expect(row(rows[0]), JSON.stringify(row.errors)).toBe(true);

    const replay = await server.request<{ events: unknown[] }>(`/ws/poll?since=${handshake.cursor}&timeout_ms=0`);
    expect(poll(replay), JSON.stringify(poll.errors)).toBe(true);
    expect(replay.events.some((event) => (event as { kind?: string }).kind === 'created')).toBe(true);
  } finally {
    socket?.close();
    await server.stop();
  }
});

/**
 * HS2-3KNAPN: the WorkerView schema forbids unknown members, so a server field the contract
 * misses fails every real response. Validate real `/workers` (default and `include_past`) and
 * `/integration-strategies` and `/development-strategy` responses from a seeded throwaway pool: worker preferences with a
 * refused request, unmatched tickets, a departed (past) worker, and a batch policy with
 * circuit-breaker thresholds.
 */
test('HS2-3KNAPN real worker view and integration strategy responses match the OpenAPI contract', async () => {
  test.setTimeout(90_000);
  const openapi = YAML.parse(
    readFileSync(fileURLToPath(new URL('../../../docs/contracts/v1/openapi.yaml', import.meta.url)), 'utf8'),
  ) as {
    paths: Record<string, { get: { responses: Record<string, { content: Record<string, { schema: object }> }> } }>;
    components: object;
  };
  const ajv = new Ajv2020({ strict: false, allErrors: true });
  const validator = (path: string) =>
    ajv.compile({
      ...openapi.paths[path].get.responses['200'].content['application/json'].schema,
      components: openapi.components,
    });
  const workerView = validator('/checkouts/{reference}/workers');
  const strategyReport = validator('/checkouts/{reference}/integration-strategies');
  const developmentReport = validator('/checkouts/{reference}/development-strategy');
  const run = promisify(execFile);
  let refused = '';
  const server = await realTicketServer({
    seed: async ({ store, root, env, cli }) => {
      // The pool's identities are explicit; an inherited AI-session identity must not leak in.
      const seedEnv = {
        ...withoutGitRepositoryEnv(env),
        HOTSHEET_WORKER_ID: undefined,
        HOTSHEET_ACTOR_ROLE: undefined,
        HOTSHEET_ACTOR_ID: undefined,
      };
      const git = (...args: string[]) => run('git', args, { cwd: root, env: seedEnv });
      const hs = async (...args: string[]) =>
        (await run(cli, ['-C', store, ...args], { cwd: root, env: seedEnv })).stdout;
      const pool = (...args: string[]) => hs('coordination', '--project', root, ...args);
      await git('init', '-q', '-b', 'main');
      await git('commit', '-q', '--allow-empty', '-m', 'synthetic base');
      await mkdir(join(root, '.github/workflows'), { recursive: true });
      await writeFile(join(root, '.github/workflows/ci.yml'), 'name: CI\non: push\n');
      await git('remote', 'add', 'origin', 'https://example.invalid/synthetic.git');
      // HS2-83KFB5: an external source coordination cannot cover by design, and one whose
      // metadata cannot be read, so the views carry real coverage gaps. No secret resolves.
      await writeFile(
        join(store, 'providers.json'),
        JSON.stringify({
          connections: [
            {
              id: 'github-contract',
              provider: 'github',
              locator: 'synthetic/contract',
              name: null,
              default: false,
              disabled: false,
              settings: { api_base: 'http://127.0.0.1:9', credential: { secret: 'must-not-resolve' } },
            },
            {
              id: 'gitlab-unreadable',
              provider: 'gitlab',
              locator: '',
              name: null,
              default: false,
              disabled: false,
              settings: {},
            },
          ],
        }),
      );
      await pool('setup');
      await pool(
        'strategy-allow',
        '--strategy',
        'batch',
        '--reason',
        'contract fixture',
        '--max-red-batches',
        '2',
        '--max-red-minutes',
        '45',
      );
      await pool('strategy-allow', '--strategy', 'ci-staged', '--reason', 'synthetic cadence contract');
      const slugs: string[] = [];
      for (const [title, tags] of [
        ['Glance bug', ['glance']],
        ['Plain bug', []],
        ['Client bug', ['client']],
      ] as const) {
        const created = await hs('new', '--title', title, '--category', 'bug');
        const slug = /Created (\S+)/.exec(created)?.[1];
        if (!slug) throw new Error(`no slug in ${created}`);
        await hs('edit', slug, '--up-next', ...tags.flatMap((tag) => ['--tag', tag]));
        slugs.push(slug);
      }
      refused = slugs[1];
      const lease = ['--route', 'human', '--lease-seconds', '3600', '--all-participants-upgraded'];
      await pool(
        'register',
        '--role',
        'integrator',
        '--id',
        'integrator-1',
        '--session',
        'int-1',
        ...lease,
        '--strategy',
        'batch',
      );
      await pool(
        'register',
        '--role',
        'worker',
        '--id',
        'worker-glance',
        '--session',
        'w1',
        ...lease,
        '--only',
        'tag:glance',
        '--prefer',
        'priority:high',
      );
      await pool(
        'register',
        '--role',
        'worker',
        '--id',
        'worker-strict',
        '--session',
        'w2',
        ...lease,
        '--only',
        'category:feature',
        '--avoid',
        'tag:client',
        '--request',
        refused,
      );
      await pool(
        'refuse-request',
        '--id',
        'integrator-1',
        '--session',
        'int-1',
        '--worker',
        'worker-strict',
        '--worker-session',
        'w2',
        '--slug',
        refused,
        '--reason',
        'filtered',
        '--detail',
        'outside the strict filter',
      );
      // Expires into a past worker: the shortest lease the CLI accepts.
      await pool(
        'register',
        '--role',
        'worker',
        '--id',
        'worker-gone',
        '--session',
        'w3',
        '--route',
        'human',
        '--lease-seconds',
        '10',
        // Its own strict filter, so it never matches the plain or client tickets.
        '--only',
        'tag:glance',
        '--all-participants-upgraded',
      );
    },
  });
  try {
    type View = {
      past_workers: number;
      integration_strategy: string | null;
      unmatched_tickets: string[];
      uncovered_sources: string[];
      coverage_gaps: Array<{ connection_id: string; name: string; reason: string }>;
      workers: Array<{ id: string; availability: string; preferences: { refusal?: { slug: string } } | null }>;
    };
    const view = (query: string) => server.request<View>(`/checkouts/${server.checkoutId}/workers?wait_ms=0${query}`);
    const current = await view('');
    expect(workerView(current), JSON.stringify(workerView.errors)).toBe(true);
    expect(current.integration_strategy).toBe('batch');
    expect(current.unmatched_tickets.length).toBeGreaterThan(0);
    expect(current.workers.find((row) => row.id === 'worker-strict')?.preferences?.refusal?.slug).toBe(refused);
    expect(current.workers.find((row) => row.id === 'worker-glance')?.preferences).not.toBeNull();
    // HS2-83KFB5: every uncovered source appears once in coverage_gaps, named, with a reason.
    expect(current.coverage_gaps.map((gap) => gap.connection_id).sort()).toEqual([...current.uncovered_sources].sort());
    expect(current.coverage_gaps.find((gap) => gap.connection_id === 'github-contract')).toMatchObject({
      name: 'GitHub synthetic/contract',
      reason: 'unsupported',
    });
    expect(current.coverage_gaps.find((gap) => gap.connection_id === 'gitlab-unreadable')?.reason).toBe('failed_read');

    // Wait on state: the short-lease worker becomes a past worker once its lease expires.
    await expect
      .poll(async () => (await view('')).past_workers, { timeout: 30_000, intervals: [500] })
      .toBeGreaterThan(0);
    const withPast = await view('&include_past=true');
    expect(workerView(withPast), JSON.stringify(workerView.errors)).toBe(true);
    expect(withPast.workers.find((row) => row.id === 'worker-gone')?.availability).toBe('disconnected');
    const hidden = await view('');
    expect(workerView(hidden), JSON.stringify(workerView.errors)).toBe(true);
    expect(hidden.workers.some((row) => row.id === 'worker-gone')).toBe(false);

    const strategies = await server.request<{ policy: { allowed: Array<{ circuit_breaker?: object }> } }>(
      `/checkouts/${server.checkoutId}/integration-strategies`,
    );
    expect(strategyReport(strategies), JSON.stringify(strategyReport.errors)).toBe(true);
    expect(strategies.policy.allowed[0]?.circuit_breaker).toEqual({ max_red_batches: 2, max_red_minutes: 45 });
    const development = await server.request<{
      policy: { version: number };
      effective: { preset: string; source: string; integration: string; local_gate: string };
    }>(`/checkouts/${server.checkoutId}/development-strategy`);
    expect(developmentReport(development), JSON.stringify(developmentReport.errors)).toBe(true);
    expect(development.policy.version).toBe(1);
    expect(development.effective).toMatchObject({
      preset: 'ticket-assured',
      source: 'unconfigured-project',
      integration: 'serial',
      local_gate: 'risk-selected',
    });
    const strict = {
      version: 2,
      default: 'ticket-assured',
      green_main_goal_minutes: 360,
      default_custom: 'strict-review',
      custom: {
        'strict-review': {
          integration: 'serial',
          local_gate: 'full-local',
          minimum_tier: 'T2',
          browser: true,
          platform: true,
          perf: false,
          test_plan_timing: 'before-work',
          ci_gate: 'CI',
          red_ci_recovery: 'attribute-and-retry',
        },
      },
    };
    const customReport = await server.request<unknown>(
      `/checkouts/${server.checkoutId}/development-strategy`,
      'PUT',
      strict,
    );
    expect(developmentReport(customReport), JSON.stringify(developmentReport.errors)).toBe(true);
    expect(customReport).toMatchObject({ effective: { preset: 'strict-review', source: 'project' } });
    const sessionReport = await server.request<unknown>(
      `/checkouts/${server.checkoutId}/development-strategy?session=strict-review`,
    );
    expect(developmentReport(sessionReport), JSON.stringify(developmentReport.errors)).toBe(true);
    expect(sessionReport).toMatchObject({ effective: { preset: 'strict-review', source: 'session' } });
    const ticketReport = await server.request<unknown>(
      `/checkouts/${server.checkoutId}/development-strategy?session=steady-main&ticket=strict-review`,
    );
    expect(developmentReport(ticketReport), JSON.stringify(developmentReport.errors)).toBe(true);
    expect(ticketReport).toMatchObject({ effective: { preset: 'strict-review', source: 'ticket' } });
    await server.request(`/checkouts/${server.checkoutId}/development-strategy`, 'PUT', {
      version: 1,
      default: 'steady-main',
      green_main_goal_minutes: 90,
    });
    const cadenceView = await view('');
    expect(workerView(cadenceView), JSON.stringify(workerView.errors)).toBe(true);
    expect(cadenceView).toMatchObject({
      cadence: {
        goal_minutes: 90,
        last_green_main_at: null,
        next_target_at: null,
        action: 'no_ready_work',
      },
    });
  } finally {
    await server.stop();
  }
});

/**
 * HS2-FMHH9Y: the resource-claim schemas forbid unknown members, so a field the contract
 * misses fails a real response. Walk acquire, conflict, show, list, wait, token-only renew
 * and release on a real server, and check the renewal token is no credential elsewhere.
 */
/**
 * HS2-ZR8B4N: a real server reads this machine's provisional-claim and claim-conflict records from the store's Git
 * directory (`claim_gate::ClaimStandings`) and reports them as `claim_standing` on list rows and ticket details. The
 * records are written in the shape `claim_gate` writes them; the claims themselves go through the real claim route.
 */
test('HS2-ZR8B4N real provisional and lost claim standings match the OpenAPI contract', async () => {
  test.setTimeout(90_000);
  const openapi = YAML.parse(
    readFileSync(fileURLToPath(new URL('../../../docs/contracts/v1/openapi.yaml', import.meta.url)), 'utf8'),
  ) as { components: { schemas: Record<string, object> } };
  const ajv = new Ajv2020({ strict: false, allErrors: true });
  const schema = (name: string) => ajv.compile({ ...openapi.components.schemas[name], components: openapi.components });
  const ticketSchema = schema('Ticket'),
    rowSchema = schema('TicketRow');
  const server = await realTicketServer();
  try {
    const created = [];
    for (const title of ['Provisional work', 'Lost work', 'Exclusive work'])
      created.push(await server.request<{ id: string; slug: string }>('/tickets', 'POST', { title }));
    const [provisional, lost, exclusive] = created;
    for (const ticket of [provisional, exclusive])
      await server.request(`/tickets/${ticket.id}/claim`, 'POST', { worker: 'worker-a', lease_minutes: 30 });
    await server.request(`/tickets/${lost.id}/claim`, 'POST', { worker: 'worker-b', lease_minutes: 30 });
    const gitPath = async (name: string) =>
      (
        await promisify(execFile)('git', ['rev-parse', '--git-path', name], {
          cwd: server.store,
          env: withoutGitRepositoryEnv(process.env),
        })
      ).stdout.trim();
    const absolute = (path: string) => (path.startsWith('/') ? path : join(server.store, path));
    await writeFile(
      absolute(await gitPath('hotsheet-provisional-claims.json')),
      JSON.stringify({ [provisional.id]: { claim_id: '01JSYNTHETICATTEMPT0000000', worker: 'worker-a' } }),
    );
    await writeFile(
      absolute(await gitPath('hotsheet-claim-conflicts.json')),
      JSON.stringify([
        {
          ticket: lost.id,
          slug: lost.slug,
          worker: 'worker-a',
          holder: 'worker-b',
          detected_at: '2026-11-24T12:00:00Z',
        },
      ]),
    );
    const rows = await server.request<Array<{ id: string; claim_standing?: unknown }>>('/tickets');
    for (const value of rows) expect(rowSchema(value), JSON.stringify(rowSchema.errors)).toBe(true);
    const standing = (id: string) => rows.find((value) => value.id === id)?.claim_standing;
    expect(standing(provisional.id)).toEqual({ kind: 'provisional' });
    expect(standing(lost.id)).toEqual({
      kind: 'lost',
      worker: 'worker-a',
      holder: 'worker-b',
      detected_at: '2026-11-24T12:00:00Z',
    });
    expect(standing(exclusive.id)).toBeUndefined();
    for (const [ticket, expected] of [
      [provisional, { kind: 'provisional' }],
      [exclusive, undefined],
    ] as const) {
      const detail = await server.request<{ claim_standing?: unknown }>(`/tickets/${ticket.id}`);
      expect(ticketSchema(detail), JSON.stringify(ticketSchema.errors)).toBe(true);
      expect(detail.claim_standing).toEqual(expected);
    }
    // The checkout list a client reads carries the same member.
    const page = await server.request<{ tickets?: Array<{ id: string; claim_standing?: unknown }> }>(
      `/checkouts/${server.checkoutId}/tickets`,
    );
    const listed = (page.tickets ?? (page as unknown as Array<{ id: string; claim_standing?: unknown }>)).find(
      (value) => value.id === provisional.id,
    );
    expect(listed?.claim_standing).toEqual({ kind: 'provisional' });
    // Released, the provisional record no longer marks the ticket.
    await server.request(`/tickets/${provisional.id}/release`, 'POST', { worker: 'worker-a' });
    expect((await server.request<{ claim_standing?: unknown }>(`/tickets/${provisional.id}`)).claim_standing).toBe(
      undefined,
    );
  } finally {
    await server.stop();
  }
});

test('HS2-FMHH9Y real resource-claim responses match the OpenAPI contract', async () => {
  test.setTimeout(90_000);
  const openapi = YAML.parse(
    readFileSync(fileURLToPath(new URL('../../../docs/contracts/v1/openapi.yaml', import.meta.url)), 'utf8'),
  ) as {
    paths: Record<
      string,
      Record<string, { responses: Record<string, { content: Record<string, { schema: object }> }> }>
    >;
    components: object;
  };
  const ajv = new Ajv2020({ strict: false, allErrors: true });
  const matches = (path: string, method: string, status: number, value: unknown) => {
    const validate = ajv.compile({
      ...openapi.paths[path][method].responses[String(status)].content['application/json'].schema,
      components: openapi.components,
    });
    expect(validate(value), `${method} ${path} ${status}: ${JSON.stringify(validate.errors)}`).toBe(true);
  };
  const server = await realTicketServer();
  try {
    const send = async (method: string, path: string, headers: Record<string, string>, body?: unknown) => {
      const response = await fetch(`${server.url}${path}`, {
        method,
        headers: { ...(body === undefined ? {} : { 'content-type': 'application/json' }), ...headers },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      return { status: response.status, body: (await response.json()) as Record<string, unknown> };
    };
    const secret = { 'X-Hotsheet-Secret': server.secret };
    const target = '/resource-claims/{namespace}/{resource}';

    const grant = await send('POST', '/resource-claims/vms/mac-1/acquire', secret, {
      duration_ms: 600_000,
      metadata: { purpose: 'contract', worker: 'hmac-sha256:synthetic-worker', label: 'enc:v1:Synthetic' },
    });
    expect(grant.status).toBe(200);
    matches(`${target}/acquire`, 'post', 200, grant.body);
    expect(grant.body.acquired).toBe(true);
    const token = grant.body.renewal_token as string;
    const bearer = { Authorization: `Bearer ${token}` };

    const refused = await send('POST', '/resource-claims/vms/mac-1/acquire', secret, {
      duration_ms: 1_000,
      wait_ms: 200,
    });
    expect(refused.status).toBe(200);
    matches(`${target}/acquire`, 'post', 200, refused.body);
    expect(refused.body.code).toBe('wait_timeout');

    const shown = await send('GET', '/resource-claims/vms/mac-1', secret);
    matches(target, 'get', 200, shown.body);
    expect(shown.body.held).toBe(true);
    const free = await send('GET', '/resource-claims/vms/mac-2', secret);
    matches(target, 'get', 200, free.body);
    expect(free.body.claim).toBeNull();
    const listed = await send('GET', '/resource-claims/vms', secret);
    matches('/resource-claims/{namespace}', 'get', 200, listed.body);
    const waited = await send('POST', '/resource-claims/vms/mac-1/wait', secret, { wait_ms: 100 });
    matches(`${target}/wait`, 'post', 200, waited.body);
    for (const answer of [refused, shown, listed, waited]) expect(JSON.stringify(answer.body)).not.toContain(token);

    const renewed = await send('POST', '/resource-claims/vms/mac-1/renew', secret, {
      renewal_token: token,
      duration_ms: 900_000,
    });
    matches(`${target}/renew`, 'post', 200, renewed.body);

    // The token alone works on its own route and nowhere else.
    const tokenRoute = '/resource-claim-token/{action}';
    const byToken = await send('POST', '/resource-claim-token/renew', bearer, { duration_ms: 1_200_000 });
    expect(byToken.status).toBe(200);
    matches(tokenRoute, 'post', 200, byToken.body);
    for (const path of ['/resource-claims/vms', '/tickets', '/checkouts'])
      expect((await send('GET', path, bearer)).status, path).toBe(401);
    const anonymous = await send('POST', '/resource-claim-token/renew', {}, { duration_ms: 1_000 });
    expect(anonymous.status).toBe(401);
    matches(tokenRoute, 'post', 401, anonymous.body);
    const wrongAction = await send('POST', '/resource-claim-token/acquire', bearer, { duration_ms: 1_000 });
    expect(wrongAction.status).toBe(501);
    matches(tokenRoute, 'post', 501, wrongAction.body);

    const released = await send('POST', '/resource-claim-token/release', bearer);
    expect(released.status).toBe(200);
    matches(tokenRoute, 'post', 200, released.body);
    expect(released.body.released).toBe(true);
    const revoked = await send('POST', '/resource-claims/vms/mac-1/release', secret, { renewal_token: token });
    expect(revoked.status).toBe(403);
    matches(`${target}/release`, 'post', 403, revoked.body);
    expect(revoked.body.code).toBe('token_invalid');
    const invalid = await send('POST', '/resource-claims/vms/mac-1/acquire', secret, { duration_ms: 0 });
    expect(invalid.status).toBe(400);
    matches(`${target}/acquire`, 'post', 400, invalid.body);
  } finally {
    await server.stop();
  }
});

test('HS2-J8PRTV real target capacity and admission responses match the OpenAPI contract', async () => {
  test.setTimeout(90_000);
  const openapi = YAML.parse(
    readFileSync(fileURLToPath(new URL('../../../docs/contracts/v1/openapi.yaml', import.meta.url)), 'utf8'),
  ) as {
    paths: Record<
      string,
      Record<string, { responses: Record<string, { content: Record<string, { schema: object }> }> }>
    >;
    components: object;
  };
  const ajv = new Ajv2020({ strict: false, allErrors: true });
  const matches = (path: string, method: string, status: string, value: unknown) => {
    const validate = ajv.compile({
      ...openapi.paths[path][method].responses[status].content['application/json'].schema,
      components: openapi.components,
    });
    expect(validate(value), `${method} ${path} ${status}: ${JSON.stringify(validate.errors)}`).toBe(true);
  };
  const server = await realTicketServer();
  try {
    const send = async (method: string, path: string, body?: unknown) => {
      const response = await fetch(`${server.url}${path}`, {
        method,
        headers: { 'content-type': 'application/json', 'X-Hotsheet-Secret': server.secret },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      return { status: response.status, body: (await response.json()) as Record<string, unknown> };
    };
    const capacity = '/checkouts/{checkout}/target-capacity';
    const admission = '/checkouts/{checkout}/target-admission';
    const hosted = `/checkouts/${server.checkoutId}`;

    // The real host's own reading: whatever it cannot measure is null, and the shape holds.
    const snapshot = await send('GET', `${hosted}/target-capacity?claim_namespaces=vms`);
    expect(snapshot.status).toBe(200);
    matches(capacity, 'get', '200', snapshot.body);
    expect(snapshot.body.checkout_id).toBe(server.checkoutId);
    expect(snapshot.body.sessions).toEqual([]);
    expect(snapshot.body.claims).toEqual([]);
    expect(snapshot.body.claim_namespaces).toEqual(['vms']);
    // No path of the fixture leaves the server with its capacity.
    for (const local of [server.root, server.store, server.home])
      expect(JSON.stringify(snapshot.body)).not.toContain(local);

    const refused = await send('POST', `${hosted}/target-admission`, { tool: 'synthetic-absent-tool', sessions: 2 });
    expect(refused.status).toBe(200);
    matches(admission, 'post', '200', refused.body);
    expect(refused.body.outcome).toBe('refused');
    expect(refused.body.admitted).toBe(0);
    expect(refused.body.reason).toBe('tool_not_installed');
    expect(refused.body.limits).toEqual({
      max_sessions: 2,
      min_free_ram_mb: 2048,
      max_cpu_percent: 90,
      min_free_disk_mb: 5120,
    });

    const invalid = await send('POST', `${hosted}/target-admission`, { tool: 'codex', sessions: 0 });
    expect(invalid.status).toBe(400);
    matches(admission, 'post', 'default', invalid.body);
    expect(invalid.body.code).toBe('invalid_request');
    const badNamespace = await send('GET', `${hosted}/target-capacity?claim_namespaces=Not_Valid`);
    expect(badNamespace.status).toBe(400);
    matches(capacity, 'get', 'default', badNamespace.body);
  } finally {
    await server.stop();
  }
});

test('HS2-W27N1D real project type and orchestration project responses match the OpenAPI contract', async () => {
  test.setTimeout(90_000);
  const openapi = YAML.parse(
    readFileSync(fileURLToPath(new URL('../../../docs/contracts/v1/openapi.yaml', import.meta.url)), 'utf8'),
  ) as {
    paths: Record<
      string,
      Record<string, { responses: Record<string, { content: Record<string, { schema: object }> }> }>
    >;
    components: object;
  };
  const ajv = new Ajv2020({ strict: false, allErrors: true });
  const matches = (path: string, method: string, status: string, value: unknown) => {
    const validate = ajv.compile({
      ...openapi.paths[path][method].responses[status].content['application/json'].schema,
      components: openapi.components,
    });
    expect(validate(value), `${method} ${path} ${status}: ${JSON.stringify(validate.errors)}`).toBe(true);
  };
  const server = await realTicketServer();
  try {
    const send = async (method: string, path: string, body?: unknown) => {
      const response = await fetch(`${server.url}${path}`, {
        method,
        headers: { 'content-type': 'application/json', 'X-Hotsheet-Secret': server.secret },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      return { status: response.status, body: (await response.json()) as Record<string, unknown> };
    };
    const root = join(dirname(server.root), 'target-project');
    mkdirSync(root, { recursive: true });
    const target = (await send('POST', '/checkouts', { root, stores: [server.store] })).body.id as string;
    const owner = `/checkouts/${server.checkoutId}/orchestration`;
    const type = '/checkouts/{owner}/orchestration/project-type';
    const projects = '/checkouts/{owner}/orchestration/projects';
    const project = '/checkouts/{owner}/orchestration/projects/{target}';

    const shown = await send('GET', `${owner}/project-type`);
    matches(type, 'get', '200', shown.body);
    expect(shown.body).toMatchObject({ project_type: 'software', declared: false, changed: false });
    const refusedAdd = await send('POST', `${owner}/projects`, { destination: { kind: 'local', checkout_id: target } });
    expect(refusedAdd.status).toBe(400);
    matches(projects, 'post', 'default', refusedAdd.body);
    const set = await send('PUT', `${owner}/project-type`, { project_type: 'orchestration' });
    matches(type, 'put', '200', set.body);
    const added = await send('POST', `${owner}/projects`, {
      destination: { kind: 'local', checkout_id: target },
      label: 'Synthetic target',
      actor: { role: 'human' },
    });
    expect(added.status).toBe(200);
    matches(projects, 'post', '200', added.body);
    for (const local of [server.root, server.store, server.home, root])
      expect(JSON.stringify(added.body)).not.toContain(local);
    const keep = await send('PUT', `${owner}/project-type`, { project_type: 'software' });
    expect(keep.status).toBe(400);
    matches(type, 'put', 'default', keep.body);
    const removed = await send('DELETE', `${owner}/projects/${added.body.target_id as string}`, {});
    matches(project, 'delete', '200', removed.body);
    expect(removed.body).toMatchObject({ unbound: true, removed: true });
  } finally {
    await server.stop();
  }
});

test('HS2-GDJXGZ real orchestration session and hosted target session responses match the OpenAPI contract', async () => {
  test.setTimeout(90_000);
  const openapi = YAML.parse(
    readFileSync(fileURLToPath(new URL('../../../docs/contracts/v1/openapi.yaml', import.meta.url)), 'utf8'),
  ) as {
    paths: Record<
      string,
      Record<string, { responses: Record<string, { content: Record<string, { schema: object }> }> }>
    >;
    components: object;
  };
  const ajv = new Ajv2020({ strict: false, allErrors: true });
  const matches = (path: string, method: string, status: string, value: unknown) => {
    const validate = ajv.compile({
      ...openapi.paths[path][method].responses[status].content['application/json'].schema,
      components: openapi.components,
    });
    expect(validate(value), `${method} ${path} ${status}: ${JSON.stringify(validate.errors)}`).toBe(true);
  };
  const server = await realTicketServer();
  try {
    const send = async (method: string, path: string, body?: unknown) => {
      const response = await fetch(`${server.url}${path}`, {
        method,
        headers: { 'content-type': 'application/json', 'X-Hotsheet-Secret': server.secret },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      return { status: response.status, body: (await response.json()) as Record<string, unknown> };
    };
    const hostedPath = '/checkouts/{checkout}/target-sessions';
    const hosted = `/checkouts/${server.checkoutId}/target-sessions`;
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    const person = { role: 'human', id: 'maintainer' };
    const origin = { owner_checkout_id: server.checkoutId, project: 'Synthetic project', run_id: null };

    // Nothing was started: the hosted listing is empty and names the pool's roles.
    const empty = await send('GET', `${hosted}?safe_point=true`);
    expect(empty.status).toBe(200);
    matches(hostedPath, 'get', '200', empty.body);
    expect(empty.body.sessions).toEqual([]);

    // A tool this machine does not have is the machine's own refusal, with its reason,
    // and nothing is reserved: the same request is refused the same way again.
    const start = { session: { operation_id: 'contract-op', tool: 'synthetic-absent-tool', origin }, actor: person };
    for (const attempt of ['first', 'repeated']) {
      const refused = await send('POST', hosted, start);
      expect(refused.status, attempt).toBe(200);
      matches(hostedPath, 'post', '200', refused.body);
      // HS2-8Z9VHH: a refused admission names its reason (here the tool that is not installed).
      expect(refused.body, attempt).toEqual({
        outcome: 'refused',
        reason: 'tool_not_installed',
        admission: { reason: 'tool_not_installed', tool: 'synthetic-absent-tool' },
      });
    }
    expect((await send('GET', hosted)).body.sessions).toEqual([]);

    // An operation this machine never saw: not stopped, with the code, and no session to take over.
    const unknown = await send('DELETE', hosted, { operation_id: 'contract-op', actor: person });
    expect(unknown.status).toBe(200);
    matches(hostedPath, 'delete', '200', unknown.body);
    // HS2-WHR4GS: the stop fences the operation so a late start launches nothing.
    expect(unknown.body).toEqual({ stopped: false, reason: 'session_not_found', fenced: true });
    const nobody = await send('PATCH', hosted, { operation_id: 'contract-op', pause: true, actor: person });
    expect(nobody.status).toBe(200);
    matches(hostedPath, 'patch', '200', nobody.body);
    expect(nobody.body).toEqual({});
    // Taking over is a person's act.
    const forbidden = await send('PATCH', hosted, { operation_id: 'contract-op', actor: { role: 'ai', id: 'w' } });
    expect(forbidden.status).toBe(403);
    matches(hostedPath, 'patch', 'default', forbidden.body);
    expect(forbidden.body.code).toBe('forbidden');
    const invalid = await send('POST', hosted, { session: { operation_id: 'has space', tool: 'codex', origin } });
    expect(invalid.status).toBe(400);
    matches(hostedPath, 'post', 'default', invalid.body);

    // The orchestrating side's records: none yet, a sync of nothing, and typed refusals.
    const sessions = '/checkouts/{owner}/orchestration/sessions';
    const listed = await send('GET', `${base}/sessions?active=true`);
    expect(listed.status).toBe(200);
    matches(sessions, 'get', '200', listed.body);
    expect(listed.body).toEqual([]);
    const synced = await send('POST', `${base}/session-sync`, { actor: { role: 'ai', id: 'scheduler-1' } });
    expect(synced.status).toBe(200);
    matches('/checkouts/{owner}/orchestration/session-sync', 'post', '200', synced.body);
    expect(synced.body).toEqual({ sessions: [] });
    const absent = '01ARZ3NDEKTSV4RRFFQ69G5FB0';
    const missing = await send('GET', `${base}/sessions/${absent}`);
    expect(missing.status).toBe(404);
    matches(`${sessions}/{session}`, 'get', 'default', missing.body);
    expect(missing.body.code).toBe('session_not_found');
    const operation = `${sessions}/{session}/{operation}`;
    for (const name of ['sync', 'safe-point', 'stop', 'takeover', 'hand-back']) {
      const answer = await send('POST', `${base}/sessions/${absent}/${name}`, { actor: person });
      expect(answer.status, name).toBe(404);
      matches(operation, 'post', 'default', answer.body);
      expect(answer.body.code, name).toBe('session_not_found');
    }
    // A start for a target nobody declared is denied before anything is recorded.
    const denied = await send('POST', `${base}/sessions`, {
      target_id: absent,
      operation_id: 'contract-start',
      tool: 'codex',
      actor: { role: 'ai', id: 'scheduler-1' },
    });
    expect(denied.status).toBe(403);
    matches(sessions, 'post', 'default', denied.body);
    expect((await send('GET', `${base}/sessions`)).body).toEqual([]);
  } finally {
    await server.stop();
  }
});

test('HS2-WN0MXZ real hosted validation responses match the OpenAPI contract', async () => {
  test.setTimeout(90_000);
  const openapi = YAML.parse(
    readFileSync(fileURLToPath(new URL('../../../docs/contracts/v1/openapi.yaml', import.meta.url)), 'utf8'),
  ) as {
    paths: Record<
      string,
      Record<string, { responses: Record<string, { content: Record<string, { schema: object }> }> }>
    >;
    components: object;
  };
  const ajv = new Ajv2020({ strict: false, allErrors: true });
  const matches = (path: string, method: string, status: string, value: unknown) => {
    const validate = ajv.compile({
      ...openapi.paths[path][method].responses[status].content['application/json'].schema,
      components: openapi.components,
    });
    expect(validate(value), `${method} ${path} ${status}: ${JSON.stringify(validate.errors)}`).toBe(true);
  };
  const server = await realTicketServer();
  try {
    const send = async (method: string, path: string, body?: unknown) => {
      const response = await fetch(`${server.url}${path}`, {
        method,
        headers: { 'content-type': 'application/json', 'X-Hotsheet-Secret': server.secret },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      return { status: response.status, body: (await response.json()) as Record<string, unknown> };
    };
    const validations = '/checkouts/{checkout}/target-validations';
    const hosted = `/checkouts/${server.checkoutId}/target-validations`;
    const execution = '01ARZ3NDEKTSV4RRFFQ69G5FE5';

    // This server process has started nothing under that id.
    const unknown = await send('GET', `${hosted}?execution_id=${execution}`);
    expect(unknown.status).toBe(200);
    matches(validations, 'get', '200', unknown.body);
    expect(unknown.body).toEqual({ execution_id: execution, state: 'unknown' });

    // The fixture project defines no profile, so nothing is launched on any platform
    // and the outcome says so with a code.
    const start = { execution_id: execution, check: { kind: 'profile', name: 'synthetic-absent' }, timeout_seconds: 5 };
    const finished = await send('POST', hosted, { ...start, wait_ms: 8000 });
    expect(finished.status).toBe(200);
    matches(validations, 'post', '200', finished.body);
    expect(finished.body.state).toBe('finished');
    expect((finished.body.outcome as { end: string }).end).toBe('profile_not_defined');
    // The same id again is the same execution, by either verb.
    const again = await send('GET', `${hosted}?execution_id=${execution}&wait_ms=0`);
    matches(validations, 'get', '200', again.body);
    expect(again.body).toEqual(finished.body);
    expect((await send('POST', hosted, start)).body).toEqual(finished.body);
    // HS2-17ABM2: stopping an execution that already ended answers its own outcome.
    const stopped = await send('DELETE', hosted, { execution_id: execution, wait_ms: 0 });
    expect(stopped.status).toBe(200);
    matches(validations, 'delete', '200', stopped.body);
    expect(stopped.body).toEqual(finished.body);
    const neverStarted = '01ARZ3NDEKTSV4RRFFQ69G5FE6';
    const unknownStop = await send('DELETE', hosted, { execution_id: neverStarted });
    matches(validations, 'delete', '200', unknownStop.body);
    expect(unknownStop.body).toEqual({ execution_id: neverStarted, state: 'unknown' });
    // HS2-YYNW9F: the fixture project offers no profiles, which is valid.
    const profilesPath = '/checkouts/{checkout}/target-validation-profiles';
    const offered = await send('GET', `/checkouts/${server.checkoutId}/target-validation-profiles`);
    expect(offered.status).toBe(200);
    matches(profilesPath, 'get', '200', offered.body);
    expect(offered.body).toEqual({ present: false, valid: true, profiles: [] });
    const late = await send('POST', hosted, { ...start, execution_id: neverStarted, wait_ms: 8000 });
    matches(validations, 'post', '200', late.body);
    expect((late.body.outcome as { end: string }).end).toBe('cancelled');

    const invalid = await send('POST', hosted, { ...start, timeout_seconds: 0 });
    expect(invalid.status).toBe(400);
    matches(validations, 'post', 'default', invalid.body);
    expect(invalid.body.code).toBe('invalid_request');
    const tooLong = await send('GET', `${hosted}?execution_id=${execution}&wait_ms=8001`);
    expect(tooLong.status).toBe(400);
    matches(validations, 'get', 'default', tooLong.body);
  } finally {
    await server.stop();
  }
});

test('HS2-WDQZ37 real hosted target gate responses match the OpenAPI contract', async () => {
  test.setTimeout(90_000);
  const openapi = YAML.parse(
    readFileSync(fileURLToPath(new URL('../../../docs/contracts/v1/openapi.yaml', import.meta.url)), 'utf8'),
  ) as {
    paths: Record<
      string,
      Record<string, { responses: Record<string, { content: Record<string, { schema: object }> }> }>
    >;
    components: object;
  };
  const ajv = new Ajv2020({ strict: false, allErrors: true });
  const matches = (path: string, method: string, status: string, value: unknown) => {
    const validate = ajv.compile({
      ...openapi.paths[path][method].responses[status].content['application/json'].schema,
      components: openapi.components,
    });
    expect(validate(value), `${method} ${path} ${status}: ${JSON.stringify(validate.errors)}`).toBe(true);
  };
  const server = await realTicketServer();
  try {
    const send = async (method: string, path: string, body?: unknown) => {
      const response = await fetch(`${server.url}${path}`, {
        method,
        headers: { 'content-type': 'application/json', 'X-Hotsheet-Secret': server.secret },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      return { status: response.status, body: (await response.json()) as Record<string, unknown> };
    };
    const gates = '/checkouts/{checkout}/target-gates';
    const hosted = `/checkouts/${server.checkoutId}/target-gates`;
    const watch = { commit: 'a'.repeat(40), repository: 'example/widgets', workflow: 'CI', branch: 'main' };
    const query = `commit=${watch.commit}&repository=example%2Fwidgets&workflow=CI&branch=main`;

    // Nothing is watched yet; reading starts nothing.
    const before = await send('GET', `${hosted}?${query}`);
    expect(before.status).toBe(200);
    matches(gates, 'get', '200', before.body);
    expect(before.body).toMatchObject({ state: 'unknown', reason: 'not_watched' });
    // HS2-K2YE6A: the fixture checkout has no Git remote naming the repository, so the
    // host refuses to watch it with its credentials, by the gate's own code.
    const refused = await send('POST', hosted, watch);
    expect(refused.status).toBe(409);
    matches(gates, 'post', 'default', refused.body);
    expect(refused.body.code).toBe('gate_repository_mismatch');
    // Without a cursor the reading is answered at once; with the current one it waits.
    const replayed = await send('GET', `${hosted}?${query}&wait_ms=8000`);
    matches(gates, 'get', '200', replayed.body);
    expect(replayed.body.reason).toBe('not_watched');
    const cursor = replayed.body.cursor as number;
    const asked = Date.now();
    const waited = await send('GET', `${hosted}?${query}&after=${cursor}&wait_ms=300`);
    expect(Date.now() - asked).toBeGreaterThanOrEqual(250);
    matches(gates, 'get', '200', waited.body);
    expect(waited.body.cursor).toBe(cursor);

    const tooLong = await send('GET', `${hosted}?${query}&wait_ms=8001`);
    expect(tooLong.status).toBe(400);
    matches(gates, 'get', 'default', tooLong.body);
    const missing = await send('GET', `${hosted}?commit=${watch.commit}`);
    expect(missing.status).toBe(400);
    matches(gates, 'get', 'default', missing.body);
    const odd = await send('POST', hosted, { ...watch, repository: 'not a repository' });
    expect(odd.status).toBe(409);
    matches(gates, 'post', 'default', odd.body);
  } finally {
    await server.stop();
  }
});

test('HS2-G3VAK4 real hosted target deployment responses match the OpenAPI contract', async () => {
  test.setTimeout(90_000);
  const openapi = YAML.parse(
    readFileSync(fileURLToPath(new URL('../../../docs/contracts/v1/openapi.yaml', import.meta.url)), 'utf8'),
  ) as {
    paths: Record<
      string,
      Record<string, { responses: Record<string, { content: Record<string, { schema: object }> }> }>
    >;
    components: object;
  };
  const ajv = new Ajv2020({ strict: false, allErrors: true });
  const matches = (path: string, method: string, status: string, value: unknown) => {
    const validate = ajv.compile({
      ...openapi.paths[path][method].responses[status].content['application/json'].schema,
      components: openapi.components,
    });
    expect(validate(value), `${method} ${path} ${status}: ${JSON.stringify(validate.errors)}`).toBe(true);
  };
  const server = await realTicketServer();
  try {
    const send = async (method: string, path: string, body?: unknown) => {
      const response = await fetch(`${server.url}${path}`, {
        method,
        headers: { 'content-type': 'application/json', 'X-Hotsheet-Secret': server.secret },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      return { status: response.status, body: (await response.json()) as Record<string, unknown> };
    };
    const deployments = '/checkouts/{checkout}/target-deployments';
    const hosted = `/checkouts/${server.checkoutId}/target-deployments`;
    const execution = '01ARZ3NDEKTSV4RRFFQ69G5FE8';
    // Nothing has run under that id.
    const unknown = await send('GET', `${hosted}?execution_id=${execution}`);
    expect(unknown.status).toBe(200);
    matches(deployments, 'get', '200', unknown.body);
    expect(unknown.body).toEqual({ execution_id: execution, state: 'unknown' });
    // The fixture project tracks no deploy script at that commit: refused, nothing runs.
    const refused = await send('POST', hosted, {
      execution_id: execution,
      commit: 'a'.repeat(40),
      script: 'scripts/deploy.sh',
      environment: 'beta',
    });
    expect(refused.status).toBe(409);
    matches(deployments, 'post', 'default', refused.body);
    expect(refused.body.code).toBe('gate_deployment_script_untracked');
    const invalid = await send('POST', hosted, {
      execution_id: execution,
      commit: 'a'.repeat(40),
      script: '../deploy.sh',
      environment: 'beta',
    });
    expect(invalid.status).toBe(400);
    matches(deployments, 'post', 'default', invalid.body);
  } finally {
    await server.stop();
  }
});

test('HS2-WBREDM real run reconciliation and control responses match the OpenAPI contract', async () => {
  test.setTimeout(90_000);
  const openapi = YAML.parse(
    readFileSync(fileURLToPath(new URL('../../../docs/contracts/v1/openapi.yaml', import.meta.url)), 'utf8'),
  ) as {
    paths: Record<
      string,
      Record<string, { responses: Record<string, { content: Record<string, { schema: object }> }> }>
    >;
    components: object;
  };
  const ajv = new Ajv2020({ strict: false, allErrors: true });
  const matches = (path: string, method: string, status: string, value: unknown) => {
    const validate = ajv.compile({
      ...openapi.paths[path][method].responses[status].content['application/json'].schema,
      components: openapi.components,
    });
    expect(validate(value), `${method} ${path} ${status}: ${JSON.stringify(validate.errors)}`).toBe(true);
  };
  const server = await realTicketServer();
  try {
    const send = async (method: string, path: string, body?: unknown) => {
      const response = await fetch(`${server.url}${path}`, {
        method,
        headers: { 'content-type': 'application/json', 'X-Hotsheet-Secret': server.secret },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      return { status: response.status, body: (await response.json()) as Record<string, unknown> };
    };
    const target = '01ARZ3NDEKTSV4RRFFQ69G5FE2';
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    const operation = '/checkouts/{owner}/orchestration/runs/{run}/{operation}';
    const person = { role: 'human', id: 'maintainer' };
    const scheduler = { role: 'ai', id: 'scheduler-1' };

    // This checkout is its own target: declare it, review it and bind it (HS2-XHQ1EH).
    const added = await send('POST', `${base}/targets`, { target_id: target, label: 'Self', expected_revision: null });
    expect(added.status).toBe(200);
    const candidate = await send('POST', `${base}/inspect`, {
      destination: { kind: 'local', checkout_id: server.checkoutId },
    });
    expect(candidate.status).toBe(200);
    const bound = await send('PUT', `${base}/targets/${target}/binding`, {
      target: { owner_checkout_id: server.checkoutId, target_id: target },
      expected_manifest_revision: added.body.revision,
      expected_binding_revision: 0,
      previous_destination: null,
      destination: candidate.body.destination,
    });
    expect(bound.status).toBe(200);
    // HS2-XHQ1EH: reading needs no permission; starting work needs a remembered rule.
    const remembered = await send('POST', `${base}/permission-rules`, {
      target_id: target,
      action: 'work_start',
      actor: person,
    });
    expect(remembered.status).toBe(200);
    matches('/checkouts/{owner}/orchestration/permission-rules', 'post', '200', remembered.body);
    const orchestration = await server.request<{ id: string }>('/tickets', 'POST', { title: 'Coordinate recovery' });
    const work = await server.request<{ id: string; connection_id: string }>('/tickets', 'POST', {
      title: 'Synthetic work',
    });

    // Nothing unfinished: an empty summary.
    const empty = await send('POST', `${base}/run-reconcile`, { actor: scheduler });
    expect(empty.status).toBe(200);
    matches('/checkouts/{owner}/orchestration/run-reconcile', 'post', '200', empty.body);
    expect(empty.body).toEqual({ runs: [] });

    const dispatched = await send('POST', `${base}/runs`, {
      ticket: { native_id: orchestration.id },
      target_id: target,
      connection_id: work.connection_id,
      native_id: work.id,
      operation_id: 'op-contract-recovery',
      worker_id: 'contract-worker',
      actor: scheduler,
    });
    expect(dispatched.status, JSON.stringify(dispatched.body)).toBe(200);
    const run = `${base}/runs/${String(dispatched.body.run_id)}`;
    const act = async (name: string, body: Record<string, unknown>, actor: object = scheduler) => {
      const answer = await send('POST', `${run}/${name}`, { ...body, actor });
      matches(operation, 'post', answer.status === 200 ? '200' : 'default', answer.body);
      return answer;
    };

    // Reconcile: the stored outcome names each source, and exactly the unverified ones.
    const reconciled = await act('reconcile', {});
    expect(reconciled.status).toBe(200);
    // HS2-7QNK7Y, HS2-BYEB60, HS2-G5H9FQ, HS2-75Q254: a run without gates, attempts or
    // sessions has every source verified (`no_gate`, `no_deployment`, `no_validation`,
    // `no_session`), so nothing is unverified.
    expect(reconciled.body.unverified).toBeUndefined();
    expect(reconciled.body.reconciliation).toMatchObject({ disposition: 'continue' });
    const findings = (reconciled.body.reconciliation as { findings: { source: string; verification: string }[] })
      .findings;
    expect(findings.map((found) => found.source)).toEqual([
      'target_reachability',
      'target_ticket',
      'target_claim',
      'run_lease',
      'ci_run',
      'deployment',
      'validation',
      'session',
    ]);
    expect(findings.slice(0, 4).every((found) => found.verification === 'verified')).toBe(true);
    expect(findings[4]).toEqual({ source: 'ci_run', verification: 'verified', reason: 'no_gate' });
    expect(findings[5]).toEqual({ source: 'deployment', verification: 'verified', reason: 'no_deployment' });
    expect(findings[6]).toEqual({ source: 'validation', verification: 'verified', reason: 'no_validation' });
    expect(findings[7]).toEqual({ source: 'session', verification: 'verified', reason: 'no_session' });

    // Pause, a refused renewal, resume, handoff and a person's correction.
    const paused = await act('pause', { reason: 'maintenance' });
    expect(paused.body.paused).toMatchObject({ reason: 'maintenance', actor_id: 'scheduler-1' });
    const refused = await act('renew', {});
    expect(refused.status).toBe(409);
    expect(refused.body.code).toBe('run_paused');
    const resumed = await act('resume', {});
    expect(resumed.body.paused).toBeUndefined();
    const handed = await act('handoff', { to_worker: 'contract-worker-2', lease_minutes: 30 });
    expect(handed.body.worker_id).toBe('contract-worker-2');
    const forbidden = await act('correct', { state: 'intervened', reason: 'operator_fix' });
    expect(forbidden.status).toBe(403);
    const corrected = await act('correct', { state: 'intervened', reason: 'operator_fix' }, person);
    expect(corrected.body.state).toBe('intervened');
    const history = corrected.body.history as { control?: string }[];
    expect(history.map((entry) => entry.control ?? '-')).toEqual(['-', '-', 'pause', 'resume', 'handoff', 'correct']);

    // Every unfinished run at once, the status view and the event stream.
    const all = await send('POST', `${base}/run-reconcile`, {});
    expect(all.status).toBe(200);
    matches('/checkouts/{owner}/orchestration/run-reconcile', 'post', '200', all.body);
    expect((all.body.runs as { state: string }[]).map((entry) => entry.state)).toEqual(['dispatched']);
    const status = await send('GET', `${run}/status`);
    matches(operation, 'get', '200', status.body);
    expect(status.body.reconciliation).toMatchObject({ disposition: 'continue' });
    const events = await send('GET', `${base}/run-events?after=0&run=${String(dispatched.body.run_id)}`);
    matches('/checkouts/{owner}/orchestration/run-events', 'get', '200', events.body);
    const kinds = (events.body.entries as { kind: string }[]).map((entry) => entry.kind);
    for (const kind of ['reconciled', 'paused', 'resumed', 'handoff', 'corrected']) expect(kinds).toContain(kind);

    const invalid = await act('correct', { state: 'paused', reason: 'x' }, person);
    expect(invalid.status).toBe(400);
  } finally {
    await server.stop();
  }
});

test('HS2-42TARA real readiness plan responses match the OpenAPI contract', async () => {
  test.setTimeout(90_000);
  const openapi = YAML.parse(
    readFileSync(fileURLToPath(new URL('../../../docs/contracts/v1/openapi.yaml', import.meta.url)), 'utf8'),
  ) as {
    paths: Record<
      string,
      Record<string, { responses: Record<string, { content: Record<string, { schema: object }> }> }>
    >;
    components: object;
  };
  const ajv = new Ajv2020({ strict: false, allErrors: true });
  const matches = (path: string, method: string, status: string, value: unknown) => {
    const validate = ajv.compile({
      ...openapi.paths[path][method].responses[status].content['application/json'].schema,
      components: openapi.components,
    });
    expect(validate(value), `${method} ${path} ${status}: ${JSON.stringify(validate.errors)}`).toBe(true);
  };
  const server = await realTicketServer();
  try {
    const send = async (method: string, path: string, body?: unknown) => {
      const response = await fetch(`${server.url}${path}`, {
        method,
        headers: { 'content-type': 'application/json', 'X-Hotsheet-Secret': server.secret },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      return { status: response.status, body: (await response.json()) as Record<string, unknown> };
    };
    const target = '01ARZ3NDEKTSV4RRFFQ69G5FF2';
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    const readiness = '/checkouts/{owner}/orchestration/readiness/{operation}';
    const runs = '/checkouts/{owner}/orchestration/runs';
    const person = { role: 'human', id: 'maintainer' };
    const scheduler = { role: 'ai', id: 'scheduler-1' };

    // This checkout is its own target: declare it, review it and bind it (HS2-XHQ1EH).
    const added = await send('POST', `${base}/targets`, { target_id: target, label: 'Self', expected_revision: null });
    expect(added.status).toBe(200);
    const candidate = await send('POST', `${base}/inspect`, {
      destination: { kind: 'local', checkout_id: server.checkoutId },
    });
    expect(candidate.status).toBe(200);
    const bound = await send('PUT', `${base}/targets/${target}/binding`, {
      target: { owner_checkout_id: server.checkoutId, target_id: target },
      expected_manifest_revision: added.body.revision,
      expected_binding_revision: 0,
      previous_destination: null,
      destination: candidate.body.destination,
    });
    expect(bound.status).toBe(200);
    // HS2-XHQ1EH: reading needs no permission; starting work needs a remembered rule.
    const remembered = await send('POST', `${base}/permission-rules`, {
      target_id: target,
      action: 'work_start',
      actor: person,
    });
    expect(remembered.status).toBe(200);
    matches('/checkouts/{owner}/orchestration/permission-rules', 'post', '200', remembered.body);
    const orchestration = await server.request<{ id: string }>('/tickets', 'POST', { title: 'Coordinate the plan' });
    const first = await server.request<{ id: string; connection_id: string }>('/tickets', 'POST', {
      title: 'Synthetic prerequisite',
    });
    const second = await server.request<{ id: string }>('/tickets', 'POST', { title: 'Synthetic dependent work' });
    const act = async (name: string, body: Record<string, unknown>) => {
      const answer = await send('POST', `${base}/readiness/${name}`, { ...body, actor: scheduler });
      matches(readiness, 'post', answer.status === 200 ? '200' : 'default', answer.body);
      return answer;
    };
    const step = (native: string, prerequisites: object[]) =>
      act('define', {
        ticket: { native_id: orchestration.id },
        target_id: target,
        connection_id: first.connection_id,
        native_id: native,
        prerequisites,
      });

    const planned = await step(first.id, []);
    expect(planned.status, JSON.stringify(planned.body)).toBe(200);
    expect(planned.body.level).toBe('confirmed');
    const dependent = await step(second.id, [
      { kind: 'ticket', target_id: target, connection_id: first.connection_id, native_id: first.id, gates: 'none' },
      { kind: 'validation', step_id: 'smoke' },
    ]);
    expect(dependent.body.level).toBe('blocked');
    expect((dependent.body.reasons as { code: string }[]).map((reason) => reason.code)).toEqual([
      'prerequisite_not_finished',
      'validation_step_missing',
    ]);
    const stepId = (dependent.body.step as { step_id: string }).step_id;

    const plan = await act('plan', {});
    expect(plan.body.counts).toEqual({ blocked: 1, confirmed: 1 });
    // HS2-J89JC7: the timeline of the same plan, its version long poll and a refusal.
    const timelinePath = '/checkouts/{owner}/orchestration/timeline';
    const timeline = await send('GET', `${base}/timeline`);
    matches(timelinePath, 'get', '200', timeline.body);
    // HS2-D9KP86: no saved policy does not block Start; it saves the defaults first.
    expect(timeline.body).toMatchObject({ phase: 'proposed', kick_off: { can_start: true, default_policy: true } });
    expect((timeline.body.waves as { kind: string; items: unknown[] }[]).map((wave) => wave.items.length)).toEqual([
      1, 1,
    ]);
    const waited = await send('GET', `${base}/timeline?after=${String(timeline.body.version)}&wait_ms=50`);
    matches(timelinePath, 'get', '200', waited.body);
    expect(waited.body.version).toBe(timeline.body.version);
    const malformed = await send('GET', `${base}/timeline?after=a%2Fb`);
    expect(malformed.status).toBe(400);
    matches(timelinePath, 'get', 'default', malformed.body);
    // HS2-S3FKJ7: the same plan read from the target side, one line per planned ticket.
    const linesPath = '/checkouts/{owner}/orchestration/target-lines';
    const lines = await send('GET', `${base}/target-lines`);
    matches(linesPath, 'get', '200', lines.body);
    expect(lines.body).toMatchObject({ orchestrated: true });
    expect(
      (lines.body.lines as { item: { target: { native_id: string } } }[])
        .map((line) => line.item.target.native_id)
        .sort(),
    ).toEqual([first.id, second.id].sort());
    const linesWaited = await send('GET', `${base}/target-lines?after=${String(lines.body.version)}&wait_ms=50`);
    matches(linesPath, 'get', '200', linesWaited.body);
    expect(linesWaited.body.version).toBe(lines.body.version);
    const linesMalformed = await send('GET', `${base}/target-lines?after=a%2Fb`);
    expect(linesMalformed.status).toBe(400);
    matches(linesPath, 'get', 'default', linesMalformed.body);
    // HS2-3D8D6V: a checkout on a remote server nothing here binds is answered, not orchestrated.
    const remoteLinesPath = '/remotes/{id}/checkouts/{checkout}/orchestration/target-lines';
    const remoteLines = await send(
      'GET',
      '/remotes/01ARZ3NDEKTSV4RRFFQ69G5FAV/checkouts/peer-0123456789ab/orchestration/target-lines',
    );
    matches(remoteLinesPath, 'get', '200', remoteLines.body);
    expect(remoteLines.body).toMatchObject({ orchestrated: false, lines: [] });
    // HS2-KB2Y51: a pass with no remote target sends nothing; a hosted withdrawal is answered.
    const pushPath = '/checkouts/{owner}/orchestration/target-lines/push';
    const pushed = await send('POST', `${base}/target-lines/push`, {});
    matches(pushPath, 'post', '200', pushed.body);
    expect(pushed.body).toMatchObject({ destinations: [] });
    const observationsPath = '/checkouts/{owner}/target-line-observations';
    const observed = await send('PUT', `${base.replace('/orchestration', '')}/target-line-observations`, {
      owner_server_id: '01ARZ3NDEKTSV4RRFFQ69G5FA0',
      sequence: 1,
      lines: [],
    });
    matches(observationsPath, 'put', '200', observed.body);
    expect(observed.body).toMatchObject({ outcome: 'withdrawn' });
    const observationRefused = await send('PUT', `${base.replace('/orchestration', '')}/target-line-observations`, {
      owner_server_id: 'not-a-ulid',
      sequence: 1,
    });
    expect(observationRefused.status).toBe(400);
    matches(observationsPath, 'put', 'default', observationRefused.body);
    const ready = await send('POST', `${base}/readiness/ready`, {});
    matches(readiness, 'post', '200', ready.body);
    expect(ready.body).toMatchObject([{ level: 'confirmed', unblocks: [stepId], priority: 'default' }]);
    const shown = await act('show', { step_id: stepId });
    expect(shown.body.since_ms).toEqual(expect.any(Number));

    // Unready work is a typed conflict; asking for an override waits for a person.
    const dispatch = {
      ticket: { native_id: orchestration.id },
      target_id: target,
      connection_id: first.connection_id,
      native_id: second.id,
      operation_id: 'op-contract-readiness',
      worker_id: 'contract-worker',
      actor: scheduler,
    };
    const refused = await send('POST', `${base}/runs`, dispatch);
    expect(refused.status).toBe(409);
    expect(refused.body.code).toBe('readiness_blocked');
    matches(runs, 'post', 'default', refused.body);
    // HS2-XHQ1EH: an AI's override is a judgment call, asked as a decision note on the goal ticket.
    const asked = await send('POST', `${base}/runs`, { ...dispatch, override_readiness: true });
    expect(asked.status).toBe(403);
    expect(asked.body.code).toBe('decision_required');
    expect(asked.body.note_id).toEqual(expect.any(String));
    matches(runs, 'post', 'default', asked.body);
    const permissions = await send('GET', `${base}/permissions`);
    matches('/checkouts/{owner}/orchestration/permissions', 'get', '200', permissions.body);
    expect(permissions.body).toMatchObject({ requests: [], retired: { grants_file: false, approvals_file: false } });
    // HS2-JWV5S1: the retired files are shown, and removing nothing is a person's idempotent act.
    const retiredPath = '/checkouts/{owner}/orchestration/retired-permissions';
    const retired = await send('GET', `${base}/retired-permissions`);
    matches(retiredPath, 'get', '200', retired.body);
    const notByAi = await send('DELETE', `${base}/retired-permissions`, { actor: scheduler });
    expect(notByAi.status).toBe(403);
    matches(retiredPath, 'delete', 'default', notByAi.body);
    const cleared = await send('DELETE', `${base}/retired-permissions`, { actor: { role: 'human', id: 'contract' } });
    matches(retiredPath, 'delete', '200', cleared.body);
    expect(cleared.body).toMatchObject({ removed: { grants_file: false }, remaining: { approvals_file: false } });

    const refreshed = await act('refresh', { step_id: stepId });
    expect((refreshed.body.steps as unknown[]).length).toBe(1);
    const events = await send('GET', `${base}/run-events?after=0&run=${stepId}`);
    matches('/checkouts/{owner}/orchestration/run-events', 'get', '200', events.body);
    expect((events.body.entries as { kind: string; reason: string }[]).map((entry) => entry.reason)).toEqual([
      'blocked',
    ]);

    const removed = await act('remove', { step_id: stepId });
    expect(removed.body.step_id).toBe(stepId);
    const gone = await act('show', { step_id: stepId });
    expect(gone.status).toBe(404);
    expect(gone.body.code).toBe('readiness_step_not_found');
    const invalid = await act('show', { step: stepId });
    expect(invalid.status).toBe(400);
  } finally {
    await server.stop();
  }
});

test('HS2-JSF985 real provisional work responses match the OpenAPI contract', async () => {
  test.setTimeout(90_000);
  const openapi = YAML.parse(
    readFileSync(fileURLToPath(new URL('../../../docs/contracts/v1/openapi.yaml', import.meta.url)), 'utf8'),
  ) as {
    paths: Record<
      string,
      Record<string, { responses: Record<string, { content: Record<string, { schema: object }> }> }>
    >;
    components: object;
  };
  const ajv = new Ajv2020({ strict: false, allErrors: true });
  const matches = (path: string, method: string, status: string, value: unknown) => {
    const validate = ajv.compile({
      ...openapi.paths[path][method].responses[status].content['application/json'].schema,
      components: openapi.components,
    });
    expect(validate(value), `${method} ${path} ${status}: ${JSON.stringify(validate.errors)}`).toBe(true);
  };
  // The readiness route answers one of several shapes; each is also held to its own.
  const shaped = (schema: string, value: unknown) => {
    const validate = ajv.compile({ $ref: `#/components/schemas/${schema}`, components: openapi.components });
    expect(validate(value), `${schema}: ${JSON.stringify(validate.errors)}`).toBe(true);
  };
  // The target project opts in to provisional readiness itself, in its committed settings.
  const server = await realTicketServer({
    seed: async ({ root }) => {
      await mkdir(join(root, '.hotsheet2'), { recursive: true });
      await writeFile(
        join(root, '.hotsheet2', 'settings.json'),
        JSON.stringify({ $hotsheetSchema: 1, speculation: { enabled: true, max_depth: 1 } }),
      );
    },
  });
  try {
    const send = async (method: string, path: string, body?: unknown) => {
      const response = await fetch(`${server.url}${path}`, {
        method,
        headers: { 'content-type': 'application/json', 'X-Hotsheet-Secret': server.secret },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      return { status: response.status, body: (await response.json()) as Record<string, unknown> };
    };
    const target = '01ARZ3NDEKTSV4RRFFQ69G5FJ3';
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    const readiness = '/checkouts/{owner}/orchestration/readiness/{operation}';
    const runs = '/checkouts/{owner}/orchestration/runs';
    const person = { role: 'human', id: 'maintainer' };
    const scheduler = { role: 'ai', id: 'scheduler-1' };

    const added = await send('POST', `${base}/targets`, { target_id: target, label: 'Self', expected_revision: null });
    expect(added.status).toBe(200);
    const candidate = await send('POST', `${base}/inspect`, {
      destination: { kind: 'local', checkout_id: server.checkoutId },
    });
    const bound = await send('PUT', `${base}/targets/${target}/binding`, {
      target: { owner_checkout_id: server.checkoutId, target_id: target },
      expected_manifest_revision: added.body.revision,
      expected_binding_revision: 0,
      previous_destination: null,
      destination: candidate.body.destination,
    });
    expect(bound.status).toBe(200);
    // HS2-XHQ1EH: reading needs no permission; starting work needs a remembered rule.
    const remembered = await send('POST', `${base}/permission-rules`, {
      target_id: target,
      action: 'work_start',
      actor: person,
    });
    expect(remembered.status).toBe(200);
    matches('/checkouts/{owner}/orchestration/permission-rules', 'post', '200', remembered.body);
    const ahead = await send('POST', `${base}/permission-rules`, {
      target_id: target,
      action: 'provisional_dispatch',
      actor: person,
    });
    expect(ahead.status).toBe(200);
    const orchestration = await server.request<{ id: string }>('/tickets', 'POST', { title: 'Coordinate the plan' });
    const first = await server.request<{ id: string; connection_id: string }>('/tickets', 'POST', {
      title: 'Synthetic prerequisite',
    });
    const second = await server.request<{ id: string }>('/tickets', 'POST', { title: 'Synthetic dependent work' });
    const dispatch = (native: string, operation: string) =>
      send('POST', `${base}/runs`, {
        ticket: { native_id: orchestration.id },
        target_id: target,
        connection_id: first.connection_id,
        native_id: native,
        operation_id: operation,
        worker_id: `worker-${operation}`,
        actor: scheduler,
      });
    const act = async (name: string, body: Record<string, unknown>) => {
      const answer = await send('POST', `${base}/readiness/${name}`, { ...body, actor: scheduler });
      matches(readiness, 'post', answer.status === 200 ? '200' : 'default', answer.body);
      return answer;
    };

    // The prerequisite is dispatched and integrated; its CI gate pends.
    const running = await dispatch(first.id, 'op-contract-prerequisite');
    expect(running.status, JSON.stringify(running.body)).toBe(200);
    const commit = 'a'.repeat(40);
    const linked = await send('POST', `${base}/runs/${String(running.body.run_id)}/gates`, {
      commit,
      repository: 'example/widgets',
      actor: scheduler,
    });
    expect(linked.status, JSON.stringify(linked.body)).toBe(200);
    const gateId = (linked.body.gate as { gate_id: string }).gate_id;
    const defined = await act('define', {
      ticket: { native_id: orchestration.id },
      target_id: target,
      connection_id: first.connection_id,
      native_id: second.id,
      prerequisites: [
        { kind: 'ticket', target_id: target, connection_id: first.connection_id, native_id: first.id, gates: 'ci' },
      ],
    });
    expect(defined.body.level).toBe('provisional');
    expect(defined.body.speculation).toBeUndefined();
    const stepId = (defined.body.step as { step_id: string }).step_id;

    // Without the orchestration policy's opt-in the refusal is the one it always was.
    const refused = await dispatch(second.id, 'op-contract-provisional');
    expect(refused.status).toBe(409);
    expect(refused.body.code).toBe('readiness_provisional');
    matches(runs, 'post', 'default', refused.body);
    const idle = await act('speculation', {});
    shaped('ProvisionalStatus', idle.body);
    expect(idle.body).toEqual({ enabled: false, work: [], repair_first: [], reevaluate: [] });

    const policy = await send('PUT', `${base}/policy`, {
      expected_revision: null,
      policy: {
        targets: [{ target_id: target, actions: ['tickets_read', 'work_start', 'gates_observe'] }],
        speculation: { allowed: true },
      },
      actor: person,
    });
    expect(policy.status, JSON.stringify(policy.body)).toBe(200);
    // Nothing the rule needs has been read yet, so it refuses, with the reason.
    // HS2-W5H48W: the prerequisite run's follower stores the target's own setting from any feed
    // page it takes once the policy opts in, and under load a page it was behind on lands here.
    // The rule then names the next unknown, the tickets' opt-out tags, which the dispatch below
    // reads. Both refusals are correct answers at this point and both match the contract; the
    // order of the rule's checks is pinned by the Rust provisional tests, which control every read.
    const unknown = await act('show', { step_id: stepId });
    shaped('StepReadiness', unknown.body);
    expect([
      { allowed: false, refusal: 'target_speculation_unknown', depth: 0, max_depth: 1 },
      { allowed: false, refusal: 'opt_out_unknown', depth: 0, max_depth: 1 },
    ]).toContainEqual(unknown.body.speculation);

    const admitted = await dispatch(second.id, 'op-contract-provisional');
    expect(admitted.status, JSON.stringify(admitted.body)).toBe(200);
    matches(runs, 'post', '200', admitted.body);
    // The target's machine reports its project's own setting with its capacity.
    const snapshot = await send('GET', `/checkouts/${server.checkoutId}/target-capacity`);
    expect(snapshot.status).toBe(200);
    matches('/checkouts/{checkout}/target-capacity', 'get', '200', snapshot.body);
    expect(snapshot.body.speculation).toEqual({ enabled: true, max_depth: 1 });
    // HS2-FJ6MDW: and alone, without a capacity sample.
    const setting = await send('GET', `/checkouts/${server.checkoutId}/target-speculation`);
    expect(setting.status).toBe(200);
    matches('/checkouts/{checkout}/target-speculation', 'get', '200', setting.body);
    expect(setting.body).toEqual({ speculation: { enabled: true, max_depth: 1 } });
    const listed = await act('speculation', {});
    shaped('ProvisionalStatus', listed.body);
    expect(listed.body).toMatchObject({
      enabled: true,
      work: [
        {
          step_id: stepId,
          operation_id: 'op-contract-provisional',
          state: 'provisional',
          depth: 1,
          gates: [{ target_id: target, native_id: first.id, commit, gate_ids: [gateId], result: 'pending' }],
        },
      ],
      repair_first: [],
      reevaluate: [],
    });
    const shown = await act('show', { step_id: stepId });
    shaped('StepReadiness', shown.body);
    expect(shown.body).toMatchObject({ level: 'in_progress', provisional: { state: 'provisional' } });
    const status = await send('GET', `${base}/runs/${String(admitted.body.run_id)}/status`);
    matches('/checkouts/{owner}/orchestration/runs/{run}/{operation}', 'get', '200', status.body);
    expect(status.body.provisional).toEqual((listed.body.work as unknown[])[0]);
    const explained = await send('POST', `${base}/scheduler/explain`, { step_id: stepId, actor: scheduler });
    expect(explained.status, JSON.stringify(explained.body)).toBe(200);
    shaped('SchedulerExplanation', explained.body);
    expect(explained.body).toMatchObject({ standing: 'running', provisional: { step_id: stepId } });
    const events = await send('GET', `${base}/run-events?after=0&run=${stepId}`);
    matches('/checkouts/{owner}/orchestration/run-events', 'get', '200', events.body);
    expect(
      (events.body.entries as { kind: string; reason: string; gate_id?: string }[])
        .filter((entry) => entry.kind === 'provisional')
        .map((entry) => [entry.reason, entry.gate_id]),
    ).toEqual([['provisional', gateId]]);

    // A person's or a worker's reopen keeps the record; errors are typed.
    const reopened = await act('reopen', { step_id: stepId, reason: 'fix_changed_contract' });
    expect(reopened.status, JSON.stringify(reopened.body)).toBe(200);
    shaped('ProvisionalWork', reopened.body);
    expect(reopened.body).toMatchObject({ state: 'reopened', reopen_reason: 'fix_changed_contract' });
    const missing = await act('reopen', { step_id: target, reason: 'fix_changed_contract' });
    expect(missing.status).toBe(404);
    expect(missing.body.code).toBe('readiness_step_not_found');
    const invalid = await act('reopen', { step_id: stepId });
    expect(invalid.status).toBe(400);
  } finally {
    await server.stop();
  }
});

test('HS2-ECWG50 real scheduler responses match the OpenAPI contract', async () => {
  test.setTimeout(90_000);
  const openapi = YAML.parse(
    readFileSync(fileURLToPath(new URL('../../../docs/contracts/v1/openapi.yaml', import.meta.url)), 'utf8'),
  ) as {
    paths: Record<
      string,
      Record<string, { responses: Record<string, { content: Record<string, { schema: object }> }> }>
    >;
    components: object;
  };
  const ajv = new Ajv2020({ strict: false, allErrors: true });
  const matches = (path: string, method: string, status: string, value: unknown) => {
    const validate = ajv.compile({
      ...openapi.paths[path][method].responses[status].content['application/json'].schema,
      components: openapi.components,
    });
    expect(validate(value), `${method} ${path} ${status}: ${JSON.stringify(validate.errors)}`).toBe(true);
  };
  // The route answers one of several shapes; each operation is also held to its own.
  const shaped = (schema: string, value: unknown) => {
    const validate = ajv.compile({ $ref: `#/components/schemas/${schema}`, components: openapi.components });
    expect(validate(value), `${schema}: ${JSON.stringify(validate.errors)}`).toBe(true);
  };
  const server = await realTicketServer();
  try {
    const send = async (method: string, path: string, body?: unknown) => {
      const response = await fetch(`${server.url}${path}`, {
        method,
        headers: { 'content-type': 'application/json', 'X-Hotsheet-Secret': server.secret },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      return { status: response.status, body: (await response.json()) as Record<string, unknown> };
    };
    const target = '01ARZ3NDEKTSV4RRFFQ69G5FF3';
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    const route = '/checkouts/{owner}/orchestration/scheduler/{operation}';
    const person = { role: 'human', id: 'maintainer' };
    const actor = { role: 'ai', id: 'scheduler-1' };

    const added = await send('POST', `${base}/targets`, { target_id: target, label: 'Self', expected_revision: null });
    expect(added.status).toBe(200);
    const candidate = await send('POST', `${base}/inspect`, {
      destination: { kind: 'local', checkout_id: server.checkoutId },
    });
    const bound = await send('PUT', `${base}/targets/${target}/binding`, {
      target: { owner_checkout_id: server.checkoutId, target_id: target },
      expected_manifest_revision: added.body.revision,
      expected_binding_revision: 0,
      previous_destination: null,
      destination: candidate.body.destination,
    });
    expect(bound.status).toBe(200);
    const orchestration = await server.request<{ id: string }>('/tickets', 'POST', {
      title: 'Coordinate the schedule',
    });
    const work = await server.request<{ id: string; connection_id: string }>('/tickets', 'POST', {
      title: 'Synthetic ready work',
    });
    const defined = await send('POST', `${base}/readiness/define`, {
      ticket: { native_id: orchestration.id },
      target_id: target,
      connection_id: work.connection_id,
      native_id: work.id,
      actor,
    });
    expect(defined.status, JSON.stringify(defined.body)).toBe(200);
    const stepId = (defined.body.step as { step_id: string }).step_id;
    const act = async (operation: string, body: Record<string, unknown>, schema: string) => {
      const answer = await send('POST', `${base}/scheduler/${operation}`, { ...body, actor });
      expect(answer.status, JSON.stringify(answer.body)).toBe(200);
      matches(route, 'post', '200', answer.body);
      shaped(schema, answer.body);
      return answer.body;
    };

    // Without a policy the scheduler is off.
    const off = await act('plan', {}, 'SchedulerSchedule');
    expect(off).toMatchObject({ enabled: false, mode: 'dry_run', placements: [], ranking: [] });

    // A policy with a placement section, for a tool no machine has, so that the
    // answer is the same wherever this runs.
    const policy = await send('PUT', `${base}/policy`, {
      expected_revision: null,
      policy: {
        tools: [{ tool: 'synthetic-tool', max_sessions: 2 }],
        targets: [
          {
            target_id: target,
            actions: ['tickets_read', 'gates_observe', 'sessions_manage'],
            tools: [{ tool: 'synthetic-tool' }],
            max_sessions: 2,
          },
        ],
        placement: { automatic: false, actor: 'scheduler-1', aging_minutes: 30, cooldown_minutes: 2 },
      },
      actor: person,
    });
    expect(policy.status, JSON.stringify(policy.body)).toBe(200);
    matches('/checkouts/{owner}/orchestration/policy', 'put', '200', policy.body);
    expect((policy.body.policy as { placement: object }).placement).toEqual({
      automatic: false,
      actor: 'scheduler-1',
      aging_minutes: 30,
      cooldown_minutes: 2,
    });

    const plan = await act('plan', {}, 'SchedulerSchedule');
    expect(plan).toMatchObject({
      enabled: true,
      mode: 'dry_run',
      ranking: [{ step_id: stepId }],
      unplaced: [{ step_id: stepId, reason: 'no_tool_available', detail: 'tool_not_installed' }],
      inputs: { automatic: false, work: 1, confirmed: 1, executor: 'available' }, // HS2-95SWQ4
    });
    const explained = await act('explain', { step_id: stepId }, 'SchedulerExplanation');
    expect(explained).toMatchObject({ running: false, standing: 'not_placed', reason: 'no_tool_available', rank: 1 });
    const byTicket = await act(
      'explain',
      { target_id: target, connection_id: work.connection_id, native_id: work.id },
      'SchedulerExplanation',
    );
    expect(byTicket.step_id).toBe(stepId);
    const tick = await act('tick', {}, 'SchedulerSchedule');
    expect(tick).toMatchObject({ mode: 'dry_run', trigger: 'manual', outcomes: [], logged: 2 });
    const log = await act('decisions', { limit: 10 }, 'SchedulerDecisionPage');
    expect(log.entries).toMatchObject([
      { kind: 'tick', mode: 'dry_run', trigger: 'manual', tick: { unplaced: 1 } },
      { kind: 'unplaced', step_id: stepId, reason: 'no_tool_available' },
    ]);

    // A malformed body and an unknown operation are typed errors.
    for (const [operation, body] of [
      ['explain', {}],
      ['tick', { dry: true }],
      ['apply', {}],
    ] as const) {
      const refused = await send('POST', `${base}/scheduler/${operation}`, body);
      expect(refused.status).toBe(400);
      matches(route, 'post', 'default', refused.body);
      expect(refused.body.code).toBe('invalid_request');
    }

    // HS2-GRRX32: a target's role sessions set to none (HS2-R2N33M) is an empty list that the
    // contract accepts in the request, the role-sessions answer and the stored policy read back.
    const none = { target_id: target, roles: [], actor: person };
    shaped('SetOrchestrationRoleSessions', none);
    const chosen = await send('PUT', `${base}/policy/role-sessions`, none);
    expect(chosen.status, JSON.stringify(chosen.body)).toBe(200);
    matches('/checkouts/{owner}/orchestration/policy/role-sessions', 'put', '200', chosen.body);
    const stored = await send('GET', `${base}/policy`);
    matches('/checkouts/{owner}/orchestration/policy', 'get', '200', stored.body);
    const placement = (stored.body.policy as { placement: { role_sessions: unknown } }).placement;
    expect(placement.role_sessions).toEqual([{ target_id: target, roles: [] }]);
    shaped('OrchestrationPolicy', stored.body.policy);
    // The stored policy with its empty choice is accepted back as a whole-policy write.
    const again = await send('PUT', `${base}/policy`, {
      expected_revision: stored.body.revision,
      policy: stored.body.policy,
      actor: person,
    });
    expect(again.status, JSON.stringify(again.body)).toBe(200);
    expect(again.body.revision).toBe(stored.body.revision);
    // HS2-TZJYKH: Pause through the runtime route keeps the choice and matches its schema.
    const paused = await send('PUT', `${base}/policy/automatic`, { automatic: false, actor: person });
    expect(paused.status, JSON.stringify(paused.body)).toBe(200);
    matches('/checkouts/{owner}/orchestration/policy/automatic', 'put', '200', paused.body);
    expect(paused.body.saved_default).toBe(false);
    expect((paused.body.policy as { placement: { role_sessions: unknown } }).placement.role_sessions).toEqual([
      { target_id: target, roles: [] },
    ]);
  } finally {
    await server.stop();
  }
});

test('HS2-2FG91X real resource requirement and contention responses match the OpenAPI contract', async () => {
  test.setTimeout(90_000);
  const openapi = YAML.parse(
    readFileSync(fileURLToPath(new URL('../../../docs/contracts/v1/openapi.yaml', import.meta.url)), 'utf8'),
  ) as {
    paths: Record<
      string,
      Record<string, { responses: Record<string, { content: Record<string, { schema: object }> }> }>
    >;
    components: object;
  };
  const ajv = new Ajv2020({ strict: false, allErrors: true });
  const matches = (path: string, method: string, status: string, value: unknown) => {
    const validate = ajv.compile({
      ...openapi.paths[path][method].responses[status].content['application/json'].schema,
      components: openapi.components,
    });
    expect(validate(value), `${method} ${path} ${status}: ${JSON.stringify(validate.errors)}`).toBe(true);
  };
  const shaped = (schema: string, value: unknown) => {
    const validate = ajv.compile({ $ref: `#/components/schemas/${schema}`, components: openapi.components });
    expect(validate(value), `${schema}: ${JSON.stringify(validate.errors)}`).toBe(true);
  };
  const server = await realTicketServer();
  try {
    const send = async (method: string, path: string, body?: unknown) => {
      const response = await fetch(`${server.url}${path}`, {
        method,
        headers: { 'content-type': 'application/json', 'X-Hotsheet-Secret': server.secret },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      return { status: response.status, body: (await response.json()) as Record<string, unknown> };
    };
    const target = '01ARZ3NDEKTSV4RRFFQ69G5FF4';
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    const schedulerRoute = '/checkouts/{owner}/orchestration/scheduler/{operation}';
    const readinessRoute = '/checkouts/{owner}/orchestration/readiness/{operation}';
    const person = { role: 'human', id: 'maintainer' };
    const actor = { role: 'ai', id: 'scheduler-1' };
    const vm = { namespace: 'vm', resource: 'macos-vm' };

    const added = await send('POST', `${base}/targets`, { target_id: target, label: 'Self', expected_revision: null });
    expect(added.status).toBe(200);
    const candidate = await send('POST', `${base}/inspect`, {
      destination: { kind: 'local', checkout_id: server.checkoutId },
    });
    const bound = await send('PUT', `${base}/targets/${target}/binding`, {
      target: { owner_checkout_id: server.checkoutId, target_id: target },
      expected_manifest_revision: added.body.revision,
      expected_binding_revision: 0,
      previous_destination: null,
      destination: candidate.body.destination,
    });
    expect(bound.status).toBe(200);
    const orchestration = await server.request<{ id: string }>('/tickets', 'POST', {
      title: 'Coordinate around a resource',
    });
    const work = await server.request<{ id: string; connection_id: string }>('/tickets', 'POST', {
      title: 'Synthetic work in the VM',
    });

    // A step declares the resources its session must hold.
    const defined = await send('POST', `${base}/readiness/define`, {
      ticket: { native_id: orchestration.id },
      target_id: target,
      connection_id: work.connection_id,
      native_id: work.id,
      resources: [vm, { namespace: 'ports', resource: '8080' }],
      actor,
    });
    expect(defined.status, JSON.stringify(defined.body)).toBe(200);
    matches(readinessRoute, 'post', '200', defined.body);
    const step = defined.body.step as { step_id: string; resources: object[] };
    shaped('PlanStep', step);
    expect(step.resources).toEqual([{ namespace: 'ports', resource: '8080' }, vm]);
    const ready = await send('POST', `${base}/readiness/ready`, { actor });
    expect(ready.status).toBe(200);
    matches(readinessRoute, 'post', '200', ready.body);
    expect(ready.body).toMatchObject([{ step_id: step.step_id, resources: step.resources }]);
    // HS2-F1BKQ0: the plan's state, which reads where every plan read would fail.
    const health = await send('POST', `${base}/readiness/health`, { actor });
    expect(health.status).toBe(200);
    matches(readinessRoute, 'post', '200', health.body);
    expect(health.body).toEqual({ state: 'ok', discard_resolves: false });
    const invalid = await send('POST', `${base}/readiness/define`, {
      ticket: { native_id: orchestration.id },
      target_id: target,
      connection_id: work.connection_id,
      native_id: work.id,
      resources: [{ namespace: 'VM', resource: 'macos-vm' }],
      actor,
    });
    expect(invalid.status).toBe(400);
    matches(readinessRoute, 'post', 'default', invalid.body);

    // The policy requires a resource of every piece of work on the target.
    const policy = await send('PUT', `${base}/policy`, {
      expected_revision: null,
      policy: {
        tools: [{ tool: 'synthetic-tool', max_sessions: 2 }],
        targets: [
          {
            target_id: target,
            actions: ['tickets_read', 'gates_observe', 'sessions_manage'],
            tools: [{ tool: 'synthetic-tool' }],
            max_sessions: 2,
          },
        ],
        // HS2-R2N33M: unset placement is Auto and applies once a plan exists; this
        // contract reads a dry-run tick, so placement is explicitly off.
        placement: {
          automatic: false,
          actor: 'scheduler-1',
          resources: [{ target_id: target, resources: [{ namespace: 'sim', resource: 'ios-17' }] }],
        },
      },
      actor: person,
    });
    expect(policy.status, JSON.stringify(policy.body)).toBe(200);
    matches('/checkouts/{owner}/orchestration/policy', 'put', '200', policy.body);
    expect((policy.body.policy as { placement: { resources: object[] } }).placement.resources).toEqual([
      { target_id: target, resources: [{ namespace: 'sim', resource: 'ios-17' }] },
    ]);

    // Someone holds the VM, taken through the claim service's own route.
    const grant = await send('POST', '/resource-claims/vm/macos-vm/acquire', { duration_ms: 600_000 });
    expect(grant.status, JSON.stringify(grant.body)).toBe(200);
    expect(grant.body.acquired).toBe(true);
    const snapshot = await send('GET', `/checkouts/${server.checkoutId}/target-capacity?claim_namespaces=vm`);
    expect(snapshot.status).toBe(200);
    matches('/checkouts/{checkout}/target-capacity', 'get', '200', snapshot.body);
    expect(snapshot.body.claims).toEqual([{ ...vm, expires_at_ms: grant.body.expires_at_ms }]);

    const act = async (operation: string, body: Record<string, unknown>, schema: string) => {
      const answer = await send('POST', `${base}/scheduler/${operation}`, { ...body, actor });
      expect(answer.status, JSON.stringify(answer.body)).toBe(200);
      matches(schedulerRoute, 'post', '200', answer.body);
      shaped(schema, answer.body);
      return answer.body;
    };
    const plan = await act('plan', {}, 'SchedulerSchedule');
    expect(plan).toMatchObject({
      unplaced: [
        {
          step_id: step.step_id,
          reason: 'resource_held',
          detail: 'vm/macos-vm',
          retry_at_ms: grant.body.expires_at_ms,
        },
      ],
      retry_at_ms: grant.body.expires_at_ms,
      contention: [
        {
          server_id: snapshot.body.server_id,
          ...vm,
          state: 'held',
          expires_at_ms: grant.body.expires_at_ms,
          waiting: [step.step_id],
        },
      ],
      inputs: { held_resources: 1, unreadable_namespaces: 0 },
    });
    const explained = await act('explain', { step_id: step.step_id }, 'SchedulerExplanation');
    expect(explained).toMatchObject({
      standing: 'not_placed',
      reason: 'resource_held',
      detail: 'vm/macos-vm',
      resources: [
        { namespace: 'ports', resource: '8080', state: 'free', declared_by: 'step' },
        { ...vm, state: 'held', expires_at_ms: grant.body.expires_at_ms, declared_by: 'step' },
        { namespace: 'sim', resource: 'ios-17', state: 'free', declared_by: 'target' },
      ],
    });
    const tick = await act('tick', {}, 'SchedulerSchedule');
    expect(tick).toMatchObject({ mode: 'dry_run', logged: 2 });
    const log = await act('decisions', { limit: 10 }, 'SchedulerDecisionPage');
    expect(log.entries).toMatchObject([
      { kind: 'tick', tick: { held_resources: 1, unreadable_namespaces: 0 } },
      { kind: 'unplaced', step_id: step.step_id, reason: 'resource_held', detail: 'vm/macos-vm' },
    ]);

    // Released with its own token: nothing waits on a resource any more.
    const released = await send('POST', '/resource-claims/vm/macos-vm/release', {
      renewal_token: grant.body.renewal_token,
    });
    expect(released.status).toBe(200);
    const after = await act('plan', {}, 'SchedulerSchedule');
    expect(after.contention).toBeUndefined();
    expect(after).toMatchObject({ unplaced: [{ step_id: step.step_id, reason: 'no_tool_available' }] });
  } finally {
    await server.stop();
  }
});
