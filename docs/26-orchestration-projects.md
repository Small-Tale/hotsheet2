# 26. Orchestration Projects

> **Status: Partial (HS2-4J6V46).** Shared declarations, reviewed machine bindings
> and the headless CLI, authenticated HTTP and MCP target adapters are implemented
> and covered by automated tests; the OpenAPI contract is regenerated. Workspace UI,
> grants, ticket relationships and dispatch remain design work under their separate
> implementation tickets.

## 26.1 Boundary and existing foundations

An orchestration project is an ordinary Hot Sheet checkout with at least one
authoritative ticket source. Its tickets plan and record work in other projects;
target tickets remain in their own authoritative git store or external provider.
Creating an orchestration project does not clone, mirror, or import a target. The
client reaches a target through an existing live-mounted server, consistent with
[08](08-distributed-and-remote.md) §8.2. A target server remains authoritative for
its tickets, claims, AI sessions, CI evidence, and deployment operations.

The current checkout registry is machine-local and maps one checkout to zero or
more ticket sources ([04](04-core-server-cli.md) §4.3). Its readable checkout id
is path-derived, so it cannot serve as a portable orchestration target id. An
orchestration target also differs from a ticket source: a target is a _project_
and may contain several sources. The existing exact claim/lease operation is the
execution primitive; orchestration does not add a second claim coordinator.

## 26.2 Shared target declaration and local binding

The orchestration checkout declares targets in a versioned
`.hotsheet2/orchestration.json` file beside its project settings. Version 1 has
`schema_version: 1`, an opaque canonical ULID `revision`, and a `targets` list. Each entry has an immutable random ULID
`target_id`, `state: active|removed`, a human label, and optional normalized,
credential-free expected repository identity. A label or repository URL is
descriptive, never an authorization token or lookup key. The file contains no
origins, bearer secrets, certificate material, local paths, or
customer data. IDs are never recycled; removal marks an entry `removed` and
retains its id. Version 1 retains all tombstones and rejects PUT requests that
drop any existing id; it has no reference-graph garbage collector. Reordering or
renaming an entry preserves its id. Duplicate ids, unsupported versions, and
malformed entries fail explicitly. Missing reads return disabled/empty without
creating a file. The file is limited to 1 MiB and 256 entries including tombstones;
labels are bounded to 256 bytes and repository hints to 2,048 bytes. Locked atomic
writes preserve either complete old or new data. Every actual mutation rotates the
opaque revision, including rename-back; timestamps do not authorize a write. A
stale revision conflicts even if the desired contents are already present. Read,
reconcile the retained id, and deliberately retry with the current revision.

Each machine binds a declared `target_id` to one existing mounted checkout in its
machine-local Hot Sheet registry. The binding records the authenticated pairing
identity of a remote server (or the local server-instance identity) and the
checkout identity, never a credential in the shared declaration. A user must
choose or confirm a binding; matching a label or repository automatically cannot
grant access. Rebinding to a different server or checkout requires an explicit
review of the old and new identities and leaves an audit event. An unbound target
is visible as unbound, not silently removed. If a remote server changes address,
its existing pairing/registry path updates the origin without changing the logical
target id. If its identity cannot be verified, the binding becomes unavailable
until reviewed. Binding keys are qualified by `(owner_checkout_id, target_id)`;
another clone does not borrow a binding just because it has the same target id.
The destination pins reviewed server/CA, checkout and source authority, not origin
or a permanent remote-registration revision. A freshly verified same-identity
address move, leaf renewal or restart therefore needs no target rebind.

`${HOTSHEET_HOME}/orchestration-bindings.json` is a separate version-1 machine
registry with public per-binding revisions and bounded old/new identity audit.
It contains no bearer, credential reference, origin or local filesystem path.
A source authority is a sorted multiset of provider-owned nonsecret identities;
labels, source ids, defaults and capability advertisements do not define it.
Metadata-only source projection does not initialize stores or contact provider
accounts, and provider-only projects do not require Git.

A valid bind/review reserves a private per-key ordering token before probing. This
is conservatively latest-started: a newer admitted attempt invalidates an older
in-flight result even if the newer candidate fails. Reservation does not change
reviewed destination, public revision, cache or audit. Candidate B's failure does
not poison a still-reviewed A. An initial failed or raced attempt can leave only a
revision-zero, unbound, unobserved intent with no history or authority. Repeated
unbind also fences pending activation without advancing an already-unbound public
revision. The registry retains at most 256 keys including intents, 32 audit entries
and 64 cached source identifiers per key; the per-key budget is 64 KiB with a
16 MiB + 1 KiB whole-file limit. Existing keys remain disableable at capacity.

