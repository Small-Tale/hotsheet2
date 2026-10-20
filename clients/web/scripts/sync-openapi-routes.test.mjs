import { describe, expect, it } from 'vitest';

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
