# 26. Orchestration Projects

> **Status: Partial (HS2-4J6V46, HS2-XHQ1EH, HS2-YCJDEB).** Shared declarations,
> reviewed machine bindings, permission requests answered through the permission
> bridge with remembered rules and the audit journal (§26.7, HS2-XHQ1EH; the client
> card and Settings rule row, §26.28; requests filed or answered headlessly reach
> clients live, HS2-E16FWJ; goal-ticket decisions listed with the answer that decided
> them, HS2-0RAZ1J; retired grant and approval files removed by a person, HS2-JWV5S1),
> the throughput policy of limits (§26.8),
> structured ticket links with target ticket operations (§26.3, HS2-AZ15GA) and a
> gated, bounded list and search of a target's open tickets (§26.3, HS2-D85G19),
> dispatch runs with status and events (§26.9, HS2-EBHDSB, HS2-7QHWHJ), target
> capacity and admission (§26.10, HS2-J8PRTV; a capacity version subscribers wait on
> while they are connected, HS2-P5GTSW, which wakes the scheduling loop when a machine
> that had no room gains it, HS2-AWB7BX), AI tool capacity states with tool
> selection (§26.11, HS2-AHG7HW), extra validation steps with immutable result
> references (§26.12, HS2-WN0MXZ), CI and deployment gates of runs (§26.13,
> HS2-5JBYPV; deployments are recorded and allowed, not yet executed; the Job
> monitor is started by whoever links or follows a gate, HS2-VVHVF0),
> recovery with reconciliation, pause, handoff and correction (§26.14, HS2-WBREDM;
> reconciliation reads the CI runs gating a run, HS2-7QNK7Y, its deployments,
> HS2-BYEB60, its validation attempts, HS2-G5H9FQ, and its sessions, HS2-75Q254),
> the plan of prerequisites with computed readiness and the dispatch guard (§26.15,
> HS2-42TARA; the single-project provisional rule shipped as HS2-2SJPCH, §26.18, and
> informs an uncoordinated claim, HS2-XYF4YT;
> a level a validation deadline changes is published at that deadline, HS2-J7N3PQ;
> step events name their step in `step_id`, HS2-J7RG42; prerequisite tickets without a
> run are followed on their target's change feed, HS2-TMA4YA; a session's host reports
> since when it has been idle, HS2-RG91SK; a placed session is told the resources its
> work needs and acquires them itself, HS2-7YSKXE),
> provisional work across targets under a policy opt-in, with its ledger, gate-result
> handling and recovery (§26.22, HS2-JSF985; off by default),
> orchestrator-owned sessions with safe-point wind-down and takeover (§26.16,
> HS2-GDJXGZ) that start with the target checkout set up for their tool (HS2-WEFJV8)
> and take up their role with a readiness-aware prompt confirmed by the pool (HS2-2DPGDY),
> the throughput scheduler with its plan, explanations, decision log and event-driven
> loop (§26.17, HS2-ECWG50), which places orchestrator-owned sessions through its
> session executor (§26.19, HS2-95SWQ4; nothing is placed until a policy sets
> `placement.automatic`; a session on another server wakes it through the target
> feed, HS2-257Q2Z) and dispatches the run of each step it places (HS2-YC2P7K),
> resource requirements of plan steps and targets with scheduling around held
> resource claims (§26.20, HS2-2FG91X; the session claims, the scheduler only reads;
> a release on another machine wakes the loop through that machine's wait, HS2-G1PKK2),
> the read-only progress and cost summary of runs across targets (§26.21, HS2-S13719;
> cost is read for targets on this machine only),
> one live session per run, adopted rather than doubled (§26.24, HS2-DG0YPA),
> a placed run no target dispatcher will assign reported and, after a bounded wait,
> claimed for its session (§26.24, HS2-T16SK0), a placed session that never joins
> its role wound down and replaced a bounded number of times (§26.24, HS2-NPTKS8),
> and a refilled session told its exact ticket through its host's session push
> (§26.24, HS2-R7QH35), and a withdrawn unconfirmed start fenced on its host so a
> late start launches nothing (§26.24, HS2-WHR4GS),
> Propose Plan, which starts the orchestration project's planning AI session to
> propose the dry-run plan from its goal tickets (§26.25, HS2-HCW7MD),
> the orchestration line on a target project's own tickets (§26.29, HS2-S3FKJ7),
> the Orchestration view with its dependency-wave timeline, the orchestration line on
> ticket rows, the card inspector and kick-off (§26.26, HS2-J89JC7),
> and the headless CLI,
> authenticated HTTP and MCP adapters are implemented and covered by automated tests,
> with end-to-end failure scenarios across two servers (§26.23, HS2-2EY9DP);
> the OpenAPI contract describes them. Project Type and the Orchestration settings
> category with its projects, project detail and limits-only policy ship in the web client
> (§26.27, HS2-W27N1D); a Software project places and dispatches nothing (HS2-J3ZF27), and Start Orchestration
> needs no placement actor (HS2-XYAHB4); per-project limits and tool accounts are edited in
> Settings (HS2-7WF6JD), and changes a CLI process writes are announced live (HS2-SHHGC6).

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
give access. Rebinding to a different server or checkout requires an explicit
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
projects under different ids and remembered rules. A target checkout can still have its own
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
- **List** (HS2-D85G19) finds a target's open tickets, so a planner reuses an existing
  ticket instead of creating a second one. It names one source of the target and
  optionally `text` (at most 200 bytes, matched as that source's own provider matches
  text), a `category` and one `tag`. The answer is one bounded page, `limit` 1 to 100
  rows (50 by default), and the provider's opaque `next_cursor` while more rows
  remain; a cursor the provider cannot read is `target_request_invalid`. Rows carry
  identifiers and labels only (native id, slug, title, status, priority, category,
  tags, Up Next, updated time, and the create operation and origin when another
  project created the ticket), never details or notes; read one in full with
  **Read**. Closed tickets (completed, verified, deleted, archived, moved) are never
  listed. Listing is a read (`tickets_read`): it asks nobody and is audited like the
  other operations. An older peer without the hosted route answers
  `target_peer_unsupported`.

The orchestrating side reaches a hosted half that performs the provider operation
where the target lives: in process for a local destination, and over the reviewed
remote registration (pinned origin, mTLS, no redirects, bounded reply) for a remote
one. The target server still enforces its own device role, so a read-only pairing
refuses writes whatever a remembered rule says. Target refusals are content-free codes:
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
hotsheet-cli ... orchestration ticket list --project <owner> --target <target-id> --connection <source> \
  [--text "..."] [--category <category>] [--tag <tag>] [--cursor <next-cursor>] [--limit 50]
```

`--source` names the orchestration ticket's connection and may be omitted when the
project has one git ticket source. An identified actor is required.

| Route                                                                                  | Methods and purpose                                                                              |
| -------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| `/checkouts/{owner}/orchestration/tickets/{create,read,update,link,unlink,links,list}` | POST orchestrating-side operations; machine-local                                                |
| `/checkouts/{checkout}/target-tickets`                                                 | Hosted half: GET by `connection_id` and `native_id` or `operation_id`; POST create; PATCH update |
| `/checkouts/{checkout}/target-tickets/list`                                            | Hosted half: GET one page of a source's open tickets (HS2-D85G19)                                |

The hosted routes are ordinary authenticated project data under the caller's device
role, because a remote orchestration project must reach them. MCP `hotsheet_targets`
adds `ticket_create`, `ticket_read`, `ticket_update`, `ticket_link`,
`ticket_unlink`, `ticket_links` and `ticket_list` (with `text`, `category`, `tag`,
`cursor` and `limit`). The runtime function is
`OrchestrationRuntime::target_ticket_list(owner, ListTargetTickets, actor, tx)` in
`crates/hotsheet-cli/src/orchestration/ticket_list.rs`. The workspace flows for the same operations are
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
binding. It writes no manifest, binding, reservation or permission. Copy only the public
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
| `/checkouts/{owner}/orchestration/inspect`                  | POST fresh candidate metadata, no binding write                             |
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
Declaring and binding a target consents to reading and following it; acting on it
needs a person's permission or a remembered rule, with the audit journal, in §26.7
(HS2-XHQ1EH).

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

**Fields a newer peer adds (HS2-T2KS8G).** Machines of different versions talk to
each other, so new fields are additive and the reading side decides what an unknown
field means:

- An answer read from a peer accepts fields it does not know and ignores them, so a
  newer host's additive field never breaks an older orchestrator's read. This covers
  the change-feed page and its events, the hosted speculation setting, the hosted
  validation, gate and session answers, and the profile check.
- A request a host reads refuses them, because ignoring a field could change what
  the request means. A newer request against an older host is reported as
  unsupported.
- A machine-local or committed file refuses them, and a newer version is an error that
  is never overwritten. So does a type that is also part of a request or such a file
  (machine limits, resource needs, session origins, validation profiles), because an
  older binary that silently ignored a new limit would loosen what it enforces.

The policy test `hs2_t2ks8g_answers_accept_and_requests_and_files_refuse_unknown_fields`
pins which types are which.

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

## 26.7 Permission requests and audit (HS2-XHQ1EH)

> Replaces the delegated grants of HS2-SBQ3T8 and the approvals of HS2-YCJDEB.
> Decided with the maintainer on 2026-11-12 ("Yes, as mocked"); it folds in
> HS2-8NJVXC, HS2-RTX8RJ, HS2-F2DG0J, HS2-BJE1C1, HS2-TZT505 and HS2-48H57C.

A binding selects a destination. Adding a project (declaring the target and binding
it to a checkout) is the consent to read and follow it, so observation never asks.
Acting on it is decided per action, the same way an AI tool's permission prompt is
decided: a person answers a **permission request**, or a remembered rule answers it.
Every target action passes one gate, `authorize`, which resolves the target freshly
first. Anything not provably allowed is refused, and the answer is written to the
audit journal either way.

### What asks and what never asks

| Kind              | Actions                                                                                                                                                              | Asks a person?                                                                                                                   |
| ----------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| Observation       | `tickets_read`, `gates_observe`: reading tickets, gates, capacity, events and runs of a declared, bound target                                                       | Never. Adding the project consented to it.                                                                                       |
| Reducing activity | Stopping or winding a session down, cancelling a run and releasing its claim or queue mark                                                                           | Never, and the policy never denies it. It needs an identified actor and is audited.                                              |
| Own records       | The orchestration project's own plan steps, validation step definitions, links and journal                                                                           | Never. They change only the orchestration project.                                                                               |
| Acting            | `tickets_write`, `work_start`, a session start (`session_start` with its tool), `validation_run`, `deployments_run`, a provisional dispatch (`provisional_dispatch`) | Yes, unless a remembered rule allows it, the actor is a person acting for themselves, or the call continues one already allowed. |

A **person's own act** (actor role `human`) needs no further approval: it is the
person deciding. It still passes the policy's limits and is audited with the basis
`person`. A call that **continues** a decision already taken, such as renewing or
releasing the lease a run holds, passes the limits and is not asked again; an
irreversible act is never continuing.

### Permission requests

When an AI session or the orchestrator's own loop wants an acting action that no
rule allows, `authorize` files one request in the machine-local ledger
`${HOTSHEET_HOME}/orchestration-permissions.json` (mode 0600) and answers 403
`permission_required` with its `request_id`. The request is keyed by the
orchestration project, the target, the action and, for a session start, the tool,
and is filed once per operation id: asking again adds nothing. It holds identifiers,
codes and labels only.

The request reaches people through the existing permission bridge
([05](05-ai-tool-plugins.md) §5.7) with the requester kind `orchestrator` and no
blocking waiter: the orchestrator is not a process waiting on a prompt. Clients show
it as the ordinary permission card, headed by the orchestrator and the target
project, with **Allow Once**, **Always Allow** and **Deny**. There is no "Allow This
AI Session": the orchestrator is not a session. Answering publishes the ordinary
`permission_resolved` event and wakes the scheduler, so a waiting step is placed
again without polling.

| Answer       | Effect                                                                                                                                                                                               |
| ------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Allow Once   | Allows that one operation. It is used only when the action succeeds, so a failed attempt can retry under the same operation id; an unused Allow Once lapses after an hour.                           |
| Always Allow | Remembers a rule for (orchestration project, target, action, tool) and allows every waiting request with the same key. Later actions under that key no longer ask. Listed in Settings → Permissions. |
| Deny         | Final for that operation. The action is refused `permission_denied`; a new operation asks again.                                                                                                     |

An unanswered request lapses after a day. Only a person answers: an `ai` or `system`
actor is refused with 403 `forbidden`, and the MCP tool offers only the read view. A
deployment to production, and a validation step that runs a command the orchestration
project chose rather than a profile the target defines, ask **each time**: no rule
covers them and Always Allow is refused for them.

A remembered rule is not tied to an actor: the actor id is self-declared, as
everywhere in Hot Sheet, so the rule is about what this orchestration project may do
in that target. Rules are not a cryptographic boundary between processes of the same
user; the authenticated target server, which still enforces its own device role,
remains that boundary. Forgetting a rule (Settings → Permissions, or `permission
forget`) makes the next action ask again. The ledger keeps at most 512 requests,
1,024 rules and 256 decision records, dropping the oldest settled entries first.

### Judgment calls are decisions on the goal ticket

A choice only a person should judge, such as starting work the plan says is not
ready (§26.15), is not a permission. When the orchestrator wants it, it adds a
`FEEDBACK NEEDED` note with `CHOICE` options to the orchestration (goal) ticket, the
way AI sessions already ask, and refuses the call with 403 `decision_required` and
the `note_id`. A person's response that names an option decides; one note is added per
operation, and asking again reads the response. A person making the same choice for
themselves needs no note.

Outside the ticket (HS2-0RAZ1J), the wait shows where people already look: the note is
an ordinary `FEEDBACK NEEDED` request, so it raises the `ticket.feedback_needed`
notification of [32](32-notifier-plugins.md) §32.8 and the ticket's feedback state, and
the Orchestration timeline marks the step "Waits for your decision" with a link to the
goal ticket (§26.26). `permission list` (and its route and MCP view) lists each decision
by identifiers under `decisions`: the target, operation, goal ticket and `note_id`. The
first time the orchestrator reads a person's response that chooses, it records it on the
decision as `answered` (the response note, the option's index and the time) and audits
`decision_answer`; a newer response that chooses differently replaces it, and reading the
same response again records nothing.

### Retired grants and approvals

`orchestration-grants.json` and `orchestration-approvals.json` from before this model
are never read as authority. `permission list` and `policy validate` report them
(`retired`) so a person can remove them; nothing deletes them automatically. The
grant and approval routes, CLI commands and MCP actions are gone.

`permission retired` (HS2-JWV5S1) shows both files with their counts, and `permission
retired --remove` deletes them and prints the counts it removed, with the route `GET`
and `DELETE /checkouts/{owner}/orchestration/retired-permissions`. Removing is a
person's act: an `ai` or `system` actor is refused with 403 `forbidden` and nothing is
removed. It is idempotent: once they are gone it removes nothing and audits nothing;
each file it removes is audited as `retired_remove` (`revoked`, with `grants_file` or
`approvals_file` as the reason). It touches no other file. The MCP tool keeps only the
read view (`permission_list` reports `retired`).

### Authorization

`authorize(target, actor, action)` requires an identified actor, resolves the target
freshly, and denies with `target_unavailable` when the result is not usable (unbound,
removed, unreachable, unauthorized, missing source, changed identity, or an unknown
owner). Then, in order: observation and reducing activity are allowed; the policy's
limits (§26.8) may deny with `policy_denied` and a `detail`; a continuing call or a
person's own act is allowed; otherwise a remembered rule or the operation's Allow Once
allows, a pending request answers `permission_required`, and a denied one
`permission_denied`. Every answer carries a `basis` (`observation`,
`reduces_activity`, `continuing`, `person`, `remembered_rule`, `allowed_once`) or a
`reason`, plus the `request_id` or `rule_id` it rests on.

`access` is a side-effect-free check against the cached binding. It files no request,
uses no Allow Once, reads no secret, writes no audit entry, and is never authority:
the action itself confirms the destination again.

### Audit journal

`${HOTSHEET_HOME}/orchestration-audit/<owner-checkout-id>.jsonl` is an append-only
journal with one JSON object per line: `seq`, `at_ms`, `actor_role`, `actor_id`,
`target_id`, `action`, `operation_id`, `outcome` (`allowed`, `denied`, `succeeded`,
`failed`, `approved`, `rejected`, `revoked`, `changed`, and `granted` in old
entries), a `reason` code, and `permission_id` (the request or rule; old entries
read their `grant_id` here). Every field is an identifier or a stable code; free text,
ticket content, credentials and origins are rejected at the journal boundary. Every
`authorize` answer, every answered request (`permission_answer`), remembered and
forgotten rule (`permission_remember`, `permission_forget`), and the reported result
of an authorized action are recorded under the same operation id.

**Repeated observation reads (HS2-SV4SMY).** An observation (`gates_observe`) outside
any operation that was allowed or succeeded is written only when it says something the
journal does not already say about its subject (the same actor's same action on the
same target) within 10 minutes (`AUDIT_COALESCE_MS`): a success is skipped when the
newest entry of its subject is the same success, and an allowance when that newest
entry is a success inside the window and the decision before it is the same allowance.
A denial, a failure, a different reason, permission or role, an operation id, a clock
that went back, or the window passing writes the entry. Every action that acts on a
target, and every decision a person makes, is always written. The check is made under
the journal lock. Without it each scheduling pass wrote an allowed and a succeeded entry
per target it read (about 200 bytes each), so a loop that runs a pass a minute over
three targets wrote 360 entries an hour and rotated older decisions out of the 2 MiB
journal within a day; now such reads write at most two entries per target and window,
whatever the pass rate.

**Repeated validation waits (HS2-5ZT4GY).** Each bounded wait on a running validation
attempt passes the gate again, so the grant and the destination are checked on every
request, and is allowed as `continuing` under its operation id. Such an entry is
written only when the newest entry of the same operation (same actor, target and
action) is not the same continued wait within the window; entries of other operations
in between do not count. A step of up to an hour, waited on every 8 seconds, so writes
its starting decision, one continued wait per 10 minutes and its outcome, instead of
one entry per wait (up to 450). A refusal of a wait, a different reason or permission,
and the outcome are always written.

A segment rotates at 1 MiB to one retained predecessor, so the journal is bounded
near 2 MiB per owner. `seq` never restarts. A reader passes `after` and gets later
entries, a `cursor` to continue from, `more`, and `gap: true` when retention removed
entries it had not seen. A torn final line from a crash is skipped and never
rewritten; the next append starts on its own line.

### Headless surface

```sh
hotsheet-cli -C <configured-host-source> orchestration permission list --project <owner-checkout-id>
hotsheet-cli --actor-role human -C <configured-host-source> orchestration permission answer <request-id> \
  --project <owner-checkout-id> --answer allow_once|always_allow|deny
hotsheet-cli --actor-role human -C <configured-host-source> orchestration permission remember <target-id> \
  --project <owner-checkout-id> --action session_start --tool codex
hotsheet-cli -C <configured-host-source> orchestration permission forget <rule-id> --project <owner-checkout-id>
hotsheet-cli --actor-role human -C <configured-host-source> orchestration permission retired --project <owner-checkout-id> [--remove]
hotsheet-cli -C <configured-host-source> orchestration permission check <target-id> --project <owner-checkout-id> \
  --actor <worker-id> --action sessions_manage --tool codex --starting --target-sessions 0 --machine-sessions 0 --tool-sessions 0
hotsheet-cli -C <configured-host-source> orchestration audit --project <owner-checkout-id> --after <seq> --limit 100
```

`permission remember` is the headless path for setting up a project ahead of any
request; it is idempotent by key and needs a declared target. `permission check`
accepts `--operation-id`, `--model`, `--target-sessions`, `--machine-sessions`,
`--tool-sessions`, `--account` and `--account-sessions`.

| Route                                                      | Methods and purpose                                                          |
| ---------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `/checkouts/{owner}/orchestration/permissions`             | GET requests with their state, remembered rules, decisions and retired files |
| `/checkouts/{owner}/orchestration/permissions/{request}`   | POST `{answer}`: `allow_once`, `always_allow` or `deny`                      |
| `/checkouts/{owner}/orchestration/permission-rules`        | POST `{target_id, action, tool?}` remember a rule ahead of a request         |
| `/checkouts/{owner}/orchestration/permission-rules/{rule}` | DELETE forget a rule                                                         |
| `/checkouts/{owner}/orchestration/retired-permissions`     | GET the retired files with counts; DELETE a person removes them              |
| `/checkouts/{owner}/orchestration/targets/{target}/access` | POST side-effect-free check against the cached binding                       |
| `/checkouts/{owner}/orchestration/audit`                   | GET journal page, `?after=<seq>&limit=<n>`                                   |

All require the bearer and literal loopback at the serving machine for every device
role. The server's own permission routes (`GET /permissions`, `POST
/permissions/{id}`, `GET` and `DELETE /permissions/rules`) carry the same requests
and rules, so a client answers an orchestrator request like any other. MCP
`hotsheet_targets` adds the read-only actions `permission_list`, `access_check` (with
`for_actor`, `target_action` and an optional `session`) and `audit`. Errors use 403
with the denial code or `forbidden`; `permission_required` and `permission_denied`
carry `request_id`, `decision_required` carries `note_id`.

## 26.8 Orchestrated throughput and its policy (HS2-YCJDEB)

Decided with the maintainer on 2026-11-03 and extended on 2026-11-04; since
HS2-XHQ1EH the policy holds limits only.

### Design

- **Goal.** Orchestration creates AI tool sessions as appropriate and maximizes
  throughput across target projects, balancing priority, urgency, compute, resources
  such as RAM, and AI tool capacity. Tools may be mixed across targets. A started
  session appears in the bottom drawer of the server that hosts it.
- **Automatic within limits and permission.** Inside the policy's limits, and with a
  person's permission or a remembered rule (§26.7), the orchestrator starts, refills
  and winds down sessions on its own and logs every decision. Judgment calls become
  decision notes on the goal ticket.
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
| `targets[]`   | `target_id`, `enabled` true, `tools[]` with optional `models`, `max_sessions` 1, and optional `machine` and `speculation` overrides.                                     |
| `wind_down`   | `idle_minutes` 15 (1 to 1440) and `keep_warm` 0, idle sessions kept per target.                                                                                          |
| `speculation` | `allowed` false, `max_depth` 1 (1 to 8 unconfirmed gates), `risk_weight` 50 (0 ranks provisional work like confirmed work, 100 last), `opt_out_tag` `speculate:never`.   |
| `placement`   | HS2-ECWG50, §26.17: `automatic` false, `actor`, `aging_minutes` 60, `cooldown_minutes` 5, and `resources[]` (§26.20). Left out of the file while it holds only defaults. |

**Retired fields.** `targets[].actions` and `approval.required_for` are read and
ignored. `validate` reports each as `target_actions_ignored` or `approval_ignored`
under `ignored`, and `set` stores the policy without them. Permission to act comes
from §26.7; a judgment call is a decision note on the goal ticket.

A target a policy lists as disabled is refused every acting action. A session start
needs the target listed with that tool; a tool listed for a target must be configured
under `tools`, and a tool configured with accounts runs only under one of them.

`validate` reports every problem at once with a stable code and the tool, account or
target it concerns. Conflicts are errors: a target cap above its machine cap, an
account cap above its tool cap, a target tool that is not configured, duplicates, and
a target the manifest does not actively declare. `set` refuses an invalid policy,
replaces the file at the expected revision, and is a no-op for identical content. It
is a person's act: an `ai` actor is refused, because the governed session must not
loosen its own bounds.

### Place in the authorization gate

`authorize` consults the policy after observation and reducing activity and before a
person is asked, so a request is never filed for an action the limits refuse.

- **No policy file.** Ticket, gate and validation actions are governed by permission
  alone, so a project that never enables throughput is unaffected. A session start is
  denied `policy_absent`: nothing automatic runs without a policy. An unreadable or
  corrupt policy denies every acting action.
- **Denials** are `policy_denied` with a `detail`: `policy_target_not_permitted`,
  `policy_target_disabled`, `policy_tool_not_permitted`, `policy_model_not_permitted`,
  `policy_load_unknown`, `policy_target_cap`, `policy_machine_cap`,
  `policy_tool_cap`, `policy_account_not_permitted`, `policy_account_cap`,
  `policy_speculation_not_allowed`, and `policy_project_software` for every acting
  action of a project whose type is Software (§26.27, HS2-J3ZF27). The audit entry
  records the detail.
- **Caps.** A session start states the current load (sessions on the target, on its
  machine, on the tool and on the account). A start without it is denied, and each
  cap denies exactly at its bound. Stopping or winding down needs no headroom.
- **Resource floors** are not evaluated here. The scheduler and the target's own
  admission check compare them with the target's capacity report (HS2-J8PRTV,
  HS2-ECWG50).

### Dry run

`dry-run` evaluates a proposed policy, or the stored one, against every actively
declared target and reports per action whether the limits allow or deny it and why,
and whether it needs permission: `not_needed` (observation), `remembered` (with the
`rule_id`), or `asks`. `automatic` is true for an action the orchestrator could take
right now without asking anyone. It writes nothing, files no permission request and
contacts no target.

### Headless surface

```sh
hotsheet-cli -C <configured-host-source> orchestration policy show --project <owner-checkout-id>
hotsheet-cli -C <configured-host-source> orchestration policy validate --project <owner-checkout-id> --file policy.json
hotsheet-cli -C <configured-host-source> orchestration policy dry-run --project <owner-checkout-id> --file policy.json
hotsheet-cli --actor-role human -C <configured-host-source> orchestration policy set --project <owner-checkout-id> --file policy.json
```

`--file` takes a policy body or the output of `policy show`, and `-` reads stdin.
`policy set` without `--expected-revision` reads the current revision first, so a
repeated setup run changes nothing while a concurrent edit still conflicts.

| Route                                              | Methods and purpose                                                      |
| -------------------------------------------------- | ------------------------------------------------------------------------ |
| `/checkouts/{owner}/orchestration/policy`          | GET stored policy; PUT replace at the expected revision                  |
| `/checkouts/{owner}/orchestration/policy/validate` | POST every problem, ignored field and retired file                       |
| `/checkouts/{owner}/orchestration/policy/dry-run`  | POST per-target outcome; machine-local because it reads remembered rules |

The policy itself is ordinary authenticated project data, like the target
declarations. MCP `hotsheet_targets` adds the read-only actions `policy_show`,
`policy_validate` and `policy_dry_run`; setting the policy is a person's CLI or HTTP
action. The workspace editor for the same policy is part of HS2-HSMH2Q.

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
run where; the target's dispatcher still assigns each exact ticket. It dispatches the
run of each step it places in dispatcher mode under `sched-run-<step id>` (§26.19,
HS2-YC2P7K). Integration,
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
- **Authority.** Dispatch passes the gate as `work_start` with the operation id, so
  it asks a person unless a remembered rule allows it (§26.7). Renewing continues
  that decision: policy limits still apply, but nobody is asked again. Cancelling
  reduces activity and never asks. Supervising with `sync` only reads.
- **Readiness.** A target ticket that a plan step works is dispatched only when that
  step is `confirmed`, or when a person overrides it for the exact operation
  (§26.15). A ticket no step works is not gated.
- **Refusals.** The target ticket is read first, so a ticket that does not exist is
  never recorded as a run. A definitive target refusal ends the run as `failed` with
  the target's code; a failure that may be transient leaves it `requested` so the
  same request can be repeated.
- **Cancel** releases a direct run's claim, or withdraws the Up Next mark of a run
  still queued, and ends the run. A pooled worker's claim is not the orchestrator's
  to release; cancelling such a run only lets go of the run. If the target cannot be
  told, the run stays active with `cancel_requested: true` and the answer lists what
  it could not verify (`target_claim` or `target_queue`). A cancel that must tell the
  target passes the gate as the caller, which needs an identified actor and is
  audited; it reduces activity, so it never asks and the policy never denies it
  (HS2-XHQ1EH, HS2-48H57C). Only a target that cannot be resolved
  (`target_unavailable`) is reported as unverified (HS2-WC3W2G).
- **Sync** reads the target ticket. A completed, verified or archived ticket
  completes the run. A queued run becomes dispatched when the ticket shows a live
  claim, recording that worker. A dispatched run whose claim was released or taken
  becomes `intervened`, and returns to `dispatched` when its own worker holds a live
  claim again. A lapsed lease is not a live claim. A sync that a concurrent one (the
  server's follower) beat to finishing the run answers the finished run, not
  `run_not_active` (HS2-2C5N1W).
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
work is §26.16.

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
`cancel_requested`, `validation` (§26.12), `gate` (§26.13), the recovery kinds of
§26.14 (`reconciled`, `paused`, `resumed`, `handoff`, `corrected`), `readiness`
(§26.15, about a plan step rather than a run) and `session` (§26.16). Each event says
whether the orchestrator did it or it was learned
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
(`/checkouts/{checkout}/target-events`, at most 8 seconds per request). The feed carries ticket creations, updates,
deletions, moves, claims and releases (claims and releases since HS2-YC2P7K). A first
request is a handshake that returns the cursor and no backlog, after which every run
of that target is caught up once. After that only runs whose ticket changed are
synced. A cursor that goes backwards or an `overflow` answer means the host no
longer has the backlog, and every run is caught up again. On failure the follower
keeps its cursor, so reconnecting replays what was missed, marks the affected runs
unreachable once, and retries with bounded backoff from one second up to a minute.
It confirms its authority through the gate when it connects, after any failure and
every ten minutes, acting as whoever dispatched the run. A target with no unfinished
run is not contacted at all, and a run a person took over is still followed.

A change written straight to a target store, such as by a headless `hotsheet-cli` in the
session's checkout rather than through a server route, reaches the feed through that
store's file watcher, so it is seen while the hosting server hosts the store (a project
open in a client, or its configured stores) (HS2-BDNJ5S). A linked Git store of the
followed checkout that the server does not host is not hosted for the feed, because an
index, a native stream and a polling bridge per store cost too much under load. Instead
the feed keeps one file watcher on that store's `tickets` directory, from a follower's
first request until no follower has asked for ten minutes. Each changed ticket file is
recorded in the change log as a `target_store_changed` event that carries only the
ticket id and is read only by the feed, so cursors and replay work as they do for hosted
stores (HS2-G5FYP6).

**Planned tickets without a run (HS2-TMA4YA).** A target is also followed while an
unfinished plan step names one of its tickets: a prerequisite, or the step's own
ticket. On the first request, after an `overflow` and on a reconnect every such ticket
is read again through the gate (`tickets_read`, one pass per target); after that only
those whose change the feed reports. What is read becomes the plan's ticket facts and
any level it changes is published as a `readiness` event (§26.15). The follower's read
is opportunistic: a ticket it cannot read changes nothing, so a passing outage never
makes a step stale (an explicit `readiness refresh` still records it as unverified). With no run or
session, the follower acts as whoever last defined a step naming the ticket. A server
starts these followers at start for every owner whose plan names target tickets, and
when a step is defined or refreshed; a finished or removed step names nothing, and a
target with nothing left to follow is not contacted.

**Session changes on the same feed (HS2-257Q2Z).** When a session an orchestrator
started exits or finishes a turn, its hosting server publishes that on its change
stream for the session's checkout, and the hosted feed lists it under `sessions`
(the cursor and the host's terminal id, nothing else). A target is also followed
while the owner has an unfinished session on it, acting as whoever started the
session when no run remains. A follower step wakes the owner's scheduler (§26.17)
when the page names one of the owner's sessions; when a ticket of the target changed
while one of those sessions works no unfinished run, since a claim taken or let go
is what makes a session busy or idle and no run sync reports it; and when it
(re)connects or the host lost its backlog. A session working an unfinished run
learns its ticket changes from that run's events instead.

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
itself and how it answers. Starting the session is §26.16; the scheduler that
asks is HS2-ECWG50.

### Capacity snapshot

A snapshot is what the server hosting a checkout measured just now. Megabytes and
percent are integers. A value the host could not measure is `null`; nothing is
estimated in its place.

| Field                                  | Meaning                                                                                                  |
| -------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `server_id`, `checkout_id`             | The machine identity and the hosted checkout the snapshot is for                                         |
| `sampled_at_ms`                        | When the shared reading was taken                                                                        |
| `memory.free_mb`, `memory.total_mb`    | Memory a new process could use without swapping, and physical memory                                     |
| `cpu.load_percent`, `cpu.logical_cpus` | Whole-machine busy share over a 250 ms window, and the core count                                        |
| `disk.free_mb`, `disk.total_mb`        | The volume that holds the checkout                                                                       |
| `tools[]`                              | Each drivable AI tool by name, whether it is `installed`, and whether the checkout has it `configured`   |
| `sessions[]`                           | Each running AI session: `session_id`, `tool`, `busy`, `on_checkout`, `resident_mb`                      |
| `claims[]`, `claim_namespaces`         | Live resource claims (namespace, resource, expiry) in the named namespaces, or in every one (HS2-YDZAFH) |

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
  (at most eight). Without a name, every namespace with claim state on the machine is
  enumerated (`ClaimBackend::namespaces`, HS2-YDZAFH) and `claim_namespaces` lists the
  ones covered, so `claims` is every held claim, an empty list when none is held. More
  than 64 namespaces, or a claim source that cannot enumerate, reads as `null`, never
  a partial list. When a named namespace's
  backend cannot be read, `claims` is `null` as a whole and `claims_offline` says
  what that backend's configuration asks for meanwhile (`fail_closed` or
  `provisional`, §26.20); the member is absent otherwise.
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

**Waiting for a change (HS2-P5GTSW).** Every snapshot carries a `version` that changes
exactly when the room it describes changes, compared in coarse steps so ordinary jitter
is not a change: free and total memory in 1 GiB steps, CPU load in 20 % steps, disk in
4 GiB steps, and exactly which sessions run (with their tool and checkout, not whether
one is mid-turn), which tools are installed and configured, which claims are held (not
their expiry), the offline policy and the speculation setting. The sampling time is
never part of it, so the same capacity gives the same version after a restart. The
machine owner's own admission limits (`admission set`) are part of it too, because they
decide room as much as the measurements do (HS2-AWB7BX).

A read that names the version it has (`after`) and a `wait_ms` is a genuine long poll:
the host samples again every three seconds (one shared reading, so waiting subscribers
share it) while that request waits, and answers as soon as the version differs, or with
the same version when the wait ends (at most 8 seconds, under the target transport's
read timeout). A version that is not current answers at once, which is how a
subscriber catches up after a reconnect. Sampling runs only while a request waits: with
no subscriber the host reads nothing, and it stops when the last one leaves. An older
host's snapshot has no `version` and offers no wait. The host health sampler
([host-health](host-health.md)) and its live CPU feed are separate and unchanged; a
machine-wide indicator (HS2-SW5ZK3) can subscribe to the same route.

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
permission request; the policy must still permit the tool and model on the target.
The start itself passes the gate again with its load and asks a person unless a
rule allows it (§26.16). Reading capacity is observation and never asks. Both are recorded in the audit
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
hotsheet-cli ... orchestration capacity show --project <owner> --target <target-id> [--claims <namespace>] \
[--after <version> --wait-seconds 8]
hotsheet-cli ... orchestration capacity show --checkout <checkout-id> [--after <version> --wait-seconds 8] # this machine
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
| `/checkouts/{checkout}/target-capacity`                       | Hosted half: GET this machine's snapshot (`?claim_namespaces=a,b&after=&wait_ms=`)  |
| `/checkouts/{checkout}/target-admission`                      | Hosted half: POST `{tool, model?, sessions?, limits?}` for this machine's answer    |
| `/checkouts/{owner}/orchestration/targets/{target}/capacity`  | GET (`?actor_id=`) or POST (`actor` in the body) a target's snapshot; machine-local |
| `/checkouts/{owner}/orchestration/targets/{target}/admission` | POST `{tool, model?, sessions?}` for the target's answer; machine-local             |

The hosted routes are ordinary authenticated project data under the caller's device
role, like the hosted ticket route, so a remote orchestration project reaches them
over its reviewed registration. A read-only device can read capacity and cannot ask
for admission. A peer that predates the routes answers `target_peer_unsupported`. MCP
`hotsheet_targets` adds the read-only actions `capacity` and `admission_check`;
`capacity` takes `version` and `wait_ms` to wait for a change (HS2-P5GTSW), as the
orchestrating routes take `after` and `wait_ms`.

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
| Codex       | reported          | reported           | reported    | reported     | reported |
| OpenCode    | unknown           | unknown            | unknown     | reported     | unknown  |
| Antigravity | unknown           | unknown            | unknown     | unknown      | unknown  |

Codex reports a request-rate limit as its own `CodexErrorInfo` variant,
`rate_limit_exceeded`, beside `usage_limit_exceeded` (HS2-2BAVAM). The rollout halt path
accepts it as a `rate_limit` signal with the same reset-time reading. OpenCode and
Antigravity stay `unknown`. OpenCode's ACP surface reports no typed limit error (it retries
a 429 inside the prompt, then ends the turn without one), so its detection waits for one
(HS2-MJZ2QV). Antigravity's one-shot `agy --print` failure carries only an exit status and
text, and reading the text would be a second message parser (HS2-86MK1Y).

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
it.

**Usage (HS2-XFMKHQ).** Every usage event the server appends ([14](14-metrics-interface.md))
becomes one `usage` signal at a single choke point, `tool_capacity_signals::usage_appended`,
called right after the append with exactly the events written and priced: driven turns
(through the dist work loop's usage sink), client-drive turns, Claude transcript usage and
the Codex rollout watch. Events from a terminal carry the account its session was started
for; driven and client-drive turns are unattributed. A retried append of the same event
yields the identical signal (tool, time, cost, and a `session` discriminator made of the
session id and a short digest of the event's own numbers and ids), which the registry
answers `duplicate`, so it counts once. Two different responses of one session in the
same millisecond with the same rounded cost differ in that digest and both count; an
earlier signal that kept only the session id could collapse them and undercount a
budget. A usage signal wakes the scheduler only when it changes the account's
standing (a first completed turn, a confirmed recovery); one that only adds cost does
not, and the budget is read at the next pass.

**Which account (HS2-PFHRS9).** A session's account is declared by whoever starts it for
one: the orchestrator, when `session_start` names an `account` (required when the
policy configures accounts for the tool). The hosted start carries it
(`NewHostedSession.account`, an identifier, left out when absent so an older host reads
a start without one) and the host keeps it on its record (`HostedSession.account`). The
same operation for another account conflicts. When a terminal halts on a limit or
resumes, the host looks its terminal id up in its hosted-session records
(`HostedSessionRegistry::account_of`) and the signal carries that account, so it limits
and recovers that one account. Any other session (a person's own terminal, a start
without an account) stays unattributed. Only the account id is read and stored: no
credential, provider profile or environment.

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
  `name`, a `program`, `args` and an optional `timeout_seconds`. The orchestration
  project names the profile and never supplies its command. The file is read where
  the target lives, at most 64 profiles; a damaged, duplicated or newer file is an
  error and never a guess. How a target project sets it up and checks it is below.
- A **command** is a program and arguments chosen by the orchestration project. It
  is started directly, without a shell. A program and its arguments are at most 4,096
  bytes and 32 arguments together.
- A program with a directory part (`scripts/check.sh`) is the checkout's own file; a
  bare name is looked up on the host's `PATH`. Both run with the target checkout as
  the working directory.

### Offering test profiles (HS2-YYNW9F)

A target project that wants to be validated by name checks in
`.hotsheet2/validation-profiles.json`:

```json
{
  "schema_version": 1,
  "profiles": [
    { "name": "smoke", "program": "scripts/check.sh", "args": ["--smoke"], "timeout_seconds": 600 },
    { "name": "lint", "program": "cargo", "args": ["clippy", "--all-targets"] }
  ]
}
```

- `name` is the identifier steps refer to; names are unique.
- `program` and `args` follow the command rules above and are started without a
  shell.
- `timeout_seconds` (1 to 3600) is the longest the project lets the profile run. A
  step's own timeout still bounds it; the shorter applies, so the orchestrating
  side's deadline is never outlived.

Check the file headlessly from the checkout that offers it, and from an
orchestration project through the gate:

```sh
hotsheet-cli ... orchestration validation profiles --project <checkout>
hotsheet-cli ... orchestration validation profiles --project <owner> --target <target-id>
```

Both print every profile and every problem and exit non-zero when there is one. A
problem is a code with the profile's position: `invalid_name`, `invalid_command`,
`invalid_timeout`, `duplicate_name`, or for the whole file `too_large`,
`invalid_json`, `schema_version_missing`, `unsupported_schema_version`,
`invalid_shape`, `too_many_profiles`. A name is shown only when it is itself valid,
and a program or argument is never echoed in a problem. An absent file is valid and
offers nothing.

The read through the gate is an observation (§26.7): it never asks a person, is
audited as `gates_observe`, and runs and writes nothing on the target. It is
`GET /checkouts/{checkout}/target-validation-profiles` on the hosting machine,
`POST .../orchestration/validations/profiles` with `target_id` on the orchestrating
one, and MCP `validation_profiles` (with `target_id`, or without it for the
checkout's own file). The Hot Sheet skill that setup installs into every project
(version 73, HS2-28E486) tells a project's agents about the file and its check, so a
target project learns to offer and keep its profiles.

Defining a step again with the same definition changes nothing. A changed definition
gets the next `revision`, and evidence produced for an earlier revision no longer
counts. A ticket holds at most 32 steps and a machine 512. A step keeps its 16 most
recent results.

### Result states

A result is one attempt at a step.

| State         | Meaning                                                                  | May become                                                   |
| ------------- | ------------------------------------------------------------------------ | ------------------------------------------------------------ |
| `running`     | The attempt was started and its outcome is not known yet                 | `passed`, `failed`, `unavailable`, `unverified`, `cancelled` |
| `passed`      | It ran and produced the expected evidence                                | nothing                                                      |
| `failed`      | It ran and did not: wrong exit code or output, timeout, signal, too loud | nothing                                                      |
| `skipped`     | It was not run because a prerequisite has not passed                     | nothing                                                      |
| `unavailable` | The target could not run it at all                                       | nothing                                                      |
| `unverified`  | It may have run, but its outcome could not be confirmed                  | nothing                                                      |
| `cancelled`   | It was stopped on request before it ended (HS2-17ABM2)                   | nothing                                                      |

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
- `cancelled`: `cancelled`. It is not a failure: the retry policy takes no further
  attempt, and a readiness prerequisite on the step is blocked with
  `validation_cancelled` until a new request passes it.

### What a result records

A result holds its `result_id`, the caller's `operation_id`, the attempt number, the
step revision, the state and reason, the actor, an optional `run_id`, and the
outcome the host observed: how the execution ended, the exit code, when it started,
how long it took, and for each of standard output and standard error the SHA-256 and
the size in bytes.

The host digests a step's output as it reads it. Output is never returned by a
hosted route, written to the result file, a journal, an event or a ticket note, and
never reaches the orchestrating machine. A step that writes more than 64 MiB to one
stream is stopped and `failed`. The note added to the orchestration ticket when an
attempt finishes names the step, the attempt, the state, the result id and the
target, and nothing else.

**Kept output (HS2-ZCR3Y7).** So that a failed step can be inspected, the machine
that ran it keeps each stream of at most 4 MiB whole, under the SHA-256 the result
records, in `${HOTSHEET_HOME}/orchestration-validation-output/<checkout-id>/<sha256>`
(directories 0700, files 0600). A longer stream keeps its digest only, since a
truncated copy would not match it. Copies expire after 14 days, and the machine keeps
at most 512 of them and 256 MiB in all, dropping the oldest; pruning happens when a
copy is written. The read path is machine-local only: `orchestration validation
output <sha256> --project <checkout-id>` and `GET
/checkouts/{checkout}/target-validation-output/{sha256}` (bearer and literal loopback)
answer the stream base64-encoded, only from that checkout's copies, and refuse a copy
that no longer matches its digest. There is no remote read and no MCP action:
output can contain customer data, so it stays on the machine that produced it.

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
HS2-2R32SD.

**Environment (HS2-W02PTA).** A step does not inherit the hosting process's
environment. It sees an allowlist of that process's variables, enough to find its
tools and behave predictably (`PATH`, `HOME`, `USER`, `LOGNAME`, `SHELL`, `TMPDIR`,
`LANG`, `LC_*`, `TZ`, and on Windows `SystemRoot`, `windir`, `ComSpec`, `PATHEXT`,
`TEMP`, `TMP`, `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, `ProgramData`,
`ProgramFiles*`, `NUMBER_OF_PROCESSORS`, `PROCESSOR_ARCHITECTURE`), plus
`HOTSHEET_VALIDATION_EXECUTION_ID` (its execution id) and `CI=true`. Everything else
is withheld: Hot Sheet's own `HOTSHEET_*` variables, the bearer and provider tokens a
server or CLI holds, cloud and package-registry credentials, `SSH_AUTH_SOCK`. A
profile declares no environment of its own; the checked-in program it runs sets
what it needs (for example `env NAME=value command`). A step still runs as the
hosting user, so files that user can read stay readable; the allowlist governs
variables only.

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
- The host identifies an execution by the result id and never runs one id twice. A
  running execution lives in the hosting process: at most 64 are remembered, four of
  them running, and a fifth is refused with `capacity_reached`. The machine also
  records each execution it accepted, before it launches, and how it ended, in
  `${HOTSHEET_HOME}/orchestration-hosted-validations.json` (0600, at most 256, the
  oldest finished forgotten first; HS2-7BTV98). After the hosting process restarts, a
  finished execution is answered from that record, so its result is still collected;
  one that was running when the process died answers `unknown` (the attempt becomes
  `unverified`, `execution_lost`) and a start of it that arrives later runs nothing;
  an id cancelled before it ran stays cancelled.
