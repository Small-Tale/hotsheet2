# Client transport contracts

`v1/openapi.yaml` describes the server-owned HTTP surface. The JSON Schemas in the
same directory describe each `/ws/sync` text frame and the `/ws/poll` response.
The OpenAPI file inventories every server router path and HTTP method. An
operation carrying `x-hotsheet-contract-depth: route-only` has its path and
method recorded but still needs concrete request and response DTOs; that work
is tracked by `HS2-11BVBS`. Core compatibility, ticket, and replay responses
have schemas now. Run `npm run contract:sync` from `clients/web` after changing
the router.
These are transport contracts: authorization, Git writes, claim decisions, and
permission policy remain on the server. A client may own a temporary draft or
optimistic display, but it must reconcile with the returned server ticket.

The authenticated `/compatibility` response supplies inclusive `protocol.min`
and `protocol.max` values. A client must check that its supported protocol range
overlaps the server's before making writes. `application_version` and revision
strings are diagnostics, not equality gates. A server may add optional object
members or new event kinds within a compatible protocol range; clients must
ignore unknown members and kinds. Removing a required field, changing its type,
or changing the meaning of an existing field requires a protocol range change.

`/ws/sync` sends future events only. `/ws/poll` without `since` returns the
current cursor without backlog; use that cursor for later polls. With `since`,
events have greater cursors. Cursors belong to one server process and are not
durable ticket identifiers. After `overflow: true`, rebuild the relevant client
projection from the list API and then resume polling from the returned cursor.
Events without a cursor are ephemeral and cannot be replayed. When a socket
disconnects, the client should use a full refresh or the long-poll replay path
before trusting a local projection.

This contract is checked against the server routes and representative server
responses in the repository test suite. The Rust wire DTOs in
`crates/hotsheet-ticketing/src/wire.rs`, the server route handlers, and current
tests remain authoritative if this documentation is found to disagree with
the shipped implementation.
