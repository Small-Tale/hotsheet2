# 26. Orchestration Projects

> **Status: Partial (HS2-4J6V46, HS2-SBQ3T8, HS2-YCJDEB).** Shared declarations,
> reviewed machine bindings, delegated grants with their audit journal (§26.7), the
> throughput policy with approvals (§26.8), structured ticket links with target
> ticket operations (§26.3, HS2-AZ15GA), dispatch runs with status and events (§26.9,
> HS2-EBHDSB, HS2-7QHWHJ), target capacity and admission (§26.10, HS2-J8PRTV), AI tool
> capacity states with tool selection (§26.11, HS2-AHG7HW), extra validation steps
> with immutable result references (§26.12, HS2-WN0MXZ), CI and deployment gates of
> runs (§26.13, HS2-5JBYPV; deployments are recorded and approved, not yet executed),
> recovery with reconciliation, pause, handoff and correction (§26.14, HS2-WBREDM),
> and the headless CLI,
> authenticated HTTP and MCP adapters are implemented and covered by automated tests;
> the OpenAPI contract describes them. Workspace UI and the scheduler remain design
> work under their separate implementation tickets.

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
`native_id`, relation (`planned`, `observed`, `created`, or `validated`), and an
idempotent operation id when orchestration created the target ticket. The target
id selects the declared project; source and native id select the ticket within
that project. Display slugs are labels only. A missing binding or inaccessible
source leaves the link intact with an unresolved reason.

The links are the optional `orchestration_links` field of the ticket file
(HS2-AZ15GA, [17](17-ticket-file-format.md)), an additive field under the rules of
[19](19-format-compatibility.md): older tickets read with an empty list and a ticket
without links emits no key. A link holds no origin, path, checkout id or other
machine-specific value, and a target is never inferred from a bare slug. The
existing `@<project-id>/<connection-id>:<native-id>` prose syntax keeps its meaning.
Two links describe the same target ticket when they share the target and source and
either the create operation or the native id; the merge driver unions links by that
subject, keeps the further relation and every known id, and honors a removal made on
one side. Only a git ticket source can store links; an orchestration ticket in a
source that cannot is refused with `orchestration_links_unsupported`.

### Target ticket operations (HS2-AZ15GA)

Every operation passes the authorization gate of §26.7 and §26.8 (`tickets_read` to
read, link and resolve; `tickets_write` to create and update) and is audited with
its outcome code. The target's own ticket source stays authoritative: operations go
through its provider, an unsupported operation is reported as
`target_operation_unsupported`, and nothing is rerouted to another source. The
target's source is always named explicitly, so a multi-source target is never
guessed, and it must be one the freshly resolved target lists.

- **Create** is idempotent by a caller-retained `operation_id` and runs in three
  recoverable steps: record a `planned` link with the operation id, create through
  the target's provider, advance the link to `created` with the native id. It reuses
  the provider-neutral transfer provenance every provider already implements, so the
  created ticket records the operation id and the orchestration ticket as its
  origin, and a repeated request returns the existing ticket (`created: false`). The
  same operation id for a different orchestration ticket is `target_conflict`. If the
  target ticket was created but the link could not be written, the result says
  `link.recorded: false` with `link_write_failed`; repeating the same request repairs
  it. A definitive refusal withdraws the planned link; a transient failure keeps it,
  because it names the operation to resume.
- **Read** returns the provider-neutral ticket. **Update** changes title, details,
  category, priority, status, tags or Up Next through the provider, optionally under
  the provider's concurrency token.
- **Link** records an existing target ticket as `observed` or `validated` after
  reading it, storing the provider's canonical native id. **Unlink** removes the
  orchestration ticket's own link without contacting or changing the target, so it
  works while a target is unbound or unreachable.
- **Links** lists an orchestration ticket's links. With `resolve` each is read
  through the gate: a resolved link carries the target ticket's title, status and
  slug and whether the target ticket names this orchestration ticket as its origin
  (the backlink); a link that cannot be read stays in the list with its reason code;
  a planned link is looked up by its operation id.

The orchestrating side reaches a hosted half that performs the provider operation
where the target lives: in process for a local destination, and over the reviewed
remote registration (pinned origin, mTLS, no redirects, bounded reply) for a remote
one. The target server still enforces its own device role, so a read-only pairing
refuses writes whatever a grant says. Target refusals are content-free codes:
`target_ticket_not_found`, `target_operation_unsupported`, `target_conflict`,
`target_provider_unauthorized`, `target_rate_limited`, `target_source_disabled`,
`target_request_invalid`, `target_peer_unsupported` (an older peer without the
route), `target_operation_failed`. Provider message text is never forwarded.

```sh
hotsheet-cli --actor-role ai --actor-id <worker-id> -C <configured-host-source> orchestration ticket create \
  --project <owner-checkout-id> --ticket <orchestration-ticket> --target <target-id> \
  --connection <target-source> --operation-id <retained-id> --title "..." [--details ...] [--tag ...]
hotsheet-cli ... orchestration ticket show --project <owner> --target <target-id> --connection <source> --id <native-id>
hotsheet-cli ... orchestration ticket update --project <owner> --target <target-id> --connection <source> --id <native-id> --status started
hotsheet-cli ... orchestration ticket link --project <owner> --ticket <orchestration-ticket> --target <target-id> --connection <source> --id <native-id> [--relation validated]
hotsheet-cli ... orchestration ticket unlink --project <owner> --ticket <orchestration-ticket> --target <target-id> --connection <source> --id <native-id>
hotsheet-cli ... orchestration ticket links --project <owner> --ticket <orchestration-ticket> [--resolve]
```

`--source` names the orchestration ticket's connection and may be omitted when the
project has one git ticket source. An identified actor is required.

| Route                                                                             | Methods and purpose                                                                              |
| --------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| `/checkouts/{owner}/orchestration/tickets/{create,read,update,link,unlink,links}` | POST orchestrating-side operations; machine-local                                                |
| `/checkouts/{checkout}/target-tickets`                                            | Hosted half: GET by `connection_id` and `native_id` or `operation_id`; POST create; PATCH update |

The hosted route is ordinary authenticated project data under the caller's device
role, because a remote orchestration project must reach it. MCP `hotsheet_targets`
adds `ticket_create`, `ticket_read`, `ticket_update`, `ticket_link`,
`ticket_unlink` and `ticket_links`. The workspace flows for the same operations are
part of HS2-HSMH2Q.

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
explicitly when ambiguous. Target ticket operations route through the target's own
source with qualified ids and an operation id (§26.3). Dispatch uses the target's
ordinary exact claim (§26.9).
Neither declaration nor binding authorizes a target mutation by itself;
target-scoped grants and their audit journal are §26.7 (HS2-SBQ3T8).

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
gates, and extra validation (§26.12). HS2-WBREDM owns restart reconciliation, and
HS2-2EY9DP owns multi-project failure fixtures. The existing remote-mount and
git-native self-claim decisions in [08](08-distributed-and-remote.md) remain in
force.

## 26.7 Delegated grants and audit (HS2-SBQ3T8)

A binding selects a destination; a **grant** is what lets an orchestration project
act there. Every target action passes one gate, `authorize`, which first resolves
the target freshly and then consults the grants. Anything not provably allowed is
denied, and the answer is written to the audit journal either way.

### Grants

A grant is a machine-local record in `${HOTSHEET_HOME}/orchestration-grants.json`
(mode 0600, version 1). It is never written to the shared manifest, a ticket or a
note, and it holds no bearer, origin, path or other credential: the credential
stays with the reviewed remote registration, and the target server still enforces
its own device role. A grant can only narrow what that pairing already allows.

| Field             | Meaning                                                                                                                                                                                                                                                      |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `grant_id`        | Canonical ULID. Retain it across retries; an identical retry is a no-op and a different body under the same id conflicts.                                                                                                                                    |
| `target`          | `(owner_checkout_id, target_id)`. Another owner with the same target id is a different key.                                                                                                                                                                  |
| `actor_id`        | The one actor the grant covers, such as a worker id. There is no wildcard.                                                                                                                                                                                   |
| `actions`         | A non-empty set of `tickets_read`, `tickets_write`, `work_start`, `gates_observe`, `validation_run`, `sessions_manage`, `deployments_run`.                                                                                                                   |
| `session_scope`   | Required with `sessions_manage` and rejected without it: the allowed tools, optionally their models, and the maximum concurrent sessions (1 to 64). A tool listed without models allows any model; a tool listed with models requires a named, listed model. |
| `expires_at_ms`   | Required, in the future, at most ninety days ahead.                                                                                                                                                                                                          |
| `destination_key` | SHA-256 of the reviewed destination the grant was issued for.                                                                                                                                                                                                |

A grant is `active`, `expired`, `revoked`, or `destination_changed`. Rebinding the
target to another server or checkout, or unbinding it, makes every existing grant
`destination_changed`: a grant never follows a target to a different destination.
Reviewing the target back onto the original destination restores it. Revocation is
final and idempotent; a later retry of the original create returns the revoked
record and never revives it. A target holds at most 32 grants and the file 1,024.
When a target is full, the oldest revoked or expired grant makes room; an active
grant is never dropped, and a target full of active grants refuses a new one.

**Who may grant.** A grant is issued or revoked by a person at the serving machine.
A request whose actor role is `ai` is refused with 403 `forbidden`, the routes are
machine-local like bindings, and the MCP tool offers only the read views. Issuing a
grant requires the target to resolve as usable at that moment, because the grant
pins that confirmed destination. Revoking needs no registered owner, manifest,
binding or reachable target, so authority can always be withdrawn.