- A step runs in a detached runner process, so it finishes and is collected after
  the hosting process restarts (HS2-M7EV18). The host writes the step's launch to a
  private spec file under `${HOTSHEET_HOME}/orchestration-validation-steps/` and starts
  `hotsheet-cli orchestration step-runner` in its own process group (process-journal
  purpose `validation_step`). The runner claims the spec once, by an atomic rename, so a
  second runner of the same id runs nothing; it runs the step and records its end in
  the registry above, which is the runner's outcome file. Any host process answers
  from that record: `running` while the runner (named by pid and start time) still
  runs, the recorded end when it has one, and `unknown` (`execution_lost`) when the
  runner is gone without one. A cancel from any host process leaves a marker the
  runner waits on with a file watcher and records `cancelled` itself. A headless CLI
  host may therefore exit while a step runs; `validation run` still waits by default
  so its caller sees the result. The runner is used where its liveness can be
  checked by start time (Linux, macOS); elsewhere a step runs inside the hosting
  process as before. The registry stays the one never-run-twice record. Each runner is
  also shown among the machine's Jobs ([33](33-jobs.md), HS2-BWAPSX): an owned Job of
  kind `validation-step` under the hosted checkout, keyed by the execution id and
  titled "Validation step" or "Deployment to `<environment>`", running under a lease
  the runner renews while the step runs, then `succeeded` (exit 0), `failed` or
  `cancelled`, with the end, exit code and duration as its outcome. It carries no
  output, no feed and no actions (a step is stopped through the orchestration's
  cancel). A runner that dies stops renewing, so its Job reads stalled and then
  lost.
