import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

import { routeCoverageErrors } from './check-openapi-routes.mjs';
import {
  contractPathCount,
  contractPrettierOptions,
  contractSyncErrors,
  readmeRouteCount,
  routeMethods,
  syncContract,
} from './sync-openapi-routes.mjs';

describe('HS2-ZVB932 contract:sync reproduces the committed contract', () => {
  const long = 'A long description that runs well past the configured print width so that Prettier has to wrap it';
  const wrapped = `openapi: 3.1.0\npaths:\n  /a:\n    get:\n      operationId: a\n      description: ${long} somewhere\n        sensible, regardless of where the author broke the line.\n      responses:\n        '200':\n          description: >-\n            Folded block text stays folded.\n`;

  it('wraps only through Prettier, so any hand wrapping converges to one fixpoint', async () => {
    const options = await contractPrettierOptions();
    const once = await syncContract('.route("/a", get(a))', wrapped, options);
    const rewrapped = wrapped.replace(' somewhere\n        sensible,', ' somewhere sensible,');
    expect(once).toContain('description: >-');
    expect(once).not.toBe(wrapped);
    expect(await syncContract('.route("/a", get(a))', once, options)).toBe(once);
    expect(await syncContract('.route("/a", get(a))', rewrapped, options)).toBe(once);
  });

  it('adds route-only stubs with path parameters and keeps existing operations', async () => {
    const options = await contractPrettierOptions();
    const next = await syncContract(
      '.route("/a", get(a).post(b))\n.route("/tickets/{id}", delete(remove_ticket))',
      wrapped,
      options,
    );
    expect(next).toContain('operationId: a\n');
    expect(next).toMatch(/post:\n {6}operationId: b\n {6}x-hotsheet-contract-depth: route-only/);
    expect(next).toMatch(/\/tickets\/\{id\}:\n {4}parameters:\n {6}- name: id\n {8}in: path/);
    expect(contractPathCount(next)).toBe(2);
    expect(await syncContract('.route("/a", get(a))', next, options)).toBe(next);
  });

  it('reads the README route count across a line break', () => {
    expect(readmeRouteCount('inventories all 12\ncurrent server router paths')).toBe(12);
    expect(readmeRouteCount('no count here')).toBeNull();
  });

  it('reports a stale contract and a stale README count', async () => {
    const source = '.route("/a", get(a))';
    const canonical = await syncContract(source, wrapped, await contractPrettierOptions());
    const readme = (count) => `inventories all ${count} current server router paths`;
    expect(await contractSyncErrors(source, canonical, readme(1))).toEqual([]);
    expect(await contractSyncErrors(source, wrapped, readme(2))).toEqual([
      'docs/contracts/v1/openapi.yaml differs from `npm run contract:sync` output; run it from clients/web',
      'docs/contracts/README.md states 2 router paths; the contract has 1',
    ]);
  });

  // Prettier on the full contract is slow inside Vitest's module runner, so check the real files in plain Node.
  it('leaves the committed contract and README count unchanged', () => {
    const script = fileURLToPath(new URL('./sync-openapi-routes.mjs', import.meta.url));
    const result = spawnSync(process.execPath, [script, '--check'], { encoding: 'utf8' });
    expect(result.stderr).toBe('');
    expect(result.status).toBe(0);
  }, 60_000);
});

describe('OpenAPI route inventory', () => {
  it('reads nested Axum method chains and namespaced handlers', () => {
    const source = `
      .route("/tickets/{id}", get(read_ticket).patch(update_ticket))
      .route(
        "/tickets/{id}/media",
        axum::routing::put(write_media)
          .layer(DefaultBodyLimit::max(MAX_BODY_BYTES)),
      )
      .route("/queued", post(provider_write_behind::queue_updates))
    `;
    expect([...routeMethods(source)]).toEqual([
      ['/tickets/{id}', { get: 'read_ticket', patch: 'update_ticket' }],
      ['/tickets/{id}/media', { put: 'write_media' }],
      ['/queued', { post: 'queue_updates' }],
    ]);
  });
});

describe('HS2-140YMZ early route/OpenAPI completeness', () => {
  const source = '.route("/tickets/{id}", get(read_ticket).patch(update_ticket))';
  const operation = { operationId: 'read_ticket', responses: { 200: { description: 'OK' } } };

  it('fails when a newly added route or method has no OpenAPI operation', () => {
    expect(routeCoverageErrors(source, { paths: {} })).toEqual(['Undocumented route: /tickets/{id}']);
    expect(routeCoverageErrors(source, { paths: { '/tickets/{id}': { get: operation } } })).toEqual([
      'Undocumented method: PATCH /tickets/{id}',
    ]);
  });

  it('requires concrete operations and rejects stale paths', () => {
    expect(
      routeCoverageErrors(source, {
        paths: {
          '/tickets/{id}': { get: operation, patch: { ...operation, 'x-hotsheet-contract-depth': 'route-only' } },
          '/removed': { get: operation },
        },
      }),
    ).toEqual(['Incomplete OpenAPI operation: PATCH /tickets/{id}', 'OpenAPI path has no server route: /removed']);
    expect(routeCoverageErrors(source, { paths: { '/tickets/{id}': { get: operation, patch: operation } } })).toEqual(
      [],
    );
  });
});