The actor id is self-declared, as everywhere in Hot Sheet. Grants scope and record
what automation on this machine does; they are not a cryptographic boundary between
processes of the same user. The authenticated target server remains that boundary.

### Authorization

`authorize(target, actor, action, session?, operation_id?)` requires an identified
actor, resolves the target freshly, and denies with `target_unavailable` when the
result is not usable (unbound, removed, unreachable, unauthorized, missing source,
changed identity, or an unknown owner). It then needs a grant for that exact actor
that is active for the confirmed destination and covers the action. A
`sessions_manage` request also names its tool and model; starting a session is
refused at the concurrency cap, while stopping or winding one down is never blocked
by the cap. The denial reported is the closest one: `no_grant`,
`action_not_granted`, `destination_changed`, `grant_revoked`, `grant_expired`,
`session_scope_required`, `tool_not_granted`, `model_not_granted`,
`concurrency_exceeded`. A granted action is then put to the throughput policy
(§26.8), which can answer `policy_denied` or `approval_required` with a `detail`
code; both the policy and a grant must allow an action.

`access` is a side-effect-free check against the cached binding. It makes no
request, reads no secret, writes no audit entry, and is never authority: the action
itself confirms the destination again.

### Audit journal

`${HOTSHEET_HOME}/orchestration-audit/<owner-checkout-id>.jsonl` is an append-only
journal with one JSON object per line: `seq`, `at_ms`, `actor_role`, `actor_id`,
`target_id`, `action`, `operation_id`, `outcome` (`allowed`, `denied`, `succeeded`,
`failed`, `granted`, `revoked`), a `reason` code, and `grant_id`. Every field is an
identifier or a stable code; free text, ticket content, credentials and origins are
rejected at the journal boundary. Grant creation and revocation, every `authorize`
answer, and the reported result of an authorized action are recorded under the same
operation id.

A segment rotates at 1 MiB to one retained predecessor, so the journal is bounded
near 2 MiB per owner. `seq` never restarts. A reader passes `after` and gets later
entries, a `cursor` to continue from, `more`, and `gap: true` when retention removed
entries it had not seen. A torn final line from a crash is skipped and never
rewritten; the next append starts on its own line.

### Headless surface

```sh
hotsheet-cli -C <configured-host-source> orchestration grant list <target-id> --project <owner-checkout-id>
hotsheet-cli --actor-role human --actor-id <you> -C <configured-host-source> orchestration grant add <target-id> \
  --project <owner-checkout-id> --actor <worker-id> --action tickets_read --action tickets_write --expires-in 14d
hotsheet-cli -C <configured-host-source> orchestration grant add <target-id> --project <owner-checkout-id> \
  --actor <worker-id> --action sessions_manage --tool claude:<model> --tool codex --max-concurrency 2 --expires-in 8h
hotsheet-cli -C <configured-host-source> orchestration grant check <target-id> --project <owner-checkout-id> \
  --actor <worker-id> --action sessions_manage --tool codex --running 1 --starting
hotsheet-cli -C <configured-host-source> orchestration grant revoke <target-id> <grant-id> --project <owner-checkout-id>
hotsheet-cli -C <configured-host-source> orchestration audit --project <owner-checkout-id> --after <seq> --limit 100
```

`grant add` takes `--expires-in` (`30m`, `8h`, `14d`) or an absolute
`--expires-at-ms`; use the absolute form with a retained `--grant-id` to retry
safely. A session launched by Hot Sheet acts as `ai` and cannot run `grant add` or
`grant revoke`.

| Route                                                              | Methods and purpose                                    |
| ------------------------------------------------------------------ | ------------------------------------------------------ |
| `/checkouts/{owner}/orchestration/targets/{target}/grants`         | GET retained grants with state; POST issue             |
| `/checkouts/{owner}/orchestration/targets/{target}/grants/{grant}` | DELETE revoke                                          |
| `/checkouts/{owner}/orchestration/targets/{target}/access`         | POST side-effect-free check against the cached binding |
| `/checkouts/{owner}/orchestration/audit`                           | GET journal page, `?after=<seq>&limit=<n>`             |

All four require the bearer and literal loopback at the serving machine for every
device role. MCP `hotsheet_targets` adds the read-only actions `grant_list`,
`access_check` (with `for_actor`, `target_action` and an optional `session`) and
`audit`. Errors use 403 with the denial code or `forbidden`.

## 26.8 Orchestrated throughput and its policy (HS2-YCJDEB)

Decided with the maintainer on 2026-11-03 and extended on 2026-11-04.

### Design

- **Goal.** Orchestration creates AI tool sessions as appropriate and maximizes
  throughput across target projects, balancing priority, urgency, compute, resources
  such as RAM, and AI tool capacity. Tools may be mixed across targets. A started
  session appears in the bottom drawer of the server that hosts it.
- **Automatic within a policy.** Inside the policy the orchestrator starts, refills
  and winds down sessions on its own and logs every decision. Anything outside it
  asks a person first.
- **Roles are unchanged.** Started sessions join the existing dispatcher, integrator
  and worker roles. The orchestrator decides how many sessions run where; the
  target's dispatcher still assigns each exact ticket, and worktree isolation with
  integrator handoff stays the only path to the shipping branch.
- **Tool capacity is configured plus observed.** The policy holds per-tool and
  per-account limits; observed limit signals are HS2-AHG7HW.
- **Central plan, local execution.** The orchestration project's server plans
  placement. Each target's server starts the session, enforces its own machine
  limits, and may refuse or reduce a request (HS2-J8PRTV).
- **Waiting is a cost.** Time blocked on CI, deployments, reviews and provider limits
  is minimized by doing unrelated work and, where the policy allows, by building
  ahead on a prerequisite whose gate is still pending (HS2-2SJPCH, HS2-JSF985).

This reverses the earlier decision in [05](05-ai-tool-plugins.md) to keep
orchestration prompt-driven; the reversal and its reason are recorded there.

### Policy file

`.hotsheet2/orchestration-policy.json` is checked in beside the target manifest:
`schema_version: 1`, an opaque ULID `revision`, and a `policy` object. A missing
file reads as disabled and is never created by a read. Every omitted section takes
its default, a newer version is an explicit unsupported-version error and is never
overwritten, and a stored policy that no longer validates is treated as corrupt
rather than evaluated. All quantities are integers: megabytes, percent and cents.

| Section       | Fields and defaults                                                                                                                                                      |
| ------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `machine`     | Default limits for any machine hosting a target: `max_sessions` 2 (1 to 64), `min_free_ram_mb` 2048, `max_cpu_percent` 90, `min_free_disk_mb` 5120.                      |
| `tools[]`     | `tool`, `max_sessions` (1 to 64), `weight` 50 (1 to 100, relative preference), and optional `accounts[]` with `account`, `max_sessions` and `budget_cents_per_day`.      |
| `targets[]`   | `target_id`, `enabled` true, `actions` (the grant action names), `tools[]` with optional `models`, `max_sessions` 1, and optional `machine` and `speculation` overrides. |
| `wind_down`   | `idle_minutes` 15 (1 to 1440) and `keep_warm` 0, idle sessions kept per target.                                                                                          |
| `approval`    | `required_for`: any of `tickets_write`, `work_start`, `validation_run`, `session_start`, `session_stop`, `role_session_start`, `provisional_dispatch`, `deployment`.     |
| `speculation` | `allowed` false, `max_depth` 1 (1 to 8 unconfirmed gates), `risk_weight` 50 (0 ranks provisional work like confirmed work, 100 last), `opt_out_tag` `speculate:never`.   |

A target the policy does not list, or lists as disabled, is not permitted. A tool
listed for a target must be configured under `tools`. A tool configured with
accounts runs only under one of them.

`validate` reports every problem at once with a stable code and the tool, account or
target it concerns. Conflicts are errors: a target cap above its machine cap, an
account cap above its tool cap, a target tool that is not configured, tools without
`sessions_manage` or the reverse, duplicates, and a target the manifest does not
actively declare. `set` refuses an invalid policy, replaces the file at the expected
revision, and is a no-op for identical content. Like grants, it is a person's act:
an `ai` actor is refused, because the governed session must not loosen its own
bounds.

### Place in the authorization gate

`authorize` consults the policy only for an action a grant already allows, so a
missing grant is always reported as such and no approval request is filed for it.

- **No policy file.** Explicit ticket, gate and validation actions are governed by
  grants alone, so a project that never enables throughput is unaffected. A
  validation step that runs a command chosen by the orchestration project still
  asks a person (§26.12).
  `sessions_manage` is denied `policy_absent`: nothing automatic runs without a
  policy. An unreadable or corrupt policy denies everything.
- **Denials** are `policy_denied` with a `detail`: `policy_target_not_permitted`,
  `policy_target_disabled`, `policy_action_not_permitted`,
  `policy_tool_not_permitted`, `policy_model_not_permitted`, `policy_load_unknown`,
  `policy_target_cap`, `policy_machine_cap`, `policy_tool_cap`,
  `policy_account_not_permitted`, `policy_account_cap`,
  `policy_speculation_not_allowed`. The audit entry records the detail.
- **Caps.** A session start states the current load (sessions on the target, on its
  machine, on the tool and on the account). A start without it is denied, and each
  cap denies exactly at its bound. Stopping or winding down needs no headroom.