- A step runs as its own process group. While it runs, the record names that group
  and the hosting process, each by pid and start time (HS2-HTS89S). When a later
  host process starts serving (the server at start-up, a CLI host on its first
  hosted validation request) or is asked to cancel the attempt, it stops a group
  whose hosting process is gone: when the group's leader is still the recorded
  process, or when no process has that pid any more (a pid is not reused while a
  group of that id exists, so the group can only be the step's remaining members). A
  pid that now names another process is left alone, and a step whose hosting process
  still runs is left to it. The attempt stays `unverified` (`execution_lost`): what
  the step would have reported is not known. Only Unix (Linux, macOS) records and
  stops groups.
- A step whose prerequisite has not passed is recorded as `skipped` without
  contacting the target and without asking anyone. A later request with a new
  `operation_id` runs it once the prerequisite passes.

The scheduler that runs steps without a caller is HS2-ECWG50.

### Cancelling a step (HS2-17ABM2)

`cancel` stops a step's running attempt: the one its `operation_id` names, or
whichever attempt of the step is running when it is omitted.

- **It never asks.** Cancelling only lowers activity on the target, so it passes the
  `validation_run` gate as reducing activity (§26.7): no person is asked, the policy
  never refuses it, and it needs an identified actor and is audited.
- **The host stops the execution** and its process group, then answers how it
  ended, waiting at most `wait_ms` (8,000). The attempt ends `cancelled` with an
  outcome whose `end` is `cancelled`. It is never recorded as `failed`.
- **A race with the step's own end** is decided by the host: a step that exited
  before the stop took effect keeps its own outcome, and the cancel records that
  (`passed`, `failed`, ...). The request that started the attempt may observe the
  same end at the same moment; whichever writes it first records it, the other
  answers the same result, and the ticket note and run event appear once.
- **A start the host has not seen yet** cannot run afterwards: the host answers
  `unknown` and remembers the id as cancelled, so a start that arrives later under
  that id runs nothing. The attempt is recorded `cancelled` without an outcome. An
  attempt the host had accepted and no longer knows is `unverified`
  (`execution_lost`), as for `run`.
- **While the stop is not confirmed** (the host is unreachable or still stopping),
  the attempt stays `running` and the answer lists `target_execution` under
  `unverified`; the same cancel again waits further.
- **Nothing running** is answered from the record: a finished attempt is returned as
  it is, without the gate or the target. A step that never ran, or an unknown
  `operation_id`, is `validation_result_not_found`.

A cancelled request is finished: running it again with the same `operation_id`
answers the cancellation, and a new `operation_id` runs the step again. The target's
own device role still applies, so a read-only pairing cannot stop an execution.

### Authority

Validation has its own permission action. It uses `validation_run` (§26.7), not the
`work_start` action that dispatch uses, so a rule that allows dispatching work does
not allow running checks and the reverse.

- **Define** and **remove** change only the orchestration project's own records:
  they run nothing and ask nobody, and need an identified actor. A step of an unbound
  or unreachable target can always be cleaned up.
- **Run** passes the gate as `validation_run` with the `operation_id`, and asks a
  person unless a remembered `validation_run` rule allows it.
- **A profile is inside a rule's scope**: the target project chose its command. **A
  command is outside it.** A `command` step asks a person for that exact operation
  every time (`permission_required`), whatever rules exist; Always Allow is refused
  for it. Continuing an allowed request, including its retries, does not ask again.
  Scoping rules to named profiles or reviewed commands is HS2-27ZYX5.
- An unusable target is a denial (`target_unavailable`) and records no result.
- The target's server still enforces its own device role: a read-only pairing cannot
  start an execution, whatever a rule says. The hosted route gives a read-write
  pairing nothing it did not have: such a caller can already open a terminal on that
  server. Permissions govern the orchestrating side's automation; they are not a
  boundary on the target, as §26.7 says of remembered rules in general.

Every gate answer and every finished attempt is in the audit journal under
`validation_run`, the latter as `succeeded` for `passed` and `failed` with the state
code otherwise. A changed definition is `validation_define` and a removal
`validation_remove`. Each bounded wait passes the gate again; a continued wait of the
same operation is recorded once per window (HS2-5ZT4GY, §26.7). When a run request names a dispatch `run_id`, that run's event
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
hotsheet-cli ... orchestration validation cancel --project <owner> --ticket <orchestration-ticket> \
  --step <name> [--operation-id <retained-id>] [--no-wait]
hotsheet-cli ... orchestration validation show|remove --project <owner> --ticket <orchestration-ticket> --step <name>
hotsheet-cli ... orchestration validation list --project <owner> [--ticket <orchestration-ticket>]
hotsheet-cli ... orchestration validation profiles --project <checkout-or-owner> [--target <target-id>]
```

`validation run` repeats its one request until the attempt is finished and the retry
policy has nothing left, and returns at once when the target could not be confirmed.
The result is in the JSON (`result.state`), not in the exit status. `--source` names
the orchestration ticket's connection and may be omitted when the project has one git
ticket source.

| Route                                                                                            | Methods and purpose                                                                                                       |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------- |
| `/checkouts/{owner}/orchestration/validations/{define,list,evidence,run,remove,cancel,profiles}` | POST orchestrating-side operations; machine-local                                                                         |
| `/checkouts/{checkout}/target-validation-profiles`                                               | Hosted half: GET the checkout's profiles, checked (HS2-YYNW9F)                                                            |
| `/checkouts/{checkout}/target-validations`                                                       | Hosted half: POST start or re-attach by `execution_id`; GET its state; DELETE stop it (HS2-17ABM2), each with a `wait_ms` |

The hosted route is ordinary authenticated project data under the caller's device
role, like the hosted ticket route. MCP `hotsheet_targets` adds `validation_define`,
`validation_list`, `validation_evidence`, `validation_run`, `validation_remove`,
`validation_cancel` and `validation_profiles`. The workspace view is HS2-9XJEZC.

## 26.13 CI and deployment gates (HS2-5JBYPV)

A **gate** ties one run to one commit of its target and records whether a provider
confirmed that commit. A CI gate says whether the integrated commit passed CI. A
deployment gate says whether that commit was deployed to an environment. Later
decisions read gates: promotion and readiness (HS2-42TARA), and provisional work on
a prerequisite whose gate is still pending (HS2-2SJPCH, HS2-JSF985).

### Evidence comes from adapters only

A gate changes state on exactly two things: a typed reading from a provider adapter,
or a person's answer to a deployment's permission request. Nothing a session writes in a note, a
ticket or a message is evidence, and no text is parsed for a verdict. A reading holds
a state, a reason code and, for a verdict, evidence: the provider, its own run or
execution id, the attempt, the commit it built or deployed, how that commit relates
to the gate's commit, and optionally an artifact name with its digest and an exit
code. Every field is an identifier, a count or a code.

A `GateProvider` adapter serves one provider name and one kind. It starts a watch or
an execution idempotently, answers the current reading from records it keeps itself,
and offers a cursor with a blocking wait that is woken by the change. Two CI adapters
ship, each the watch Job of [33](33-jobs.md): `github-actions` (HS2-3R9V0R) and
`gitlab-ci` (HS2-TPAHYA, §33.11). The machine's one Job monitor observes the
provider; this machine reads the Job store and waits on its file changes, so no
client polls. A `gitlab-ci` gate takes the project's full path as `repository`
(`group/[subgroup/…]project`, or the path a declared non-GitHub identity names) and
a pipeline `name` as `workflow`, by default `*` (any push pipeline of the branch);
the same coverage rules apply. GitLab's API exposes no artifact digest, so a GitLab
verdict records no artifact. A GitLab gate of a target on another machine is watched
on that machine like a GitHub one (HS2-2222QP): the hosted `target-gates` route takes
`provider` (absent means `github-actions`), and the host's repository check compares
the full project path with its checkout's remotes. Other providers are HS2-17HVNP.

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
`gate_provider_unavailable`, `evidence_missing`, `evidence_not_covering`,
`deployment_executor_unavailable`, `deployment_start_unconfirmed`,
`gate_deployment_script_untracked` and `execution_lost` (HS2-G3VAK4).

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
with a link, a deployment request, or a subscriber of the event stream, acts as
the linking actor, which only observes and never asks, and confirms that every ten
minutes. A
headless caller uses `gate sync --wait-seconds`, which waits the same way. Nothing
polls on a timer. Reads answer from this machine's records and contact nothing.

For a target on this machine, the watch runs here with this machine's GitHub
credentials. For a target bound to another machine, it runs on that machine (see
"Watched where the target lives" below). A watch only advances while the machine's Job
monitor ([33](33-jobs.md)) runs, so whoever links or follows a gate makes sure of it
(HS2-VVHVF0):

- `gate link` through the CLI, the server or MCP starts the detached monitor when no
  monitor holds the machine's slot and the gate still waits. A settled gate starts
  nothing. The answer's `watching` says whether a monitor runs afterwards.
- `gate sync` does the same before it waits, and so does each step of the server's
  gate follower. That check is a lock probe made while the step is awake anyway: a
  running monitor is found without launching anything, and a stopped, crashed or idle
  one is started again at the next step. A start that fails is retried after 30
  seconds, doubling up to 15 minutes.
- A server that starts follows every owner with a gate still waiting on a provider,
  also when the gate's run is finished, so a watch left behind by a restart is
  observed again with nobody asking.
- All three call one workflow, `DetachedJobMonitor` in the shared runtime, which `job
watch start` uses too. `gate link --no-monitor` starts none.
  `HOTSHEET_JOB_MONITOR_AUTOSTART=off` keeps a process from starting one on its own;
  the repository's test configuration sets it. A test build (the isolated
  `test-permission-vault` artifact that browser and script fixtures launch outside
  Cargo) starts none unless the variable is `on`, because those fixtures inherit the
  developer's environment and a monitor asks `gh` for a real token.

`authorize` reads the target again, up to three times, when its resolution lost a
race to a concurrent one, so a follower confirming its authority does not turn a
simultaneous request into `target_unavailable`.

### Deployments

`gate deploy` asks for a deployment of the commit the run's CI gate passed. The
request names the environment, the target's checked-in script as a path relative to
its repository root, and an operation id.

- **Precondition.** Every standing CI gate of the run passed, for one commit.
  Otherwise the request is refused with `gate_not_passed` and nobody is asked.
- **Authority.** It passes the gate as `deployments_run` under its operation id and
  asks a person unless a remembered rule allows it (§26.7).
- **A person for production.** A request marked `production` cannot be undone, so it
  asks a person every time: no rule covers it and Always Allow is refused for it.
  While waiting, the gate is `awaiting_approval` and the answer carries the
  `request_id`. Repeating the request after the answer moves it on; a denial ends it
  as `failed` with `permission_denied`. An Allow Once is used once.
- **Idempotent.** The same operation id returns the same gate, asks nobody again
  and starts nothing twice. The same id for other content conflicts. An environment
  with a deployment in progress refuses a second with `gate_deployment_active`.
- **Result.** Only a deployment adapter's typed evidence settles the gate.

### The target's own deploy script (HS2-G3VAK4)

The `target-script` provider runs the target's checked-in deploy script on the
machine that hosts the target, never from the orchestrating machine's checkout and
never from anyone's working tree. Once `gate_deploy` passed the permission gate, the
orchestrator sends the deployment to the target's destination (the same machine
for a local target, the reviewed transport for a remote one):

- **Hosted half.** `POST /checkouts/{checkout}/target-deployments` with
  `{execution_id, commit, script, environment, wait_ms}`, where `execution_id` is
  the gate's id. The host checks that the commit exists in the hosted checkout and
  that the script is tracked there as an executable file (mode `100755`); anything
  else is refused with `gate_deployment_script_untracked` (409) and nothing runs. It
  checks the commit out into a private detached worktree under
  `${HOTSHEET_HOME}/orchestration-deployments/`, runs `<script> <environment>` in it
  as a hosted execution and removes the worktree when it ends. A hosted execution
  is the same as a validation step's: the step environment of §26.12 (an allowlist
  plus its ids, never the orchestrating machine's credentials), its own process
  group, a detached runner where available, at most an hour, recorded once under
  the gate's id so a repeat never runs it twice. No copy of its output is kept: a
  deployment's output may name secrets. The script uses the credentials the hosting
  machine's own user has (for example a cloud CLI's login under `HOME`).
  `GET ...?execution_id=&wait_ms=` reads it, a long poll of at most 8 seconds.
- **Gate.** Running is `pending`; exit 0 is `passed` and anything else `failed`
  (reason `deployment_<end>`), each with evidence of the provider, the execution id,
  the exit code, `duration_ms`, the deployed commit (exact: the worktree held that
  commit) and the artifact the deployment was authorized for. A refused script is
  `unknown` with `gate_deployment_script_untracked`; a runner that died without an
  end is `unknown` with `execution_lost`. Gate sync and the follower read it as
  they read a hosted CI gate.
- **Repeats.** The same request again asks nobody and starts nothing. A start the
  host did not confirm (unreachable) is `unknown` with
  `deployment_start_unconfirmed`, and the same request sends it again as a
  continuation of the decision already made, so nobody is asked twice.
- A provider name no adapter serves is still recorded as `unknown` with
  `deployment_executor_unavailable`: allowed, and not run. Run reconciliation reads
  a hosted deployment from its host (§26.14, HS2-WZ0NKH). Per-target secret variables
  for the script are not provided.

### Watched where the target lives (HS2-WDQZ37)

The target's server is authoritative for its own CI evidence (§26.1). When a run's
target is bound to another machine, a `github-actions` CI gate is watched there, with
that machine's provider credentials and its Job monitor; the orchestrating machine
holds no provider credentials for it and keeps only the typed readings it is answered.

- **Hosted half.** `POST /checkouts/{checkout}/target-gates` with `{commit,
repository, workflow, branch, again}` starts or finds the watch Job in the hosting
  machine's Job store (idempotent, like a local link; it is tied to no ticket there)
  and answers `{state, reason, evidence, watch_ref, cursor, watching}`.
  `GET /checkouts/{checkout}/target-gates?commit=&repository=&workflow=&branch=&after=&wait_ms=`
  reads it: with `after` equal to the current cursor (the host's Job store revision)
  it parks until the host's watch records change or at most 8 seconds pass
  (`MAX_HOSTED_GATE_WAIT_MS`, under the transport's read timeout); an older cursor is
  answered at once, so a reconnect misses nothing. Neither starts anything but the
  watch and its observer. Both are ordinary authenticated project data: they only
  observe, and the orchestrating machine passed its own gate (`gates_observe`) first. The watch
  uses the hosting machine's provider credentials, so it is only for a repository
  the hosted checkout's own Git remotes name (`owner/name`, any URL spelling, case
  ignored). Any other is refused with `gate_repository_mismatch` (409) before
  anything starts, and the refusal is recorded in that checkout's audit journal on
  the host as `hosted_gate_watch` `denied` (HS2-K2YE6A); the orchestrating machine
  answers its link with the same code.
- **Orchestrating half.** `gate link` asks the host to watch; `gate sync` and the gate
  follower read through the hosted route and wait on the host's long poll instead of
  the local Job store. A reading is recorded exactly as a local adapter reading is
  (same transitions, events and first-seen time). A host that answers `not_watched`
  (a gate linked before the host watched it) is asked to watch it.
- **Fallbacks.** A peer without the route (an older server) leaves the gate to this
  machine's adapter, as before. A peer that cannot be reached or refuses leaves the
  gate as it stands: a sync returns it unchanged, and the follower spends the rest
  of its wait on this machine's records instead of asking again at once. A host that
  answers a wait at once with nothing new is treated the same way.
- Deployment gates of the `target-script` provider are always run and read on the
  target's machine, local or remote (HS2-G3VAK4, below).

### Artifact identity (HS2-3YC577)

A green CI verdict records what the run built. When the `github-actions` watch
source judges a run green, it reads that run's artifact list once per run and
attempt (conditionally, by ETag) and keeps the unexpired artifacts that carry a
`sha256:` digest. The watch Job's outcome records `artifact_count`, and
`artifact_name` with `artifact_digest` (lowercase hex, without the prefix) when
exactly one artifact qualifies. Several artifacts record none, because nothing says
which one a deployment ships, unless the link names one: a CI gate's scope may carry
an `artifact` name (`gate link --artifact <name>`, MCP `ci_artifact`; HS2-TQG6AT),
which travels in the watch Job's external reference (a sixth part, so earlier
references keep their spelling) and on the hosted `target-gates` route. The verdict
then records exactly the artifact of that name, or none when the run did not build
it. Another artifact of the same commit is a gate of its own, as another workflow
would be; linking the same name again returns the same gate. An artifact
list GitHub refuses (403) or does not have (404) leaves the verdict standing without
an artifact; any other failure is retried with the watch's backoff before the
verdict is reported. A red or cancelled run reads no artifacts.

The adapter puts that artifact into the CI gate's evidence when its name and digest
are ones a gate record holds (an identifier and a SHA-256), and leaves it out
otherwise, so an unusual provider name never fails the reading. A deployment request
that names an `artifact` is then checked against the passed CI gates of the run:
when any of them records an artifact, the request's must equal one of them, name and
digest, or the request is refused with `gate_artifact_mismatch` (409) before anyone
is asked and before any gate is recorded; the deployment then rests on the gate that
built it (HS2-TQG6AT). When no passed gate records one, or the request names none,
the request is decided as before.

### Headless surface

```sh
hotsheet-cli ... orchestration gate link <run-id> --project <owner> --commit <40-hex> [--repo owner/name] [--workflow CI] [--branch main] [--artifact <name>]
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
`deployment_request`. Answering a deployment's permission request is a person's act
in a client, the CLI or HTTP (§26.7).

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

| Source                | What is compared                                            | Reported today   |
| --------------------- | ----------------------------------------------------------- | ---------------- |
| `target_reachability` | Whether the target answered at all                          | Yes              |
| `target_ticket`       | The target ticket's existence and status                    | Yes              |
| `target_claim`        | Who holds a live claim, against the run's state and worker  | Yes              |
| `run_lease`           | The lease the run recorded, against the lease on the ticket | Yes              |
| `ci_run`              | CI runs gating the run                                      | Yes (HS2-7QNK7Y) |
| `deployment`          | Deployments started for the run                             | Yes (HS2-BYEB60) |
| `validation`          | Validation attempts of the run on the hosts of their steps  | Yes (HS2-G5H9FQ) |
| `session`             | Orchestrator-owned sessions working the run                 | Yes (HS2-75Q254) |

When the target cannot be confirmed, all four target sources are `unverifiable` with
the reason the read failed for, such as `target_unavailable`. The other four sources
are this machine's records and the records of the machines that host the work, so they
are answered whether or not the target ticket can be read. They take the same typed
input (what the run's records say is in flight, and what is live) and classify it the
same way: the same work is `verified`, live work the run never recorded is
`unrecorded_live_work`, recorded work that is gone is `recorded_work_missing`, and two
different pieces are `different_live_work`. Every source is reported since HS2-75Q254;
the reason `source_not_wired` remains for a source a future version adds before it is
read, and it does not hold a run back, because the run records no such work. Any other
unverifiable reason does. A verified finding may carry a reason too: a code that says
what the verified answer rests on, such as `no_gate`.

### CI runs (HS2-7QNK7Y)

The CI source compares the run's standing CI gates (§26.13) with what the CI
adapter's own records say about each, for GitHub Actions the watch Jobs. It reads
this machine's records only and contacts no provider, so it is answered whether or not
the target is reachable.

| A standing CI gate                                  | The adapter shows                                             | Finding                                                                    |
| --------------------------------------------------- | ------------------------------------------------------------- | -------------------------------------------------------------------------- |
| None linked                                         | -                                                             | `verified`, reason `no_gate`                                               |
| `passed`                                            | Not read                                                      | `verified`                                                                 |
| Waiting (`pending`, or `unknown` with `watch_lost`) | The same watch, still going                                   | `verified`                                                                 |
| Any                                                 | Another live watch, or one the record did not name            | `verified`, reason `watch_adopted`                                         |
| Waiting                                             | A verdict, or a watch stopped on purpose                      | `verified`: the work ended in the open                                     |
| Waiting                                             | No watch at all                                               | `mismatched`, `recorded_work_missing`                                      |
| Waiting                                             | A watch whose observer was lost                               | `unverifiable`, `watch_lost`                                               |
| Any but `passed`                                    | Its records cannot be read, or no adapter serves the provider | `unverifiable`, `gate_provider_unavailable` or `gate_provider_unsupported` |

Several gates are combined: any unverifiable gate makes the source unverifiable, then
a missing watch outranks a live one. Reconciliation records nothing about the gate
itself; the gate follower or the next `gate sync` brings the gate in line with the
same reading. A live watch makes `may_start(ci_run)` false, and `gate link` never
starts a second watch while one is live: the watch is found or reattached by its
target, and only a finished one is watched again.

### Deployments (HS2-BYEB60)

The deployment source reads the run's standing deployment gates by their retained
operation ids. A `target-script` deployment runs on the target's machine (§26.13,
HS2-G3VAK4), and reconciliation asks that machine (HS2-WZ0NKH): for a gate that is
`pending`, or `unknown` with `deployment_starting` or `deployment_start_unconfirmed`, it
reads the execution (`GET target-deployments`, no wait, starting nothing) under a
continuing `deployments_run` gate, so nobody is asked again. Still running is live work
under the operation id; an end the host recorded is written to the gate as a sync
would write it; an execution the host does not know is `recorded_work_missing`; a host
that cannot be asked leaves the source unverifiable with its code. For a provider no
adapter serves, the only thing reconciliation can verify is that nothing was executed,
and it says so in the finding's reason:

| A standing deployment gate                                 | Finding                                      |
| ---------------------------------------------------------- | -------------------------------------------- |
| None requested                                             | `verified`, reason `no_deployment`           |
| `awaiting_approval`                                        | `verified`, reason `awaiting_approval`       |
| `unknown` with `deployment_executor_unavailable` (allowed) | `verified`, reason `deployment_not_executed` |
| `passed`, `failed`, or withdrawn                           | `verified`                                   |

`verified` here means: this machine's records show that no execution was started for
the operation, and no executor exists on this machine that could have started one. It
does not mean anything was deployed.

A gate recorded as executing (`pending`, or `unknown` with `deployment_starting`) can
only come from a registered executor adapter, and only that adapter can say what
became of it. Its own records are read by the gate, never the provider:

| The executor answers                               | Finding                                                               |
| -------------------------------------------------- | --------------------------------------------------------------------- |
| The execution is going on                          | `verified`; `may_start(deployment)` is false                          |
| A verdict                                          | `verified`: the execution ended                                       |
| `unknown` with `execution_not_found`               | `mismatched`, `recorded_work_missing`                                 |
| Any other `unknown`, or its records cannot be read | `unverifiable` with its code (`deployment_state_unknown` without one) |
| No executor is registered for the provider         | `unverifiable`, `deployment_executor_unavailable`                     |

A retry after an interruption never starts a duplicate: `gate deploy` with the same
operation id returns the existing gate and starts nothing once it left
`awaiting_approval`, and a live execution is adopted, not replaced.

### Validation attempts (HS2-G5H9FQ)

An interrupted validation step is a source of its own, `validation`, rather than part
of the run record: its attempt runs on the machine that hosts the step's target
(§26.12), and only that machine can say whether the execution still exists. The run
record only names the run; the attempts that name it live in
`orchestration-validation.json`.

Reconciliation reads every retained attempt that names the run. A finished attempt is
settled. For each attempt still `running` it asks the hosting machine about that
execution id, with no wait, under a continuing `validation_run` gate as the run's
supervisor, so nobody is asked again. It never asks the host to start anything.

| The host answers                                   | Recorded                                        | Finding                                   |
| -------------------------------------------------- | ----------------------------------------------- | ----------------------------------------- |
| No attempt names the run                           | -                                               | `verified`, reason `no_validation`        |
| The execution is still running                     | Nothing; it stays `running`                     | `verified`; `may_start(validation)` false |
| It finished                                        | Its outcome, as a continuing request would      | `verified`                                |
| It does not know an execution it had confirmed     | `unverified`, `execution_lost`; never run again | `verified`, reason `execution_lost`       |
| It does not know one it never confirmed            | Nothing; it stays `running`                     | `unverifiable`, `execution_unconfirmed`   |
| The same, after the attempt's deadline             | `unverified`, `deadline_passed`                 | `verified`, reason `deadline_passed`      |
| It cannot be reached, or the gate refuses the read | Nothing; it stays `running`                     | `unverifiable` with the code              |

A lost execution is `verified` because the record now says what happened: the attempt
is `unverified`, its evidence counts as unverified for readiness (§26.15), and a new
operation id runs the step again. An `unverified` attempt keeps its reason as the
finding's reason on every later reconciliation. A repeated step request with the same
operation id still returns or continues the same attempt (§26.12); reconciliation
never creates one.

### Sessions (HS2-75Q254)

The session source reads the orchestrator-owned sessions started for the run
(§26.16). Each active one is first read from its host with the same safe-point read the
scheduler makes, as the run's supervisor. That brings the record in line: a session
that crashed or died with its server becomes `lost`, and a person's takeover at the
host is learned. A host that cannot be asked leaves the source `unverifiable` with
`session_unknown`, or the refusal's code.

The run's latest session (in the order they were started) is then judged against
whether the run needs a live session now. A dispatched run waits, and needs none,
while one of its CI or deployment gates waits on a provider or a person, while its
target ticket is in `final_testing`, or while a validation attempt of it runs. An
intervened run waits the same way: a session that dies releases its worker's claim, so
its run may read `intervened` before reconciliation looks (HS2-N6ZATC). A run that is
queued, requested or taken over and waits on nothing is not being worked.

| The run's latest session                    | The run                         | Finding                                                         |
| ------------------------------------------- | ------------------------------- | --------------------------------------------------------------- |
| None                                        | Any                             | `verified`, reason `no_session`                                 |
| `running`                                   | Any                             | `verified`; adopted, `may_start` false                          |
| `stopped` (wound down, or by a person)      | Any                             | `verified`, reason `session_stopped`                            |
| `requested`, never confirmed by its host    | Any                             | `verified`, reason `start_unconfirmed`                          |
| `failed`                                    | Any                             | `verified`                                                      |
| `lost`                                      | Being worked                    | `mismatched`, `recorded_work_missing`                           |
| `lost`                                      | Waiting on a gate or validation | `verified`, reason `waiting_on_gate` or `waiting_on_validation` |
| `lost`                                      | Not being worked                | `verified`, reason `session_lost`                               |
| Any, while an older session of the run runs | Any                             | `mismatched`, `different_live_work` or `unrecorded_live_work`   |

A deliberate wind-down is therefore never read as lost work, which the scheduler's
wind-down of a session whose run waits on CI (§26.19) relies on.

**Adopt or wind down.** A surviving session is adopted: the run continues with it and
nothing is started. When reconciliation finds the run finished, its orchestrator-owned
sessions are left to the scheduler if the policy sets `placement.automatic` (its idle
wind-down applies `idle_minutes` and `keep_warm`); otherwise nobody else would stop
them, so each is asked to stop at a safe point its host confirms. One that is not at a
safe point keeps running, and a person's session is never touched.

**Never a duplicate.** `may_start(session)` is false while a session runs, and
`run_session_holder(owner, run_id)` names the session that still holds a run's work.
A start or a refill for a run it names is refused, and the scheduler adopts the
holder instead (HS2-DG0YPA, §26.24).

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
shown by `run status`.

**What the scheduler does with it (HS2-AH8CRS).** Reconciliation itself starts no
work. An applied scheduler pass (§26.17) acts on the stored disposition of every
unfinished run, decided by the pure `disposition_action`:

- `retry_dispatch` on a run still `requested` and not paused: the dispatch is
  repeated with the run's retained operation id, ticket, target, mode and worker. It
  passes the gate as `work_start` like any dispatch (§26.7), so a remembered rule or
  an Allow Once allows it and otherwise a person is asked. It is tried at most three
  times for one stored outcome; a new reconciliation outcome allows it again.
- `needs_attention`: one `FEEDBACK NEEDED` note on the run's goal ticket per stored
  outcome, by identifiers and codes (the run, the target, the state and the findings),
  pointing at `run status` and the run's controls. Nothing is decided for the person;
  the note is recorded in the decision ledger so a later pass does not repeat it.
- `paused` (or a paused run whatever its outcome) and `hold_unverified`: left alone.
- `wait_for_dispatcher`, `continue`, `retry_cancel` and `finished`: nothing to do;
  reconciliation repeats a cancel itself.

Each act is a decision-log record of kind `reconcile` (the run, `redispatch` or
`needs_attention`, the outcome such as `queued`, `dispatched`, `asked` or a refusal
code, and the request id a refused dispatch waits on). A dry run acts on nothing. The answer's `unverified` lists exactly the sources that are
`unverifiable`. An unchanged outcome is not rewritten and produces no event; a new
one is a `reconciled` event whose reason is the disposition. A finished run keeps no
outcome, and neither does a run whose state changed since (a sync that moved it
from `queued` to `dispatched`, for example): the outcome was assessed on the earlier
state, so `run status` shows none until the next reconciliation stores the one for
the new state (HS2-68DWAM). A target that cannot be confirmed is part of the answer,
not an error.

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

**One operation at a time per run (HS2-EZV83C).** A handoff releases one claim and
takes another before it records anything, and a cancel releases the claim before it
records `cancelled`. Each claim change wakes the follower, whose sync could read the
target in between and record the orchestrator's own change as a person's intervention
(`dispatched -> intervened -> dispatched`). So every operation that reads a run, acts
on its target and records the result (sync, dispatch, renew, cancel, handoff, pause,
resume, correct) holds that run's operation lock throughout
(`RunRegistry::operation_lock`: one of 64 striped lock files under
`orchestration-run-operations/` in the machine home, shared by every process and
reentrant on one thread). A sync that waited reads the run again under the lock, so it
aligns with the finished operation, never with the record it read before. It takes
the lock before it answers for a finished run too: a run's record is written before its
events, so a sync that found the run completed by the follower's sync answered while
the `state completed` event and the readiness it moves were still unpublished
(HS2-RZQFG1). A resume
releases the lock before its reconciliation, and no locked section takes another
run's lock.

Reconciliation and handoff pass the gate (§26.7) as `tickets_read` and a continuing
`work_start`. Pause, resume and correct change only the orchestration project's own
record and must work while the target is unreachable, so they do not need a fresh
target resolution and never ask: they require an identified actor, are written to the
audit journal as `run_pause`, `run_resume` and `run_correct`, and a correction is
refused for an AI actor (HS2-BJE1C1, settled by HS2-XHQ1EH).

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
scheduler's (HS2-AH8CRS), and the workspace view is HS2-XM692S.

## 26.15 Prerequisites and readiness (HS2-42TARA)

Orchestrated work across projects has an order: a change in one target must be
finished, and its gates green, before work in another starts on top of it. The
**plan** records that order, checked in with the orchestration project so every
machine that orchestrates it plans the same steps (HS2-YDY5HJ), and **readiness** is
computed on each machine from what that machine has confirmed. Nothing is written to a target, and
no provider's own dependency or blocking field is read or changed: a GitHub, Jira,
GitLab or git ticket keeps its own semantics.

### Plan steps and prerequisites

A **step** is one orchestration ticket's work on one exact target ticket,
`(target_id, connection_id, native_id)`. A target ticket has at most one step per
owning checkout; planning it again for the same orchestration ticket changes that
step, and for another orchestration ticket conflicts. The step id is a ULID the
plan assigns. A step holds at most 16 **prerequisites**, each named once:

| Kind         | Fields                                                                | Holds when                                                                          |
| ------------ | --------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| `ticket`     | `target_id`, `connection_id`, `native_id`, `gates`, optional `commit` | That qualified target ticket is confirmed finished and the required gates are green |
| `validation` | `step_id`, optional orchestration `ticket` (the step's own otherwise) | The latest evidence of that validation step (§26.12) is `passed`                    |

`gates` is `ci` when omitted (every standing CI gate passed), `deployment` (CI and
the requested deployment passed) or `none` (the ticket being finished is enough).
The gates are those of the latest run on the prerequisite ticket (§26.13), or of
`commit` when the prerequisite pins one. Superseded gates count for nothing, as in
every gate roll-up.

A prerequisite on a ticket that another step works is an edge of the plan graph, in
whatever target that ticket lives, so a cycle across projects is found. A
prerequisite on a ticket no step works is a plain condition. Defining a step reads
every ticket it names through the gate as `tickets_read` and stores the provider's
canonical ids, so a slug is only a label; a ticket that cannot be read is not
planned. The step and those reads are stored in one write, so no evaluation sees a new
step before its tickets are observed and publishes it as `stale` (HS2-E7HR4R). A step
cannot require its own ticket. An owner plans at most 256 steps.

### Where the plan lives (HS2-YDY5HJ)

The plan has two halves, kept apart so that intent is shared and no machine's facts
cross:

| Half                      | File                                                              | Holds                                                                                                        |
| ------------------------- | ----------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| Step definitions (intent) | `.hotsheet2/orchestration-plan.json` in the orchestration project | Each step: orchestration ticket, target ticket, prerequisites, resources, preferred tool and model, revision |
| Observations and levels   | `${HOTSHEET_HOME}/orchestration-plan.json` on each machine        | The last read of each ticket the steps name and the level this machine last published per step               |

The checked-in file sits beside the target manifest and the policy and follows their
pattern: version 1, at most 4 MiB, replaced under a lock with a fresh document
`revision` on every change, never created by a read. A shared step carries no owner
checkout id, because a checkout id is a per-machine path identity: each machine reads
the steps as its own checkout's. The machine-local file (private, locked, atomically
replaced, version 1, at most 8 MiB) keeps the reads and levels by owner and step id,
beside the runs, gates and validation records readiness is also computed from.
Damaged or newer data in either file, for example merge markers left by a text merge,
is an error that is never overwritten.

**Merging.** Every change is a read-modify-write of the checked-in file under its
lock, merged by step id: a step on one side only is kept and an identical step is
kept; the same step id with a different definition, or one target ticket under two
step ids, is refused with 409 `plan_conflict` and nothing is written. A conflict is
never guessed. `define` takes an optional `expected_revision`, the revision of the
ticket's step the caller read (0 when none was planned): a step changed since, on this
machine or another, is refused with `plan_conflict`. Without it the change applies to
the step as it is now. Removing a step removes it from the checked-in file; each
machine forgets its own reads and level of a step that is gone at its next write.

**Readiness stays local.** A machine that reads a checked-in step it never observed
evaluates it from its own records only, so the step is `stale` there until that
machine reads the tickets itself; its published levels and readiness events are its
own. Runs, gates and validation evidence never cross machines.

**Migration.** A plan kept on a machine before HS2-YDY5HJ still has its step
definitions in the machine-local file. Reads merge them with the checked-in steps (a
conflict is refused); the next change, or `readiness migrate`, moves them into the
checked-in file and keeps their published levels local. Migration is idempotent.
When a local definition conflicts with a checked-in one it is refused and nothing
moves; a person may pass `--discard-local` (`discard_local`) to drop exactly the
conflicting local definitions, which an AI caller may not.

```sh
hotsheet-cli ... orchestration readiness migrate --project <owner> [--discard-local]
hotsheet-cli ... orchestration readiness define ... --expected-revision <n>
```

A step may also name up to four **resources** its session must hold (§26.20). They
are not prerequisites: readiness ignores them, and the scheduler waits on them.

### Levels

Evaluation is a pure function
(`crates/hotsheet-ticketing/src/orchestration/readiness.rs`) over the steps and typed
facts: per ticket the last confirmed read and whether it is stale, the tickets an
unfinished run stands on, rolled-up gates, and validation evidence. The same steps
and facts give the same answer in any order.

| Level         | Meaning                                                                                   | Dispatched         |
| ------------- | ----------------------------------------------------------------------------------------- | ------------------ |
| `confirmed`   | Every prerequisite is confirmed. The default level for starting work                      | Yes                |
| `provisional` | Nothing is against it, but a prerequisite rests on an integrated commit not yet confirmed | By policy (§26.22) |
| `blocked`     | A prerequisite is confirmed not to hold                                                   | Only by override   |
| `stale`       | Nothing is known against it, but something it rests on could not be confirmed             | Only by override   |
| `cyclic`      | Its prerequisites lead back to itself                                                     | Only by override   |
| `in_progress` | An unfinished run already stands on its ticket                                            | Existing run rules |
| `finished`    | Its own ticket is confirmed finished                                                      | Nothing to start   |

A step's own state is decided first: an unfinished run, then a confirmed finished
ticket, then a cycle, then its own ticket being unread, unreachable or missing. After
that the worst prerequisite sets the level (`blocked`, then `stale`, then
`provisional`) and every prerequisite that does not hold is listed as a reason: a
stable code, the prerequisite, and for a provisional one the commits it rests on.

- **Confirmed target state.** A ticket counts as finished when a successful read
  showed it `completed`, `verified` or archived. The read is the newest of the plan's
  own and the last confirmed observation of any run on that ticket (§26.9). While
  the target cannot be confirmed, or before any read, the prerequisite is `stale`
  (`target_unreachable`, `not_observed`) even when the last read said finished. A
  ticket the target no longer has is `blocked` (`prerequisite_ticket_missing`).
- **Gates.** `failed` blocks (`gate_failed`). No standing gate blocks a finished
  ticket (`gate_not_linked`), and an `unknown` gate makes it stale (`gate_unknown`).
  An unfinished ticket with nothing integrated is `prerequisite_not_finished`.
  Deployments: `deployment_not_requested`, `deployment_pending`, `deployment_failed`
  block and `deployment_unknown` is stale. Whether a pending deployment should be
  provisional is HS2-QBERH6.
- **Provisional.** A CI gate that is `pending` on a linked commit means the
  prerequisite is integrated and awaiting its gate (`gate_pending`); a gate that
  passed while the ticket is not yet confirmed finished is the same level
  (`prerequisite_not_finished`). `base_commits` records the commits such a step would
  be built on. This is the provisional level of §26.8: it is a distinct computed
  answer, listed by `ready` beside confirmed work. Whether it is dispatched is the
  policy's decision: §26.22 across targets and HS2-2SJPCH within a project. Without
  the opt-in it is not.
- **Validation.** `validation_step_missing`, `validation_not_run`,
  `validation_step_changed`, `validation_failed`, `validation_running`,
  `validation_skipped`, `validation_unavailable` and `validation_cancelled` block; other unverified evidence
  is stale (`validation_unverified`).
- **Cycles.** Every step of a cycle is `cyclic` (`plan_cycle`) with the sorted step
  ids of its cycle, whatever the targets say: the plan is wrong and nothing in it
  starts by itself. A step outside the cycle whose prerequisite is inside it is
  blocked with `prerequisite_in_cycle`. Changing one prerequisite dissolves it.

Each step also lists `unblocks`: the unfinished steps that name its ticket.

### Dispatch guard and override

`run dispatch` (§26.9) reads the target ticket, records that read as the step's own
confirmed state, and refuses a planned ticket whose step is not `confirmed` with
409 `readiness_blocked`, `readiness_stale`, `readiness_cyclic` or
`readiness_provisional`. Nothing is claimed and no run is recorded. A provisional
step is the one level a policy can let through (§26.22). Repeating a
dispatch that was already admitted is the same run and is not judged again.

A person can let one exact operation through. A person's dispatch sent with
`override_readiness` is their own decision and proceeds. When an AI session asks
for it, the override is a judgment call (§26.7, HS2-XHQ1EH): the orchestration
ticket gets one `FEEDBACK NEEDED` note with the `CHOICE` options "Start anyway" and
"Wait until it is ready", and the dispatch answers 403 `decision_required` with its
`note_id` until a person responds. "Start anyway" lets that operation proceed;
"Wait until it is ready" answers `readiness_override_denied` from then on. Work that
is ready needs and asks nothing. The audit journal records each answer under
`readiness_override` with the operation id and the level that was overridden (or
`decision_required`, `decision_wait`), and the orchestration ticket gets an activity
note naming the step, the level, the operation and who decided, by identifiers only.
The scheduler never sets the flag.

### Reevaluation and events

Readiness is recomputed where the things it reads change: wherever a run event is
published (state, observation, target unreachable or reachable again, recovery),
wherever a gate changes, when a validation attempt outside any run starts or ends,
and after a plan change, a refresh or a dispatch's own read. A step whose level
differs from the one last published gets one `readiness` event in the run event
stream of §26.9. Its `reason` is the new level. A step may have no run, so its
readiness and provisional events name it in their own `step_id` field and carry no
`run_id`, which stays for runs only (HS2-J7RG42). `run events --run <step-id>` replays
one step's levels: the filter takes a run or a step. Entries written before the field
existed carried the step in `run_id`; they read as step events. Repeated evaluation publishes nothing,
concurrent evaluations publish a change once, and a level reached again later is its
own event. Removing a step publishes one last `readiness` event for it, with reason
`removed` (HS2-SQS266, §26.24). The scheduler (HS2-ECWG50) waits on that stream;
nothing polls.

`readiness refresh` reads the named tickets again through the gate and lists under
`unverified` those it could not confirm. A prerequisite ticket no run stands on is
also followed on its target's change feed (HS2-TMA4YA, §26.9): its change is read
again through the gate and the level it changes is published, with nobody asking for
a refresh.

**Changes that only time causes (HS2-J7N3PQ).** One level changes by time alone: a
running validation attempt that outlives its deadline is `unverified`, so a step that
waits on it becomes `stale`. Every evaluation of an owner's plan (a read, or an event
that reevaluates it) arms one timer for the first instant one of the owner's running
attempts passes its deadline (`next_lapse_ms`). It is a one-shot wake-up at a known
time, never an interval, and an earlier deadline replaces a later armed one. When it
fires the plan is evaluated at that instant, which publishes the changed level as a
`readiness` event (with that instant as its time) and arms the next deadline, if any. An
attempt that finishes first makes the wake-up publish nothing. A timer lives as long as
its process; after a restart the next evaluation of the plan arms it again.

