import { readFileSync } from 'node:fs';
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
  for (const [path, methods] of router) {
    const operations = openapi.paths[path] as Record<string, unknown>;
    const parameterNames = new Set(
      ((operations.parameters ?? []) as Array<{ name?: string; $ref?: string }>).map(
        (parameter) => parameter.name ?? openapi.components.parameters[parameter.$ref?.split('/').at(-1) ?? '']?.name,
      ),
    );
    for (const [, name] of path.matchAll(/\{([^}]+)\}/g))
      expect(parameterNames.has(name), `${path}: ${name}`).toBe(true);
    for (const [method, handler] of Object.entries(methods)) {
      expect(operations[method], `${method.toUpperCase()} ${path}`).toBeDefined();
      const operation = operations[method] as Record<string, unknown>;
      if (operation['x-hotsheet-contract-depth'] === 'route-only') expect(operation.operationId).toBe(handler);
    }
  }
  const ids = Object.values(openapi.paths).flatMap((value) =>
    Object.entries(value as Record<string, unknown>)
      .filter(([method]) => ['get', 'post', 'put', 'patch', 'delete'].includes(method))
      .map(([, operation]) => (operation as { operationId: string }).operationId),
  );
  expect(new Set(ids).size).toBe(ids.length);

  const ajv = new Ajv2020({ strict: false });
  ajv.addSchema(contract('change-event.schema.json'));
  const change = ajv.getSchema(
    'https://raw.githubusercontent.com/Small-Tale/hotsheet2/main/docs/contracts/v1/change-event.schema.json',
  );
  expect(change).toBeDefined();
  const poll = ajv.compile(contract('poll-envelope.schema.json'));
  const schema = (name: string) => ({ ...openapi.components.schemas[name], components: openapi.components });
  const compatibility = ajv.compile(schema('Compatibility'));
  const ticket = ajv.compile(schema('Ticket'));
  const row = ajv.compile(openapi.components.schemas.TicketRow);
  const server = await realTicketServer();
  let socket: WebSocket | undefined;
  try {
    const metadata = await server.request('/compatibility');
    expect(compatibility(metadata), JSON.stringify(compatibility.errors)).toBe(true);
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

    const created = await server.request<{ id: string }>('/tickets', 'POST', { title: 'Contract response' });
    expect(ticket(created), JSON.stringify(ticket.errors)).toBe(true);
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