- **Resource floors** are not evaluated here. The scheduler and the target's own
  admission check compare them with the target's capacity report (HS2-J8PRTV,
  HS2-ECWG50).

### Approvals

When the policy lists a decision under `approval.required_for`, `authorize` needs an
operation id and answers `approval_required` until a person approves that exact
`(target, actor, gate, operation id)`. The first request files one `pending` record
in `${HOTSHEET_HOME}/orchestration-approvals.json`; asking again adds nothing. An
approval is consumed by the one call it allows and must be used within an hour; a
denial is final for that operation; an unanswered request lapses after a day. Reads
and gate observation never wait for a person. A provisional dispatch asks under
`provisional_dispatch` first. Answers are audited; an `ai` actor cannot give one.

### Dry run

`dry-run` evaluates a proposed policy, or the stored one, against every actively
declared target and reports per action whether the policy allows it, needs approval
or denies it and why. With an actor id it adds that actor's grant decision and an
`automatic` flag for actions both would allow right now. It writes nothing, files no
approval request and contacts no target, so "the policy allows it, the grant does
not" is visible before anything is enabled.

### Headless surface

```sh
hotsheet-cli -C <configured-host-source> orchestration policy show --project <owner-checkout-id>
hotsheet-cli -C <configured-host-source> orchestration policy validate --project <owner-checkout-id> --file policy.json
hotsheet-cli -C <configured-host-source> orchestration policy dry-run --project <owner-checkout-id> --file policy.json --actor <worker-id>
hotsheet-cli --actor-role human -C <configured-host-source> orchestration policy set --project <owner-checkout-id> --file policy.json
hotsheet-cli -C <configured-host-source> orchestration approval list --project <owner-checkout-id>
hotsheet-cli --actor-role human -C <configured-host-source> orchestration approval approve <approval-id> --project <owner-checkout-id>
```

`--file` takes a policy body or the output of `policy show`, and `-` reads stdin.
`policy set` without `--expected-revision` reads the current revision first, so a
repeated setup run changes nothing while a concurrent edit still conflicts.
`grant check` accepts `--target-sessions`, `--machine-sessions`, `--tool-sessions`,
`--account` and `--account-sessions` to state the load of a session check.

| Route                                                   | Methods and purpose                                            |
| ------------------------------------------------------- | -------------------------------------------------------------- |
| `/checkouts/{owner}/orchestration/policy`               | GET stored policy; PUT replace at the expected revision        |
| `/checkouts/{owner}/orchestration/policy/validate`      | POST every problem in a proposed or the stored policy          |
| `/checkouts/{owner}/orchestration/policy/dry-run`       | POST per-target outcome; machine-local because it reads grants |
| `/checkouts/{owner}/orchestration/approvals`            | GET requests with their state; machine-local                   |
| `/checkouts/{owner}/orchestration/approvals/{approval}` | POST `{approve}`; machine-local                                |

The policy itself is ordinary authenticated project data, like the target
declarations. MCP `hotsheet_targets` adds the read-only actions `policy_show`,
`policy_validate`, `policy_dry_run` and `approval_list`; setting the policy and
answering an approval are a person's CLI or HTTP actions. The workspace editor for
the same policy is part of HS2-HSMH2Q.

## 26.9 Dispatch runs (HS2-EBHDSB)

A **run** is this machine's record of one attempt to get an exact target ticket
worked on. It holds no lock of its own. The target ticket's claim and lease are the
only lock, there is no second claim coordinator, and a run records what the target
answered.

### Two ways to dispatch

- **`direct`** takes the exact ticket's claim for a named worker through the
  target's own claim and lease operations. The orchestrator renews and releases that
  lease. A ticket someone else holds is refused by the target and the run fails with
  `target_conflict`; nothing is forced.
- **`dispatcher`** is for a target that runs its own dispatcher. The orchestrator
  marks the ticket Up Next and waits. The target's dispatcher assigns it and its
  worker claims it. The orchestrator never assigns inside another project's pool and
  never names the worker.

The scheduler (HS2-ECWG50) decides the target, tool and model and how many sessions
run where; the target's dispatcher still assigns each exact ticket. Integration,
the project's push policy and worktree handoff stay entirely with the target's own
workflow: a run only claims or queues.

### Run record and states

Runs are stored in `${HOTSHEET_HOME}/orchestration-runs.json` (mode 0600, version 1,
at most 1,024; the oldest finished run is forgotten first and an active run never
is). A run has a `run_id`, the caller's `operation_id`, the orchestration ticket,
the exact target ticket by canonical native id, the mode, the worker, the lease
expiry, a `cancel_requested` flag and a history of at most 64 transitions that always
keeps the first. Every history entry is a time, the states, a reason code and an
actor id; repeated and reversed transitions are all kept.

| State                              | Meaning                                                | May become                                                     |
| ---------------------------------- | ------------------------------------------------------ | -------------------------------------------------------------- |
| `requested`                        | Recorded before the target was asked                   | `queued`, `dispatched`, `completed`, `failed`, `cancelled`     |
| `queued`                           | Marked Up Next for the target's dispatcher             | `dispatched`, `completed`, `failed`, `cancelled`, `intervened` |
| `dispatched`                       | The target ticket is claimed for the run's worker      | `completed`, `failed`, `cancelled`, `intervened`               |
| `intervened`                       | A person or another worker holds or released the claim | `dispatched`, `completed`, `failed`, `cancelled`               |
| `completed`, `failed`, `cancelled` | Final                                                  | nothing                                                        |

Every other edge is refused with `run_invalid_transition`, and a finished run
refuses any change with `run_not_active`. A renewed lease is an update, not a
transition. A person's correction is the one exception to the edges (§26.14).

### Rules

- **Duplicates.** The same operation id returns the same run and never claims
  twice, including a run that already failed. The same id for a different ticket,
  mode or worker is a conflict.
- **One active run per target ticket**, whatever the operation or worker
  (`run_target_active`). A worker that already holds an active direct run on a target
  takes no second ticket there (`run_worker_busy`).
- **Authority.** Dispatch passes the gate as `work_start` with the operation id, so a
  policy that asks a person first applies. Renewing and cancelling continue that
  decision: grants and policy limits still apply, but nobody is asked again.
  Supervising with `sync` needs `tickets_read`.
- **Refusals.** The target ticket is read first, so a ticket that does not exist is
  never recorded as a run. A definitive target refusal ends the run as `failed` with
  the target's code; a failure that may be transient leaves it `requested` so the
  same request can be repeated.
- **Cancel** releases a direct run's claim, or withdraws the Up Next mark of a run
  still queued, and ends the run. A pooled worker's claim is not the orchestrator's
  to release; cancelling such a run only lets go of the run. If the target cannot be
  told, the run stays active with `cancel_requested: true` and the answer lists what
  it could not verify (`target_claim` or `target_queue`).
- **Sync** reads the target ticket. A completed, verified or archived ticket
  completes the run. A queued run becomes dispatched when the ticket shows a live
  claim, recording that worker. A dispatched run whose claim was released or taken
  becomes `intervened`, and returns to `dispatched` when its own worker holds a live
  claim again. A lapsed lease is not a live claim.
- **On the orchestration ticket** the run adds a link to the target ticket and an
  activity note with the run id at dispatch and when it finishes. The note holds
  identifiers only.

Exact claims exist only for git ticket sources; another provider answers
`target_operation_unsupported` to a direct dispatch. Dispatcher mode works wherever
the provider can set Up Next.

### Headless surface

```sh
hotsheet-cli ... orchestration run dispatch --project <owner> --ticket <orchestration-ticket> \
  --target <target-id> --connection <source> --id <target-ticket> --operation-id <retained-id> \
  --worker <worker-id> [--worker-label ...] [--lease-minutes 30]
hotsheet-cli ... orchestration run dispatch ... --via dispatcher
hotsheet-cli ... orchestration run list --project <owner> [--active]
hotsheet-cli ... orchestration run show|renew|cancel|sync <run-id> --project <owner>
```

| Route                                                             | Methods and purpose                                                                  |
| ----------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| `/checkouts/{owner}/orchestration/runs`                           | GET runs (`?active=true`); POST dispatch; machine-local                              |
| `/checkouts/{owner}/orchestration/runs/{run}`                     | GET one run; machine-local                                                           |
| `/checkouts/{owner}/orchestration/runs/{run}/{renew,cancel,sync}` | POST supervise; machine-local                                                        |
| `/checkouts/{checkout}/target-claims`                             | Hosted half: POST claim, PATCH renew, DELETE release of an exact ticket for a worker |

The hosted claim route is ordinary authenticated project data under the caller's
device role, like the hosted ticket route. MCP `hotsheet_targets` adds
`run_dispatch`, `run_list`, `run_show`, `run_renew`, `run_cancel` and `run_sync`.
Reconciliation after a restart is §26.14, and starting the sessions that do the
work is HS2-GDJXGZ.

### Status, events and following targets (HS2-7QHWHJ)

**Last confirmed state.** A successful read of the target ticket is stored on the
run as `observed`: when it was read, the ticket's status and phase, who holds the
claim and until when. An older or replayed read never replaces a newer one. While
the target cannot be confirmed the run carries `unreachable` with the time the
outage began and its reason code, and `observed` stays as it was. The status view
(`run status`, `GET .../runs/{run}/status`) answers from the record alone and adds
`reachability` (`reachable`, `unreachable`, or `unknown` when never observed),
`stale` and `last_confirmed_at_ms`, so a viewer always sees how far the shown state
can be trusted.