The declaration belongs to the orchestration _checkout_, independently of which
source holds its tickets. The same target can appear in several orchestration
projects under different ids and grants. A target checkout can still have its own
orchestration declaration. Cycle detection belongs to dispatch planning, not
target registration; a cycle never triggers recursive work automatically.

The headless remote-server identity/registry boundary is implemented separately by
HS2-1Y45MZ ([08](08-distributed-and-remote.md) §8.2.1). It exposes reviewed server/CA
identity and fresh path-free checkout discovery. HS2-4J6V46 adds the target model
and bounded checkout-resolution projection; dispatch remains separate work.
Cached remote observations never authorize target resolution or mutation.

## 26.3 Ticket relationships and identity

An orchestration ticket can link zero or more target tickets. Each link is a
structured reference with `target_id`, target `connection_id`, provider-native
`native_id`, relation (`planned`, `created`, `observed`, or `validated`), and an
idempotent operation id when orchestration created the target ticket. The target
id selects the declared project; source and native id select the ticket within
that project. Display slugs are labels only. Resolution follows existing
connection aliases and rejects ambiguity or a retargeted source. A missing
binding or inaccessible source leaves the link intact with an unresolved reason.

These links require a new optional field in the canonical ticket schema; the
implementation must update [17](17-ticket-file-format.md), the parser/writer,
provider mapping, index/backlinks, and format compatibility together. Do not
encode machine-specific origins in a ticket or infer a target from a bare slug.
The existing `@<project-id>/<connection-id>:<native-id>` syntax retains its
current meaning; the orchestration field supplies the logical target id without
changing prose-link parsing in this design ticket. Provider-native backlink
support can arrive later, but an orchestration ticket must always retain its own
outbound relationship.

## 26.4 Headless and service contract

CLI, authenticated HTTP and MCP use one shared runtime over the core transaction.
The CLI emits JSON for all target operations. `--project` is the registered owning
checkout id; `-C` selects the existing host source that owns `providers.json`, not a
target's default Git store. Missing host configuration produces an actionable
error and never creates a hidden store or source. For a provider-only project:

```sh
hotsheet-cli -C <configured-host-source> orchestration target list --project <owner-checkout-id>
hotsheet-cli -C <configured-host-source> orchestration target inspect <destination-checkout-id> --project <owner-checkout-id>
hotsheet-cli -C <configured-host-source> orchestration target inspect <remote-checkout-id> --registration-id <reviewed-remote-id> --project <owner-checkout-id>
```

`inspect` is a fresh machine-local read-confirm-bind step. It uses the existing
reviewed registry, credential owner and transport, then returns a path-free
candidate `destination` and `projection`, `stale:false`, `usable:false`, and no
binding. It writes no manifest, binding, reservation or grant. Copy only the public
candidate destination into an explicit bind request; no credential export or
manual secret workaround is needed. A later bind probes again and checks its own
CAS and proof, so a candidate cannot survive a later retarget or revoke as authority.

- `target list` reads shared declarations without peer requests or secret access.
  `list --target-id <id>` reads that exact stale local binding for reconciliation.
- `target add --target-id <retained-ULID> --label <label>` creates an active entry.
  Omit `--expected-revision` only for an absent manifest; otherwise pass the last
  read opaque revision. Omitting the id allocates and returns a random id, but
  callers retain an id before retryable setup. Never use a label as an idempotency
  key or silently allocate a replacement after a revision conflict. MCP
  `target_id` and `registration_id` must be strings when present; null and other
  JSON types fail before backend dispatch. Only omission selects generation or
  local inspect defaults; tool inputSchema is not runtime enforcement.
- `target rename <id> --label <label> --expected-revision <revision>` and
  `target remove <id> --expected-revision <revision>` preserve immutable identity;
  remove retains the tombstone. Rename also accepts
  `--expected-repository-identity <URI>` or `--clear-expected-repository-identity`
  to correct the descriptive hint with the same revision precondition; supplying
  both fails without a write. MCP and full declaration PUT expose the same metadata
  correction without changing target identity or binding authority.
- `target bind <id> --expected-manifest-revision <revision>
--expected-binding-revision <number> --destination '<public candidate JSON>'`
  explicitly confirms identity. A review additionally supplies
  `--previous-destination '<exact old destination JSON>'`; initial/unbound requests
  use revision zero and a null previous destination.
- `target unbind <id> --expected-binding-revision <number>` supplies the exact old
  destination when bound. It and `list --target-id` bypass `-C` resolution, checkout,
  provider, manifest, identity and runtime construction for orphan cleanup. An
  existing `--project` MCP wrapper uses only its cached service for these two
  cleanup requests, without reopening/registering a lost owner or starting a
  server; a stale endpoint returns an ordinary connection/auth error.
- `target resolve <id>` explicitly probes and returns fresh selection metadata only
  after final guards. Fresh metadata is never delegated permission to mutate tickets.

HTTP declares the same request/response types:

| Route                                                       | Methods and purpose                                                         |
| ----------------------------------------------------------- | --------------------------------------------------------------------------- |
| `/checkouts/{owner}/orchestration/targets`                  | GET declarations; POST add; PUT complete retained-id replacement            |
| `/checkouts/{owner}/orchestration/targets/{target}`         | PATCH rename; DELETE tombstone                                              |
| `/checkouts/{owner}/orchestration/targets/{target}/binding` | GET cached machine binding; PUT explicit bind/review; DELETE machine unbind |
| `/checkouts/{owner}/orchestration/targets/{target}/resolve` | GET fresh target resolution                                                 |
| `/checkouts/{owner}/orchestration/inspect`                  | POST fresh candidate metadata, no binding/grant write                       |
| `/remote-checkouts/{checkout}/resolution`                   | GET bounded path-free hosting metadata                                      |

All endpoints require the normal bearer and retain device-role authorization.
Every binding read/write, resolve and inspect additionally requires literal
loopback at the serving machine; an off-box writer cannot use its enrollment as a
credential proxy. Ordinary declaration and hosting-metadata reads remain available
to authorized readers. MCP `hotsheet_targets` exposes the same actions and inherits
the selected owning checkout. The tool checks machine locality before any backend
request; the HTTP backend also checks the actual destination origin. Only the
machine-wide `hotsheet_remotes` tool excludes implicit project selection.

The hosting endpoint advertises optional `resolution_metadata_version:1` in basic
checkout discovery. Older peers without it, or unsupported versions, report unknown/
unsupported metadata rather than invented Git capabilities. The strict version-1
projection is limited to 256 KiB, 64 sources, and 32 capability query fields of
80 bytes each. It returns verified provider identities and advertised capabilities,
never raw locators, account names, credential references or a full provider descriptor.
Git identities come from existing verified store metadata; external identities use
provider-owned validated endpoint and repository/project fields. URL/userinfo/query/
fragment/control/whitespace-bearing malformed locators require review before an
identity can be published. Explicit configured endpoint ports retain their identity.

Requests naming an orchestration ticket identify its owning checkout and source
explicitly when ambiguous. Target ticket writes later route through the target's
ordinary checkout/source API with qualified ids and an operation id
(HS2-AZ15GA). AI dispatch uses its ordinary exact claim path (HS2-EBHDSB).
Neither declaration nor binding authorizes a target mutation by itself;
target-scoped permissions and audit are HS2-SBQ3T8.

## 26.5 Failure states and compatibility

The read model distinguishes `unbound`, `connecting`, `reachable`, `unreachable`,
`unauthorized`, `source_missing`, and `identity_changed`. These are observed
states, not ticket status changes. Last confirmed target data carries a timestamp
and is marked stale while unreachable; a mutation never executes against stale
state. A target with several sources requires an explicit source for a create.
Unsupported provider operations report capability errors, not a fallback write
to a different source. Concurrent manifest writes use a revision precondition and
return a conflict rather than dropping another editor's target. Stable server and
checkout identity checks precede source availability; empty or partially missing
source identity reports `source_missing`, while an entirely available changed
source authority reports `identity_changed`.

Network calls occur outside locks. Finalization holds checkout-source → provider
configuration → manifest → binding → local remote-registry guards through durable
commit and response construction. Inspection uses the relevant lock subset and
constructs its response under those guards. The remote proof fences local registry
review/revoke/observation changes; peer metadata remains verified as-of its bounded
response, not a distributed atomic transaction. Private ordering tokens are neither
public revisions nor timestamps. List caches are always stale and never usable.

Old checkouts have no orchestration file and behave exactly as ordinary projects.
Enabling orchestration creates a version-1 file without changing existing ticket
bytes or checkout/source links. Existing cross-project text links are not
automatically promoted to structured orchestration relationships. When the new
ticket field is implemented, older tickets read with an empty link list; writers
must follow the version guard and rollout procedure in [19](19-format-compatibility.md)
before producing the new field. A newer manifest version remains readable only
as an explicit unsupported-version error; it is never overwritten by an older
client. Removing orchestration configuration does not delete target tickets.

## 26.6 Relationship to other work

This design defines explicit _orchestration-run_ membership, not a global
cross-server queue. HS2-42B2FD owns general cross-server aggregate views;
HS2-7QHWHJ can reuse its transport and
identity work while showing only linked runs. HS2-AZ15GA, HS2-EBHDSB,
HS2-5JBYPV, and HS2-WN0MXZ implement ticket operations, dispatch, CI/deploy
gates, and extra validation. HS2-WBREDM owns restart reconciliation, and
HS2-2EY9DP owns multi-project failure fixtures. The existing remote-mount and
git-native self-claim decisions in [08](08-distributed-and-remote.md) remain in
force.
