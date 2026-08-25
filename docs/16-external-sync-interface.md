# 16. Pluggable Ticket-Provider Interface

> **Status: Revised design (HS2-QJ5TCT, 2026-08-26).** This supersedes the
> sync-first design from HS2-CARMDM. Hot Sheet's git-backed files remain the
> default and fullest-featured ticket provider, but they are no longer required.
> GitHub Issues, GitLab Issues, Jira, and future trackers may be authoritative
> providers directly, without a parallel Hot Sheet ticket repository.

## 16.1 Product rule

Teams should not have to adopt and reconcile a second issue tracker just to use Hot
Sheet's worklist, clients, CLI, MCP tools, or AI workflows. All those surfaces operate
on a normalized ticket API. A project connects one or more ticket providers:

- **`git` (default):** the current Markdown/frontmatter files, git history, semantic
  merges, offline writes, and git-native coordination.
- **`github`:** GitHub Issues is authoritative; issue numbers, labels, assignees,
  comments, and close state are read and written in GitHub.
- **`gitlab` / `jira`:** the corresponding tracker is authoritative using the same
  host contract and provider-specific mapping.

No external-provider connection implicitly creates a git ticket store. A project/code
repository may connect to multiple ticket systems—for example GitHub Issues for public
bugs, Jira for company planning, and a local git provider for private scratch work.

## 16.2 Provider identity and ticket references

A configured provider connection has a stable project-scoped id and a discoverable
locator:

```jsonc
{
  "id": "github-main",
  "provider": "github",
  "locator": "small-tale/hotsheet2",
  "name": "GitHub issues",
  "default": true,
  "settings": { "credential": { "secret": "github-small-tale" } }
}
```

The durable identity of a ticket is **`(connection_id, native_id)`**. Hot Sheet also
returns a stable, displayable reference such as `github-main#482` or `jira-eng:ENG-42`.
The git provider continues to expose its existing short slugs (`HS2-ABC123`) and
ULIDs. Providers must resolve their own references; the host must not assume every
tracker uses ULIDs, globally unique slugs, or file paths.

Cross-ticket fields such as `blocked_by` carry qualified references whenever the
target belongs to another connection. A project registry makes connection ids and
locators discoverable to clients and agents.

## 16.3 Normalized model, not a lowest-common-denominator schema

The host exposes one normalized `Ticket`, `TicketDraft`, `TicketPatch`, `Note`, and
`TicketQuery` vocabulary. Providers map native records to that vocabulary and retain
provider-native metadata needed for lossless updates (for example GitHub node ids,
Jira transition ids, ETags, or cursors). The normalized model includes the Hot Sheet
workflow concepts used across clients and automation: title/details, open/closed
outcome, status, category, priority, tags, Up Next, assignment/review, dependencies,
notes, attachments, timestamps, and coordination.

This is not permission to emulate unsupported fields in an invisible second ticket
store. A provider must do one of the following, declared per capability:

1. map the concept to an ordinary native field;
2. store it through an explicit provider-native extension configured by the team
   (labels, Jira custom fields, a namespaced issue-body block, etc.);
3. report it unsupported/read-only.

Clients and automation degrade deliberately based on capabilities. They do not show a
successful control whose result cannot be persisted by the authoritative provider.

## 16.4 Host contract and capabilities

`hotsheet-ticketing` owns the provider-neutral domain contract. A provider supplies
identity, capabilities, mapping, and CRUD/query behavior; clients never call provider
APIs directly.

```rust
trait TicketProvider {
    fn descriptor(&self) -> ProviderDescriptor;
    fn capabilities(&self) -> ProviderCapabilities;

    fn query(&self, query: &TicketQuery) -> Result<Page<Ticket>>;
    fn get(&self, id: &NativeTicketId) -> Result<Ticket>;
    fn create(&self, draft: &TicketDraft) -> Result<Ticket>;
    fn update(&self, id: &NativeTicketId, patch: &TicketPatch) -> Result<Ticket>;
    fn add_note(&self, id: &NativeTicketId, note: &NoteDraft) -> Result<Note>;

    // Optional operations are guarded by capabilities.
    fn claim(&self, id: &NativeTicketId, claim: &ClaimRequest) -> Result<Claim>;
    fn watch(&self, cursor: Option<&Cursor>) -> Result<ChangeStream>;
}
```

Capabilities are structured rather than a single “supported” flag. At minimum they
cover create/update/delete, searchable/filterable fields, notes, attachments,
assignment/review, dependencies, Up Next, close reasons, claims/leases, atomic batch,
offline mutation, history, watch/webhooks, and provider-side idempotency. A
conformance suite verifies both implemented behavior and honest rejection of absent
capabilities.

The host owns multi-provider routing, aggregation, pagination, normalized validation,
server/API/MCP presentation, caching, credential resolution, and capability-aware
errors. Providers own remote calls, native mapping, concurrency tokens, rate-limit
interpretation, and provider-specific durable metadata.

## 16.5 Default git provider

