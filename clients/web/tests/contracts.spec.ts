import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { expect, test } from '@playwright/test';
import Ajv2020 from 'ajv/dist/2020.js';
import WebSocket from 'ws';
import YAML from 'yaml';

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
  expect(Object.keys(openapi.paths)).toHaveLength(169); // HS2-PWNPM2 command launch routes
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
  expect(Object.keys(openapi.paths)).toHaveLength(169); // HS2-PWNPM2 command launch routes
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
      '/notifications',
      `/checkouts/${server.checkoutId}/providers`,
      `/checkouts/${server.checkoutId}/repository/status`,
      `/checkouts/${server.checkoutId}/guidance`,
      `/checkouts/${server.checkoutId}/guidance/updates`,
    ]) {
      const documentedPath = path.replace(server.checkoutId, '{reference}');
      validateResponse(documentedPath, await server.request(path));
    }
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