### Reads for the scheduler

`OrchestrationRuntime::ready_work(owner_id, tx) -> Result<Vec<ReadyItem>, OrchestrationError>`
lists the work that could start now, confirmed items first and then provisional ones,
each by step id. A `ReadyItem` holds the step id, the orchestration ticket, the
target ticket, the `level`, the `reasons` and `base_commits` of a provisional item,
what it `unblocks`, when it became ready, and the target ticket's own `priority`,
`up_next` mark and creation time as last read. `readiness(owner_id, step_id, tx)`
answers one step and `readiness_plan(owner_id, tx)` every step with counts per level
and each cycle once. All three read this machine's records, pass no gate, contact no
target and write nothing.

### Headless surface

```sh
hotsheet-cli ... orchestration readiness define --project <owner> --ticket <orchestration-ticket> \
  --target <target-id> --connection <source> --id <target-ticket> \
  [--requires <target-id>/<connection>:<ticket>]... [--gates none|ci|deployment] \
  [--requires-validation <step>]... [--prerequisite '<json>']...
hotsheet-cli ... orchestration readiness show <step-id> --project <owner>
hotsheet-cli ... orchestration readiness plan|ready --project <owner>
hotsheet-cli ... orchestration readiness refresh [<step-id>] --project <owner>
hotsheet-cli ... orchestration readiness remove <step-id> --project <owner>
hotsheet-cli ... orchestration run dispatch ... --override-readiness
```

`--gates` applies to every `--requires` of the command; `--prerequisite` takes one
prerequisite as JSON for what the flags do not say, such as a pinned `commit`.

| Route                                                                                | Methods and purpose                               |
| ------------------------------------------------------------------------------------ | ------------------------------------------------- |
| `/checkouts/{owner}/orchestration/readiness/{define,show,plan,ready,refresh,remove}` | POST plan and readiness operations; machine-local |

MCP `hotsheet_targets` adds `readiness_define`, `readiness_show`, `readiness_plan`,
`readiness_ready`, `readiness_refresh` and `readiness_remove`, and `run_dispatch`
takes `override_readiness`. Plan steps are the orchestration project's own records: defining, refreshing and
removing one reads the target at most and never asks. Removing a step needs an
identified actor and is audited (`readiness_remove`; HS2-TZT505, settled by
HS2-XHQ1EH). It cancels the step's unfinished scheduler runs first, as the remover;
cancelling reduces activity and asks no one (HS2-YFPZFP, §26.24).

Milestones and wider cross-project planning stay with their own tickets: the
milestone model (HS2-8T71YW, HS2-A820C1, HS2-PX50E8, HS2-6E0H3X), cross-project
planning (HS2-HX5MH3) and aggregate views (HS2-42B2FD). A milestone as a prerequisite
kind is HS2-2CR7H3, and the workspace view of the plan is HS2-15G5T4.

## 26.16 Orchestrator-owned sessions (HS2-GDJXGZ)

The orchestrator starts AI sessions on the servers that host its targets, lists the
ones it owns, asks whether one may be wound down, and stops it. This section is those
four operations and what a person can do to any such session. Deciding how many
sessions run where is the scheduler's (HS2-ECWG50).

A session is started by the machine that hosts the target, through the same AI
terminal launch its bottom drawer uses: the plugin's interactive launch with the
chosen tool, model and effort, in the target checkout. It is not a second launch
path. The session therefore appears in that server's drawer like any other AI
terminal, with the permission routing, worker id and claim release every AI terminal
has ([05](05-ai-tool-plugins.md) §5.4).

### Session record and states

The orchestrating machine keeps its sessions in
`${HOTSHEET_HOME}/orchestration-sessions.json` (mode 0600, version 1, at most 1,024;
the oldest finished session is forgotten first and an active one never is). A session
has a `session_id`, the caller's `operation_id`, the target, the `role`, the tool,
model, effort and account, the run it was started for, the session it `replaces`, an
`owner`, a `paused` flag, where its host says it lives (`hosted`: the server id, the
terminal id and the worker id its claims carry), and a history of at most 32
transitions that always keeps the first. History holds times, states, reason codes and
actor ids only.

| State       | Meaning                                                     | May become                     |
| ----------- | ----------------------------------------------------------- | ------------------------------ |
| `requested` | Recorded before the host was asked                          | `running`, `stopped`, `failed` |
| `running`   | The host started it                                         | `stopped`, `lost`              |
| `stopped`   | Wound down at a safe point, or stopped by a person          | nothing                        |
| `failed`    | The host refused the start, or it could not be made         | nothing                        |
| `lost`      | The host no longer has it and nobody stopped it: it crashed | nothing                        |

Every other edge is refused with `session_invalid_transition`, and a finished session
refuses any change with `session_not_active`. A finished session never restarts. Its
replacement is a new session with its own operation id that names it in `replaces`,
and only a session that is gone can be replaced.

`requested` and `running` sessions count as load for the policy's caps (§26.8): on
the target, on the target's machine, on the tool and on the account. A session a
person took over still runs, so it still counts until it stops.

### Starting

1. The request is checked against this machine's records first. The same operation id
   returns the same session, whatever became of it, and never launches twice. The same
   id for another target, tool, model, effort, role, account or run is a conflict. A
   target has at most one active `dispatcher` and one active `integrator` session
   (`session_role_active`), and a run at most one live session
   (`session_run_held`, §26.24). None of these asks a person anything; only the run
   check asks the target, about the run's other live sessions.
2. The start passes the gate (§26.7) as `sessions_manage` for the tool and model, with
   the load counted from the records and the operation id. A `dispatcher` or
   `integrator` start asks under `role_session_start` when the policy lists it, a
   worker under `session_start`. A start that is being repeated does not count against
   its own caps.
3. The session is recorded as `requested`, and the target's machine is asked with the
   policy's machine limits for that target.
4. The host reserves a terminal id for the operation id in
   `${HOTSHEET_HOME}/orchestration-hosted-sessions.json`, asks its own admission
   (§26.10), checks its session cap again on a fresh terminal listing, sets the
   target checkout up for the tool (below), launches, saves the label and marks the
   reservation started. A reservation that never launched is forgotten, so the same
   request can be made again.
5. The host's answer makes the session `running`. A refusal makes it `failed` with the
   host's reason code in its history (an admission reason such as `below_ram_floor`,
   `session_cap_reached`, or a session code) and the caller gets
   `session_admission_refused`, `session_role_active`, `session_start_failed` or
   `session_host_unavailable`. A failure that may be transient leaves the session
   `requested`, and the same request repeats it.

**Tool setup in the target checkout (HS2-WEFJV8).** The tool starts in the target
checkout and reads its MCP, instruction and hook configuration from there. The
drawer's launch path once set up the store of the server that serves it, which is a
different directory whenever the target checkout is not that store, so a target
nobody had opened in a client started with none of its configuration (the drawer's
launch now sets up the checkout containing its `cwd` too, HS2-MBDKWQ). Before every
launch the host therefore runs the target project's own headless setup for the one
tool ([05](05-ai-tool-plugins.md) §5.1a, the same `setup` the CLI runs), in the
checkout, with the MCP entry pointing at the checkout's linked Git ticket store (the
store the project-open refresh chooses), or at the hosting server's store when the
checkout links none. It is idempotent, so a repeated or replacement start rewrites
nothing that is current. A checkout that still holds an HS1 store, an unknown tool
or a setup that fails refuses the start as `tool_setup_failed`; nothing launches and
the session ends as `failed` with that reason (`session_start_failed` to the caller).
The start's answer reports how many files the setup wrote (`tool_setup.wrote`). A
headless host (§26.4) runs the same setup in its own process, with the machine's
plugin directory, before it asks the local server to launch.

What the session then sees, as a real-server test establishes with a fake agent
that reports its launch: its working directory is the target checkout; the
checkout holds the tool's instruction file and MCP configuration, and the MCP entry
names the checkout's own store; its environment carries the worker id its claims
use (`HOTSHEET_WORKER_ID`), the terminal id, the actor role `ai`, the server URL and
secret for permission routing, and `HOTSHEET_PROJECT` naming the checkout.

Setup writes the instruction file and skills into the checkout as project files;
the machine-local MCP and hook files are excluded from Git. The first setup in a
checkout therefore leaves uncommitted files, and until someone commits them the
safe point lists `uncommitted_changes` for every session there (HS2-TRP2MJ narrows
that check). A terminal a person opens from the drawer in another checkout is still
set up against the serving store (HS2-MBDKWQ).

**Label.** The host names the terminal tab `<orchestration project>: run <last eight
characters of the run id>`, or `<orchestration project>: <role>` when the session
serves no single run. The label is the saved tab name every client of that server
already shows ([06](06-clients.md), HS2-89FPV1), so the drawer needs no new component. A Playwright
test reads it in the real drawer at wide and narrow widths, in both color schemes
(HS2-KTHJN2); the drawer truncates a long label before its run suffix (HS2-8KZF59). The
link from the orchestration workspace is HS2-HSMH2Q.

**Roles.** A session joins the target's existing role model
([05](05-ai-tool-plugins.md) §5.9.1). The `planner` role is not a target role: it is
the orchestration project's own planning session (§26.25). The host refuses a `dispatcher` or
`integrator` start while the target's coordination pool shows a live session in that
role, whoever started it. The target's dispatcher still assigns each exact ticket.

**Taking up the role (HS2-2DPGDY).** The host asks the new session to run its role
skill (`hotsheet-worker`, `hotsheet-dispatcher` or `hotsheet-integrator`) with a
prompt that also names the worker id to register in the pool with. A session started
for a run also asks the target's dispatcher for that run's exact ticket, by its slug
where the host can read it (`origin.ticket`, HS2-YC2P7K). Every
orchestrator session's environment carries `HOTSHEET_SESSION_ROLE` and
`HOTSHEET_ROLE_SKILL` as well, whatever happens to the prompt. The prompt reaches
the tool one of two ways:

- **At launch.** A plugin that declares `[launch] prompt_args` (for example
  `["{prompt}"]`) receives it as its first prompt on the command line. Nothing has to
  be ready, and the answer is `at_launch`. No built-in plugin declares it yet
  (Antigravity is HS2-KK3CCE).
- **By session push** ([13](13-drive-transport-interface.md) §13.12). A tool is rarely
  ready the instant its terminal starts: Claude and Codex have no push address until
  their SessionStart hook reports one, and OpenCode's server may not listen yet. A
  push that finds the tool not ready leaves the prompt `pending`.

| `role_prompt` | Meaning                                                                     |
| ------------- | --------------------------------------------------------------------------- |
| `delivered`   | Pushed into the running session                                             |
| `at_launch`   | Passed as the tool's first prompt at launch                                 |
| `pending`     | Not delivered yet; tried again when the tool may have become ready          |
| `unsupported` | The tool cannot take a message from its host; only the environment names it |
| `failed`      | The tool refused it, every try found it not ready, or its window passed     |

A pending prompt is tried again only when the hosting server learns something that
may have made the tool ready: its push address was registered, or it finished a turn
(the busy feed's edge that also wakes the scheduler, §26.19). There is no timer. It
is tried at most four times in all, within ten minutes of the start; a pending prompt
past either bound reads as `failed` without anything having to notice. The state is
kept with the hosted session in `orchestration-hosted-sessions.json`
(`role_delivery`), so a session a headless process started is retried by the server
that runs it. The decision is a pure function (`RoleDelivery::after`, `at`).

**Joining.** Only the target's coordination pool confirms that a session took up
its role: its worker id registered, unexpired, in its role's slot (a worker entry,
the dispatcher or the integrator). The hosted listing reports `joined` (`null` when
the pool cannot be read), and this machine's record keeps `role_prompt` and
`joined_at_ms`, the first time it learned of the join, from each sync. A changed
`role_prompt` and the join are published to the session's run as `session_role_<code>`
and `session_joined`, or move the scheduler's wake marker for a session without a
run. A session the scheduler placed that does not join in time, or whose prompt
failed, is wound down and replaced (HS2-NPTKS8, §26.24). Whether an early
bootstrap paste reaches a Codex TUI that is not ready is HS2-N1GZFE.

### Safe points and winding down

Only the hosting machine can say whether a session is at a safe point. It establishes
five facts, and anything it cannot establish counts against the session:

| Blocker                               | When                                                                                                                                      |
| ------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `mid_turn`                            | The tool is in the middle of a turn                                                                                                       |
| `held_claim`                          | A live ticket claim in one of the checkout's git stores is held under the session's worker id                                             |
| `uncommitted_changes`                 | A worktree of the checkout has uncommitted changes, untracked files included                                                              |
| `pending_handoff`                     | An assignment of the session in the target's coordination pool is not `done`; a dispatcher or integrator answers for every unfinished one |
| `session_unknown`                     | The terminal host could not be asked                                                                                                      |
| `claims_unknown`                      | A ticket store could not be read                                                                                                          |
| `worktree_unknown`, `handoff_unknown` | The checkout is not a git work tree, git could not answer, or the pool's state file could not be read                                     |

The answer lists every blocker at once in that order, with the counts it found. A
session whose process is already gone has nothing left to interrupt and is safe. The
decision is a pure function (`decide_safe_point`) of the facts.

`uncommitted_changes` looks at every worktree of the checkout, because a terminal
cannot yet be tied to the worktrees its session created. One session's unfinished
work, or a person's edits, holds every orchestrator session of that checkout until it
is committed. Narrowing this to a session's own worktrees is HS2-TRP2MJ.

A stop is decided by `decide_stop` from who owns the session, who asks, whether they
force it, and the safe point:

- The orchestrator stops only a session it still owns and only at a safe point. Not at
  one, the answer is `session_not_safe` and nothing is stopped.
- The hosting machine checks the safe point itself when the stop arrives. The
  orchestrator's own belief that a session is idle is never enough.
- `force` stops a session regardless. It is a person's explicit request; for any other
  actor it is `session_force_not_permitted`. A person's stop without `force` respects
  the safe point like the orchestrator's.
- A stop the orchestrator makes passes the gate as `sessions_manage` and asks a person
  first when the policy lists `session_stop`.

The scheduler applies `wind_down.idle_minutes` and `keep_warm` (§26.8); this section
only guarantees that a wind-down it asks for cannot interrupt work.

### A person's session

A person can stop, pause or take over any orchestrator-owned session. A takeover or a
pause sets `owner` to `human`, a pause also sets `paused`, and neither is undone by
the orchestrator. From then on the orchestrator leaves the session alone: a wind-down
is refused `session_human_owned` on the orchestrating machine and again by the hosting
machine, which keeps its own record of who owns each session.

- These are a person's acts. An AI actor is refused, and MCP has no action for them.
- A person needs no permission to act on a session of their own orchestration project. The
  target is resolved freshly and the act is recorded in the audit journal (§26.7) as
  `session_takeover`, `session_pause` or `session_stop` with its outcome.
- A takeover changes this machine's record first, so it works while the target cannot
  be reached. The answer then lists `target_session` in `unverified`.
- A takeover made at the hosting machine is learned by the orchestrator when it next
  syncs the session, or from the host's refusal of a wind-down.

Pausing changes ownership only; the tool is not interrupted. Whether a person can hand
a session back is an open decision (HS2-KR28FS).