**Event stream.** Every change to a run is published to a bounded append-only
journal per owning checkout, `${HOTSHEET_HOME}/orchestration-run-events/`, with the
same sequence cursor, rotation and gap reporting as the audit journal. Event kinds
are `state`, `lease`, `observed`, `target_unreachable`, `target_reachable`,
`cancel_requested`, `validation` (§26.12), `gate` (§26.13) and the recovery kinds of
§26.14 (`reconciled`, `paused`, `resumed`, `handoff`, `corrected`), with `session`
reserved for the ticket that adds it. Each event says whether the orchestrator did it or it was learned
from the target, and carries identifiers, states and codes only.

- **Ordering.** Events are recorded in arrival order and keep the time the fact was
  true at its source, so a late report is visible as late rather than rewriting order.
- **Duplicates.** An event can carry a key that identifies the fact. A second report
  with the same key for the same run is not recorded again, whoever reports it and in
  whatever order. Re-reading unchanged target state produces no event at all.
- **Replay.** A reader passes its cursor and gets later events, optionally for one
  run; the cursor advances past other runs' events too. `gap: true` says retention
  removed events it had not seen. With `wait_ms` (at most 20 seconds) an empty answer
  waits for the next event, woken by the journal changing. Clients do not poll.

**Following a target.** A server follows each target that has an unfinished run by
holding one bounded request open on the target's hosted change feed
(`/checkouts/{checkout}/target-events`, at most 8 seconds per request). A first
request is a handshake that returns the cursor and no backlog, after which every run
of that target is caught up once. After that only runs whose ticket changed are
synced. A cursor that goes backwards or an `overflow` answer means the host no
longer has the backlog, and every run is caught up again. On failure the follower
keeps its cursor, so reconnecting replays what was missed, marks the affected runs
unreachable once, and retries with bounded backoff from one second up to a minute.
It confirms its authority through the gate when it connects, after any failure and
every ten minutes, acting as whoever dispatched the run. A target with no unfinished
run is not contacted at all, and a run a person took over is still followed.

A follower starts when the server starts with unfinished runs (§26.14), when a run
is dispatched through the server or when someone reads the event stream, and ends
with the last unfinished run of its target. The hosted
feed comes from the hosting server's own change stream; a process without one, such
as the bare CLI, answers `target_peer_unsupported`, and `run sync` remains the
headless way to bring a run up to date. This view covers explicitly dispatched runs
only; general cross-server aggregate views are HS2-42B2FD.

```sh
hotsheet-cli ... orchestration run status <run-id> --project <owner>
hotsheet-cli ... orchestration run events --project <owner> [--after <seq>] [--run <run-id>] [--wait-seconds 20]
```

| Route                                                | Methods and purpose                                                                            |
| ---------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `/checkouts/{owner}/orchestration/runs/{run}/status` | GET the run with its last confirmed target state; machine-local                                |
| `/checkouts/{owner}/orchestration/run-events`        | GET events after `after`, optional `run`, `limit` and `wait_ms`; machine-local                 |
| `/checkouts/{checkout}/target-events`                | Hosted: GET ticket changes after `since` with an optional `wait_ms`; no `since` is a handshake |

MCP `hotsheet_targets` adds `run_status` and `run_events`.

## 26.10 Target capacity and admission (HS2-J8PRTV)

The orchestrator plans placement, and each target's own server decides whether a
session may start on its machine. This section covers what a machine reports about
itself and how it answers. Starting the session is HS2-GDJXGZ; the scheduler that
asks is HS2-ECWG50.

### Capacity snapshot

A snapshot is what the server hosting a checkout measured just now. Megabytes and
percent are integers. A value the host could not measure is `null`; nothing is
estimated in its place.

| Field                                  | Meaning                                                                                                |
| -------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| `server_id`, `checkout_id`             | The machine identity and the hosted checkout the snapshot is for                                       |
| `sampled_at_ms`                        | When the shared reading was taken                                                                      |
| `memory.free_mb`, `memory.total_mb`    | Memory a new process could use without swapping, and physical memory                                   |
| `cpu.load_percent`, `cpu.logical_cpus` | Whole-machine busy share over a 250 ms window, and the core count                                      |
| `disk.free_mb`, `disk.total_mb`        | The volume that holds the checkout                                                                     |
| `tools[]`                              | Each drivable AI tool by name, whether it is `installed`, and whether the checkout has it `configured` |
| `sessions[]`                           | Each running AI session: `session_id`, `tool`, `busy`, `on_checkout`, `resident_mb`                    |
| `claims[]`, `claim_namespaces`         | Live resource claims (namespace, resource, expiry) in the namespaces the caller named                  |

- **Tools** come from the plugin registry. A tool is installed when its program is on
  the server's `PATH`; nothing is executed to find out, so
  `HOTSHEET_AI_TOOL_PROBES=off` changes nothing here. A tool is configured unless the
  checkout's `enabled_plugins` setting leaves it out.
- **Sessions** are the server's live AI terminals. `resident_mb` is the resident
  memory of the session's process and its descendants, from the process id in the
  launch journal and one `ps` listing of ids and sizes. It is `null` when the
  platform or the journal cannot supply it. `on_checkout` says whether the session was
  started inside the checkout.
- **Claims** are reported for the namespaces the caller names with `claim_namespaces`
  (at most eight). The claims API cannot enumerate namespaces yet (HS2-YDZAFH), so
  without a name `claims` is `null`, not an empty list.
- **Privacy** follows HS2-SW5ZK3. A process is a session id, a tool name and numbers.
  No process path, command line, working directory, user name or claim metadata is in
  a snapshot, for a local or a remote reader.
- **Platforms.** macOS and Linux report every value ([host-health](host-health.md)).
  On Windows free and total memory, CPU load and `resident_mb` are `null` until
  HS2-4A5AV7; disk, tools and sessions are reported.

### Sampling only on demand

There is no capacity sampler. A reading is taken when a request asks for a snapshot or
an admission answer, and it is shared for three seconds: callers queue on one lock, so
a burst is one reading. A server that nobody asks reads nothing. The host health
sampler ([host-health](host-health.md)) is separate and unchanged.

This ticket ships request-driven snapshots only. A capacity stream on the live channel
that runs while a subscriber exists is HS2-P5GTSW.

A headless process hosts no sessions. `hotsheet-cli` and the MCP core backend ask the
Hot Sheet server running on the same machine for its snapshot and report every value
as unknown when none is running.

### Admission

"May `sessions` sessions of tool T start on checkout C now?" is answered by the
machine that hosts C, from three inputs:

1. its own snapshot;
2. the limits the orchestrator sends, which are the policy's `machine` limits for that
   target (§26.8), or the policy defaults when a caller sends none;
3. its owner's admission file, `${HOTSHEET_HOME}/orchestration-admission.json`.

The file can only make the answer stricter. For each limit the enforced value is the
stricter of the two: the smaller `max_sessions` and `max_cpu_percent`, the larger
`min_free_ram_mb` and `min_free_disk_mb`. A looser value in the file changes nothing.

```json
{
  "schema_version": 1,
  "admission": {
    "max_sessions": 2,
    "min_free_ram_mb": 4096,
    "max_cpu_percent": 80,
    "min_free_disk_mb": 10240,
    "allow_unknown": false,
    "default_footprint_mb": 1024,
    "tool_footprint_mb": { "codex": 1500 }
  }
}
```

Every key is optional. `max_sessions` is 0 to 64, where 0 refuses every session. The
footprint of one session is the owner's estimate for the tool, else
`default_footprint_mb`, else 1024 MB. If a running session of the same tool is
observed to use more, the observed value is used. The answer names the value and its
source (`default`, `configured` or `observed`). An absent file is all defaults; a
damaged or newer one is an error, never a looser default.

The answer is `accepted` (all asked for may start), `reduced` (`admitted` says how
many) or `refused`, with the limits that were enforced and one reason code:

| Reason                | When                                                                      |
| --------------------- | ------------------------------------------------------------------------- |
| `tool_not_installed`  | The host does not know the tool or its program is missing                 |
| `tool_not_configured` | The checkout opted out of the tool                                        |
| `session_cap_reached` | Running AI sessions leave no room, or less room than asked, under the cap |
| `above_cpu_ceiling`   | CPU load is above `max_cpu_percent`                                       |
| `below_disk_floor`    | Free disk is below `min_free_disk_mb`                                     |
| `below_ram_floor`     | Free RAM minus the floor fits no session, or fewer than asked             |
| `capacity_unknown`    | A needed measurement is `null` and the owner did not set `allow_unknown`  |

A limit that a known measurement breaks always refuses with its own reason. Unknown
measurements are listed in `unknown` and refuse as `capacity_unknown` unless the owner
set `allow_unknown`, in which case that check is skipped. `reduced` names the tighter
of the session cap and the RAM room. The decision is a pure function
(`decide_admission`) of the snapshot, the file and the request.

On the orchestrating side the question passes the gate (§26.7) as `sessions_manage`
for the tool and model. It starts nothing, so it carries no load and files no
approval; grant and policy must still permit the tool and model on the target. The
start itself passes the gate again with its load and any approval (HS2-GDJXGZ).
Reading capacity passes the gate as `gates_observe`. Both are recorded in the audit
journal, including a target's failure code.

### Headless surface

