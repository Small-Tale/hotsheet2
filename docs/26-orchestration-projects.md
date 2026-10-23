# 26. Orchestration Projects

> **Status: Design only (HS2-4JFP0S).** An orchestration project has its own
> tickets and coordinates explicitly named target projects. No orchestration
> storage, commands, or routes in this document are shipped yet. The
> implementation tickets created by HS2-1WQDH3 own those surfaces.

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
`schema_version: 1` and a `targets` list. Each entry has an immutable random ULID
`target_id`, `state: active|removed`, a human label, and optional normalized,
credential-free expected repository identity. A label or repository URL is
descriptive, never an authorization token or lookup key. The file contains no
origins, bearer secrets, certificate material, local paths, or
customer data. IDs are never recycled; removal marks an entry `removed` and
retains its id while any orchestration ticket refers to it. Reordering or
renaming an entry preserves its id. Duplicate ids, unsupported versions, and
malformed entries fail explicitly.

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
until reviewed.

The declaration belongs to the orchestration _checkout_, independently of which
source holds its tickets. The same target can appear in several orchestration
projects under different ids and grants. A target checkout can still have its own
orchestration declaration. Cycle detection belongs to dispatch planning, not
target registration; a cycle never triggers recursive work automatically.

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

CLI and authenticated server operations share one core transaction and the same
validation errors. Proposed headless surface:

- `hotsheet-cli orchestration target list|add|rename|remove --project <checkout>`
  reads or changes the shared declaration. `add` allocates and returns the id;
  `remove` records a tombstone when links exist.
- `hotsheet-cli orchestration target bind --project <checkout> --target <id> --server <registered-server-id> --checkout <reference>`
  changes only the machine-local binding; `unbind` takes the project and target
  ids.
- `hotsheet-cli orchestration target resolve --project <checkout> --target <id> --json`
  returns declared, bound, reachable, and source-capability state without
  exposing secrets.
- `GET/PUT /checkouts/{id}/orchestration/targets` operates on declarations with
  a revision precondition. `GET/PUT/DELETE` at
  `/checkouts/{id}/orchestration/bindings/{target_id}` operates on the serving
  machine's local binding with an explicit identity confirmation on rebind.
  Equivalent MCP read and mutation tools use the same core functions and auth
  checks.

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
return a conflict rather than dropping another editor's target.

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
