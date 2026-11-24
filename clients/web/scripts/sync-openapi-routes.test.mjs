import { describe, expect, it } from 'vitest';

import { routeCoverageErrors } from './check-openapi-routes.mjs';
import { routeMethods } from './sync-openapi-routes.mjs';

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