```sh
hotsheet-cli ... orchestration tool-capacity show --project <owner>
hotsheet-cli ... orchestration tool-capacity signal --project <owner> --tool <tool> \
  [--account <account>] --kind rate_limit|usage_limit|recovered|usage \
  [--at-ms <unix-ms>] [--resets-at-ms <unix-ms>] [--cost-cents <n>] [--session <id>]
hotsheet-cli ... orchestration tool-capacity select --project <owner> --target <target-id> \
  [--tool <tool>] [--model <model>] [--load <tool>[/<account>]=<sessions>]...
```

| Route                                                    | Methods and purpose                                          |
| -------------------------------------------------------- | ------------------------------------------------------------ |
| `/checkouts/{owner}/orchestration/tool-capacity`         | GET states of the policy's tool accounts; machine-local      |
| `/checkouts/{owner}/orchestration/tool-capacity/signals` | POST one signal for a configured tool; machine-local         |
| `/checkouts/{owner}/orchestration/tool-capacity/select`  | POST the selection for one target; reads only; machine-local |

A signal through the route must name a tool, and an account when given, that the
owner's policy configures. An AI actor may report a limit or usage but is refused
`recovered` with `forbidden`: the session a limit holds back must not lift it. A
route signal that changes the state is recorded in the audit journal (§26.7) as
`tool_capacity_<kind>` with the new state code. MCP `hotsheet_targets` adds the
read-only `tool_capacity` and `tool_select`; it reports no signals.
hotsheet-cli ... orchestration capacity show --project <owner> --target <target-id> [--claims <namespace>]
hotsheet-cli ... orchestration capacity show --checkout <checkout-id> # this machine
hotsheet-cli orchestration admission show
hotsheet-cli orchestration admission set [--max-sessions N] [--min-free-ram-mb N] \
[--max-cpu-percent N] [--min-free-disk-mb N] [--allow-unknown true|false] \
[--default-footprint-mb N] [--tool-footprint <tool>=<mb>] [--clear <limit>]
hotsheet-cli ... orchestration admission check --project <owner> --target <target-id> \
--tool <tool> [--model <model>] [--sessions N]
hotsheet-cli ... orchestration admission check --checkout <checkout-id> --tool <tool>

```

`admission set` is idempotent: the same command again writes nothing. It is a person's
act at the machine; an AI actor is refused, and there is no HTTP or MCP route that
writes the file, so a remote orchestrator can never loosen it. `--clear` takes
`max_sessions`, `min_free_ram_mb`, `max_cpu_percent`, `min_free_disk_mb`,
`default_footprint_mb` or `tool_footprint:<tool>`. There is no `status` command, so
capacity lives under `orchestration`.

| Route                                                         | Methods and purpose                                                                 |
| ------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| `/checkouts/{checkout}/target-capacity`                       | Hosted half: GET this machine's snapshot (`?claim_namespaces=a,b`)                  |
| `/checkouts/{checkout}/target-admission`                      | Hosted half: POST `{tool, model?, sessions?, limits?}` for this machine's answer    |
| `/checkouts/{owner}/orchestration/targets/{target}/capacity`  | GET (`?actor_id=`) or POST (`actor` in the body) a target's snapshot; machine-local |
| `/checkouts/{owner}/orchestration/targets/{target}/admission` | POST `{tool, model?, sessions?}` for the target's answer; machine-local             |

The hosted routes are ordinary authenticated project data under the caller's device
role, like the hosted ticket route, so a remote orchestration project reaches them
over its reviewed registration. A read-only device can read capacity and cannot ask
for admission. A peer that predates the routes answers `target_peer_unsupported`. MCP
`hotsheet_targets` adds the read-only actions `capacity` and `admission_check`.

## 26.11 AI tool capacity (HS2-AHG7HW)

The policy (§26.8) configures how many sessions each tool and account may run and,
optionally, what an account may spend per day. Tool capacity adds what was observed:
whether a tool account is usable now, and if not, until when. The scheduler
(HS2-ECWG50) reads it to choose a tool and to move work to another allowed tool when
one is limited. This section starts and stops no session.

### What a plugin can report

Each plugin manifest declares in `[capacity]` which signals this machine can observe
from the tool ([05](05-ai-tool-plugins.md) §5.3). Each value is `reported` or
`unknown`; there is no value that means a limit never happens. The bundled plugins
declare exactly what the existing detection observes:

| Tool        | Rate-limit errors | Usage-limit errors | Reset times | Usage events | Cost     |
| ----------- | ----------------- | ------------------ | ----------- | ------------ | -------- |
| Claude      | reported          | reported           | reported    | reported     | reported |
| Codex       | unknown           | reported           | reported    | reported     | reported |
| OpenCode    | unknown           | unknown            | unknown     | reported     | unknown  |
| Antigravity | unknown           | unknown            | unknown     | unknown      | unknown  |

Detecting the signals that are still unknown is HS2-2BAVAM.

### Signals

A signal is one observation about a tool account: `rate_limit`, `usage_limit`,
`recovered`, or `usage`. It carries the tool id, an optional account id, the time, an
optional reset time (limits), an optional cost in cents (usage) and an optional opaque
session id. It has no message or other free-text member, and an unknown member is
refused, so provider text and customer content cannot enter a signal, the registry or
the audit journal.

The server records signals from the detection it already has, without reading a
provider message a second way:

- A terminal halt whose error type `stuck_causes::is_usage_limit` accepts becomes a
  limit signal for the halt's tool. Its reset time is what `stuck_causes::reported_reset`
  reads from the message, or absent. This covers Claude's `StopFailure` hook and the
  Codex rollout watch, which share one halt path.
- When a session halted on a limit takes a new turn (the tool's resume hook, or a new
  Codex turn), a `recovered` signal is recorded. Ending the session, closing its
  terminal, or a person dismissing the stopped state proves nothing and records
  nothing.
- A halt that is not a limit, and a halt whose tool is not known, record nothing.

A failed record is logged by code and never fails the halt or resume that carried
it. `tool_capacity_signals::usage_signal` maps a usage event to a `usage` signal;
feeding usage events automatically is HS2-XFMKHQ. Sessions do not carry an account
identity yet, so detected signals are unattributed (HS2-PFHRS9).

### States

Each `(tool, account)` has one state at any instant:

| State               | Meaning                                                                  |
| ------------------- | ------------------------------------------------------------------------ |
| `available`         | A turn was observed to succeed and nothing restricts the account.        |
| `limited` + `until` | A limit is in force, or the day's budget is spent, until the given time. |
| `degraded`          | Usable only as a probe: recovery is unconfirmed or the signals disagree. |
| `unknown`           | The plugin cannot report limits, or nothing has been observed.           |

`unknown` and `degraded` are never treated as `available`. Each state other than
`available` carries a reason code: `rate_limit`, `usage_limit`, `budget_exceeded`,
`recovery_unconfirmed`, `inconsistent_signals`, `unattributed_limit`, `not_reported`,
`not_observed` or `budget_unknown`.

The transitions are a pure function of the record and one signal, and the state is a
pure function of the record, the plugin's declaration, the account's budget and the
time:

- A limit with a reported reset time limits the account until that time. After it
  passes, the account is `available` again on the provider's word.
- A limit without a reset time backs off for 5 minutes, doubling with each limit
  since the last confirmed recovery, up to 4 hours. When a backoff ends, the account
  is `degraded` until a completed turn (`usage`) or a `recovered` signal confirms it.
  A limit hit by that probe continues the sequence with the next longer backoff.
- Further limit reports while a limit is in force belong to the same episode and do
  not lengthen it; a reset time learned during the episode replaces the guess.
- A turn that completes during a guessed backoff contradicts it: the account becomes
  `degraded` and the next completed turn makes it `available`. A turn that completes
  before a reported reset time does not outrank the provider.
- `recovered` ends a limit at once and resets the backoff sequence.
- A plugin that reports no limit errors stays `unknown` whatever else is observed. A
  limit that was nevertheless reported for it is still shown as `limited`.

Applying a signal is idempotent and order-safe. The identical signal answers
`duplicate`. A limit or recovery older than the newest state change, and a limit older
than a later completed turn, answer `stale`. The record keeps its last 16 signals;
a signal older than a full window is also `stale`, because it cannot be told from a
repeat. None of these changes anything or writes the file.

### Accounts and budgets

Records are keyed by tool and account, and a signal changes only its own record, so
one account being limited never limits another. An unattributed limit on a tool with
configured accounts makes each account that is not itself limited `degraded` with
`unattributed_limit` until that limit ends: the limit is real, but which account it
belongs to is not known.

`budget_cents_per_day` is compared with the cost observed during the current UTC day.
At or over the budget, the account is `limited` with `budget_exceeded` until the next
UTC day. When the plugin does not report costs, or any turn that day arrived without a
cost, the day's total is unknown and the account is `unknown` with `budget_unknown`;
it is never called available on the strength of a budget that cannot be checked.
Observed cost that already reaches the budget limits the account even when more is
unknown.

### Selection

Selection answers, for one target and an optional preferred tool and model, the
ordered tool accounts a session may start on, and every excluded one with its reason.
It is pure: its inputs are the policy, the capacity records, the plugin declarations,
the caller's observed session counts per tool account, and the time.

- A target the policy does not list, a disabled target, or one without
  `sessions_manage` answers `denied` with the policy's own denial code.
- A tool account is excluded as `limited`, `tool_cap`, `account_cap`,
  `model_not_permitted`, `tool_not_configured` or, for a requested tool the target does
  not list, `tool_not_permitted`.
- The remaining candidates are ordered by state (`available`, then `degraded`, then
  `unknown`), then the requested tool, then the policy weight, then free sessions.
  Each carries `verified`, which is true only for `available`.

So a limited tool fails over to the next allowed tool or account, and a tool that
cannot be verified is offered last and marked. The session counts are required: an
omitted list is refused, and an empty list states that nothing is running. Target and
machine caps stay with the policy gate (§26.8) and machine admission (HS2-J8PRTV).

### Storage

`${HOTSHEET_HOME}/orchestration-tool-capacity.json` is a private, locked,
atomically replaced file at version 1, holding at most 256 records. It belongs to the
machine, not to one project, because a tool account's limit is the same for every
project that uses it. A read never creates it. Corrupt or newer-version data is an
error on every route and is never overwritten. At capacity, the longest-quiet record
with no limit, no unconfirmed recovery and no spend for the current day is forgotten;
when every record still holds something in force, a new tool account is refused with
`capacity_reached`.
```