**At the hosting machine (HS2-6MNMVB).** A person there acts on a session by its hosted
operation id, with no orchestration project of their own:
`orchestration session takeover|pause|stop <operation-id> --checkout <checkout-id>`
(`stop` respects the safe point unless `--force`), which calls the hosted
`target-sessions` route. `GET /terminals` names the hosted session behind each such
terminal (`orchestrator`: checkout, operation id, project label, owner, paused), and
the drawer tab's context menu offers **Take Over from Orchestrator** and **Take Over
and Pause** while the orchestrator owns it. A takeover, pause or stop writes a token to
`${HOTSHEET_HOME}/orchestration-hosted-session-signal/` from whichever process made it
(this server's hosted route or a headless CLI), and each server, waiting on it with the
platform file watcher, publishes `terminal_orchestrator`, so every client refetches its
terminals and stops offering the takeover (HS2-DX1PQB). The server reads the signal's
current token before it starts that waiter, so a takeover recorded while the waiter is
still starting is announced too (HS2-TWSXK7). A start that launches a terminal writes the same token, so every drawer on the
hosting machine's clients lists the new session at once, before its tool connects to Hot
Sheet; a repeated start (`existing`) or a refused one launched nothing and announces
nothing (HS2-R0HSNK). The orchestrator learns of it on its next sync or from
the host's refusal of a wind-down. Typing into the terminal does not take it over;
whether it should is HS2-8BY9Y1. Closing the tab is a person's stop (HS2-HVKS7M): the drawer sends it through the hosted stop with `force`, which records `person_stopped`, so the orchestrator never reads it as a crash (`host_lost`).

### Crashes

`sync` reads a session from its host. A session the host started and no longer has
becomes `lost` with `host_lost`; one the host has no record of at all becomes `lost`
with `host_forgot`. A session still `requested` that the host never saw is left as it
is, so its start can be repeated or it can be stopped, which ends it as
`never_started`. While the terminal host cannot be asked nothing is concluded: the
session keeps its state and its safe point lists `session_unknown`. A lost session
frees its place under the caps, and its replacement is an ordinary start that names it.

A session started for a run publishes each change to that run's event stream (§26.9)
as kind `session`, with a reason code such as `session_running`, `session_lost` or
`session_takeover`. A session without a run has no stream to publish on; each of its
changes moves the scheduler's wake marker instead (§26.19). Reconciliation (§26.14)
reads the sessions of a run from their hosts and judges them (HS2-75Q254);
`session_work` gives the same judgement from this machine's records alone.

### What the scheduler calls

`OrchestrationRuntime` in `crates/hotsheet-cli/src/orchestration/sessions.rs`:

| Function                                                          | Purpose                                                          |
| ----------------------------------------------------------------- | ---------------------------------------------------------------- |
| `session_start(owner, StartSession, actor, tx)`                   | Start, or return the session the operation id already names      |
| `session_list(owner, active)`                                     | This machine's records; contacts nothing                         |
| `session_load(owner, target_id, tool, account)`                   | The load the policy's caps count                                 |
| `session_sync(owner, session_id, actor, tx)` and `sessions_sync`  | Bring one record, or every unfinished one, in line with its host |
| `session_safe_point(owner, session_id, actor, tx)`                | The host's safe-point answer with every blocker                  |
| `session_stop(owner, session_id, StopSession, actor, tx)`         | Wind down at a safe point, or a person's stop                    |
| `session_takeover(owner, session_id, TakeoverSession, actor, tx)` | A person takes it over, optionally pausing it                    |
| `session_work(owner, run_id)`                                     | `Observed<ExternalWork>` for reconciliation                      |

Reading and syncing pass the gate as `sessions_manage` continuing an existing decision:
policy limits apply, nothing starts, and nobody is asked.

### Headless surface

```sh
hotsheet-cli ... orchestration session start --project <owner> --target <target-id> \
  --operation-id <retained-id> --tool <tool> [--model <model>] [--effort <effort>] \
  [--account <account>] [--role worker|dispatcher|integrator] [--run <run-id>] \
  [--replaces <session-id>]
hotsheet-cli ... orchestration session list --project <owner> [--active]
hotsheet-cli ... orchestration session show|safe-point <session-id> --project <owner>
hotsheet-cli ... orchestration session sync [<session-id>] --project <owner>
hotsheet-cli ... orchestration session stop <session-id> --project <owner>
hotsheet-cli ... orchestration session request <session-id> --project <owner>
hotsheet-cli --actor-role human ... orchestration session stop <session-id> --project <owner> --force
hotsheet-cli --actor-role human ... orchestration session takeover|pause <session-id> --project <owner>
hotsheet-cli ... orchestration session list --checkout <checkout-id> [--safe-point] # this machine
hotsheet-cli --actor-role human ... orchestration session takeover|pause <operation-id> --checkout <checkout-id>
hotsheet-cli --actor-role human ... orchestration session stop <operation-id> --checkout <checkout-id> [--force]
```

| Route                                                                                 | Methods and purpose                                                                                   |
| ------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| `/checkouts/{owner}/orchestration/sessions`                                           | GET records (`?active=true`); POST start; machine-local                                               |
| `/checkouts/{owner}/orchestration/sessions/{session}`                                 | GET one record; machine-local                                                                         |
| `/checkouts/{owner}/orchestration/sessions/{session}/{sync,safe-point,stop,takeover}` | POST supervise; machine-local                                                                         |
| `/checkouts/{owner}/orchestration/session-sync`                                       | POST sync every unfinished session; machine-local                                                     |
| `/checkouts/{checkout}/target-sessions`                                               | Hosted half: GET list (`?operation_id=`, `?safe_point=true`), POST start, DELETE stop, PATCH takeover |
| `/checkouts/{checkout}/target-sessions` (PUT)                                         | Hosted half: tell a running session its exact ticket (HS2-R7QH35)                                     |

The hosted route is ordinary authenticated project data under the caller's device
role, like the hosted ticket route. A read-only device can list and cannot start,
stop or take over. A read-write device can start a session within the machine's
admission limits (§26.10); whether hosting should need an explicit opt-in is an open
decision (HS2-CRBR8X). Hosted refusals are answers with a reason code, not HTTP
errors, so the code survives the reviewed transport. A peer that predates the route answers
`target_peer_unsupported`. MCP `hotsheet_targets` adds `session_start`,
`session_list`, `session_show`, `session_sync`, `session_safe_point`,
`session_request` (HS2-R7QH35) and `session_stop`; its role argument is
`session_role`.

A headless process owns no terminals. `hotsheet-cli` and the MCP core backend use the
ordinary terminal routes of the Hot Sheet server running on the same machine, so a
session they start still lands in that server's drawer, and without a running server a
start fails closed. The start uses the drawer's launch path as it is, including where
that path runs the tool's setup; the target checkout's own setup runs first
(HS2-WEFJV8).

## 26.17 Throughput scheduler (HS2-ECWG50)

The scheduler decides where and how many AI sessions run: for each target, tool and
model it starts a session, keeps one or winds one down. It decides how many sessions
run where; the target's dispatcher still assigns each exact ticket. Starting and
stopping a session is the executor's act (HS2-GDJXGZ), reached through one seam.

### Inputs

One pass reads only typed inputs, and the core
(`crates/hotsheet-ticketing/src/orchestration/scheduler.rs`, `plan`) is a pure
function of them and the time:

| Input           | Source                                                                                                      |
| --------------- | ----------------------------------------------------------------------------------------------------------- |
| Ready work      | `ready_work` (§26.15). `confirmed` items are placed; `provisional` ones only by the policy's opt-in, §26.22 |
| Orphaned runs   | Steps whose scheduler run is unfinished and that no unfinished session works (§26.19); placed as confirmed  |
| Policy          | §26.8, with its `placement` section                                                                         |
| Target capacity | One snapshot per target with confirmed work (§26.10), read through the gate as `gates_observe`              |
| Sessions        | The executor's list, as the scheduler's own session view                                                    |
| Tool capacity   | The machine's capacity records and plugin declarations (§26.11)                                             |
| Held resources  | Live resource claims in the namespaces the work names, per machine, and the namespaces that were unreadable |
| Memory          | Per target, the last start, the last wind-down and the refusals since the last start                        |

A target is contacted only when it has confirmed work, the policy lets sessions run
there and no refusal is being waited out. Without an identified actor no target is
contacted and its work reads `target_unavailable` (`actor_required`). A target whose
machine cannot be read gets no session: nothing is assumed in a snapshot's place.

The resources work names are those its plan step declares and those the policy
requires of its target (§26.20).

**Preferred tool and model (HS2-TH53NN).** A plan step's optional `tool` and `model`
(§26.25) reach the work as preferences, never as requirements:

- A free session of the target that has them takes the work first.
- Otherwise a new session of the preferred tool starts.
- When the preferred tool cannot start (limited, capped, not installed, or the
  preferred model offered by no allowed tool), another free session of the target
  takes the work. With none free, any allowed tool account does.
- The preferred model goes only with the preferred tool, or with any tool when no tool
  is preferred and that tool offers it. Another tool uses its own first listed model.

A due date and a milestone date are inputs of the core that no ticket provider
supplies yet: no provider exposes a due date and milestones do not exist (HS2-QY5YRN).

### Policy section

`placement` is optional and is left out of the stored policy while it holds only
defaults, so a policy written earlier is unchanged.

| Field                     | Default | Meaning                                                                                                   |
| ------------------------- | ------- | --------------------------------------------------------------------------------------------------------- |
| `automatic`               | `false` | When false every pass is a dry run: decisions are computed and logged, nothing applied                    |
| `actor`                   | none    | The actor id the server's loop acts as. Optional: without it the orchestrator acts as the project (below) |
| `aging_minutes`           | 60      | Waiting this long raises work by one priority band (1 to 10080)                                           |
| `cooldown_minutes`        | 5       | A target's session count does not reverse direction within this time (0 to 1440)                          |
| `dispatcher_wait_minutes` | 2       | How long a placed run waits for a live target dispatcher before its session claims it (0 to 1440, §26.24) |
| `join_minutes`            | 10      | How long a placed session has to join its role before it is wound down and replaced (1 to 1440, §26.24)   |

**Who the loop acts as (HS2-XYAHB4).** Adding a project is the consent to read it and
acting asks a person (§26.7), so no grant holder needs naming: without `actor` the
orchestrator acts as the orchestration project, `orchestrator.<owner checkout id>`
(`PlacementPolicy::acting_actor`; an owner id that is not a bounded identifier falls
back to `orchestrator`). Its reads, starts and stops are audited under that id.

Validation adds `invalid_placement_actor`,
`invalid_aging_minutes`, `invalid_cooldown_minutes`,
`invalid_dispatcher_wait_minutes` and `invalid_join_minutes`.
`dispatcher_wait_minutes` and `join_minutes` are written only when they differ from
their defaults.

### Scoring

Each piece of work gets a score in points, with every term reported:

| Term       | Points                                                                                             |
| ---------- | -------------------------------------------------------------------------------------------------- |
| `priority` | 1000 per band: `lowest` 0, `low` 1, `default` or unknown 2, `high` 3, `highest` 4                  |
| `up_next`  | 1500 when the target ticket is marked Up Next                                                      |
| `due`      | 2000 overdue, 1000 due within a day, 300 due within a week                                         |
| `unblocks` | 200 per unfinished step waiting on it, at most five                                                |
| `red_gate` | 2000 when a step is blocked by a failed gate of this ticket, plus 250 per such step up to eight    |
| `aging`    | 1000 per `aging_minutes` since the work became ready, without a ceiling                            |
| risk       | Provisional work keeps `100 - speculation.risk_weight` percent of its sum (0 ranks like confirmed) |

Ties go to the work that has been ready longer, then to the lower step id; under a
policy that opts in to provisional work, confirmed work goes before provisional work
of the same score first (§26.22). Because
aging has no ceiling, low-priority work eventually outranks anything that keeps
arriving. The repair of a red gate outranks new work of equal priority, and more so
the more work stands on that gate.

### Packing

Work is placed best first. For each item, in this order:

1. A provisional item is `provisional_not_dispatched` unless the policy opts in for
   its target; then the provisional rule decides, and a refused item is
   `speculation_refused` with the rule's code (§26.22).
2. A resource it needs that someone holds, or that better-ranked work took in this
   pass, is `resource_held`. A held resource is unavailable capacity until its claim
   expires. A resource whose claims could not be read is `resource_unknown` unless
   its backend allows provisional claims (§26.20).
3. A **free session** of the target takes it (`keep`, `refill`): an idle session
   first, then one whose run waits on a gate. No session starts for it.
4. `policy_denied` with the policy's own code, then `target_cap`.
5. `refusal_backoff` while the target is left alone after a refusal, then `cooldown`
   after a recent wind-down there.
6. `target_unavailable` when the machine was not read.
7. Tool accounts are tried in the order of `select` (§26.11). A `degraded` account
   takes one session at a time (`probe_in_flight`). For each candidate the machine
   check is `decide_admission` (§26.10) with the policy's limits for the target and
   one session more than this pass already placed on that machine. A tool the
   machine does not have moves on to the next candidate; a cap or floor ends the
   search as `machine_refused` with the limit (`below_ram_floor`,
   `session_cap_reached`, `above_cpu_ceiling`, `below_disk_floor`,
   `capacity_unknown`). No candidate left is `no_tool_available` with the nearest
   cause.

So a limited tool moves work to another allowed tool, a machine below its RAM floor
takes nothing, targets on one machine share its room, and every start counts
against the target, machine, tool and account caps before the next is considered.
A start on an account that is not verified available is marked `probe`.

A session is **free** when it is idle with no unfinished run, or when its run waits
on CI, a deployment or a person's answer: time spent waiting on an external system is
capacity. A session whose run is unfinished is working, between turns or not. Each
start and each refill dispatches its step's run (§26.19, HS2-YC2P7K), so the step is
in progress from then on and is no longer ready work.

Sessions that nothing was found for:

| Decision    | Reason                               | When                                                                          |
| ----------- | ------------------------------------ | ----------------------------------------------------------------------------- |
| `keep`      | `busy`, `waiting_on_gate`            | It works a run, or waits on a gate with nothing ready                         |
| `keep`      | `keep_warm`, `idle_grace`            | One of the `keep_warm` most recently idle, or idle under `idle_minutes`       |
| `keep`      | `cooldown`                           | The target started a session within the cooldown                              |
| `keep`      | `awaiting_safe_point`                | It should stop and is not at a safe point                                     |
| `keep`      | `human_takeover`, `not_owned`        | A person took it over, or this owner's orchestrator did not start it          |
| `keep`      | `winding_down`                       | A wind-down was already asked for                                             |
| `wind_down` | `idle`                               | Idle past `idle_minutes`, beyond `keep_warm`, at a safe point                 |
| `wind_down` | `over_cap`                           | The target's cap was lowered below what runs; the freest go first             |
| `wind_down` | `policy_not_permitted`               | The policy no longer permits the target, tool or model                        |
| `wind_down` | `never_joined`, `role_prompt_failed` | A placed session did not join its role in time, or its prompt failed (§26.24) |
| `keep`      | `never_joined`                       | The same, after its run's replacements; detail `replacements_exhausted`       |

A session the scheduler does not own counts as load and is never refilled or wound
down. A busy session is never interrupted.

**Oscillation guard.** A session is wound down for idling only after `idle_minutes`,
and a target does not reverse direction within `cooldown_minutes`: no start after a
wind-down and no wind-down after a start. Work that comes and goes is taken by the
same session.

**Refusals.** After a start is refused by the executor, the target or its machine,
or a pass finds the machine without room or unreadable, the target is not read or
asked again for one minute, doubling per refusal to 30 minutes. A start clears it.

A schedule names `retry_at_ms`, the earliest time one of its decisions changes by
time alone: an idle time, a cooldown, a backoff, a tool limit's reset or a claim's
expiry. A claim released before its expiry wakes the loop instead (§26.20).

### Applying a decision

A pass is **applied** only when the policy sets `placement.automatic`, the caller did
not ask for a dry run, and an identified actor is given. Otherwise it is a **dry
run**: computed, logged by a `tick`, and nothing is started or wound down. Merging
this section therefore starts no session anywhere until a person enables it.

Inside the policy the scheduler acts and logs. For each start or wind-down it asks
the side-effect-free `access` check with the load and a stable operation id
(`sched-start-<step id>`, `sched-stop-<session id>`):

- **Allowed.** The request goes to the executor, whose own gate passes the start
  with the same load and operation id (§26.10) and uses an Allow Once a person gave
  only when the session starts. The scheduler does not use it.
- **Not allowed.** The action passes `authorize`, which files one permission request
  (§26.7) for a session start with no remembered rule and audits the answer. The
  outcome is the gate's code, such as `permission_required` with the request id, and
  nothing is done. A person's answer wakes the scheduler and lets the next pass
  through; a denial is final for that operation.

A wind-down reduces activity, so it never asks and the policy never denies it: a
session on a target the policy disabled is wound down like any other (HS2-48H57C).

The seam is `SessionExecutor`
(`crates/hotsheet-cli/src/orchestration/session_control.rs`):

```rust
pub trait SessionExecutor: Send + Sync {
    fn list(&self, runtime: &OrchestrationRuntime, owner_id: &str, tx: &mut Transaction<'_>)
        -> Result<Vec<ScheduledSession>, SessionExecutorError>;
    fn start(&self, runtime: &OrchestrationRuntime, request: &PlacementStart,
        tx: &mut Transaction<'_>) -> StartOutcome;
    fn wind_down(&self, runtime: &OrchestrationRuntime, request: &WindDownSession,
        tx: &mut Transaction<'_>) -> WindDownOutcome;
}
```

Every call is handed the runtime it serves and the pass's transaction, so an executor
keeps no state and acts on the pass's clock.

`StartOutcome` is `Started { session_id }`, `Refused { reason }` or `Unavailable`;
`WindDownOutcome` is `Requested`, `NotAtSafePoint`, `NotFound`, `Refused { reason }`
or `Unavailable`. A runtime built without an executor has `ExecutorUnavailable`:
nothing is listed, and an applied start or wind-down ends as the typed, logged
outcome `executor_unavailable`. An executor that cannot list its sessions marks the
pass `failed`, and nothing is started on a guess. The executor the server, the CLI
and MCP run with is `OwnedSessions` (§26.19). Role sessions (a target's dispatcher or
integrator) are HS2-TK9ADC.

### Decision log and memory

Every `tick` appends to `${HOTSHEET_HOME}/orchestration-scheduler-decisions/`, one
bounded journal per owner with a sequence cursor: one `tick` record with what the
pass read (the policy revision, counts of work, sessions, targets, limited tools and
held resources, and whether the executor answered) and one record per `start`,
`keep`, `wind_down` and `unplaced` decision, one `assignment` record per placed
run nobody will assign (§26.24), and one `reconcile` record per run whose stored
reconciliation the pass acted on (§26.14, HS2-AH8CRS), with its reason, its score parts, the
figures it was decided on (sessions against the target, machine and tool caps, free
RAM and the session footprint, the tool account's state) and, when applied, the
outcome and operation id. Records hold identifiers, codes and numbers only.

`${HOTSHEET_HOME}/orchestration-scheduler.json` holds the placement memory per
owner (private, locked, atomically replaced, version 1). A read never creates it, a
dry run never changes it, and damaged or newer data is an error that is never
overwritten.

### Reevaluation

The server runs one loop per owner whose policy enables automatic placement, started
at server start and when a policy or scheduler request arrives, and ended when the
policy no longer enables it or the server begins stopping. A stopping server's
followers end the same way, it launches no session (the start stays requested), and
it reports nothing about its terminals as they go down, so a server started after it
over the same home runs its own loop and followers at once (HS2-KM9CTG). A loop step that
was already waiting when its server began stopping runs no pass either (HS2-VRC0CQ): the
session host reports `stopping`, and the step ends before its pass. Woken by the
successor's writes to the same home, it once placed a start that its own stopping host
refused, which failed that start in the successor's place and moved the replacement to a
later operation generation. A step runs a pass:

- when the loop starts;
- when the owner's run event stream (§26.9) has something new: run, readiness, gate,
  validation, session and reconciliation events. A lease renewal is not a reason;
- when the scheduler's wake marker changed: a tool limit or recovery was recorded
  (§26.11), the policy was replaced, a session without a run started, ended or
  finished a turn (§26.19), a plan step's definition changed, a resource the last
  pass deferred work for was released (§26.20), or a target follower learned of a
  session change on another server (§26.9, HS2-257Q2Z);
- when the time the last pass named in `retry_at_ms` has come.

A change that publishes events and also moves the marker publishes first: defining a
plan step writes its readiness events, then moves the marker (HS2-1JFVCJ). A follower
woken by the marker before the event existed ran its pass as a `signal`, and the
`event` pass that followed decided the same and logged nothing.

Otherwise it waits on the journal and the marker themselves. There is no interval.
A failed pass, such as an executor that cannot list its sessions, backs off from one
second, doubling to a minute. A pass whose decisions equal the last logged ones logs
nothing, so an event that changes nothing leaves no record. The session an applied
start began is part of the decision, so a replacement start is always logged.
Passes of one owner run one at a time on this machine (HS2-7Q428Z, §26.24), so a
start is decided, applied and logged once however many passes are asked for at once.

**A machine that gains room (HS2-AWB7BX).** When a pass of the loop leaves work
unplaced because a target's machine had no room (`machine_refused`, or a
`refusal_backoff` whose reason is a capacity refusal: `session_admission_refused` or an
admission reason such as `below_ram_floor`), the loop follows that target's capacity
with the wait of §26.10 (HS2-P5GTSW), from the version the pass read and with the same
claim namespaces. It passes the gate as `gates_observe` (it never asks; again after 10
minutes) on a thread of its own, at most 16 targets at a time and once per target. A
changed version lifts the target's capacity refusal (`SchedulerStore::lift_capacity_refusal`)
and wakes the loop, which asks the machine again at once. While a target is followed a
capacity refusal there sets no retry time: the change ends it, not the clock. The
refusal backoff stays the fallback wherever no event can arrive: a one-shot `plan` or
`tick`, an older host whose snapshot has no version, a host that cannot be reached or
answers without waiting, a gate refusal, or after 24 hours. The follow then ends, wakes
the loop once so the retry time applies again, and the target is not followed again
until a read sees a version. The pass that first records a refusal still sets its retry
time, because the follow starts after it. A change of the owner's policy lifts every
capacity refusal of the owner too, since new limits can give a machine room. Identical capacity reads are recorded once per
target and 10-minute window (HS2-SV4SMY, §26.7).

### Headless surface

```sh
hotsheet-cli ... orchestration scheduler plan --project <owner>
hotsheet-cli ... orchestration scheduler explain <step-id> --project <owner>
hotsheet-cli ... orchestration scheduler explain --target <target-id> --connection <source> \
  --id <canonical-ticket-id> --project <owner>
hotsheet-cli ... orchestration scheduler decisions [--after <cursor>] [--limit N] --project <owner>
hotsheet-cli ... orchestration scheduler tick [--dry-run] --project <owner>
```

`plan` computes the schedule and changes and logs nothing. `explain` answers
`running`, `finished`, `not_planned`, `not_ready` (with the readiness reasons),
`start`, `keep` or `not_placed` with the reason (`cancelled_by_person` for a step a
person's cancel holds, §26.24), its detail, the rank and score,
when to expect a change, and each resource the work requires (§26.20). A running
step whose run waits for a target dispatcher that is not there answers reason
`no_live_dispatcher` (or `pool_unknown`) with `assignment` and the claim time
(§26.24). `tick` is one pass. A headless process hosts no sessions and
asks the server on the same machine for capacity (§26.10); with none running the
machine reads as `capacity_unknown`.

| Route                                                                      | Methods and purpose                      |
| -------------------------------------------------------------------------- | ---------------------------------------- |
| `/checkouts/{owner}/orchestration/scheduler/{plan,explain,decisions,tick}` | POST scheduler operations; machine-local |

MCP `hotsheet_targets` adds `scheduler_plan`, `scheduler_explain`,
`scheduler_decisions` and `scheduler_tick` (with `dry_run`). The workspace view is
HS2-77SRHC.

## 26.18 Provisional readiness inside one project (HS2-2SJPCH)

§26.15 computes a `provisional` level with `base_commits` and refuses it at the
dispatch guard. That refusal is unchanged. HS2-2SJPCH built the rule and its
recovery for one project's dispatcher, integrator and worker loop; the design is
[05](05-ai-tool-plugins.md) §5.9.1, "Provisional readiness of a prerequisite awaiting
CI".

Cross-target provisional work (§26.22, HS2-JSF985) is decided by the same pure rule
in `hotsheet_ticketing::provisional`:

```rust
pub fn blocker_readiness(
    policy: &SpeculationPolicy,
    dependent: &Ticket,
    blocker: &Ulid,
    tickets: &[Ticket],
    probe: &dyn GateProbe,
) -> BlockerReadiness;

pub fn ticket_readiness(
    policy: &SpeculationPolicy,
    dependent: &Ticket,
    tickets: &[Ticket],
    probe: &dyn GateProbe,
) -> TicketReadiness;
```

`BlockerReadiness` is `Confirmed { by }`, `Provisional(ProvisionalBase)` or `Blocked {
reason }`. A `ProvisionalBase` names the prerequisite, the `base_commit` to build on,
the pending `gate` (result and run) and the `depth` of unconfirmed gates stacked
there. `GateProbe` supplies the two facts the rule cannot compute: whether a commit is
reachable from the shipping branch, and the newest reading of its CI gate. The
single-project probe reads Git and the CI watch Jobs; a cross-target probe can answer
from `commit_gate_state` (§26.13). `SpeculationPolicy` carries the opt-in, the depth
limit (default 1) and the integration rule, and `OPT_OUT_TAG` is `speculate:never`.

The transitions after a gate result (confirmed, at risk with a note naming the run,
rebased onto the fix, reopened and never discarded) live in
`hotsheet_cli::coordination::speculation` as a ledger that is independent of the pool
schema, so the cross-target form reuses `SpeculationLedger::record`, `apply`,
`rebase`, `reopen` and the owed-note bookkeeping with its own storage (§26.22).

An agent outside any pool uses the same rule to inform, not to choose (HS2-XYF4YT):
`claim_prerequisites(policy, claimed, tickets, probe)` returns, for each `blocked_by`
prerequisite, `AwaitingCi(ProvisionalBase)` or `GateRed { blocker, slug, gate_commit,
gate }`, and nothing for any other state. `claim`, `claim-next` and `renew` add these to
their answers under an enabled policy, with a red gate reported once per claim; see
[05](05-ai-tool-plugins.md) §5.9.1, "Uncoordinated claims". `LiveGateProbe::open`
builds the single-project probe without a pool.

## 26.19 Placing sessions (HS2-95SWQ4)

`OwnedSessions` (`crates/hotsheet-cli/src/orchestration/session_executor.rs`) is the
scheduler's executor (§26.17) over orchestrator-owned sessions (§26.16). The
scheduler decides; the executor maps each decision onto the session runtime and each
answer onto a typed outcome. `OrchestrationRuntime::live` attaches it, so the server's
loop, `hotsheet-cli` and MCP place sessions the same way. It adds no route, command
or MCP action, and nothing is placed while the policy leaves `placement.automatic`
off.

### What the scheduler is shown

`list` reads this machine's unfinished session records and brings each in line with
its host first, with one safe-point request per session (§26.16). A crashed session is
therefore `lost`, and a session a person took over at the host is theirs, before the
scheduler decides anything.

| Field            | From                                                                                                             |
| ---------------- | ---------------------------------------------------------------------------------------------------------------- |
| `owner_id`       | The owning checkout. Only its own records are listed                                                             |
| `run_id`         | The run the session works, while it is unfinished. A session whose run finished is listed without one            |
| `activity`       | `idle` when the host found it between turns with no claim, no pending handoff and no unfinished run; else `busy` |
| `safe_point`     | The host's safe-point answer. Anything the host could not establish is not safe                                  |
| `idle_since_ms`  | When this machine first saw the session idle                                                                     |
| `human_takeover` | A person owns it, whether they took it here or at the host                                                       |
| `winding_down`   | Never set: a stop is immediate at a safe point and refused away from one                                         |

- **Who asks.** The hosts are read as the policy's `placement.actor`, else as the
  orchestration project's orchestrator (§26.17, HS2-XYAHB4), through the gate as
  `sessions_manage` continuing a decision. Without a policy no host is read: each
  record is listed as it stands, busy and not safe, so a dry run never shows a
  wind-down it could not verify.
- **A host that cannot be read.** The record stays as it is and still counts. It reads
  as busy and not safe, and keeps the idle time already seen. A policy that disabled
  the target denies the read too, so its sessions are kept for a person to stop
  (HS2-48H57C).
- **A start nobody confirmed.** A `requested` record whose host does not have the
  session is not a session and is not listed. The next start for its work repeats it.
  One that is five minutes old (`UNCONFIRMED_START_MS`, longer than any one request
  to a host) and still unknown to its host is ended as `never_started`. Nothing ran,
  so nothing is interrupted. A start for a changed tool, model or account ends it at
  once instead (HS2-7VV79Q, below).
- **Idle time (HS2-RG91SK).** A host says since when a session has been idle: its
  terminal stamps the moment a turn finishes (the OSC 133 prompt after command output),
  or its launch, and clears it while a turn runs. The time travels as `idle_since_ms` on
  the terminal listing (`GET /terminals`, the broker's terminal info) and in the safe
  point's facts, and the scheduler uses it, so a turn that began and ended between two
  passes restarts the idle time. A host that does not report it (an older one) falls
  back to the first listing here that saw the session idle, kept in
  `${HOTSHEET_HOME}/orchestration-session-idle.json` (private, version 1): a listing
  that sees it working forgets the time, one that cannot ask leaves it, and a finished
  session is dropped. The host's safe point, not this time, is still what protects
  work. Safe point facts are read leniently, so a host that reports more is understood.
- Sessions other owners or people run on a target's machine are not listed. They
  reach the scheduler through the machine's capacity snapshot (§26.10).

### Starting

The scheduler's operation id (`sched-start-<step id>`) is stable for a piece of work.
A session's own operation id is one session's for good (§26.16), and a finished
session never restarts. The executor joins the two:

| The latest session started for the work | The start                                                                                |
| --------------------------------------- | ---------------------------------------------------------------------------------------- |
| None                                    | A new session under the scheduler's id                                                   |
| `requested` or `running`                | The same start again: the same session is returned, and nothing launches twice           |
| `failed`, `stopped` or `lost`           | A new session under `<id>.<n>` (`.2`, `.3`, …) that names the finished one in `replaces` |

Another live session that already works the step's run is adopted instead, and a
person's or an unconfirmed one refuses the start (`session_run_held`, §26.24).

So a repeated request never launches a second session, a refused start can be tried
again after its backoff, and a crashed session is replaced by the next pass that
still finds its work ready.

**The gate.** The start passes the gate under the scheduler's operation id, so an
Allow Once a person gave for that id is the one used, once the session is running,
and a replacement asks again unless a rule allows it. The
load the gate checks is, for the target, the machine, the tool and the account, the
larger of the scheduler's count and the count from this machine's records. The
scheduler's count includes sessions the records do not hold (other owners' and
people's on the machine), and the records include starts made since the scheduler
counted. Neither undercuts the other.

| Answer of the session runtime                           | Outcome                                                                                                      |
| ------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| The session is `running`                                | `started` with its session id                                                                                |
| The host refused, or could not launch                   | `refused` with the host's own reason (`below_ram_floor`, `session_start_failed`, `session_host_unavailable`) |
| The gate denied it after the scheduler's own check      | `refused` with the gate's code (`policy_denied`)                                                             |
| The start or its step's run waits for a person          | `permission_required` with the request id as the detail; no backoff, the answer wakes the loop (HS2-XHQ1EH)  |
| The host had launched it earlier and it has ended since | `refused` with how it ended                                                                                  |
| Anything else, such as a host that did not answer       | `refused` with the error's code; the start stays `requested`                                                 |
| The acting actor has no id                              | `executor_unavailable`                                                                                       |

A different tool, model or account for work whose earlier start is still unconfirmed
ends that start at once through its host and starts the new choice (HS2-7VV79Q,
§26.24).

### Winding down

A wind-down is `session_stop` without force, as the system acting for the actor who
asked, passed through the gate under the scheduler's `sched-stop-<session id>`. The
host checks the safe point itself when the stop arrives, so nothing the scheduler
believes can interrupt work.

| Answer                                                  | Outcome                              |
| ------------------------------------------------------- | ------------------------------------ |
| Stopped                                                 | `wind_down_requested`                |
| `session_not_safe`                                      | `not_at_safe_point`; nothing stopped |
| No such session, already finished, or lost when asked   | `session_not_found`                  |
| A person owns it (known here, or learned from the host) | `refused` with `session_human_owned` |
| The gate or the target refused                          | `refused` with the code              |

A person's session is refused before its host is asked anything. A stop never asks a
person, so a stop the host refuses as not safe uses up nothing (HS2-RTX8RJ, settled
by HS2-XHQ1EH).

### How the loop learns of a session

A session with a run publishes each change on that run's event stream, which wakes
the loop. Nothing asks on a timer:

- Every change this machine records for a session without a run (started, failed,
  lost, stopped, taken over) moves the scheduler's wake marker (§26.17).
- The hosting server moves the same marker when an AI terminal an orchestrator
  started exits or finishes a turn, including one killed from its drawer. It learns
  that from the busy feed it already keeps for every AI terminal. The loop then runs
  a pass and reads the session from its host.
- A pass that keeps an idle session names the time its idle period ends in
  `retry_at_ms`, and the loop waits for that time.

The marker is this machine's. A host on another server publishes the same exit or
finished turn on its hosted target feed, and the owner's follower of that target
(§26.9), which keeps following while the owner has an unfinished session there,
moves the owner's marker; so does a claim taken or let go there while the session
works no unfinished run (HS2-257Q2Z). Removing the last step of a plan publishes no
event (HS2-SQS266).

### Dispatching the step (HS2-YC2P7K)

The scheduler dispatches the run of each step it places a session for, so the
session has an exact ticket and the step leaves the ready work. The executor does
it, under the scheduler's actor:

1. **The run first.** `run_dispatch` (§26.9) in `dispatcher` mode, under operation id
   `sched-run-<step id>`, with the step's orchestration ticket and target ticket. It
   passes the gate as `work_start`, so it asks a person unless a remembered
   `work_start` rule allows it, and the policy's limits apply. The readiness guard (§26.15) applies as to any dispatch. The target ticket
   is marked Up Next for the target's dispatcher, which still assigns it; the
   orchestrator never names the worker. A refused dispatch refuses the start with
   the gate's or the target's code, and no session starts.
2. **Then the session, linked to the run.** It is labelled with the run, publishes on
   its stream, and its role prompt (§26.16) asks the target's dispatcher for that
   exact ticket. A start being repeated keeps the run it named, or is linked to the
   run once it runs.

| The latest scheduler run of the step   | The dispatch                                     |
| -------------------------------------- | ------------------------------------------------ |
| None                                   | A new run under `sched-run-<step id>`            |
| Unfinished                             | The same run: nothing is dispatched twice        |
| Failed, or cancelled by an AI actor    | A new run under `<id>.<n>` (`.2`, `.3`, …)       |
| Cancelled by a person                  | None: the step is held (HS2-HV9ED3, §26.24)      |
| Cancelled because the step was removed | None: the step is gone (HS2-YFPZFP, §26.24)      |
| Completed                              | None: the step is finished and is not ready work |

**Repeats and replacements.** From the dispatch on, the step is `in_progress`, so
later passes start nothing for it. A step whose scheduler run is unfinished and that
no unfinished session works (its session start was refused, or its session was
lost) is placeable work again: the next start for it, after any refusal backoff, is
linked to the same run. A run that waits on a gate, its ticket in `final_testing` or a
validation attempt needs no session, so it is not placed again until it needs work
(HS2-3REXQ7, §26.24). A pass lists the sessions, and so learns of a lost one,
before it reads the work.

**Refill.** A free session given ready work (`keep`, `refill`) gets that work's run
the same way: the executor's `assign` dispatches it and links it to the session,
published as `session_assigned` on the run, and the decision log records `assigned`
with the run. A session with an unfinished run reads as busy, so it is never refilled;
one whose run finished is free again. The refilled session is then told the new
ticket through its host's session push (HS2-R7QH35, §26.24); the target's dispatcher
still assigns it.

**Following the run.** After a pass, the server makes sure the run's target is
followed (§26.9): a claim, a release or a completion of the target ticket syncs the
run from the target's change feed, which now carries claims and releases. A
dispatcher-mode run becomes `dispatched` with the worker that claimed, and a finished
ticket completes it; the step is then `finished`, and its session is free and is
wound down after its idle time.

Removing a step from the plan leaves its run queued and its session busy until a
person cancels the run (HS2-YFPZFP). A target with no live dispatcher or integrator
never assigns the ticket; the loop reports that and, after a bounded wait, claims it
for the session (HS2-T16SK0, §26.24). Ticket writes made outside the hosting
server reach its change feed, from hosted and unhosted linked stores alike (HS2-BDNJ5S,
HS2-G5FYP6).

### Not wired

Reconciliation's session source (§26.14) is unchanged. `session_work` would report a
session that was wound down while its run waits on a gate as missing work
(HS2-75Q254).

### Reconciliation

Reconciliation's session source (§26.14, HS2-75Q254) tells a wind-down from a loss: a
session the scheduler wound down at a safe point is `session_stopped`, never missing
work, and a session lost while its run waits on a gate or a validation attempt is no
missing work either.

## 26.20 Scheduling around claimable resources (HS2-2FG91X)

Some work needs a thing only one process may use at a time, such as the one macOS VM
a machine has. The resource-claim service ([36](36-resource-claims.md)) leases such
a resource to one holder. Orchestration declares which resources work needs, reads
who holds them, and does not start work that would only wait.

**The scheduler never claims.** It does not acquire, renew or release a resource for
anyone. The session that does the work acquires the resource itself with the claim
service's primitives (§36.10), keeps the renewal token, renews while it works and
releases when done (§36.1, §36.4). A stuck session lets its lease lapse. The
scheduler's part ends at naming the resources in the start request
(`PlacementStart.resources`).

**Handing them to the session (HS2-7YSKXE).** The executor passes those resources into
the session start (`StartSession.resources`, the hosted start's `resources`, at most
four), and the host keeps them on its session record. The session's prompt names each
one and tells it to acquire them itself with
`hotsheet-cli resource acquire <namespace>/<resource> --duration … --wait 5m` (or
`hotsheet_resource_claim`), keep the renewal token, renew while it works and release
when done; the `hotsheet-worker` skill's resource claims section teaches the same
pattern, and that nobody claims on its behalf. The same start operation with other
resources is a conflict at the host. A start with no resources sends none, so an older
host reads it; one with resources is refused by a host that predates them. A person or
tool can start a session with them too (`session start --resource`, MCP `session_start`
`resources`). The broader shared skill of HS2-SHDW2Q (fencing, ticket-claim guidance,
bound backends) remains.

### Declaring a requirement

A requirement is `{ "namespace", "resource" }` in the claim service's names (§36.1),
written `<namespace>/<resource>` on a command line. It is declared in two places:

| Where       | How                                                                      | Applies to                        | Limit        |
| ----------- | ------------------------------------------------------------------------ | --------------------------------- | ------------ |
| A plan step | `resources` of `readiness define` (§26.15); CLI `--resource`, repeatable | That step's work                  | 4 per step   |
| The policy  | `placement.resources[]`, each `{ "target_id", "resources" }` (§26.8)     | Every piece of work on the target | 4 per target |

Work requires the union of both, each resource once. A step's set is stored sorted,
so the same set in another order is the same definition; a changed set is a new
revision and wakes the scheduler; defining a step without `resources` withdraws
them. A requirement is not a prerequisite: readiness (§26.15) does not look at it.

Validation refuses a malformed name, a resource named twice and more than four, on
a step before any target is read. Policy validation adds `invalid_resource`,
`invalid_resource_count`, `duplicate_resource`, `duplicate_resource_target` and
`resources_without_target` (the entry names a target the policy does not list). A
policy without requirements is stored as before.

### Whose resource it is

A namespace is machine-scoped (§36.1), so a resource name means something only on
one machine. A requirement is read on **the machine that hosts the target**: the
capacity read of §26.10 lists that machine's live claims for the namespaces its
ready work names, over the reviewed transport when the target is hosted elsewhere.
The same name on two machines is two resources; two targets on one machine share
that machine's. Each claim reaches the scheduler as a namespace, a resource and an
expiry, never a holder, a token or metadata.

One read lists at most eight namespaces. A namespace beyond that is unknown, not
assumed free (HS2-KCEHVR).

### What a pass decides

For each piece of confirmed work, in rank order, each required resource is in one
state:

| State        | Meaning                                                                               | The work                                                |
| ------------ | ------------------------------------------------------------------------------------- | ------------------------------------------------------- |
| `free`       | No live claim, and no better-ranked work was given it                                 | May be placed                                           |
| `held`       | A live claim holds it until `expires_at_ms` unless renewed or released                | `resource_held`, retried at the expiry                  |
| `taken`      | Nobody holds it; better-ranked work was given it in this pass                         | `resource_held`                                         |
| `unreadable` | Its claims could not be read and the backend fails closed                             | `resource_unknown`                                      |
| `unverified` | Its claims could not be read and the backend allows provisional claims                | May be placed; the next one is `resource_held`          |
| `not_read`   | The machine that hosts the target was not read (unavailable, or in a refusal backoff) | The target's own reason; never handed to a free session |

Work is given every resource it requires when it is placed, whether a session
starts for it or a free session takes it, so one resource is given to one piece of
work per pass. The first resource that defers work, in the stored order, is the one
the reason's `detail` names. Work deferred for one resource takes none of its
others. A claim is over at its expiry, so work may be placed at that instant.

### Release, expiry and renewal

- **Expiry.** A `held` resource's expiry is a retry time of the schedule
  (`retry_at_ms`). The loop looks again then. A holder that renewed is still
  holding, and the new expiry is the new retry time.
- **Release.** After a pass that deferred work for a `held` resource of its own
  machine, the loop waits for that resource with the claim service's wait
  primitive (`wait_until_free`, §36.12), on one thread per resource and at most 16
  at a time. When the wait ends, the scheduler's wake marker is written and the
  loop runs a pass. The wait is the backend's release signal (§36.6), not a poll,
  and a resource is waited on once however many passes defer work for it.
- **Another machine (HS2-G1PKK2).** For a `held` resource on the machine of a remote
  target, the loop waits with that machine's own wait. It finds the owner's target
  bound to that server, passes the gate as `gates_observe` (it never asks; again
  after 10 minutes), and long-polls the hosted
  `GET /checkouts/{checkout}/target-resource-wait?namespace=&resource=&wait_ms=` over
  the reviewed target transport. That route waits on the hosting machine's claim
  backend with the same wait primitive for at most 8 seconds (`MAX_RESOURCE_WAIT_MS`,
  under the transport's read timeout) and answers `{free}`. Free (a release, or the
  lease lapsing) writes the wake marker; still held (renewed, or nothing happened)
  asks again at once. The watch ends without a wake when the peer cannot be asked
  (unreachable, a revoked registration, an older peer without the route), the gate
  refuses, a held answer comes back long before the wait asked for (a backend that
  cannot wait, so nothing spins), or after 24 hours. Then the expiry is still the
  retry time, and the next pass arms the watch again. Remote watches count against
  the same 16 and are keyed by server, so a resource of the same name on this
  machine is another watch.

A one-shot `plan`, `explain` or `tick` waits on nothing.

### When a claim backend cannot be reached

What to do is a choice of the backend's configuration (§36.7), and the machine that
hosts the target reports it with its capacity (`claims_offline`):

| Offline policy          | A pass                                                                                                    |
| ----------------------- | --------------------------------------------------------------------------------------------------------- |
| `fail_closed` (default) | Work that needs a resource of that namespace is `resource_unknown`. Work that needs nothing is unaffected |
| `provisional`           | The best-ranked work starts with the resource marked `unverified`; its session takes a provisional claim  |

Under `fail_closed`, an applied pass that started nothing on that target counts
the unread claims as a refusal, so its machine is asked again after one minute,
doubling to 30 (§26.17), not on every event. Under `provisional` a second piece of
work is never started on the same unverified resource in one pass, and when the
backend returns, the claim the session took is read like any other. A provisional
claim is never treated as exclusive.

Backend configurations do not exist yet (HS2-JJWESD). Every namespace is served by
the local machine backend, which is never offline; a namespace whose stored state
cannot be read fails closed. The `provisional` policy is reached only through the
claim-source seam (`ClaimSource`), which the configured backends will implement
(HS2-KCEHVR). An older peer that reports no policy fails closed.

### Reading contention

`scheduler plan` and `tick` add `contention`: one entry per resource that deferred
work, or that work was started on unverified, with `server_id` (the machine),
`namespace`, `resource`, `state`, `expires_at_ms` for a held one, `unreadable`
(`backend_unreachable` or `namespace_limit`), `taken_by` (the step given it) and
`waiting` (the steps deferred, best first). It is left out when empty. The pass
record of the decision log counts `held_resources` and `unreadable_namespaces`,
and each deferred step's record carries the reason, the resource and the expiry.

`scheduler explain` adds `resources`: every resource the work requires with its
state, its machine, the expiry or the step that was given it, and `declared_by`
(`step` or `target`). The orchestration workspace shows none of this yet
(HS2-5GRXFN).

A step's run is dispatched before its session starts (§26.19), so the step is in
progress, and out of the ready work, before the session can claim anything: its own
claim never defers its own step (HS2-AK75HT). Only a run whose session was lost is
placed again while the lost session's claim may still be live; it waits for that
claim's expiry like any other holder's.

### Headless surface

```sh
hotsheet-cli ... orchestration readiness define --ticket <slug> --target <target-id> \
  --connection <source> --id <ticket> --resource vm/macos-vm --project <owner>
hotsheet-cli ... orchestration policy set --file <policy.json> --project <owner>
hotsheet-cli ... orchestration scheduler plan --project <owner>
hotsheet-cli ... orchestration scheduler explain <step-id> --project <owner>
```

No orchestrating route was added: `readiness/define`, the policy body, the capacity
snapshot and the scheduler responses carry the new members, and MCP
`hotsheet_targets` `readiness_define` takes `resources`. The hosted
`/checkouts/{checkout}/target-resource-wait` (HS2-G1PKK2), like the hosted
`/checkouts/{checkout}/target-gates` of §26.13 (HS2-WDQZ37), is machine-to-machine
plumbing of the scheduling loop and is ordinary authenticated project data; a person
or tool waits on the same backend with `hotsheet-cli resource show <ns>/<res> --wait`
or the `resource-claims` routes.

## 26.21 Progress and cost summary (HS2-S13719)

One read answers how an owning checkout's runs are going across its targets: lead
time, queue wait, blocked time, retries, CI and deployment outcomes, and known AI
cost. It carries identifiers, states, counts, durations and amounts only: no ticket
title, body, note or command output. It answers from this machine's records, asks no
one and writes nothing, like the gate reads of §26.13. It contacts no target except a
remote target's server for a work item's cost, once per remote work item (HS2-B6236S,
below).

It is a pure fold (`orchestration/summary.rs`, `summarize_progress`) over the runs
and their histories and the run event journal (§26.9), validation results (§26.12),
gates (§26.13), control actions (§26.14) and orchestrator-owned sessions (§26.16).
The answer does not depend on the order in which records are read.

### Known or unknown, never zero

Every measure is `{"status":"known","value":…}` or
`{"status":"unknown","reason":"<code>"}`. An aggregate counts its known and unknown
parts separately and lists the reasons, so a total never silently leaves something
out. The reason codes:

| Code                       | Meaning                                                                 |
| -------------------------- | ----------------------------------------------------------------------- |
| `run_unfinished`           | The run has not finished, so it has no lead time yet                    |
| `never_dispatched`         | The run was never claimed on its target                                 |
| `history_truncated`        | The run's bounded history no longer holds the entries the measure needs |
| `events_trimmed`           | Retention removed run events older than the oldest one kept             |
| `clock_inconsistent`       | Recorded times run backwards                                            |
| `no_gate_linked`           | No CI gate stands for the run                                           |
| `no_deployment_requested`  | No deployment was requested for the run                                 |
| `gate_unverified`          | A gate stands with no trustworthy verdict                               |
| `target_unbound`           | The target has no reviewed destination on this machine                  |
| `target_remote`            | The target's remote server does not offer the hosted cost read          |
| `target_unreachable`       | The remote target's server could not be asked for the cost              |
| `usage_source_unsupported` | The target ticket's source is not a Git store                           |
| `usage_store_unavailable`  | The usage records could not be opened or read                           |
| `no_usage_recorded`        | The usage records hold nothing for the target ticket                    |
| `cost_not_read`            | No cost reading was taken for the work item                             |
| `decisions_trimmed`        | Retention removed scheduler decisions the measure needs (HS2-XK7TJS)    |
| `cap_not_configured`       | The policy sets no session cap for the tool or machine (HS2-XK7TJS)     |

### Measures of a run

- **Lead time**: from the run's creation to the transition into the state it
  finished in.
- **Queue wait**: from the run's creation to its first claim on the target.
- **Blocked time**: the time the run was paused (§26.14), taken over by a person
  (`intervened`), or its target could not be confirmed (the `target_unreachable` and
  `target_reachable` events). `total_ms` counts overlapping causes once, so it can be
  less than the sum of `paused_ms`, `intervened_ms` and `unreachable_ms`. An interval
  still open is measured to the run's finish, or to `as_of_ms` for an unfinished run.
  A run history is bounded to 64 entries and the event journal is trimmed; a run
  whose history is full, or that began before the oldest event kept in a trimmed
  journal, reports the measure as unknown instead of a lower figure.
- **Retries**: validation attempts after the first that name the run, sessions
  started to replace a lost or failed one, and handoffs. These are counts of retained
  records.
- **CI and deployment outcome**: the rolled-up state of the run's standing gates
  (§26.13). A gate in `unknown` is `gate_unverified`, not a verdict.

### Work items and attribution

A **work item** is one target ticket, `(connection_id, native_id)`. Dispatching the
same target ticket again is a second run of the same work item (a `redispatch`), and
two targets whose checkouts share a ticket source reach the same work item, because a
Git source's connection id identifies the store.

Attribution rule `work_item_earliest_run`: a work item's cost is counted once and
belongs to the target of its earliest run. A session is counted once by its host's
id for it (server and terminal), whatever number of records name it, and belongs to
the target of its earliest record. Each run belongs to its own target. So every run,
work item and session is in exactly one target's figures, and the targets add up to
the totals.

Narrowing to one orchestration ticket (`ticket`, with an optional `connection_id`)
keeps only that ticket's runs. A target ticket dispatched for two orchestration
tickets then appears in both narrowed answers and once in the whole one.

### Known AI cost

Cost comes from the usage records of [14](14-metrics-interface.md), the same source
as the analytics dashboards (HS2-38RJMK): `metrics::ticket_cost` reads the events
attributed to the target ticket in its Git store. It adds the settled rollup's cost
for the ticket and the priced raw events newer than the rollup, so an event is not
counted on both sides. There is no second telemetry source.

- For a target whose reviewed destination is a checkout on this machine, the store is
  read here.
- **Remote targets (HS2-B6236S).** The target's server reads its own store: the
  summary asks `GET /checkouts/{checkout}/target-tickets/cost?connection_id=&native_id=`
  through the reviewed transport, which answers the same reading (identifiers and
  metrics only). Under the permission model (§26.7) this is an observation,
  `tickets_read`: it never asks anyone and is audited as a read by the orchestration
  project's own orchestrator (`orchestrator.<owner>`, §26.17). The host's own unknowns
  pass through unchanged. A server without the route reports `target_remote`, and one
  that cannot be reached or answers otherwise reports `target_unreachable`. The
  summary's per-work-item loop asks once per work item and prefers a readable target,
  so a cost is never counted twice.
- Raw usage events are per device, so the figure is what the machine that holds the
  target ticket's store recorded.
- `unpriced_events` counts newer events with no cost; when it is above zero the
  figure is a lower bound.
- A ticket with nothing recorded reports `no_usage_recorded`, not a cost of zero.
- `cost.complete` on a rollup is true only when every work item has a reading and no
  counted event lacks a cost. Only then is `known_usd` a total.
- When a work item was reached through several targets, a target that can be read is
  preferred over one that cannot.

### Throughput and utilization (HS2-XK7TJS)

`utilization` measures the whole owning checkout; narrowing to one orchestration
ticket does not narrow it, because sessions and the scheduler's decisions are shared
by every ticket. It is a second pure fold (`orchestration/summary_use.rs`,
`summarize_utilization`) over the session records (§26.16), the scheduler's decision
journal (§26.17) and the policy's caps (§26.8).

- **Sessions per tool and per machine** (`tools`, `machines`): distinct sessions,
  running time (`session_ms`, from each transition into `running` to the transition
  out of it, or to `as_of_ms`), the most running at once (`peak_sessions`) and the cap.
  A session is counted once by its host's id for it (server and terminal), whatever
  number of records or targets name it. A session that never ran is not counted; one
  whose bounded history lost its start makes `session_ms` `history_truncated`.
- **Utilization** (`utilization_permille`): running time over the cap's allowance from
  the first time a session ran (`from_ms`) to `as_of_ms`, in thousandths. The tool cap
  is the policy's `max_sessions` for the tool. A machine is the server that hosts the
  session, and its cap is the strictest machine limit (§26.20) of the targets its
  sessions ran for. With no cap the measure is `cap_not_configured`.
- **Idle time** (`idle_ms`): the time the scheduler kept sessions without work, from
  a `keep` decision with reason `idle_grace` or `keep_warm` to the next decision about
  that session that is not one, or to `as_of_ms`.
- **Limited time** (`limited`): per step, the time it stayed unplaced because no tool
  account could take it (`no_tool_available`, §26.18) or a resource it needs was held
  or unreadable (`resource_held`, `resource_unknown`, §26.20), until a decision placed
  it, with the number of steps each cause held back.

The decision journal logs only passes that changed something, so an episode lasts
until the next recorded decision about its subject. When retention removed decisions,
idle and limited time are `decisions_trimmed` rather than a lower figure. The panel
that shows these measures waits on design sign-off (HS2-2XJSJK).

### Waiting on external systems (HS2-9FXASE)

`external_wait` is owner-wide like `utilization`. It is a third pure fold
(`orchestration/summary_wait.rs`, `summarize_external_wait`) over the gates (§26.13),
the scheduler's decision journal (§26.17) and a tally of the provisional ledger
(§26.22). Each system reports `wait_ms` and `waited`, the number of runs (for gates)
or plan steps (for provider limits) that waited at all.

- **CI** (`ci`) and **deployments** (`deployments`): the time gates of that kind were
  `pending`, from each transition into it to the transition out, or to `as_of_ms`. The
  gates of one run and kind are joined, so a gate that supersedes another does not
  count the same wait twice. With no gate of that kind the wait is
  `no_gate_linked` or `no_deployment_requested`. A gate whose bounded history (32
  entries) no longer holds its first transition makes the wait `history_truncated`.
- **Reviews** (`reviews`): the time deployment gates were `awaiting_approval`, waiting
  for a person to approve the deployment, joined the same way. Code review is not
  recorded by the orchestrator, so it is not measured.
- **Provider limits** (`provider_limits`): the time a plan step stayed unplaced
  because every tool account it could use was rate- or quota-limited
  (`no_tool_available` with detail `limited`, §26.11), until its next other decision.
  A step held back by a local cap is limited time of HS2-XK7TJS, not a provider wait.
- **Idle while waiting** (`idle_waiting_ms`): the time the scheduler kept sessions
  whose only work was to wait on their ticket's gate (`waiting_on_gate`).
- **Provisional work** (`provisional`): records started provisionally, those rebased
  onto a fix after a red gate, and those reopened after one. The ledger keeps at most
  1024 records and forgets settled ones first, so at that bound the counts are
  `history_truncated`.

Journal measures are `decisions_trimmed` when retention removed decisions.

### Not measured yet

The workspace view is HS2-XJEH08; the summary panel waits on design sign-off
(HS2-2XJSJK).

### Headless surface

```sh
hotsheet-cli ... orchestration summary --project <owner>
hotsheet-cli ... orchestration summary --project <owner> --ticket <ticket-id> \
  [--connection-id <source>]
```

`GET /checkouts/{owner}/orchestration/summary[?ticket=<id>&connection_id=<source>]`
is machine-local. The hosted `GET /checkouts/{checkout}/target-tickets/cost` (HS2-B6236S)
is ordinary authenticated project data, so a remote orchestration project can reach it. MCP `hotsheet_targets` has the `summary` action with an optional
`ticket` of `{native_id, connection_id?}`. All three call
`OrchestrationRuntime::progress_summary`.

## 26.22 Provisional work across targets (HS2-JSF985)

A step at the `provisional` level (§26.15) waits on a gate that is still pending. A
project can decide that such work starts anyway, so sessions stay productive while
target CI runs, and that a gate's result then settles what was built on it. It is
**off unless the orchestration policy opts in**. Without `speculation.allowed`, and
without a target entry that sets `speculation: true`, the dispatch refusal, the
schedule and every output are exactly what they were before this section: nothing is
read, computed or stored for it, and no response gains a member. A test pins that.

### The decision

One pure function, `provisional_standings`
(`crates/hotsheet-ticketing/src/orchestration/provisional.rs`), answers for every
provisional step whether it may start now. It does not restate the rule of §26.18: it
projects the plan onto that rule's input and calls `blocker_readiness`.

| The rule's input (HS2-2SJPCH)                                | What it is across targets                                                              |
| ------------------------------------------------------------ | -------------------------------------------------------------------------------------- |
| A closed prerequisite ticket                                 | A prerequisite that holds (§26.15)                                                     |
| A ticket in `final_testing` with a recorded `CI gate commit` | A prerequisite whose standing CI gates name one commit                                 |
| `blocked_by`                                                 | The prerequisites, not yet holding, of the step that works the ticket                  |
| The `speculate:never` tag                                    | The policy's `opt_out_tag` on the target ticket, as last read                          |
| `GateProbe::gate`, `GateProbe::reachable`                    | The rolled-up gate of the commit on this machine (§26.13); only a watched gate attests |
| `SpeculationPolicy { enabled, max_depth }`                   | Every policy below allows it; the smallest depth limit among them                      |

What the rule cannot know is decided first, and an unknown is a refusal:

- **Both policies.** The orchestration policy must allow speculation for the working
  target and for each prerequisite's target (`speculation.allowed`, or the target
  entry's own `speculation`). Each of those target projects must also opt in itself,
  through its committed `speculation` setting ([05](05-ai-tool-plugins.md) §5.9.1). A
  target's machine reports that setting in its capacity snapshot (§26.10) as
  `speculation: { enabled, max_depth }`, and only when the project opts in, so the
  snapshot of every other project is unchanged. A target that answers without it has
  not opted in (`target_speculation_disabled`); a target that could not be read is
  `target_speculation_unknown`.
- **Depth.** The limit is the smallest of the policy's `speculation.max_depth` and
  the `max_depth` of every involved target. Unconfirmed gates are counted through
  the plan, across targets, by the rule's own count.
- **Opt-out.** Tags are read only while the policy opts in, so a plan defined before
  the opt-in holds none and its steps are `opt_out_unknown`. A dispatch and a
  scheduling pass read the named tickets again through the gate first, as
  `readiness refresh` does.
- **Several commits (HS2-08F1M8).** A prerequisite whose latest run links standing
  CI gates about more than one commit (workflows gated at different commits) is
  built on with every commit as a base: each commit is judged by its own gates as if
  the prerequisite pinned it, a green commit is no base, a red one refuses as a
  single red gate does, and the depth counts them side by side, not stacked. The
  record carries one gate per commit, and the work is confirmed only when all are
  green. A commit whose own gates are unknown is still `gate_commit_ambiguous`.
- **Not speculated on.** A prerequisite that needs a `deployment` while its CI gate
  pends (`deployment_gate_pending`, HS2-QBERH6).

Refusal codes are the ones above, `policy_speculation_not_allowed`,
`prerequisite_speculation_not_allowed`, `target_speculation_disabled`, and the rule's
own (`speculation_opted_out`, `speculation_depth_exceeded`, `gate_red`,
`gate_superseded`, `gate_cancelled`, `gate_commit_reachability_unknown`). The answer
is `ProvisionalStanding`: `allowed`, `refusal`, the prerequisite it is about, `depth`,
`max_depth`, and `bases`, each a prerequisite ticket with its commit, its gate ids
and its depth.

A prerequisite whose gate already passed while its ticket is not yet confirmed
closed is allowed with no base: nothing is pending beneath it.

### Reading a target's own setting (HS2-FJ6MDW)

A target project's own setting is read alone with `GET
/checkouts/{checkout}/target-speculation`, which answers the project's committed
setting (`null` when it does not opt in) and samples nothing. The dispatch guard and
a scheduling pass read it through the gate as `gates_observe`; a pass that already
sampled a target's capacity uses the setting that came with it. An older host
without the read is asked for its capacity as before.

The target's change feed (§26.9, `target-events`) carries the same setting with every
answer. While the owner's policy opts in, a follower that sees it change stores it,
publishes the new readiness and wakes the scheduler; an unchanged answer stores and
wakes nothing. Like every observation, a dry-run pass still stores what it read: it is
what the target said, not a decision.

### Dispatch

`run dispatch` (§26.9) on a provisional step asks the policy. Without the opt-in for
the step's target the answer is `readiness_provisional`, as before. With it:

1. The own setting of each involved target is read fresh as `gates_observe`, and the
   tickets the step names are read again as `tickets_read` (observation never asks),
   so a prerequisite that moved on since the last read is judged as it stands now.
2. The rule decides. A refusal is 409 `readiness_provisional_refused`, audited as
   `provisional_dispatch` with the rule's code; `readiness show` names it under
   `speculation`.
3. Building ahead asks a person for that exact operation under
   `provisional_dispatch` (§26.7) unless a remembered rule allows it:
   `permission_required` until someone answers, an Allow Once used once the run is
   dispatched, and a denial is final for that operation.
4. The work is dispatched and recorded in the ledger with the gates it rests on. The
   orchestration ticket gets one activity note naming the step, the operation, each
   gate and commit, and the depth, by identifiers only.

A step that rests on no pending gate is dispatched and not recorded: there is nothing
a gate result could change.

### Scheduling

Under the opt-in the scheduler (§26.17) reads the rule's answer for every provisional
item and treats an allowed one like confirmed work: free sessions first, then every
cap, limit and resource. A start is marked `provisional` with the reason
`provisional_work`; a refused item is `speculation_refused` with the rule's code.

- **Rank.** Provisional work keeps `100 - speculation.risk_weight` percent of its
  score, and confirmed work goes before provisional work of the same score. At the
  default weight of 50 a provisional item ranks with confirmed work two priority
  bands lower.
- **Depth.** Only work the rule allowed is placed, so no placement stacks more
  unconfirmed gates than the smallest limit.
- **Real sessions.** A start is applied only when the policy also sets
  `placement.automatic`; otherwise the pass is a dry run that shows the start it
  would make. An applied start passes the policy with the provisional flag, so
  `speculation` and the `provisional_dispatch` permission apply to it, and the
  started work is recorded under the start's operation id (`sched-start-<step id>`).
  A later dispatch of the same step keeps that record.
- **Targets read.** A target with only provisional work is contacted when the policy
  opts in for it, and the targets of its prerequisites are asked for their own
  setting. Nothing more is contacted than that.

### Ledger and gate results

Work started this way is recorded in the ledger of §26.18, `SpeculationLedger`, kept
per owning checkout in `${HOTSHEET_HOME}/orchestration-provisional.json` (private,
locked, atomically replaced, version 1, never created by a read) with what the
ledger does not hold: the step, the targets, and the gate ids. The file also keeps
each target's own setting as last read, so reads answer without contacting a target.

**Retention (HS2-J3SMMT).** The ledger holds at most 1024 records
(`MAX_LEDGER_RECORDS`). Every write that changes the file first forgets the oldest
`confirmed` and `reopened` records beyond that bound, by their latest event, together
with their step and target entries, as the gate registry forgets settled gates.
`provisional` and `at_risk` work is never forgotten, so the file can exceed the bound
only while that much work is still open.

`provisional_reconcile` brings the ledger in line with this machine's gate records.
For each gate commit a record still waits on, it reads what §26.15 would read for
that prerequisite now, the standing CI gates of the latest run on its ticket or of
the pinned commit, and applies it:

| Gate                                                         | Effect on the work built on it                           | Event         |
| ------------------------------------------------------------ | -------------------------------------------------------- | ------------- |
| `passed`                                                     | `confirmed` once every gate it rests on is green         | `confirmed`   |
| `failed`                                                     | `at_risk`, with the provider run named and one note owed | `at_risk`     |
| Stands for another commit now (the fix)                      | Rebased onto that commit and its gate; at risk no longer | `rebased`     |
| `pending` after `unknown`                                    | `provisional` again                                      | `provisional` |
| `superseded` with nothing in its place, `unknown`, cancelled | Stays `provisional`, listed under `reevaluate`           | `reevaluate`  |

The transitions are the single-project ones. A red gate stays red until a green
reading or a rebase replaces it; a re-run that is pending, a cancelled run or a lost
watch never clears the risk. A fix is either a newer commit linked on the same run,
which supersedes the red gate (§26.13), or the gate of a new run on the prerequisite
ticket. A replacement that is itself red is repaired first, and the work stays at
risk on the gate it knows. At-risk work recovers by `rebase_onto_fix`, or by
`reverify_by_covering_run` when its own run already links a CI gate. Nothing is
discarded: a record keeps every transition.

Each change is one `provisional` event in the run event stream (§26.9). Like a
readiness event it names the step as `step_id` (HS2-J7RG42); `reason` is the new state and `gate_id`
the gate behind it. Each at-risk mark is one activity note on the orchestration
ticket naming the step, the operation, the prerequisite ticket, its commit, the gate
and the provider run. The ledger's lock is held from finding a note owed to marking
it written, so a follower and a request reconciling at once write it once.

**Repair urgency.** At-risk work counts as work stacked on the failed gate: each such
step adds to the `red_gate` score of the work on the prerequisite ticket (§26.17),
beside the steps that gate blocks. `explain` on that step lists them as
`provisional_dependents`.

**Reopen.** When the fix invalidates the work, `readiness reopen <step> --reason
<code>` marks it `reopened` with a reason code. It needs an identified actor, is
audited as `provisional_reopen`, publishes a `reopened` event and adds a note.
Confirmed work cannot be reopened, a reopened record is never confirmed by a later
result, and the step's next provisional start is a new record. The run on the target
is not touched; cancelling it or handing it off is the run's own control (§26.14).

### When it runs

Nothing here polls. The ledger is reconciled where a gate change is published
(§26.13), so a result reaches the work in the same step that recorded it; in `run
reconcile`, for one run or all of an owner's, which is also what a server does on
start and reconnect (§26.14); and at the start of every scheduling pass. Applying the
same gate records again changes nothing and publishes nothing, so a result that
arrived while the orchestrator was down is applied exactly once by whichever of
these runs first. While a gate pends, the scheduling loop runs no pass, contacts no
target and rewrites nothing; the `provisional` event of a result is what wakes it.

### Views

- `readiness speculation` lists every record with its state, depth, gates and their
  results, `repair_first` (each failed prerequisite with the steps stacked on it) and
  `reevaluate`.
- `readiness show`, `plan` and `ready` add `speculation` (the rule's answer for a
  provisional step) and `provisional` (the recorded work of a step).
- `run status` adds `provisional` for a run whose target ticket has recorded work.
- `scheduler explain` adds `speculation`, `provisional` and `provisional_dependents`,
  so it names which work is provisional on which gate from either side.

The workspace view of provisional state is HS2-0D60T8.

### What is not reused from the single-project code

- **The probe.** The single-project `GateProbe` reads Git and the CI watch Jobs of
  one checkout. A target may be on another machine, so the cross-target probe
  answers from the gate records of §26.13 instead.
- **Pool storage.** `Coordinator::speculation_*` reads and writes `speculation.json`
  under the worker pool's lock. An orchestration owner has no pool; the same ledger
  type is stored in the orchestration file above.
- **The integration rule (HS2-ERYQDV).** `integration_rule` refuses a pool worker's
  `mark --phase integrating`. Integration of target work stays with the target's own
  workflow (§26.8), which this machine does not drive, so the run reports the rule
  rather than this machine enforcing it:
  - the working target reports its own rule beside `enabled` and `max_depth`, in its
    capacity snapshot and `target-speculation`, only when it is `parked`;
  - the record stores that rule, and the dispatch note says when integration is
    parked;
  - each piece of work shows `integration` and `held`. `held` means it must stay off
    the target's shipping branch now: parked and not yet green, at risk, or reopened.
    The session and the target's integrator read it with the run's status.
- **Notes on the target ticket.** The single-project loop notes the dependent and the
  failing ticket. Across targets the note goes on the orchestration ticket: nothing
  is written to a target for this.

### Acting on at-risk and reopened work (HS2-2K0DM8)

What the orchestrator does next stays within policy. A judgment call is asked on the
orchestration ticket, and the person answers with the run's existing controls.

- **At-risk work still on its branch** that a fix rebased (`rebased` event): the
  orchestrator-owned session working its run is told to rebase onto the fix commit
  and rerun the affected gates (session request kind `rebase`, with `commit`). Like
  every tell it passes the gate as `sessions_manage` continuing, asks nobody, and is
  never sent to a person's session. Each fix commit is told once per piece of work;
  a tell that could not be delivered is retried by the next reconciliation. Work
  already on its target's shipping branch is re-verified by the next covering run
  and gets no tell.
- **Reopened work whose run is still active** is a judgment call, so nothing is done
  to the run. The orchestration ticket gets one `FEEDBACK NEEDED` note with a
  `CHOICE`:
  - cancel the run (`run cancel`), then dispatch the step again once its
    prerequisites are confirmed; that start is a new record on confirmed state;
  - hand it off (`run handoff`);
  - keep it.

  A person's cancel holds the step as §26.24 describes, so it is dispatched again
  only by that person's next dispatch. Reopened work whose run has ended asks nothing.

### Not built

- Automatic re-dispatch after a reopen: a person's cancel holds the step, and the
  orchestrator never cancels a reopened run on its own.

### Headless surface

```sh
hotsheet-cli ... orchestration readiness speculation --project <owner>
hotsheet-cli ... orchestration readiness reopen <step-id> --reason <code> --project <owner>
hotsheet-cli ... orchestration readiness show <step-id> --project <owner>
hotsheet-cli ... orchestration scheduler explain <step-id> --project <owner>
hotsheet-cli ... orchestration run reconcile --project <owner>
```

No route was added: `speculation` and `reopen` are operations of
`/checkouts/{owner}/orchestration/readiness/{operation}`, machine-local like the
plan. MCP `hotsheet_targets` adds `readiness_speculation` and `readiness_reopen`.
The policy fields are those of §26.8; the target's own setting is set with
`coordination speculation-policy` in that project.

## 26.23 End-to-end coverage across servers (HS2-2EY9DP)

Each section above has its own real-server test. A shared fixture exercises the
whole loop across them, through the shipped server routes, the server's own
scheduling loop and followers, the hosted halves over mTLS, the stdio MCP tool, the
CLI and a real Job monitor process, so that a seam between separately built parts
shows up as a failing scenario that names it.

**The fleet** (`crates/hotsheet-server/tests/remote_servers/e2e.rs`). Two servers on
synthetic homes: the controller hosts the orchestration project and target A in the
store it serves, and a peer reached over a reviewed mTLS registration hosts target B.
Each target runs a different fake AI tool (the workspace `hs-fake-agent` as a
Claude-like tool that takes its role by session push and a Codex-like tool that takes
it at launch). CI evidence comes from a loopback GitHub Actions provider observed by
the Job monitor the server starts. Nothing leaves loopback; no AI tool, provider
account or GitHub repository is contacted. Where a scenario plays an agent (joining
the pool, claiming, finishing), it does what the worker skill does, through the
hosting server's own ticket routes. The fleet declares and binds each target, which
is the consent to read it, and a person remembers the `work_start` and
`session_start` rules its loop needs ahead of time through `permission-rules`, the
route behind `permission remember` (§26.7, HS2-XHQ1EH), so no scenario waits on a
permission card.

| Scenario                                                | What it shows                                                                                                                                                                                                                                                         |
| ------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `e2e_loop` the loop completes a plan across two servers | A readiness plan where B waits on A's ticket and CI; automatic placement starts a session on each server in turn; A's gate goes green through the monitor; both tickets complete through the change feeds; both sessions wound down                                   |
| a restarted controller resumes the plan                 | Restarts while A's gate waits and while B's remote session works: the gate is observed again, the remote session is adopted, nothing starts twice                                                                                                                     |
| a partial target outage                                 | The peer drops off the network: B's run reads `unreachable` and stale, A's work completes meanwhile, nothing replaces B's session, and B completes from the replayed feed when the peer returns                                                                       |
| lost access mid-run                                     | The target unbound mid-run: typed denials in the answers and the audit journal, nothing started or stopped, and binding it again lets the run finish                                                                                                                  |
| cancelling a remote run                                 | A person's cancel holds the step (§26.24): nothing is dispatched for it, its session is told and wound down once its worker lets go, and a person's new dispatch ends the hold                                                                                        |
| duplicate requests                                      | Concurrent passes over HTTP and the stdio MCP tool and replayed scheduler operation ids give one run, one session and one terminal                                                                                                                                    |
| out-of-order and replayed events                        | A worker hands the ticket over and it is finished while the controller cannot see the peer; the run settles once from the latest state, and repeated syncs and reconciliations record nothing                                                                         |
| a refusing peer and a limited tool                      | A rate-limited tool fails over to the other allowed tool; the peer owner's admission (set by the CLI) refuses the start; after the backoff the replacement start lands on the peer                                                                                    |
| an external-provider target (HS2-30XYPY)                | A third target on the peer reads its tickets from a `github` connection, answered by a loopback provider from a local store; a direct dispatch is refused as `target_operation_unsupported`, a dispatcher-mode run marks the ticket Up Next and a cancel withdraws it |
| a contended single-holder resource (HS2-30XYPY)         | Another holder claims the VM on the peer's machine; the loop defers the step that requires it as `resource_held` and starts nothing, and places it once the claim is released                                                                                         |

No scenario is quarantined. The scenarios found nine defects. Fixed: HS2-YZBCX5
(§26.7), HS2-2C5N1W (§26.9), HS2-KM9CTG (§26.17), HS2-257Q2Z (a remote session freed
without a run event is wound down from the session change the peer's feed carries,
§26.9), HS2-WC3W2G (a cancel the gate denies is answered as the denial), HS2-68DWAM (a
shown reconciliation no longer outlives a later state change), HS2-3REXQ7 (a session
lost while its run waits on CI is no longer replaced, §26.24), HS2-7Q428Z (concurrent
passes each logged a start, §26.24) and HS2-HV9ED3 (a person's cancel of a scheduler
run is respected, §26.24). Open: HS2-HJ64ZX (a race in the out-of-order scenario's
event snapshot). The external-provider and contended-resource scenarios are in
`e2e_coverage.rs` (HS2-30XYPY).
Checks that need a real AI tool, a real GitHub repository or Windows and Linux hosts
are in [the manual test plan](manual-test-plan.md#orchestration-end-to-end-with-real-tools-and-hosts-hs2-2ey9dp).

## 26.24 Keeping the automatic loop moving

The automatic loop (§26.17, §26.19) must not get stuck without saying so. This
section collects what keeps it moving when a session or a target does not do what
the loop expected.

### One live session per run (HS2-DG0YPA)

A run has at most one live session. A session start for a run (`run_id`) under a new
operation id, and a link of a running session to a run (a refill, §26.19), is
refused with `session_run_held` (HTTP 409) while another `requested` or `running`
session of the same owner holds that run. The check is made in the session registry
under its file lock, so two starts that race cannot both be recorded. The same
operation id is still the same start, whatever holds the run.

| The run's other live session, after asking its host | `session start`                      | The scheduler's start                   |
| --------------------------------------------------- | ------------------------------------ | --------------------------------------- |
| None                                                | Starts                               | Starts                                  |
| `running`, the orchestrator's, same target          | `session_run_held`                   | Adopts it: outcome `adopted`, no launch |
| `running`, a person's                               | `session_run_held`                   | Refused, `session_run_held`             |
| `requested` (its host has not confirmed it)         | `session_run_held`                   | Refused, `session_run_held`             |
| Its host cannot be asked                            | `session_run_held`: nothing assumed  | Refused, `session_run_held`             |
| It crashed (the host no longer has it)              | Learned as `lost`; the start goes on | Learned as `lost`; the start goes on    |

- **The holder is asked first.** Before deciding, a start syncs each other live
  session of the run with its host (§26.16, "Crashes"), as the starting actor. A
  holder that crashed is then `lost` and holds nothing, so its replacement starts,
  naming it in `replaces` when the caller does. A holder whose host cannot be asked
  keeps the run: a start is refused rather than doubled.
- **The scheduler adopts.** `OwnedSessions::placed_start` checks the run's holder
  before it starts anything. A running orchestrator-owned holder on the step's target
  is adopted: `StartOutcome::Adopted { session_id, run_id }`, logged as outcome
  `adopted` with that session and run and audited as `scheduler_start`. Nothing
  launches, and the target's placement memory is unchanged, so an adoption starts no
  cooldown. Any other holder is a typed refusal, logged with detail `session_run_held`
  and backed off like any refused start (§26.17).
- **A refill of a held run** is refused by the link (`assign` answers `refused`
  with `session_run_held`) and links nothing.
- `run_session_holder(owner, run_id)` answers the run's latest live session; it is
  the read reconciliation (§26.14) and the start path share. Reconciliation still
  reports two live sessions for one run as `different_live_work`, for records an
  older build or another process left.
- The pure core is `run_holder(sessions, run_id)` in
  `crates/hotsheet-ticketing/src/orchestration/sessions.rs`.

No route, command or MCP action was added: the start, the refill and the decision
log carry the new code and outcome.

### A run no target dispatcher will assign (HS2-T16SK0)

A placed step's run is queued in dispatcher mode (§26.19): the target's dispatcher,
or a legacy integrator, assigns its ticket. A target whose coordination pool has
neither live never assigns it. The loop now notices that, says so, and after a
bounded wait lets the session claim the ticket itself. Starting a dispatcher or
integrator session for the target is HS2-TK9ADC and is not done here.

**Which runs.** A run waits on its target's pool when a running worker session the
orchestrator owns is linked to it, and the run is in dispatcher mode, `queued`, not
paused and not being cancelled. Each pass reads the pool once per target with such a
run, through the hosted session listing (its `roles`: `dispatcher_live`,
`integrator_live`), as the pass's actor through the gate (`sessions_manage`
continuing). Nothing else is read for it.

**The decision** is the pure `decide_assignment`
(`crates/hotsheet-ticketing/src/orchestration/assignment.rs`):

| The target's pool                               | `state`               | What a pass does                                                               |
| ----------------------------------------------- | --------------------- | ------------------------------------------------------------------------------ |
| A live dispatcher or legacy integrator          | `awaiting_dispatcher` | Nothing: it assigns the ticket                                                 |
| Neither live, within the wait                   | `no_live_dispatcher`  | Reports it; the schedule's `retry_at_ms` names the end of the wait             |
| Neither live, the wait over, the session joined | `no_live_dispatcher`  | An applied pass claims the ticket for the session's worker id                  |
| Could not be read (or no actor to read as)      | `pool_unknown`        | Reports it and concludes nothing; no time is named, the next event looks again |

The wait is `placement.dispatcher_wait_minutes` (default 2, §26.17), counted from
when the run was queued. A claim is only given to a session that joined the pool in
its role (§26.16, "Joining"); one that has not is reported with `joined: false` and
no time, and its joining, published as `session_joined`, wakes the loop. A session
that never joins is replaced (below). A pass decides from what it reads then: a dispatcher that
comes up during the wait is left to assign, and one that leaves (its lease lapses)
brings the claim back. The decision has no memory of its own, so a restarted server
names the same time from the run's records.

**The claim.** An applied pass claims the run's target ticket for the session's own
worker id (the hosted `/target-claims` claim the direct dispatch uses), through the
gate as `work_start` continuing the run's own dispatch, so a permission the dispatch
needed is not asked again. The run becomes `dispatched` to that worker with reason
`claimed_for_session`, a note goes on the orchestration ticket, and the act is
audited as `scheduler_assignment`. A ticket someone else claimed first is refused as
a conflict and the run is synced at once, so it learns who holds it and nothing is
tried again. Any other failure backs the target off like a refused start (one minute,
doubling to 30, §26.17), and the backoff's end is the next claim time. A dry run
reports and never claims.

**Reported** in three places:

- the `assignments` member of `scheduler plan` and `tick` (run, session, step, state,
  the two pool flags, when it was queued, `claim_at_ms`, and an applied pass's
  `outcome`, `claimed` or `refused` with the code);
- a decision-log record of kind `assignment` for each run in `no_live_dispatcher` or
  `pool_unknown`, with the session, step and run, the reason, `retry_at_ms` (the
  claim time) and the outcome. `awaiting_dispatcher` is not logged;
- `scheduler explain` for the running step: reason `no_live_dispatcher` or
  `pool_unknown`, `retry_at_ms`, and the `assignment`.

The session's role prompt asked it to request its ticket from the dispatcher. After
the claim the ticket is held under its worker id, and the session is told so by a
`claimed` request (HS2-R7QH35, below).

No route, command or MCP action was added: `plan`, `tick`, `explain` and
`decisions` carry the new members on every surface.

### A session that never joins its role (HS2-NPTKS8)

A session confirms it took up its role only by registering in the target's pool
(§26.16, "Joining"). One whose role prompt failed, or that never registers, would
otherwise count as busy capacity for its step for good. The scheduler acts on it.

**Which sessions.** Running worker sessions the scheduler placed (operation id
`sched-start-*`), owned by the orchestrator and linked to a run. A person's session,
and one started by hand with `session start`, are never acted on.

**The decision** is the pure `decide_join`
(`crates/hotsheet-ticketing/src/orchestration/joining.rs`), from this machine's
records as the pass's listing brought them in line with the host:

| The session                                                 | What a pass does                                                           |
| ----------------------------------------------------------- | -------------------------------------------------------------------------- |
| Joined (`joined_at_ms`), whenever that was                  | Nothing, ever                                                              |
| Not joined, within `join_minutes` of running                | Kept; the end of that time is a retry time of the schedule                 |
| Its role prompt `failed`                                    | `wind_down`, reason `role_prompt_failed`, at once                          |
| Not joined when `join_minutes` is up                        | `wind_down`, reason `never_joined`                                         |
| Either, after two sessions of its run were stopped unjoined | `keep`, reason `never_joined`, detail `replacements_exhausted`: a person's |

`placement.join_minutes` defaults to 10, the role prompt's own delivery window, so a
prompt still pending then has failed anyway (1 to 1440).

**Winding down and replacing.** The decision replaces the session's own placement in
the computed schedule before anything is applied, so `plan`, `tick`, the decision
log and the audit show it like any wind-down. It is applied by the ordinary
wind-down (§26.19): through the gate under `sched-stop-<session>`, stopped only at a
safe point its host confirms, and never forced; a session not at one is kept and
tried again on the next event. Such a wind-down starts no cooldown on the target,
because it is a replacement, not a change of direction. The stop is published on the
run's stream, and the next pass finds the run with no live session and places it
again (§26.19, "Repeats and replacements"): a new session under
`sched-start-<step>.<n>` that names the old one in `replaces`, on the same run.

**Bounded.** Replacements are counted from the records: sessions of the same run
that stopped without ever joining. After two, the next session is kept and reported,
so a tool that cannot take up its role is not restarted for ever. Nothing runs on a
timer.

### Telling a refilled session its ticket (HS2-R7QH35)

A session started for a run is asked for that run's exact ticket by its role prompt
(§26.16). A session given a run later, by a refill (§26.19) or by a claim made for it
because no dispatcher is live (above), is told the ticket by a **request**, through
its hosting server's session push with the same readiness-aware delivery as the role
prompt.

| `kind`     | When                                                                   | The message asks the session to                                     |
| ---------- | ---------------------------------------------------------------------- | ------------------------------------------------------------------- |
| `assigned` | Its run is waiting for the target's dispatcher (a refill)              | Request the named ticket from the dispatcher with `hotsheet-worker` |
| `claimed`  | Its run is `dispatched` to its own worker id (the claim of HS2-T16SK0) | Work the named ticket, which is claimed for it, renewing the claim  |

**Hosted half.** `PUT /checkouts/{checkout}/target-sessions` takes
`{operation_id, ticket, kind}`. The host records the request on the hosted session
(`request`: the ticket's canonical id, the kind, when it was asked and its delivery)
and pushes it once, naming the ticket by its slug where the host can read it. A tool
that is not ready leaves it `pending`; the same events that retry a pending role
prompt (its push address registered, a turn ended) retry the request, after the
role, at most four times within ten minutes of the request; past either bound it
reads `failed`. There is no timer. The same ticket and kind again keep the request as
it is, so nothing is pushed twice; another ticket or kind replaces it. A person's
session is refused `session_human_owned`, an ended one `session_not_active`. The
hosted listing reports `request_prompt`.

**Orchestrating half.** `session_tell_ticket(owner, session_id, actor, tx)` sends the
request for the session's current, unfinished run, through the gate as
`sessions_manage` continuing, and publishes `session_request_<code>` on the run's
stream. The executor calls it after a refill links the run, and the assignment
fallback after its claim; both are best effort, so the assignment stands whatever
the push answered. Surfaces: `POST .../orchestration/sessions/{session}/request`,
`hotsheet-cli ... orchestration session request <session-id>`, MCP
`hotsheet_targets` `session_request`; each answers the session with
`request_prompt`.

### A run that waits needs no session (HS2-3REXQ7)

The scheduler's orphaned-run rule (§26.19) and reconciliation's session source
(§26.14, HS2-75Q254) now share one reading, `run_waits(owner, run, now)`: an
unfinished run waits, and needs no live session, while a CI or deployment gate of it
waits on a provider or a person, while its target ticket is in `final_testing`, or
while a validation attempt of it runs (`waiting_on_gate`, `waiting_on_validation`).
`session_need` answers `Waiting` for a dispatched run in that case, and the
scheduler does not treat such a run as orphaned work when its session is lost, so no
replacement starts while it waits. When the gate fails, the ticket leaves
`final_testing` or the attempt ends, the run needs work again and the next pass
places it on the same run.

A session is never refilled with the run it already holds: `assign` answers
`refused` with `session_holds_run` and links nothing. A session's record that a
concurrent reader (the loop, a follower) finished between a read and its update is
answered as that reader left it rather than as an error, so reconciliation does not
report a session it could not read in that race.

### One pass at a time (HS2-7Q428Z)

The server's loop, a manual `scheduler tick` over HTTP, the CLI and the stdio MCP
tool may all ask for a pass of the same owner at once. Each pass used to read the
step as ready, apply a start (the executor's operation ids made it the same session)
and log its own `started` record, so the log showed one start per concurrent pass.

Every pass (`tick`, and each pass of the loop) now holds the owner's pass lock,
`${HOTSHEET_HOME}/orchestration-scheduler-pass-<owner>.json.lock`, from before it
reads anything until it has logged. A second pass waits for the first and then reads
what it did: the session is running and busy with the run, so it starts and logs
nothing more. The lock is a file lock, so processes over the same home serialize too.
`plan` and `explain` change nothing and take no lock. The pure helper is
`scheduler_pass_lock(home, owner)`.

### A person's cancel is respected (HS2-HV9ED3)

**Decision.** A person who cancels a run the scheduler dispatched means it: the loop
does not dispatch that step again until a person dispatches its ticket again or
redefines the step, and the session working the run is told and wound down. Before
this, the next pass dispatched the step again as `sched-run-<step>.<n>` and the
session was told nothing, which undid the cancel. An AI actor's cancel keeps that
behaviour: the step is dispatched again.

- **Recorded.** `run_cancel` by a person records the run's reason as
  `cancelled_by_person` (an AI actor's stays `cancelled`).
- **Held.** A step is held while its latest scheduler run ended
  `cancelled_by_person`, no run of the same target ticket was dispatched after that,
  and the step was not changed (`updated_at_ms`) since. A held step is left out of
  the scheduler's work; `scheduler explain` answers `not_placed` with reason
  `cancelled_by_person`. Dispatching the ticket again (any `run dispatch`), changing
  the step's definition, or removing and planning it again ends the hold.
- **The session.** An orchestrator-owned session linked to the cancelled run stays
  listed as busy with it, so it is never refilled. Its placement becomes a
  `wind_down` with reason `run_cancelled`; before the wind-down an applied pass tells
  it through the same request path as HS2-R7QH35, with kind `cancelled` (stop, set
  your work aside, release your claim), pushed once. The wind-down is the ordinary
  one, at a safe point its host confirms: the claim its worker still holds keeps it
  until the worker lets go, and the session's turn end wakes the loop. Such a
  wind-down starts no cooldown. A person's session is left alone.
- The Up Next mark a dispatched dispatcher-mode run set is not withdrawn by this.

### Removing a step withdraws its run (HS2-YFPZFP)

**Decision.** Removing a planned step withdraws its work. Before, the step's
scheduler run (`sched-run-<step>`) stayed queued in the target, Up Next marked, and
the session working it stayed busy with it, so it was never wound down until a
person cancelled the run.

- **The run.** `readiness remove` cancels each unfinished scheduler run of the step
  (`sched-run-<step>` and its `.n` generations) before the step is removed, as the
  remover, through the ordinary cancel: a queued dispatcher-mode run withdraws its
  Up Next mark, a direct run releases its claim, and an already assigned run is
  recorded cancelled. The run's reason is `step_removed`. A run someone dispatched
  by hand for the same ticket is not the scheduler's and is left alone; a run that
  is already finished keeps its own reason.
- **Refused or unreachable.** A gate refusal of a cancel refuses the removal with
  that typed denial, and the plan, the run and the target are unchanged. A target
  that cannot be told leaves the run `cancel_requested`, as any cancel does, for a
  later cancel or reconciliation to finish, and the removal goes on.
- **The session.** A session linked to a `step_removed` run is treated like one whose
  run a person cancelled (above): it stays busy with the run, so it is never refilled
  while its worker may still hold the ticket, is told with a `cancelled` request, and
  its placement becomes `wind_down` `run_cancelled`, at a safe point its host
  confirms and without a cooldown. A person's session is left alone.
- **Again.** Removing the step again is `readiness_step_not_found` and cancels
  nothing. Planning the ticket again is a new step whose first run is
  `sched-run-<new step>`.

### Removing a step is an event (HS2-SQS266)

Removing a plan step used to publish nothing when it emptied the plan, and nothing
either when no other step's level moved, so the scheduling loop ran no pass and a
session that served the step was not considered for wind-down until some other
event or a named retry time. `readiness remove` now records one `readiness` event
for the removed step itself: `step_id` is the step (HS2-J7RG42), `reason` is `removed`, and its
dedupe key is `removed-<step id>`. It is the step's last event, so
`run events --run <step-id>` ends with it, and every follower of the owner's stream,
the loop above all, is woken by it like by any level change. A step id is never
reused: removing it again is `readiness_step_not_found` and publishes nothing, and
planning the same target ticket again is a new step whose levels start afresh.

### A changed choice ends an unconfirmed start (HS2-7VV79Q)

**Decision.** A start the host never confirmed stays `requested` and is repeated
under its own operation id while the scheduler's choice stands. When the scheduler
meanwhile chose another tool, model or account for the same work, the repeat used to
be a `conflict`, refused on every pass until the five-minute rule ended the old start
(`UNCONFIRMED_START_MS`). Now `placed_start` ends it at once through its host, with
the same stop the five-minute rule uses (no force, as the placement actor), and only
on the host's word:

| The host, asked to stop the unconfirmed start           | Then                                                                         |
| ------------------------------------------------------- | ---------------------------------------------------------------------------- |
| Has no record of it, or a reservation it never launched | Ended `never_started`; the new choice starts under `<op>.<n>`, `replaces` it |
| Launched it after all, at a safe point                  | Wound down; the new choice starts the same way                               |
| Launched it, not at a safe point                        | `refused` with the stop's code; nothing else changes                         |
| Cannot be asked                                         | `refused` with the error's code; the five-minute rule still applies          |

The new session works the same run: a scheduler run is dispatched once per step
(§26.19). A session its host confirmed is never ended this way: a changed choice for
it is still refused. The race the five-minute bound guarded against remains in a
shorter window: a start request still in transit that reaches the host after the stop
would launch under an operation the orchestrator ended; the host now fences it
(HS2-WHR4GS, below).

### A withdrawn start stays withdrawn (HS2-WHR4GS)

A stop that withdraws an operation its host never launched fences that operation on
the host, so a start for it that arrives later launches nothing. The fence is the
host's own record, kept by the hosted half of §26.16 (`HostedSessionRegistry`):

| The stop finds                        | The host                                           | The stop answers                                |
| ------------------------------------- | -------------------------------------------------- | ----------------------------------------------- |
| No record of the operation            | Fences it                                          | `stopped: false`, `session_not_found`, `fenced` |
| A reservation it never launched       | Releases it and fences it                          | `stopped: true`, `fenced`                       |
| A launched session (running or ended) | Fences nothing; stops it at a safe point as before | as before                                       |

- **A fenced start** is refused before anything is reserved: `outcome: refused` with
  the session code `session_withdrawn`, however often it arrives.
- **A stop during a start.** When the stop releases a reservation while its start is
  launching, the start finds its record gone once the tool is up, stops the terminal it
  just launched and answers `session_withdrawn`. Nothing runs untracked either way.
- **Bounds.** A fence lasts 15 minutes (`WITHDRAWN_FENCE_MS`), longer than any one
  request and than the five-minute rule for an unconfirmed start; a clock that went
  back keeps it. At most 1,024 fences are kept, the oldest dropped first. Fencing again
  restarts the time.
- **Storage.** `${HOTSHEET_HOME}/orchestration-withdrawn-sessions.json` (private,
  version 1), written under the hosted sessions file's lock and separate from it, so a
  binary without the fence still reads the sessions file. A damaged fence file refuses
  starts rather than letting one through.
- **Older hosts.** `fenced` is additive and absent from a host without the fence. The
  orchestrator ends the start as `never_started` either way; against an older host the
  race of HS2-7VV79Q remains.

The orchestrator never reuses a withdrawn operation id: the new choice starts under
`<op>.<n>`, so a fence never holds back live work.

## 26.25 Proposing the plan from goal tickets (HS2-HCW7MD)

The maintainer's decision on the workspace design (HS2-HSMH2Q, Design v3): "On Start
(or Propose Plan), an orchestrator AI session reads the goal tickets and proposes
target tickets, prerequisites and placement as the dry-run plan". The orchestration
project's own tickets are its **goal tickets**. **Propose Plan** starts one AI
session, the **planner**, that turns the open goal tickets into the plan of §26.15
with the existing operations. Nothing is placed: the plan stays a dry run until a
person presses Start Orchestration (the policy's `placement.automatic`, §26.17).

### Where the planner runs

The planner runs in the orchestration project itself, on this machine, through the
hosted half of the orchestrator-owned sessions (§26.16) with the project as its own
checkout. That is the drawer's ordinary AI terminal launch through the injected
session host (the server in process; a headless process through the local server's
terminal routes; a scripted host in tests), so the planner:

- is set up for its tool in the project with the project's own headless setup
  (HS2-WEFJV8) and passes this machine's admission (§26.10);
- shows in the drawer as `<orchestration project>: planner`;
- carries `HOTSHEET_SESSION_ROLE=planner` and `HOTSHEET_ROLE_SKILL=hotsheet-planner`
  and gets its prompt at launch or by session push, retried on readiness events with
  the same bounds as a role prompt (HS2-2DPGDY).

The orchestrating half of §26.16 (the permission gate, the session registry, the
scheduler's caps) is not used, because starting the planner acts on no target. `planner` is a
role of its own: it joins no coordination pool (`joined` is `null`), `session start`
refuses it for a target (`invalid_request`), and a hosted planner start must name the
checkout it runs in as its orchestration project.

### Propose and show

| Request                                                                   | What it does                                                                                     |
| ------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| `propose` with `tool`, optional `model`, `effort`, `goal`, `operation_id` | Starts the planner, or answers with the one already there                                        |
| `show`                                                                    | Where planning stands: `idle`, `planning` or `ended`, the live or latest planner, the open goals |

A press is decided by `decide_propose`, a pure function of this machine's planner
records and what the terminal host says now
(`crates/hotsheet-ticketing/src/orchestration/planning.rs`):

1. A retained `operation_id` this machine knows is that planner, whatever became of
   it. The same id never launches twice.
2. Otherwise a live planner, or one reserved and never launched, answers the press
   (`outcome: existing`). A planner whose host cannot be asked counts as live, so
   it is never doubled. One planner per orchestration project runs at a time; a press
   naming another goal while one works answers with that one.
3. Otherwise a new planner starts (`outcome: started`) under the caller's id or a
   fresh one, which is the `proposal_id` its prompt names.

Before anything launches the press is refused with `plan_no_targets` (the project
declares no target), `plan_no_goals` (no open goal ticket) or `plan_goal_not_open`
(the named goal is closed), all HTTP 409. A host that cannot launch answers as a
session start does (`session_host_unavailable`, `session_start_failed`,
`session_admission_refused`) and leaves nothing reserved, so the same press can be
repeated. A press needs an identified actor and is audited as `plan_propose` with its
operation id and `plan_started`, `plan_existing` or the refusal code.

`goal` names one goal ticket (slug or id, with its `connection_id` when the project
has several git sources); without it every open goal ticket is planned. The answer
lists the goals oldest first with the plan steps this machine holds for each (at most
256 listed; `goal_count` counts all), the number of declared targets, the planner's
hosted session, and how its prompt was delivered. `show` adds the plan's step count
and `placement_automatic`, so a view can say the plan is a dry run.

### What the planner does

The `hotsheet-planner` role skill (`plugins/shared/role-skills/hotsheet-planner.md`,
mirrored by `scripts/sync-role-skills.mjs` and installed by Claude and Codex setup)
uses the existing gated operations only:

- reads the declared targets (`list`, `resolve`), the goal tickets, each goal's links
  (`ticket links --resolve`), the plan (`readiness plan`) and every target ticket a
  goal names (`ticket show`);
- searches each target's open tickets for the work a goal describes with
  `ticket list` (HS2-D85G19) before it plans any of it;
- reuses a linked target ticket, links an existing one it named or found
  (`ticket link`, `observed`), or, only after searching, creates one (`ticket create`)
  under the operation id
  `plan-<goal-slug>-<work-key>`, so proposing again returns the ticket created before;
- defines one step per target ticket (`readiness define`) with its prerequisites,
  up to four resources (§26.20) and a preferred `tool`; defining a step again changes
  it, so a re-proposal updates the plan in place;
- turns a judgment call it cannot settle into a `FEEDBACK NEEDED` note with `CHOICE`
  options on the goal ticket, once, and plans the rest;
- never dispatches a run, starts a session, ticks the scheduler, changes the policy
  or answers a permission request, and ends with a `## Proposed plan` note per goal naming the
  tickets and steps by identifier.

Re-proposing is idempotent by goal ticket and target ticket: target tickets by the
create operation id and the goal's links, steps by the one-step-per-target-ticket
rule of §26.15.

**Preferred tool and model.** A plan step has an optional `tool` (a plugin id) and,
since HS2-TH53NN, an optional `model`, set by `readiness define --tool --model` (MCP
`readiness_define` `tool`, `model`) and shown with the step and in the ready work.
Changing either is a new revision; the same definition again changes nothing. The
scheduler tries them first (§26.17).

**Permissions.** Every action the planner takes on a target goes through the gate of
the operation it calls (§26.7): reading never asks, and an acting call no remembered
rule allows files an orchestrator permission request. The planner treats
`permission_required`, `permission_denied` and `decision_required` as a person's
decision, never something to retry around. Nothing in Propose Plan itself asks a target anything.

**Searching first.** The planner finds existing target tickets through what the goal
names, what is linked and the gated list and search of §26.3 (HS2-D85G19). The
workspace's Propose Plan button is HS2-J89JC7.

### Headless surface

```sh
hotsheet-cli ... orchestration plan propose --project <owner> --tool <tool> \
  [--model <model>] [--effort <effort>] [--goal <ticket>] [--source <connection>] \
  [--operation-id <retained-id>]
hotsheet-cli ... orchestration plan show --project <owner>
hotsheet-cli ... orchestration readiness define ... --tool <tool>
```

| Route                                                      | Methods and purpose                                        |
| ---------------------------------------------------------- | ---------------------------------------------------------- |
| `/checkouts/{owner}/orchestration/planning/{propose,show}` | POST Propose Plan and where planning stands; machine-local |

MCP `hotsheet_targets` adds `plan_propose` (with `tool`, `model`, `effort`, `goal` and
`operation_id`) and `plan_show`, and `readiness_define` takes `tool`. The runtime
functions are `OrchestrationRuntime::plan_propose(owner, ProposePlan, actor, tx)` and
`plan_status(owner, tx)` in `crates/hotsheet-cli/src/orchestration/planning.rs`.

## 26.26 The Orchestration view and its timeline (HS2-J89JC7)

An orchestration project's main view gains an **Orchestration** tab after Workers
(Lucide `workflow`). It shows the plan of §26.15 as a timeline of **dependency waves**
from the newest work on the left to the oldest on the right, as the maintainer asked
("oldest (like done) should be rightmost and newest should be leftmost"). The design
was signed off on HS2-HSMH2Q (Design v3).

### The timeline read

`OrchestrationRuntime::orchestration_timeline(owner, after, wait_ms, tx)`
(`crates/hotsheet-cli/src/orchestration/timeline.rs`) gathers this machine's records and
hands them to the pure fold `timeline` in
`crates/hotsheet-ticketing/src/orchestration/timeline.rs`. It reads the plan and its
readiness (§26.15), the runs (§26.9), the sessions this owner's orchestrator started
(§26.16), the latest logged scheduler decision per step (§26.17), pending permission
requests (§26.7), the bindings and the policy. It contacts no target, passes no gate and writes nothing.

| Field          | Meaning                                                                                                    |
| -------------- | ---------------------------------------------------------------------------------------------------------- |
| `declared`     | The owner has a target declaration: it is an orchestration project, even with no target yet                |
| `phase`        | `no_projects`, `no_plan`, `proposed` (nothing ever ran), `running` (`placement.automatic`), or `paused`    |
| `kick_off`     | `can_start`, `can_pause`, the policy revision, and why Start is refused (`start_blocked`)                  |
| `waves`        | Oldest first; each wave has a `kind`, an `ordinal`, an `estimated_start_ms` and what the estimate rests on |
| `version`      | Changes exactly when what the timeline shows changes; time alone does not change it                        |
| `wave_minutes` | The wave length estimates use: the median lead time of finished runs, or 60 minutes until one finishes     |

**Waves.** Finished steps are **Done**. Once orchestration was kicked off, a step with an
unfinished run is **Working Now**; every other step's wave is its depth over the
unfinished plan steps it waits on: nothing unfinished before it, or only work that is
running, is **Next**, one step deeper is **Then**, and anything deeper, and every step of
a cycle, is **Later**. Before kick-off the waves are numbered from the steps with nothing
unfinished before them (**First**, **Second**, …), after a Done wave when something is
finished. Working Now's estimate is when its earliest work started; Next starts when that
work should end (its start plus one wave length) or now, and each later wave one wave
length after the one before. A proposed plan's First starts on kick-off.

**Items.** Each item names its step, its goal (orchestration) ticket with slug and title,
its target project and ticket, the target's location (`local`, `remote` with the paired
server's label, or `unbound`), its tool (the live session's, else the scheduler's last
decision's, else the plan step's preferred tool of §26.25), its run summary, what it waits
on (`waits_on`, each prerequisite ticket with the step that works it), and its **state**:

| State                    | When                                                                                    |
| ------------------------ | --------------------------------------------------------------------------------------- |
| `done`                   | Its ticket is confirmed finished                                                        |
| `running`                | An unfinished run stands on it                                                          |
| `provisional`            | An unfinished run stands on it ahead of a pending gate (§26.22)                         |
| `waits_for_permission`   | The orchestrator's action on it waits for a person (see below)                          |
| `waits_for_decision`     | It is provisional or stale and its goal ticket has an unanswered `FEEDBACK NEEDED` note |
| `ready`                  | Confirmed and nothing waits                                                             |
| `waits_for_prerequisite` | Provisional, blocked, stale or cyclic otherwise                                         |

A target ticket's slug, title, status, priority and category are read from its store when
the target is a checkout on this machine. For a remote target the timeline has the slug
the plan's last read recorded (§26.15 now keeps it, an identifier) and nothing more.
Nothing the read finds is written anywhere.

**Remote titles (HS2-HDHT5E).** While the view is open, the web client reads each remote
target ticket that has no title once through the orchestration project's gated ticket
read (`POST /checkouts/{owner}/orchestration/tickets/read`, §26.3; adding the project
was the consent to read it, so it never asks), as this browser's person. At most four
reads run at a time and 24 per timeline version. The titles live in the view's memory
only: they are never written to this machine's records, a refused or failed read is
not retried while the view stays open and the card keeps its slug with "<slug> in
<project>", and switching project or closing the view forgets them. Each read is
audited in the owner's journal (identifiers only), like any target ticket read. A title
changed on the remote shows the next time the view opens.

**Waiting for permission, one seam.** The permission a step waits on comes from one
function: the scheduler's latest start decision for the step whose outcome is
`permission_required`, with the orchestrator request id as its detail (§26.7, §26.19),
reads as `kind: permission` while that request is still pending in the ledger. The
action is the one asked: `session_start` for `sched-start-<step>`, `work_start` for the
step's run `sched-run-<step>`. Once a person answers, the step no longer waits, before
the next pass runs. A judgment call (going ahead on an unconfirmed prerequisite) is a
decision note on the goal ticket (HS2-XHQ1EH).

**Waiting for a change.** With `after` equal to the current version and `wait_ms` (at most
20 seconds) the read is a long poll. It watches the owner's run event journal, its
scheduler decision log, the scheduler's wake marker, the permission signal of §26.28 and,
while a decision is pending, the owner's ticket store, and answers as soon as the version
differs, or with the same version at the deadline. Nothing polls on an interval. A person's
answer therefore ends the wait at once (HS2-ANQXQ4): an answer to the permission request a
step waits on changes the permission ledger, and a response to the goal ticket's decision
note changes the owner's store, without waiting for the next run event or scheduler pass.
The waiter's one-second safety re-check reads the timeline again too, from local files
only, so a change no watched directory reported (an event the platform watcher dropped)
ends the wait within a second instead of at the deadline (HS2-1C77XH).
A read on the server also starts the target followers of §26.9.

### The view

- **Tab.** The view switcher offers Orchestration only for an Orchestration project
  (HS2-W27N1D's Project Type creates the declaration), and never in the terminal rail. A
  project that is not one falls back to its ticket view. The client reads the project
  type together with the ticket sources while the project opens, before its first paint,
  so the Orchestration View button and the Orchestration settings category are in place
  from the start rather than inserted under the pointer later (HS2-AMCSKS). A type read
  in Settings or a type change supersedes it; a server that cannot say leaves it unknown
  and the view's own declaration read decides, as before.
- **Board.** The production ticket board (`TicketBoard`, Kerf `ColumnTrack`) with the
  waves reversed: Later, Then, Next, Working Now, Done; before kick-off Fourth … First.
  It opens scrolled to its right end with Working Now in view; on a phone (paged) it
  opens on Working Now. In each column the cards are grouped by project, and a project
  group collapses in the view's own scope. Kerf has no start-at-end option, so the app
  wiring sets the scroll position once per board layout and when the board's width
  changes (KF-QHJKA4).
- **Cards** are the shared `TicketRow` with its new **orchestration line**: the state in
  words with a tone (Ready, Waits on API-12, Waits for permission, Waits for your
  decision on LCH-1, Provisional on WEB-33; running and done say it through the status
  badge), where it runs (machine · tool) and the goal ticket. List and Columns take the
  same prop, and a target project's own List and Columns show it on its planned
  tickets (§26.29, HS2-S3FKJ7).
- **Inspector.** Selecting a card shows Where It Runs, then what it waits on (Show
  Request; Open <goal>, which opens the goal ticket; or its prerequisites), then its Run with the person's controls (Pause or
  Resume, Hand Off…, Correct… and Cancel Run…, through the run routes of §26.9 and
  §26.14, each a person's act with a confirmation where undo cannot help), then the
  run's gates, validation steps and latest events.
- **Show Request** (HS2-DBQSWB) opens the request the step waits on where every
  permission request is answered: the project's Notifications, Pending, with keyboard
  focus on that exact card (found by the orchestrator request id the card carries). The
  card shows the focus ring while it has focus and is never a Tab stop. A request this
  browser has not read yet is read first; one already answered leaves Pending showing and
  says so. On a phone the inspector sheet closes so the card is in view.
- **Kick-off.** With no plan, **Propose Plan** starts the planning session of §26.25 in
  the tool the person drives with in this project. A proposed or paused plan has
  **Start Orchestration**, which turns `placement.automatic` on through the policy route
  of §26.8; it is disabled, with the reason, while there is no policy or the project's
  type is Software (`project_software`, HS2-J3ZF27). No placement actor is needed: the
  orchestrator acts as the orchestration project (HS2-XYAHB4). A Software project's kept
  automatic policy does not read as running. While running, **Pause** turns it off; running work
  continues. A proposed plan is labeled as the dry run it is.
- **Empty.** A declared project with no target shows _No Projects to Orchestrate Yet_
  with **Choose Projects…**, which opens Settings until the Orchestration category of
  HS2-W27N1D exists (HS2-QFS6SG). Keyboard: cards are reachable with Tab and selected
  with Enter or Space. Up and Down move to the card above or below in the same column
  and Left and Right to the nearest column that has cards (at the same position or its
  last card), each selecting that card's plan step (HS2-T4V1TX). Moves stop at the
  board's edges, a collapsed project group is skipped, and the column comes fully into
  view; a modified arrow is left alone. On a phone an arrow move does not open the
  inspector sheet over the board, as Enter, Space and a tap do.

### Headless surface

```sh
hotsheet-cli ... orchestration timeline --project <owner> [--after <version> --wait-seconds 20]
```

| Route                                       | Methods and purpose                                                      |
| ------------------------------------------- | ------------------------------------------------------------------------ |
| `/checkouts/{owner}/orchestration/timeline` | GET the timeline; `after` and `wait_ms` wait for a change; machine-local |

MCP `hotsheet_targets` adds `timeline` (with `version` and `wait_ms`).

## 26.27 Project Type and the Orchestration settings (HS2-W27N1D)

Signed off on HS2-HSMH2Q (Design v3, the maintainer's answers): a project becomes an
orchestration project by its **Project Type**, chooses the projects it orchestrates in
**Settings → Orchestration**, and opens each one's detail from its row. The Orchestration
view and the ticket row's orchestration line are HS2-J89JC7; the planning session behind
Propose Plan is HS2-HCW7MD.

### Project Type

`project_type` is a shared, checked-in project setting (`.hotsheet2/settings.json`,
`Scope::Shared`): `software` or `orchestration`. When it is absent the project is
Software, unless it already has a target declaration: such a project declared targets
before Project Type existed and reads as Orchestration (HS2-J3ZF27). A value this
version does not know is refused rather than read as Software. Switching is one decision
table in the core (`plan_type_change`), and every client calls the same runtime workflow:

| From          | To            | Projects added | Result                                                                                   |
| ------------- | ------------- | -------------- | ---------------------------------------------------------------------------------------- |
| any           | same type     | any            | No change; an Orchestration project whose declaration is missing gets it back.           |
| Software      | Orchestration | any            | The setting changes; the empty target declaration is created on first use.               |
| Orchestration | Software      | none           | The setting changes.                                                                     |
| Orchestration | Software      | one or more    | Refused unless `keep_projects` is true; then the setting changes and nothing is dropped. |

Back to Software keeps the declaration, this machine's bindings and the policy, so a
later switch to Orchestration finds every project where it was. The web client asks
first, with the shared confirmation, and the General hint then says how many projects
stay. Each change is audited as `project_type_set` with the new type as its reason.

**A Software project orchestrates nothing (HS2-J3ZF27).** Whatever its kept policy
says, the server runs no scheduling loop for it, and `scheduler_tick` and
`scheduler_plan` are dry runs that place nothing. Every acting action on its targets
(run dispatch, session start, ticket writes, validation, deployments, building ahead),
including a person's own, is refused by the policy step of `authorize` as
`policy_denied` with the detail `policy_project_software`, audited like any denial;
`access` and the policy dry run report the same detail. Reading and following its
projects stay allowed, and so does reducing activity: sessions and runs already going
keep going until a person, or `session_stop` and `run_cancel`, winds them down. A
switch wakes the scheduler, and a switch back to Orchestration starts the loop again
when the kept policy places automatically.

### Adding and removing a project

Adding a project declares a target (§26.2) and binds it to an open checkout on this
machine or on a reviewed remote registration in one step: the checkout is inspected
first, so an unreachable or unknown one adds nothing; then the target is declared and
bound with the fresh candidate. Adding is a person's consent to let the orchestration
project read and follow that project (HS2-XHQ1EH): an AI actor is refused, nothing more is
written, and whatever the orchestrator later does there still passes the authorization
gate, which asks a person for acting actions (§26.7). The project type must be Orchestration and a project cannot orchestrate itself.
It is idempotent: a project already bound to the same checkout (same kind, remote
registration and checkout) is returned with `added: false`, and a caller-kept
`target_id` binds that declared target after an attempt whose bind failed. Audited as
`project_add`.

Removing a project removes this machine's binding, then tombstones the declaration, so
its id is never reused. It only reduces what the orchestrator can reach, so any actor
may do it; sessions already running there keep running until stopped. Repeating it
changes nothing. Audited as `project_remove`.

### Settings → Orchestration

Project Settings gains a project-scoped **General** category first (Project Type) and,
for an Orchestration project only, **Orchestration** right after it. The navigator's
category list and `SettingsCategory` carry both; Orchestration falls back to General when
the type changes to Software while it shows.

- **Projects.** One row per active declaration with its name, where it is bound (local
  on this computer, or remote on the reviewed server's name), and a connection badge from
  the cached binding. Its "…" menu has Show Project Tab (enabled while that project's tab
  is open) and Stop Orchestrating…. An empty project shows _No projects yet._
- **+ Add Project.** The other open projects first, local and remote with their machine,
  checked when already added; then **Open Another Project…**, which runs the regular
  open-project flow (HS2-BT4KN6). The project it opens appears in its own tab, is added,
  and the orchestration project's settings show again. A cancelled flow adds nothing.
- **Detail.** Selecting a row opens its detail in the inspector. Each part is its own read
  through an existing route, so a part the gate refuses says so in place and hides
  nothing else: binding and connection (a fresh `resolve` first, because the reads after
  it check the same binding), capacity and admission per policy tool, AI tool capacity,
  held resources (the namespaces the policy's `placement.resources` names for this
  project), and the orchestrator's active sessions there with **Show in Drawer** (while
  that project's tab shows the session's terminal), **Take Over…** and **Stop** (at a safe
  point; mid-turn asks first and then stops with `force`). **Stop Orchestrating…** removes
  the project after the shared confirmation. The app names its person `hotsheet-app` for
  reads and session actions that need an identified actor; adding the project is the
  consent to read it, so capacity and admission reads succeed without asking anyone
  (HS2-XHQ1EH, HS2-Y3N3PH). The id stays: the gate needs an identified actor, and
  observation never asks. Admission is asked only for the tools the policy lets
  sessions use on that project; with none the detail says _No AI tool is allowed here
  yet_ rather than showing the policy's refusal. A part that is refused says why by the
  refusal's code (the policy's limits, an unreachable project, a pending permission
  request), never "not allowed yet". Show in Drawer works the same for a remote project: its tab is
  `remote:<registration_id>:<checkout_id>` from the bound destination, the desktop bridge
  gives that tab the peer's terminals launched in its checkout (HS2-JJKVJJ), and the
  session's `hosted.session_id` is the peer's terminal id (HS2-6E32N4).
- **Policy, limits only.** Place sessions automatically, machine defaults, per-tool session
  caps and weights (an empty Sessions field leaves the tool out), each tool's
  **accounts** with their sessions and daily budget (HS2-7WF6JD: + Add Account and a
  remove button per row; a row without a name is dropped; the budget is typed in dollars
  and stored as whole cents, and more than two decimals is refused, never rounded),
  wind-down and build-ahead, saved with **Apply Policy** at the stored revision. The form
  never shows the policy's retired `targets[].actions` or `approval.required_for`
  (HS2-XHQ1EH: the server ignores and drops them); every member it does not show is
  written back unchanged.
- **A project's limits (HS2-7WF6JD).** The detail's **Limits** section, between Binding
  and Capacity and Admission, edits that project's entry in the policy (`targets[]`):
  Orchestrate in This Project (`enabled`), AI Tools Here (the policy's configured tools),
  **<Tool> Models** for each checked tool (HS2-YSY7AR: `tools[].models`, separated by
  commas, empty for any model; unchecking a tool drops its models), Sessions Here
  (`max_sessions`, at most the machine's limit), Build Ahead (as the policy says, on or
  never: `speculation`), **Required Resources** (HS2-YSY7AR: this project's
  `placement.resources` entry of §26.20, `<namespace>/<resource>` names separated by
  commas, at most four; empty removes the entry) and Use Different Machine Limits
  (`machine`, the four machine fields). Models and resources are checked as the server
  checks them before anything is sent. A project the policy does not list yet says so,
  since no session may start there. **Apply Limits** stores the policy at the stored
  revision with only that project's entry and resources entry replaced in place, or
  appended; every other project, resources entry and member is unchanged. An edit in
  progress survives a reload from `orchestration_changed`; a saved one follows the
  stored policy again, and admission and held resources are read again.

A project opened from another server cannot add projects here: the add binds on the
serving machine. The category says so and disables + Add Project (HS2-92FJXM).

### Live updates

The server publishes an `orchestration_changed` change event, `store` and `id` the owning
checkout, after every successful owner request that changes what these settings show
(type, declaration, binding, project add or remove, policy, session start, stop, takeover
or sync) and after each scheduling pass that acted. The web client reads the shown
category again on that event, on any `orchestration_session` event, and on a stream
resync; it never polls.

**Changes a CLI process writes (HS2-SHHGC6).** A `hotsheet-cli` process can change the
type, the declaration, this machine's bindings or the policy without a server request.
Once a client reads an owner's settings (GET `project-type`, `targets` or `policy`), the
server watches the files they come from with a native file watcher, without recursion:
the owner's `.hotsheet2` directory (watched through the project root until it exists)
and the machine home. A file event wakes it; after a 150 ms pause in events it reads
`orchestration_fingerprint` (a digest of the effective type, the declaration, the
policy revision and each declared target's destination, last failure and whether it
was confirmed; the times every resolve renews are left out, so reading the settings
never changes it) and publishes the same `orchestration_changed` event only when the
digest differs from the last one seen. A change the server makes itself is marked in
flight before it runs and records the new digest before it announces; while one is in
flight the watcher defers its comparison (at most 30 seconds), so the server's own
change is announced once (HS2-2EKDSM). A change that begins after the watcher's
quiet check, while it reads the settings, is left to the server too: the watcher
records and announces what it read only if no change of the server's own began since
its check, decided under the same lock that starts a change (HS2-N8VRFC). The watcher
reads nothing on a timer and
ends when the server stops. The watched paths are `orchestration_watch_paths`.

### Headless surface

```sh
hotsheet-cli -C <host-source> orchestration project-type --project <owner> [--set software|orchestration [--keep-projects]]
hotsheet-cli --actor-role human -C <host-source> orchestration project add <checkout-id> --project <owner> [--registration-id <id>] [--label <name>] [--target-id <ULID>]
hotsheet-cli -C <host-source> orchestration project remove <target-id> --project <owner>
```

| Route                                                | Methods and purpose                                                     |
| ---------------------------------------------------- | ----------------------------------------------------------------------- |
| `/checkouts/{owner}/orchestration/project-type`      | GET the type, declaration and active project count; PUT switch the type |
| `/checkouts/{owner}/orchestration/projects`          | POST add a project (declare and bind); machine-local                    |
| `/checkouts/{owner}/orchestration/projects/{target}` | DELETE remove a project (unbind and tombstone); machine-local           |

MCP `hotsheet_targets` adds `project_type` and `project_type_set` (with `keep_projects`).
Adding a project is a person's act and has no MCP action.

## 26.28 Orchestrator permissions in clients (HS2-XHQ1EH)

The permission requests and remembered rules of §26.7 use the client surfaces every
other AI permission uses ([05](05-ai-tool-plugins.md) §5.7); no orchestration screen
is needed to answer one.

- **The card.** A request arrives as the ordinary `permission_asked` nudge and
  permission card. Its status line reads `Orchestrator:` and the action sentence, such
  as "Create or change tickets in API", and its details name the requester (the
  orchestrator of the orchestration project), the action, the target project and,
  for a session start, the tool. The decision row offers Allow Once, Always Allow
  and Deny only; there is no Allow This AI Session and no chat answer.
- **No automatic answer** (HS2-42WS4Z). A project's Auto-allow or Auto-deny setting
  decides AI tool requests only. An orchestrator card shows no countdown, is never
  decided after 0 seconds or after a delay, and never continues into the Always rule
  review an automatic Allow offers, because only a person answers it. The server
  refuses that review (`continue_review`, `/permissions/{id}/remember`) for an
  orchestrator request.
- **Always Allow review.** Always Allow opens the usual confirmation, which names the
  rule owner (the orchestrator of the orchestration project, in the target project)
  and its lifetime ("Persistent, until revoked") and offers no rule-type or pattern
  choice, because the rule is keyed exactly. Confirm Always Allow remembers it.
- **Settings → Permissions.** A remembered orchestrator rule is listed with the other
  remembered answers. Its row names the action sentence, "Applies to: The
  orchestrator of" the orchestration project, and "Target project:" the target. The
  action sentence is set in body type, not the code type of a command (HS2-7NGC69), and shown in full with no Expand action (HS2-AY0XPM). It
  can be revoked there; editing its match, decision or scope is disabled.
- **Settling.** A request answered anywhere else (the CLI, another client, a rule
  remembered ahead) settles the card with `permission_resolved`. A request filed by
  a headless CLI while no server ran appears when the server starts or a client next
  reads the pending permissions.
- **Live from the CLI** (HS2-E16FWJ). Every change to the ledger (a request filed,
  answered, remembered or forgotten) writes a fresh token to
  `${HOTSHEET_HOME}/orchestration-permission-signal/token`. Each server waits on it
  with the platform file watcher and, when it changes, brings its bridge in line
  with the ledger. A request a headless CLI files therefore reaches connected
  clients as the ordinary `permission_asked` nudge, and a CLI answer settles the
  card, without any client reading `GET /permissions`. Idle, the follower publishes
  nothing and makes no request; it reads the local token at most once a second.

Remembered orchestrator rules live in the machine-local orchestration ledger, not in
the encrypted AI permission vault; `GET /permissions/rules` merges them into the
list it returns.

## 26.29 The orchestration line in a target project (HS2-S3FKJ7)

A project that an orchestration project on this machine orchestrates shows, in its own
List and Columns, the orchestration line of each ticket the plan works: the same line
the Orchestration view's cards carry (§26.26), built by the same mapping. The goal's
tooltip names the orchestration project the goal ticket belongs to ("Goal LCH-1: Ship
sign-on, in Launch"). Tickets no plan works have no line.

### The target-side read

`OrchestrationRuntime::orchestration_target_lines(checkout, after, wait_ms, tx)`
(`crates/hotsheet-cli/src/orchestration/target_lines.rs`) finds the orchestration
projects whose reviewed binding on this machine names the checkout as a local
destination (`BindingRegistry::bound_here`, at most 16 owners), reads each one's
timeline (§26.26), and keeps one line per target ticket through the pure fold
`target_lines` (`crates/hotsheet-ticketing/src/orchestration/target_lines.rs`). Like the
timeline it passes no gate, contacts no target and writes nothing; adding the project
was the consent to read it.

| Field          | Meaning                                                                                        |
| -------------- | ---------------------------------------------------------------------------------------------- |
| `orchestrated` | Some orchestration project on this machine binds the checkout, even with no planned ticket yet |
| `lines`        | One per target ticket, by connection and native id                                             |
| `version`      | Changes exactly when the lines change; estimates moving with the clock do not change it        |

Each line has the owner's checkout id and name, `proposed` (that plan was never
started, so the state reads as a dry run: "After API-12" rather than "Waits on API-12"),
and the timeline `item`. When several plan steps work one ticket, in one orchestration
project or several, the line shows the most pressing: running or provisional work, then
a wait for a person (permission or decision), then ready or waiting work, and finished
work last; ties go to the owner bound first, then the step id. `also_planned` counts
the others.

With `after` equal to the current version and `wait_ms` (at most 20 seconds) the read is
a long poll on the same records the timeline watches; a checkout nothing orchestrates
still waits to the deadline, so a reader that keeps asking never spins. Like the
timeline, it reads once more after its watcher starts and again at each one-second
safety re-check, so a change no watched directory reported ends the wait within a
second (HS2-F8174X).

### The client

The web client reads the lines once when a project's List or Columns shows, after the
next paint, so a warm project switch paints its cached tickets first (HS2-8YHY6V). While
the answer says `orchestrated`, a cursor long poll keeps them current with bounded backoff
on failure; a project nothing orchestrates holds no request open and is read again
when its ticket views show again (for example after adding it in the orchestration
project's settings and returning to its tab). Leaving the ticket views stops the read;
the lines last read stay in memory, so returning to the project paints them with its
cached tickets until the next read replaces them.

### A target on a remote server (HS2-3D8D6V)

When an orchestration project binds a target on a reviewed remote server
(`Remote { registration_id, checkout_id }`), the desktop that runs the orchestration
project answers that remote checkout's lines itself:
`OrchestrationRuntime::orchestration_remote_target_lines(registration_id, checkout_id,
after, wait_ms, tx)` matches the remote bindings (`BindingRegistry::bound_remote`) instead
of the local ones and otherwise is the same read: the same fold, version and long poll.
It reads this machine's records only and never contacts the remote server; an unknown
registration or checkout answers `orchestrated: false`. The same checkout id on another
registration is a different checkout.

The client routes a remote project's tab (`remote:<registration>:<checkout>`) to this
machine's server, through any open local project's API path, so the line shows in that
tab's List and Columns while the plan stays on this machine. With no local project open
the tab asks the remote project's own server. A viewer on another machine sees the line
through the owner's push, below.

### Pushing lines to a remote target's viewers (HS2-KB2Y51)

So that anyone who views a remote target sees its orchestration lines, the machine that
runs the orchestration project pushes them to the target's server, which keeps them as
transient, display-only observations. They are never plan records: nothing is written
to git or a ticket store on either side, and the owner's run, session, permission and
decision identifiers never leave the owner's machine.

**Payload.** `PUT /checkouts/{checkout}/target-line-observations` on the target's server,
sent over the reviewed remote binding (the transport `target-tickets` uses):
`owner_server_id` (the pushing machine's server id), `sequence` (increasing with each
push from that machine), `ttl_ms`, and `lines`: the part-A lines for that checkout,
trimmed to display facts (`display_line`: state, goal slug and title, ticket label,
tool and placement, prerequisites). Each push replaces that machine's lines; an empty
push withdraws them. The receiver drops lines whose connection is not one of the
checkout's own sources and reads the rest as running on its own machine
(`location: local`).

**Lifetime and expiry.** `ttl_ms` is clamped to 30 seconds through 15 minutes (default
10 minutes). The owner renews at half the lifetime while it has lines; lines an owner
stops renewing expire, and expired entries are pruned on every read and write.

**Two owners.** Each owner machine is its own entry (at most 8 per checkout), merged
after the receiving machine's own lines by the same fold (`target_lines_observed`); a
line names its machine in `observed_from`, and the same owner checkout id on two
machines is two steps. A repeated or older `sequence` is answered `stale` and changes
nothing, so an out-of-order push cannot roll lines back.

**Revocation.** Unbinding the target on the owner withdraws its lines on the next pass.
A revoked pairing or an unreachable server is not told; its lines expire.

**Who may post.** Any caller allowed to write project data on the target: a read-write
paired device or certificate, the grant the owner already uses to create target
tickets. A read-only device is refused by the existing write fence. Bounds: 256 lines and
256 KiB per push, 4 KiB per line, 30 pushes per owner machine and minute (429
`rate_limited`), 8 owner machines per checkout (409).

**Storage.** One private file per checkout under
`HOTSHEET_HOME/orchestration-line-observations/` (`LineObservations`). The target-lines
read merges the live entries, and its long poll also watches that directory, so a push
ends a waiting read at once; an expiry is seen at the one-second safety re-check.

**When the owner pushes.** The server's follower (`AppState::spawn_target_line_push`)
recomputes each remote destination's lines on a run event, a scheduler wake or the
one-second safety re-check, all from local files, and sends only a changed version, a
half-lifetime renewal, or a retry after a failure with a backoff of 2 seconds doubling
to 5 minutes, which is the re-assert after a reconnect. Pushes to one destination are at
least 2 seconds apart, so a burst of changes coalesces. A restarted server re-asserts
every destination once. With no remote target bound it sends nothing.
`OrchestrationRuntime::push_target_lines(state, owner, tx)` is the pass.

The client already reads the target project's own `target-lines`, so a viewer of the
target sees the pushed lines.

### Both halves in a remote tab (HS2-SHBSQF)

With a local project open, a remote project's tab reads both halves: this machine's
answer for the remote checkout (above) and the remote project's own `target-lines`, which
holds what other owner machines pushed. `mergeTargetLines` keeps this machine's line for
every ticket it plans (so this machine's own pushed copy never shows twice) and fills the
other tickets from the remote server. The two cursors travel as one combined version
`<here>~<there>` (`-` for a half not read yet). A long poll waits on both halves at once,
one open request each, answers as soon as either half answers and cancels the other; the
next wait reuses the other half's cursor. A half that fails keeps its last answer, or is
asked again with no cursor on the next wait, while the other half still waits; only when
both fail does the read fail, so the client's bounded backoff applies. No timer issues a
request. With no local project open the tab reads the remote server alone.

The remote server answers from its own point of view, so a line it places on its own
machine (`location: local`, every pushed line among them) runs on the remote host as
this browser sees it. A remote tab reads such a line as a remote placement named by the
tab's host (`Project.remoteHost`: the peer's live host name, else the reviewed origin's
host, as in the tab's name), and "Remote server" when the host is unknown; it never
reads "This machine" (HS2-CW8EY3). A line the remote server places elsewhere keeps its
label.

### Headless surface

```sh
hotsheet-cli ... orchestration target-lines --project <target> [--after <version> --wait-seconds 20]
hotsheet-cli ... orchestration target-lines --registration-id <remote> --project <remote checkout> [--after ...]
hotsheet-cli ... orchestration target-lines-push --project <orchestration project>
```

| Route                                                           | Methods and purpose                                                                                  |
| --------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `/checkouts/{checkout}/orchestration/target-lines`              | GET the lines; `after` and `wait_ms` wait for a change; machine-local                                |
| `/remotes/{id}/checkouts/{checkout}/orchestration/target-lines` | GET a remote checkout's lines from this machine's bindings (HS2-3D8D6V); machine-local               |
| `/checkouts/{owner}/orchestration/target-lines/push`            | POST one push pass to the project's remote targets (HS2-KB2Y51); machine-local                       |
| `/checkouts/{checkout}/target-line-observations`                | PUT another machine's display-only lines for a checkout served here (HS2-KB2Y51); read-write callers |

MCP `hotsheet_targets` adds `target_lines` (with `version` and `wait_ms`), called with
the target project as `checkout`; with `registration_id`, `checkout` names a checkout on
that remote server (HS2-3D8D6V). `target_lines_push` runs one push pass with the
orchestration project as `checkout` (HS2-KB2Y51).

- **A locked vault** (HS2-4K43DK). While the AI permission vault is locked or still
  loading, the full list answers 503 and Settings shows "Remembered Answers Locked".
  The orchestrator's rules do not depend on the vault: `GET
/permissions/rules?source=orchestrator` lists them alone, and Settings shows them
  below the locked notice, which says they still apply. Always Allow on an
  orchestrator card and revoking an orchestrator rule work in both states.