The existing implementation is extracted behind `TicketProvider` as the built-in
`git` provider without changing its on-disk format or weakening its guarantees:

- Markdown/YAML ticket files remain human-readable and directly editable.
- Git remains authoritative for that provider; SQLite remains a rebuildable cache.
- semantic merges, store sync, cross-store copy/move, offline writes, and git-native
  claims remain git-provider capabilities;
- existing CLI flags and `.hotsheet/store` links keep working as shorthand for one
  default git connection.

This compatibility requirement makes the abstraction an extraction, not a rewrite of
the working storage engine.

## 16.6 External authoritative providers

GitHub/GitLab/Jira providers read and mutate their native tracker directly. Local
SQLite may cache normalized records and cursors for speed/offline viewing, but it is
never an independent source of truth and can be discarded. Project configuration and
machine-local credentials are not a ticket repository.

Writes use provider concurrency controls where available (ETag/version/update token)
and return a typed conflict when the remote changed. Rate limits, auth failures, and
unsupported transitions remain distinguishable errors. Offline writes are allowed
only for providers that can durably queue and safely replay idempotent mutations;
otherwise external tickets are read-only while offline.

Provider-native ids and URLs remain visible so users can move naturally between Hot
Sheet and their organization's tracker.

## 16.7 Aggregation and routing

Queries can target one connection or aggregate all connections in a project. The host
fans out, normalizes, applies capability-aware filters, and returns a stable page with
qualified ids. Mutations always route to exactly one connection. New tickets use the
explicit connection or the project's configured default; ambiguity is an error.

Cross-provider operations are explicit compositions, not background synchronization
and not assumed atomic transactions. Copying a ticket asks the destination provider
to create a mapped draft. Moving creates at the destination and closes the source only
after creation succeeds; rollback/partial failure is surfaced explicitly.

Every transfer carries a stable operation id plus the source's qualified reference.
The destination provider records that provenance using a native idempotency facility
or a namespaced metadata marker. Retrying the same operation—including from another
collaborator—resolves the already-created destination ticket instead of creating a
duplicate. This metadata supports deduplication and traceability; it does not establish
an ongoing mirror relationship.

## 16.8 Auth, loading, and trust

Credentials live in the shipped OS-keychain registry and settings contain only secret
references. The server owns provider credentials; clients and MCP callers receive no
tokens. Built-in providers may live in a dedicated `hotsheet-providers` crate (HTTP
dependencies, no terminal dependency). External provider executables use the existing
trusted plugin loading model and a versioned IPC contract rather than Rust ABI dynamic
libraries.

## 16.9 No automatic cross-provider mirroring

Hot Sheet does **not** continuously mirror a ticket between two authoritative
providers. In a multi-user team, independent workers can observe and replicate the
same remote update before either one's asynchronous git commit arrives; retained
external numbers help deduplicate some creations but do not solve ordering, partial
failure, comment replay, conflicting edits, or split-brain ownership. A reliable
two-way mirror would become a distributed system of its own.

Use direct provider access for ongoing work. Use the explicit idempotent copy/move
operations in §16.7 for one-time transfer or migration. After a copy, the two tickets
are independent unless the user explicitly moves or copies again. Existing
`external` frontmatter from the superseded sync design is not part of the required
git ticket schema; an importer may consume it as migration provenance.

## 16.10 Testing requirements

- One provider-neutral contract suite runs against every provider fixture.
- The git provider retains its real-temp-repo unit/integration/E2E coverage.
- External providers use deterministic API fakes for mapping, pagination, rate-limit,
  concurrency, retry, and capability tests plus opt-in credential-gated live tests.
- Server/CLI/MCP E2E must run the same user flows against at least the git provider
  and a non-git reference provider.
- Aggregation gets transition/adversarial tests for partial outage, duplicate native
  ids in different connections, pagination, retries, and idempotent cross-provider
  copy/move under concurrent attempts.

## 16.11 Build plan

- **HS2-QJ5TCT:** revise and approve the provider architecture (this document).
- **HS2-ZVZP80:** extract the core `TicketProvider` contract, adapt the current git
  implementation, and add routing, aggregation, capabilities, and conformance.
- **HS2-JAXS4Z:** implement direct authoritative GitHub Issues as the reference
  external provider.
- **HS2-0RK4YC:** implement GitLab Issues and Jira against the proven contract.
- **HS2-A90JRH:** implement explicit idempotent cross-provider copy/move.
- **HS2-VFXFFP:** add provider-connection and capability-aware client UX. All
  core/CLI/server/provider behavior must be independently testable first.

## 16.12 Cross-references

- Git provider format and guarantees: [02-ticket-storage.md](02-ticket-storage.md)
- Index/cache behavior: [03-indexing-and-query.md](03-indexing-and-query.md)
- Server, CLI, settings, and secure keys: [04-core-server-cli.md](04-core-server-cli.md)
- Plugin loading/trust model: [05-ai-tool-plugins.md](05-ai-tool-plugins.md)
- Assignment/review semantics: [10-assignment-and-collaboration.md](10-assignment-and-collaboration.md)