## 26.12 Extra validation steps (HS2-WN0MXZ)

An orchestration ticket can carry named **validation steps**: checks that run in a
target project's own environment and leave evidence the orchestration project can
rely on later. A step and its results belong to the orchestrating machine. The
target runs the step and reports what it observed; it stores nothing about it.

### Step

A step is attached to one orchestration ticket and is named by a `step_id` that is
unique within that ticket.

| Field             | Meaning                                                                                                         |
| ----------------- | --------------------------------------------------------------------------------------------------------------- |
| `step_id`         | The step's name.                                                                                                |
| `target_id`       | The declared target project the step runs in.                                                                   |
| `check`           | `{kind: "profile", name}` or `{kind: "command", program, args}`.                                                |
| `prerequisites`   | At most eight other steps of the same ticket whose latest evidence must be `passed` first.                      |
| `evidence`        | What passing means: `exit_code` (0 when omitted) and optionally `stdout_sha256`, the digest of standard output. |
| `timeout_seconds` | 1 to 3600; 300 when omitted.                                                                                    |
| `retry`           | `max_attempts`, 1 to 5; 1 when omitted. Only a `failed` attempt is retried.                                     |

- A **profile** is a test profile the target project defines itself, in its checked-in
  `.hotsheet2/validation-profiles.json`: `schema_version: 1` and `profiles`, each a
  `name`, a `program` and `args`. The orchestration project names the profile and
  never supplies its command. The file is read where the target lives, at most 64
  profiles; a damaged, duplicated or newer file is an error and never a guess.
- A **command** is a program and arguments chosen by the orchestration project. It
  is started directly, without a shell. A program and its arguments are at most 4,096
  bytes and 32 arguments together.
- A program with a directory part (`scripts/check.sh`) is the checkout's own file; a
  bare name is looked up on the host's `PATH`. Both run with the target checkout as
  the working directory.

Defining a step again with the same definition changes nothing. A changed definition
gets the next `revision`, and evidence produced for an earlier revision no longer
counts. A ticket holds at most 32 steps and a machine 512. A step keeps its 16 most
recent results.

### Result states

A result is one attempt at a step.

| State         | Meaning                                                                  | May become                                      |
| ------------- | ------------------------------------------------------------------------ | ----------------------------------------------- |
| `running`     | The attempt was started and its outcome is not known yet                 | `passed`, `failed`, `unavailable`, `unverified` |
| `passed`      | It ran and produced the expected evidence                                | nothing                                         |
| `failed`      | It ran and did not: wrong exit code or output, timeout, signal, too loud | nothing                                         |
| `skipped`     | It was not run because a prerequisite has not passed                     | nothing                                         |
| `unavailable` | The target could not run it at all                                       | nothing                                         |
| `unverified`  | It may have run, but its outcome could not be confirmed                  | nothing                                         |

A finished result is immutable. Reporting the same outcome again is a no-op and any
other change is refused with `validation_result_final`; an edge the table does not
list is `validation_invalid_transition`. A result begins `running`, or directly
`skipped` or `unavailable` when nothing was started.

Each state other than `passed` and `running` carries a reason code:

- `failed`: `exit_code_mismatch`, `output_mismatch`, `timed_out`,
  `terminated_by_signal`, `output_limit_exceeded`.
- `skipped`: `prerequisite_not_passed`.
- `unavailable`: `profile_not_defined`, `program_not_found`, `launch_failed`, or the
  target's own refusal code, such as `target_provider_unauthorized` for a read-only
  pairing, `target_peer_unsupported` for a peer without the route, or
  `capacity_reached`.
- `unverified`: `execution_lost` (the host accepted the attempt and no longer knows
  it), `deadline_passed` (a running attempt outlived its timeout by more than a
  minute), `exit_code_unknown`, `output_unknown`.

### What a result records

A result holds its `result_id`, the caller's `operation_id`, the attempt number, the
step revision, the state and reason, the actor, an optional `run_id`, and the
outcome the host observed: how the execution ended, the exit code, when it started,
how long it took, and for each of standard output and standard error the SHA-256 and
the size in bytes.

The host reads a step's output only to digest it and keeps none of it. Output is
never returned by a route, written to the result file, a journal, an event or a
ticket note. A step that writes more than 64 MiB to one stream is stopped and
`failed`. The note added to the orchestration ticket when an attempt finishes names
the step, the attempt, the state, the result id and the target, and nothing else.
Keeping a bounded copy of the output on the target's own machine is HS2-ZCR3Y7.

### Evidence

`evidence` answers "what is the latest validation evidence for this orchestration
ticket and step". It returns the step, the latest result exactly as recorded, the
retained attempts, and the `state` that evidence amounts to now. That state is the
latest result's state, except that it is `unverified` when:

- the step never ran (`not_run`);
- the step's definition changed since the result (`step_changed`);
- a `running` attempt is past its deadline (`deadline_passed`).

The read contacts no target, passes no gate and writes nothing. It is what a
readiness decision (HS2-42TARA) consumes: `OrchestrationRuntime::validation_evidence`
in the shared runtime, over `ValidationRegistry::evidence` in the core.

### Running a step

The step runs on the machine that hosts the target, in the target checkout, reached
like every hosted half: in process for a local destination and over the reviewed
remote registration for a remote one. On Unix the step gets its own process group
and the whole group is stopped at the timeout. Windows process handling is
HS2-2R32SD, and the environment a step inherits is HS2-W02PTA.

The remote transport allows a request about ten seconds and a step up to an hour,
so an execution outlives one request. A `run` request starts the attempt and waits
at most `wait_ms` (8,000) for it:

- While the answer is `running`, the same request again waits on the same execution.
  Each wait is one request held open on the host until the step ends or the wait is
  over. Nothing polls on a timer between requests.
- When the answer is `failed` and `attempts_left` is not zero, the same request again
  takes the next attempt. Each attempt is its own immutable result.
- A finished request is answered from the record, without the gate or the target.
  The same `operation_id` never runs a finished step again; a new id does.
- The same `operation_id` for another step conflicts. Another request for a step
  whose attempt is still running is refused with `validation_step_running`.
- If the target cannot be reached, or refuses a read of an execution it already
  accepted, the attempt stays `running` and the answer lists `target_execution` under
  `unverified`. Nothing is concluded from silence. If the target refuses to start the
  step at all, the result is `unavailable` with the target's code.
- The host identifies an execution by the result id and never runs one id twice. It
  remembers executions in memory only: at most 64, four of them running, and a fifth
  is refused with `capacity_reached`. After the hosting process restarts it answers
  `unknown` and the attempt becomes `unverified`, never a second run (HS2-7BTV98). A
  headless CLI that hosts a local target itself must therefore stay until the step
  ends, which is why `validation run` waits by default.
- A step whose prerequisite has not passed is recorded as `skipped` without
  contacting the target and without asking anyone. A later request with a new
  `operation_id` runs it once the prerequisite passes.

There is no cancel yet (HS2-17ABM2). The scheduler that runs steps without a caller
is HS2-ECWG50.

### Authority

Validation has its own delegated action. It uses `validation_run` (§26.7), not the
`work_start` action that dispatch uses, so a grant to dispatch work does not allow
running checks and the reverse.

- **Define** needs a `validation_run` grant on the step's target. It runs nothing
  and asks nobody.
- **Run** passes the gate as `validation_run` with the `operation_id`. A policy that
  lists `validation_run` under `approval.required_for` (§26.8) asks a person for every
  run.
- **A profile is inside the granted automation scope**: the target project chose its
  command. **A command is outside it.** A `command` step always answers
  `approval_required` until a person approves that exact operation through the
  approvals of §26.8, whether or not a policy file exists and whatever it lists.
  Continuing an approved request, including its retries, does not ask again. Scoping
  grants to named profiles or reviewed commands is HS2-27ZYX5.
- **Remove** is a person's without any grant, so a step of an unbound or unreachable
  target can always be cleaned up; an AI actor needs the grant.
- An unusable target is a denial (`target_unavailable`) and records no result.
- The target's server still enforces its own device role: a read-only pairing cannot
  start an execution, whatever a grant says. The hosted route gives a read-write
  pairing nothing it did not have: such a caller can already open a terminal on that
  server. Grants and approvals govern the orchestrating side's automation; they are
  not a boundary on the target, as §26.7 says of grants in general.

