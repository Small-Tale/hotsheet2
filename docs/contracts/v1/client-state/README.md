# Portable client-state fixtures v1

These JSON files pin client-owned transitions that SwiftUI and Android can run
against the same server contract as the web client. `schema.json` validates the
fixture envelope and step shapes. The web conformance runner is
`clients/web/src/client-state-fixtures.test.ts` and calls shipped web modules;
run it from `clients/web` with
`zsh -ic 'npx vitest run src/client-state-fixtures.test.ts'` on the project Mac.

Every fixture has `format: "hotsheet-client-state/v1"`, a stable `id`, a `kind`,
an optional `initial` input, and ordered `steps`. Execute each step, then compare
its `expect` object **exactly** with the named observable state. Arrays are
ordered; absent keys are absent, not null. A runner must fail on an unknown
format, kind, or action. A fixture must not be regenerated from its runner's
output: the checked-in expectations are the independent parity oracle.

| Kind           | Input action                                                                             | Observable output                                                                                                                                                                                                 |
| -------------- | ---------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `collections`  | `snapshot`, `search`, `select` (optional `range`/`toggle`), `select_all`, `view`, `sort` | Visible slug order after collection and advanced-expression filtering, selected slugs in visible order, optional range anchor. The view change clears selection.                                                  |
| `history`      | `begin_edit`, `ack`, `remote`, `undo`, `redo`, `queue_undo`                              | Current ticket fields after each step. `begin_edit` projects locally before the pending acknowledgment; queued undo waits for that acknowledgment. Remote field changes survive stale undo/redo.                  |
| `stale_search` | `request` for a named custom view, `resolve` with that request's rows                    | Visible slugs and selected view. Resolve steps may arrive out of request order; an older result must not replace the active view's result.                                                                        |
| `replay`       | Poll `response` or transport `error`                                                     | Every requested `since` cursor (`null` means no cursor), reconciliation reasons, delivered event kinds, and refresh count. A failed request restarts with a cursorless handshake; overflow forces reconciliation. |

The row inputs carry only fields needed by these client projections. The web
runner supplies the other required wire fields with deterministic local values;
native runners may do the same. Poll `response` objects use the v1
[`poll-envelope.schema.json`](../poll-envelope.schema.json) shape. No fixture
defines Git writes, claim arbitration, authorization, provider rules, or ticket
sort order on the server: those remain server-owned. Collection ordering here
is the browser's local workspace sort over already returned rows.

For a native runner, parse and validate the JSON, drive the same user or network
transition through the native client-state layer, serialize the observable
fields named above, and compare after **every** step. Keep the scenarios in
sequence; running each step from a fresh state would miss stale responses,
queued history, range anchors, reconnects, and overflow. Add a new fixture for
a new cross-client behavior and bump the format version if an existing action
or expected field changes meaning. Additive optional inputs can remain in v1
when old runners can ignore them safely.