Every gate answer and every finished attempt is in the audit journal under
`validation_run`, the latter as `succeeded` for `passed` and `failed` with the state
code otherwise. A changed definition is `validation_define` and a removal
`validation_remove`. Each bounded wait passes the gate again and is recorded; folding
those is HS2-5ZT4GY. When a run request names a dispatch `run_id`, that run's event
stream (§26.9) carries a `validation` event when the attempt starts and when it
finishes, with the state as its reason and the result id in its key.

### Storage

`${HOTSHEET_HOME}/orchestration-validation.json` on the orchestrating machine holds
step definitions and results (private, locked, atomically replaced, version 1, at
most 8 MiB). A read never creates it. Damaged or newer data is an error on every
route and is never overwritten.

### Headless surface

```sh
hotsheet-cli ... orchestration validation define --project <owner> --ticket <orchestration-ticket> \
  --step <name> --target <target-id> --profile <profile> [--requires <step>]... \
  [--expect-exit <code>] [--expect-stdout-sha256 <hex>] [--timeout-seconds 300] [--max-attempts 1]
hotsheet-cli ... orchestration validation define ... --program <program> [--arg <argument>]...
hotsheet-cli ... orchestration validation run --project <owner> --ticket <orchestration-ticket> \
  --step <name> --operation-id <retained-id> [--run <run-id>] [--no-wait]
hotsheet-cli ... orchestration validation show|remove --project <owner> --ticket <orchestration-ticket> --step <name>
hotsheet-cli ... orchestration validation list --project <owner> [--ticket <orchestration-ticket>]
```

`validation run` repeats its one request until the attempt is finished and the retry
policy has nothing left, and returns at once when the target could not be confirmed.
The result is in the JSON (`result.state`), not in the exit status. `--source` names
the orchestration ticket's connection and may be omitted when the project has one git
ticket source.

| Route                                                                            | Methods and purpose                                                                          |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `/checkouts/{owner}/orchestration/validations/{define,list,evidence,run,remove}` | POST orchestrating-side operations; machine-local                                            |
| `/checkouts/{checkout}/target-validations`                                       | Hosted half: POST start or re-attach by `execution_id`; GET its state, both with a `wait_ms` |

The hosted route is ordinary authenticated project data under the caller's device
role, like the hosted ticket route. MCP `hotsheet_targets` adds `validation_define`,
`validation_list`, `validation_evidence`, `validation_run` and `validation_remove`.
Listing and checking a target's profiles headlessly is HS2-YYNW9F, and the workspace
view is HS2-9XJEZC.

## 26.13 CI and deployment gates (HS2-5JBYPV)

A **gate** ties one run to one commit of its target and records whether a provider
confirmed that commit. A CI gate says whether the integrated commit passed CI. A
deployment gate says whether that commit was deployed to an environment. Later
decisions read gates: promotion and readiness (HS2-42TARA), and provisional work on
a prerequisite whose gate is still pending (HS2-2SJPCH, HS2-JSF985).

### Evidence comes from adapters only

A gate changes state on exactly two things: a typed reading from a provider adapter,
or a person's answer to a deployment approval. Nothing a session writes in a note, a
ticket or a message is evidence, and no text is parsed for a verdict. A reading holds
a state, a reason code and, for a verdict, evidence: the provider, its own run or
execution id, the attempt, the commit it built or deployed, how that commit relates
to the gate's commit, and optionally an artifact name with its digest and an exit
code. Every field is an identifier, a count or a code.

A `GateProvider` adapter serves one provider name and one kind. It starts a watch or
an execution idempotently, answers the current reading from records it keeps itself,
and offers a cursor with a blocking wait that is woken by the change. One adapter
ships: `github-actions`, the watch Job of [33](33-jobs.md) (HS2-3R9V0R). The Job
monitor observes GitHub; this machine reads the Job store and waits on its file
changes. Other CI providers are HS2-TPAHYA.

### Coverage

A verdict counts only when its evidence covers the gate's commit:

- `exact`: the provider built or deployed that very commit.
- `ancestor`: the provider built a descendant on the shipping branch that contains
  it, as the watch source attests. Only a CI gate accepts this. A deployment counts
  only when it deployed exactly the gated commit.

A verdict with no evidence, evidence from another provider, or evidence about another
commit is recorded as `unknown` with `evidence_missing` or `evidence_not_covering`.
It is never recorded as passed. A commit id is always the full 40 hex characters; a
short or symbolic name is refused.

### States

| State               | Meaning                                                | May become                                                          |
| ------------------- | ------------------------------------------------------ | ------------------------------------------------------------------- |
| `unknown`           | Nothing trustworthy is known; the reason code says why | any other state                                                     |
| `awaiting_approval` | A deployment waits for a person                        | `pending`, `unknown`, `failed`, `superseded`                        |
| `pending`           | Watched or started, no verdict yet                     | `passed`, `failed`, `unknown`, `superseded`                         |
| `failed`            | The newest covering run or execution failed            | `pending` (watched again), `passed` (a later attempt), `superseded` |
| `passed`            | A covering run or execution succeeded                  | `superseded`                                                        |
| `superseded`        | The link no longer stands for the run                  | nothing                                                             |

Reason codes for `unknown` include `not_watched`, `watch_lost`, `watch_cancelled`,
`gate_provider_unavailable`, `evidence_missing`, `evidence_not_covering` and
`deployment_executor_unavailable`.

Readings can arrive late, twice or out of order. A reading whose evidence ranks
below the recorded evidence (an older run, or an earlier attempt of the same run) is
ignored. A verdict is never replaced by `unknown` or, once passed, by anything but
supersession. A repeated reading writes nothing and produces no event. A later
attempt with the same verdict is recorded as news. A provider cannot move a gate
into or out of `awaiting_approval`. History keeps the first entry and the most
recent 31.

### Supersession

Gates of one run with the same kind, provider and CI scope (repository, workflow,
branch), or the same deployment environment, share a slot. Linking another commit in
a slot marks the older gate `superseded` with `newer_gate_linked` and the id of its
replacement, and keeps its evidence and history. A deployment that has not started
(`awaiting_approval` or `unknown`) and rests on a superseded CI gate is withdrawn
with `required_gate_superseded`. A provider also reports `superseded` with
`branch_moved` when the shipping branch no longer contains the commit.

The rolled-up state of a run or a commit counts standing gates only. Its `ci` value
is `failed` if any standing CI gate failed, else `unknown` if any is unknown, else
`pending`, and `passed` only when every one passed. With no standing CI gate it is
`unknown` with `no_gate`. `deployment` is rolled up the same way and is absent when
none was requested.

### Linking and following

`gate link` passes the gate as `gates_observe`, records the link in
`${HOTSHEET_HOME}/orchestration-gates.json` (mode 0600, version 1, at most 2,048; the
oldest superseded gate is forgotten first, then the oldest settled one, and a gate
that still waits never is) and starts the watch. Repeating it returns the same gate
and, after a failed or stopped watch, watches again. The repository defaults to the
GitHub repository the target declares; a different repository for such a target is
refused with `gate_repository_mismatch`. If the adapter cannot start, the link stands
as `unknown` and the same request starts it later.

Every change is published to the run event stream (§26.9) as a `gate` event with the
gate's id and new state, once however many readers saw it. A server follows the
gates of an owning checkout that still wait on a provider: one follower holds a
bounded wait open on the adapter's records and ends when no gate waits. It starts
with a link, a deployment request, or a subscriber of the event stream, acts under
the linking actor's `gates_observe` grant, and confirms that every ten minutes. A
headless caller uses `gate sync --wait-seconds`, which waits the same way. Nothing
polls on a timer. Reads answer from this machine's records and contact nothing.

The watch runs on the orchestrating machine with that machine's GitHub credentials;
watching on the target's own server is HS2-WDQZ37. A watch only advances while the
Job monitor runs: `gate link` in the CLI starts it, and a link through the server
reports `watching: false` until one runs (HS2-VVHVF0).

`authorize` reads the target again, up to three times, when its resolution lost a
race to a concurrent one, so a follower confirming its authority does not turn a
simultaneous request into `target_unavailable`.

### Deployments

`gate deploy` asks for a deployment of the commit the run's CI gate passed. The
request names the environment, the target's checked-in script as a path relative to
its repository root, and an operation id.

- **Precondition.** Every standing CI gate of the run passed, for one commit.
  Otherwise the request is refused with `gate_not_passed` and nobody is asked.
- **Authority.** It passes the gate as the grant action `deployments_run` under its
  operation id.
- **A person for production.** A request marked `production` always waits for a
  person, with or without a policy file. Any other request waits when the policy
  lists `deployment` under `approval.required_for`. While waiting, the gate is
  `awaiting_approval` and the answer carries the approval id (§26.8). Repeating the
  request after the answer moves it on; a denial ends it as `failed` with
  `approval_denied`. An approval is used once.
- **Idempotent.** The same operation id returns the same gate, asks nobody again
  and starts nothing twice. The same id for other content conflicts. An environment
  with a deployment in progress refuses a second with `gate_deployment_active`.
- **Result.** Only a deployment adapter's typed evidence settles the gate.

No deployment adapter ships. This repository has no hosted deployment
(`AGENTS.md`, HS2-51HMCC), so an authorized deployment is recorded as `unknown` with
`deployment_executor_unavailable`: approved, and not run. Running the target's script
on the target's server with target-scoped credentials, after verifying that it is
tracked at the gated commit, is HS2-G3VAK4. Artifact identity from CI is HS2-3YC577.

### Headless surface

```sh
hotsheet-cli ... orchestration gate link <run-id> --project <owner> --commit <40-hex> [--repo owner/name] [--workflow CI] [--branch main]
hotsheet-cli ... orchestration gate list --project <owner> [--run <run-id>] [--target <target-id> --commit <40-hex>]
hotsheet-cli ... orchestration gate show <gate-id> --project <owner>
hotsheet-cli ... orchestration gate sync <run-id> --project <owner> [--wait-seconds 20]
hotsheet-cli ... orchestration gate deploy <run-id> --project <owner> --environment <name> --script scripts/deploy.sh --operation-id <retained-id> [--production]
```

| Route                                                     | Methods and purpose                                                        |
| --------------------------------------------------------- | -------------------------------------------------------------------------- |
| `/checkouts/{owner}/orchestration/runs/{run}/gates`       | POST link the CI gate of a commit; machine-local                           |
| `/checkouts/{owner}/orchestration/runs/{run}/gate-sync`   | POST read the adapters' records, with an optional `wait_ms`; machine-local |
| `/checkouts/{owner}/orchestration/runs/{run}/deployments` | POST request a deployment; machine-local                                   |
| `/checkouts/{owner}/orchestration/gates`                  | GET the standing gates by `run`, or `target_id` and `commit`, rolled up    |
| `/checkouts/{owner}/orchestration/gates/{gate}`           | GET one gate with evidence and history                                     |

MCP `hotsheet_targets` adds `gate_link`, `gate_list`, `gate_show`, `gate_sync` and
`deployment_request`. Approving a deployment is a person's CLI or HTTP action.

The runtime reads other tickets call are
`OrchestrationRuntime::run_gate_state(owner_id, run_id)` and
`commit_gate_state(owner_id, target_id, commit)`. Both return the rolled-up
`GateSummary` from this machine's records.

## 26.14 Recovery and reconciliation (HS2-WBREDM)

A run outlives the process that dispatched it. When that process stops, the target
keeps moving: a claim is released, a dispatcher assigns the ticket, the ticket is
finished. **Reconciliation** brings each unfinished run back in line with the world
before anything is resumed, and says exactly what it could not check. It reads; it
never starts work.

### Sources and findings

Reconciliation is a pure function over typed observations
(`crates/hotsheet-ticketing/src/orchestration/reconcile.rs`). Each external
**source** is observed as verified facts or as unverifiable with a reason code, and
yields one **finding**: `verified`, `mismatched` or `unverifiable`.

| Source                | What is compared                                            | Reported today                                  |
| --------------------- | ----------------------------------------------------------- | ----------------------------------------------- |
| `target_reachability` | Whether the target answered at all                          | Yes                                             |
| `target_ticket`       | The target ticket's existence and status                    | Yes                                             |
| `target_claim`        | Who holds a live claim, against the run's state and worker  | Yes                                             |
| `run_lease`           | The lease the run recorded, against the lease on the ticket | Yes                                             |
| `ci_run`              | CI runs gating the run                                      | `unverifiable`, `source_not_wired` (HS2-7QNK7Y) |
| `deployment`          | Deployments started for the run                             | `unverifiable`, `source_not_wired` (HS2-BYEB60) |
| `session`             | Orchestrator-owned sessions working the run                 | `unverifiable`, `source_not_wired` (HS2-75Q254) |

When the target cannot be confirmed, all four target sources are `unverifiable` with
the reason the read failed for, such as `target_unavailable`. The last three sources
take the same typed input (what the run recorded it started, and what is live) and
already classify it: the same work is `verified`, live work the run never recorded is
`unrecorded_live_work`, recorded work that is gone is `recorded_work_missing`. No
ticket reports them yet, so they are passed as `source_not_wired`. That reason does
not hold a run back, because the run records no such work; any other unverifiable
reason does.

**Never a duplicate.** New work of a kind may be started for a run only when that
source was read and shows none going on (`Assessment::may_start`). A source that
could not be verified never allows a start, and a live claim or live session is
adopted rather than replaced. Repeating a dispatch with its operation id returns the
same run (§26.9), and a target ticket is created at most once per operation id
(§26.3).

### Alignment and disposition

The rules by which a run follows its target ticket live in the same module
(`align`), and `run sync` applies them. They are the rules of §26.9, plus two an
interruption needs:

- A `requested` dispatcher run whose ticket already shows a live claim becomes
  `dispatched` for that worker: the Up Next mark was set and the answer lost, and the
  ticket is not queued twice.
- A ticket finished before its dispatch was recorded completes the run
  (`requested` to `completed`), and a taken-over run whose ticket disappeared fails
  (`intervened` to `failed`). Without these edges such a run could never settle.

After alignment the run gets a **disposition**:

| Disposition           | Meaning                                                                              |
| --------------------- | ------------------------------------------------------------------------------------ |
| `continue`            | The run's worker holds the target ticket                                             |
| `wait_for_dispatcher` | Queued; the target's dispatcher has not assigned it                                  |
| `retry_dispatch`      | Nothing is held in the target; the same dispatch may be repeated                     |
| `retry_cancel`        | A cancel never reached the target; reconciliation repeats it when the target answers |
| `needs_attention`     | Record and world disagree in a way only a person can settle                          |
| `hold_unverified`     | Something the run depends on could not be verified; nothing is resumed               |
| `paused`              | A person paused it; it outranks everything else                                      |
| `finished`            | The run is over                                                                      |

The outcome (disposition and findings) is stored on the run as `reconciliation` and
shown by `run status`. The answer's `unverified` lists exactly the sources that are
`unverifiable`. An unchanged outcome is not rewritten and produces no event; a new
one is a `reconciled` event whose reason is the disposition. A finished run keeps no
outcome. A target that cannot be confirmed is part of the answer, not an error.

### When it runs

- **Server start.** The server starts a follower (§26.9) for every target with an
  unfinished run. With none, nothing starts and no target is contacted.
- **Connect and reconnect.** A follower's first successful step reconciles every run
  of its target, and so does the first step after any failure, a feed overflow or a
  cursor reset. In between, only runs whose ticket changed are synced.
- **On request**, for one run or all of an owner's. The bare CLI has no follower, so
  this is its path.

Nothing is driven by a timer: the triggers are the server starting, the follower's
own connection events and explicit requests.

A follower and a request can resolve the same target at the same moment, and every
resolution rewrites the binding's observation, so the later writer conflicts. The
gate resolves again (up to eight times) instead of reporting the target as
unavailable: contention on this machine says nothing about the target.

### Control actions

Each action is its own entry in the run's history (`control` names it) and its own
run event (`paused`, `resumed`, `handoff`, `corrected`), however often it is repeated
or reversed. An action that changes nothing writes nothing.

- **Pause** stops the orchestrator acting on the target for that run: a renewal, a
  repeated dispatch and a handoff are refused with `run_paused`, and reconciliation
  does not repeat a pending cancel. The target is not told and its claim is left
  alone. The run is still read, so its record stays true.
- **Resume** ends the pause and reconciles at once.
- **Handoff** moves the run to another worker. Naming a worker that already holds
  the ticket adopts that claim. Otherwise a direct run releases its own claim and
  claims the ticket for the new worker. A claim someone else holds is never forced
  away (`target_conflict`), a worker that already holds a direct run on the target
  takes no second one, and a dispatcher run can only adopt. Without a worker the run
  lets go of its claim and becomes `intervened`.
- **Correct** rewrites the record by hand: any unfinished run may be moved to any
  other state, backwards included, with a reason code. A finished run stays finished,
  because reopening it could put two active runs on one target ticket. Only this
  machine's record changes, and the next successful read of the target still wins.

Reconciliation and handoff pass the gate (§26.7) as `tickets_read` and a continuing
`work_start`. Pause, resume and correct change only this machine's record and must
work while the target is unreachable, so they do not need a fresh target resolution:
they require an identified actor, are written to the audit journal as `run_pause`,
`run_resume` and `run_correct`, and a correction is refused for an AI actor. Whether
they should also need a grant is HS2-BJE1C1.

### Headless surface

```sh
hotsheet-cli ... orchestration run reconcile [<run-id>] --project <owner>
hotsheet-cli ... orchestration run pause <run-id> --project <owner> [--reason <code>]
hotsheet-cli ... orchestration run resume <run-id> --project <owner>
hotsheet-cli ... orchestration run handoff <run-id> --project <owner> [--to-worker <id>] \
  [--worker-label ...] [--lease-minutes 30]
hotsheet-cli --actor-role human ... orchestration run correct <run-id> --project <owner> \
  --state <state> --reason <code> [--worker <id>]
```

| Route                                                                                  | Methods and purpose                                |
| -------------------------------------------------------------------------------------- | -------------------------------------------------- |
| `/checkouts/{owner}/orchestration/run-reconcile`                                       | POST reconcile every unfinished run; machine-local |
| `/checkouts/{owner}/orchestration/runs/{run}/{reconcile,pause,resume,handoff,correct}` | POST reconcile or control one run; machine-local   |

MCP `hotsheet_targets` adds `run_reconcile` (all runs without `run_id`), `run_pause`,
`run_resume`, `run_handoff` and `run_correct`. Acting on a disposition is the
scheduler's (HS2-AH8CRS), validation steps join reconciliation in HS2-G5H9FQ, and
the workspace view is HS2-XM692S.
