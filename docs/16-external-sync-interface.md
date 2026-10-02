# 16. Pluggable Ticket-Provider Interface

> **Status: Partial implementation (HS2-QJ5TCT/HS2-ZVZP80, 2026-08-26).** This supersedes the
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
  "settings": { "credential": { "secret": "github-small-tale" } },
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

    fn query_page(&self, query: &TicketQuery, cursor: Option<&str>, limit: usize)
        -> Result<ProviderTicketPage>;
    fn summary(&self, now: &str, day_starts: &[String]) -> Result<ProviderTicketSummary>;
    fn get(&self, id: &NativeTicketId) -> Result<Ticket>;
    fn create(&self, draft: &TicketDraft) -> Result<Ticket>;
    fn update(&self, id: &NativeTicketId, patch: &TicketPatch) -> Result<Ticket>;
    fn add_note(&self, id: &NativeTicketId, note: &NoteDraft) -> Result<Note>;
    fn edit_note(&self, id: &NativeTicketId, note_id: &str, text: &str) -> Result<Ticket>;
    fn delete_note(&self, id: &NativeTicketId, note_id: &str) -> Result<Ticket>;

    // Optional operations are guarded by capabilities.
    fn claim(&self, id: &NativeTicketId, claim: &ClaimRequest) -> Result<Claim>;
    fn watch(&self, cursor: Option<&Cursor>) -> Result<ChangeStream>;
}
```

Capabilities are structured rather than a single “supported” flag. At minimum they
cover create/update/delete, searchable/filterable fields, note create/edit/delete, attachments,
assignment/review, dependencies, Up Next, close reasons, claims/leases, atomic batch,
atomic Not Working report,
offline mutation, history, watch/webhooks, and provider-side idempotency. A
conformance suite verifies both implemented behavior and honest rejection of absent
capabilities.

The host owns multi-provider routing, aggregation, normalized validation,
server/API/MCP presentation, caching, credential resolution, and capability-aware
errors. Providers own remote calls, opaque native page cursors, bounded page retrieval,
streaming count summaries, native mapping, concurrency tokens, rate-limit interpretation,
and provider-specific durable metadata. The built-in compatibility fallback may materialize
a provider query, but GitHub, GitLab, and Jira implement the bounded contract directly.

## 16.5 Default git provider

The existing implementation is extracted behind `TicketProvider` as the built-in
`git` provider without changing its on-disk format or weakening its guarantees:

- Markdown/YAML ticket files remain human-readable and directly editable.
- Git remains authoritative for that provider; SQLite remains a rebuildable cache.
- semantic merges, store sync, cross-store copy/move, offline writes, and git-native
  claims remain git-provider capabilities;
- Not Working stages evidence payloads and publishes the ticket metadata/status rename
  last, so readers observe either the completed ticket or the whole reopened report;
- existing CLI flags and `.hotsheet2/store` links keep working as shorthand for one
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
An unqualified id is probed against every linked source. A source whose native-id shape
cannot represent the id (a git ULID sent to GitHub, which numbers its issues) simply does not
hold it; a source that fails to answer (auth, rate limit, network) only surfaces its error
when no other source owns the id. Id-shape errors name the provider that raised them
(HS2-GKERTK).

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

Built in the foundation: provider-neutral string identities and qualified references;
structured capabilities and typed provider errors; deterministic git connection ids;
`TicketProvider` plus the default `GitProvider` adapter; project connection config
validation (non-secret settings only); partial-failure aggregation; provider identity
on ticket wire rows; `hotsheet-cli providers`, `hotsheet_providers` MCP, `GET
/providers`, and provider-scoped git REST routes. The completed provider increments
are summarized below.

Built in the transfer increment: `copy_between`/`move_between` preserve normalized
content, notes, assignment, and review requests; reject dependencies that lack an
explicit mapping and fields unsupported by the destination; and store
`transfer_operation_id` plus the qualified `transferred_from` reference. Deterministic
destination and note ids make retries converge, while an operation lock prevents
same-process collaborators from racing file updates. Move reports the already-created
destination if closing the source fails, so retry can recover without duplication.
Every copied or moved destination starts as Not Started and outside Up Next; workflow
state belongs to the destination project and is never inherited from the source.
The provider-neutral routes are `POST /provider-transfers/copy|move`; CLI and MCP expose
`provider-copy|move` equivalents. The older git-store copy/move surfaces retain their
same-ULID/tombstone compatibility semantics.

Built in the GitHub reference-provider increment: `hotsheet-extsync` maps GitHub issue
numbers/URLs, body/state/state-reason, configured `category:`/`priority:`/`status:`
labels, ordinary labels, assignees, and comments directly to the normalized contract.
It paginates while excluding pull requests, sends incremental `since` queries, exposes
webhook invalidations for authoritative re-read, checks opaque optimistic-concurrency
tokens, and maps authentication/rate-limit/conflict failures to typed provider errors.
Unsupported claims, dependencies, review requests, Up Next, and query
dimensions are declared or rejected rather than discarded. Attachments are supported only
through a configured assets repository (below).

**GitHub attachments through an assets repository (HS2-HSA64D).** GitHub has no
issue-attachment API usable by app or personal tokens (its `user-attachments` upload is a
browser-only flow), so, like the original Hot Sheet GitHub plugin, the provider commits each
file to a configured **assets repository** through the Contents API and links it from one
issue comment. The connection settings keep the original plugin's keys:
`attachment_repo` (`owner/repo`), `attachment_folder` (default `hotsheet-attachments`), and
`attachment_branch` (default `main`); the headless path is `github-connect
--attachment-repo/--attachment-folder/--attachment-branch/--no-attachments`, and a
reconnect without them keeps the repository. The provider reports `attachments: true` only
when `attachment_repo` is set; otherwise an upload fails with an explicit capability error.

- **Upload:** `PUT /repos/{assets}/contents/{folder}/{attachment-id}-{safe-name}` on the
  branch (the plugin's `[A-Za-z0-9._-]` sanitizer). The attachment id keeps the path unique
  and makes a retry address the same file; an existing file is reused.
- **Link comment:** `![name](url)` for images and `[name](url)` otherwise, where the URL is
  the permanent `raw.githubusercontent.com/{assets}/{branch}/{path}` form on github.com (not
  the short-lived `download_url`) and the file page's `/raw/` form on GitHub Enterprise. The
  comment ends with a hidden `<!-- hotsheet-attachment:<id> {…} -->` marker carrying the
  filename, path, repository, branch, blob sha, and batch/purpose/actor metadata. A retried
  upload finds the marker and changes nothing.
- **Projection:** marker comments become the ticket's `attachments` (created at the comment
  time) and are not repeated as notes. Ordinary comments, including ones with similar text,
  stay notes. Like notes, attachments appear on detail reads, not list pages.
- **Reading:** `attachment_bytes` reads the blob (`git/blobs/{sha}`) through the
  authenticated API, so the checkout attachment route serves private assets repositories
  to the browser without exposing a token or a short-lived signed URL.
- **Editing:** the new `attachment_edit` capability is `false`. Renaming, deleting,
  re-labelling, annotating, video posters, and local file actions stay git-only and are
  refused by name (`provider connection '…' (github) does not support this operation`).
  Deleting the committed file is left to the assets repository's owner.
- **Transfers:** copying or moving a ticket with attachments checks the destination's
  `attachments` capability before creating anything, so a destination without attachment
  support refuses the transfer up front instead of leaving a partial copy.

GitHub has only `open`/`closed` plus a `completed`/`not_planned` close reason, so the
provider carries the rest of Hot Sheet's state on **provider-owned labels** it writes and
reads itself and never surfaces as tags (HS2-K8R3T8). `state` decides open versus closed and
a label refines it:

| Hot Sheet                                          | GitHub                                                                                             |
| -------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| Not started / Started / Backlog                    | open; `status:started` or `status:backlog` when not Not started                                    |
| Completed                                          | closed, no `status:` label                                                                         |
| Verified / Archived                                | closed + `status:verified` / `status:archived`                                                     |
| Closed as Completed / Not planned                  | `state_reason` `completed` / `not_planned`, no `closed:` label                                     |
| Closed as Duplicate / Obsolete / Works as designed | `state_reason` `not_planned` + `closed:duplicate` / `closed:obsolete` / `closed:works-as-designed` |
| `duplicate_of`                                     | `duplicate-of:<qualified ticket reference>`                                                        |

Every write to a closed issue re-sends its current `state_reason` and `closed:` /
`duplicate-of:` labels, because GitHub silently resets an omitted reason to `completed`;
this is what lets a Works-as-designed ticket move to Verified and stay Verified. Reopening
drops the closed-only labels so the next close starts clean, and a stale open-only or
closed-only label on the other state is ignored on read. `verified_at` is reported as the
issue's `closed_at` when the issue carries `status:verified`. Project `providers.json`
contains connection metadata and a credential reference only; CLI/server resolve the
token through the OS-key registry (or its explicit environment override). GitHub CRUD
is available through `/providers/{connection}/tickets`, provider-aware MCP targeting,
and `provider-ls|get|new|edit|close` CLI commands. No operation writes a mirrored git
ticket. GitHub has no issue-creation idempotency key, so its capability advertises
`provider_idempotency: false`; transfer provenance is still recorded and retries
resolve it, while the host coordinator serializes same-process attempts.
The opt-in live contract was validated against `Small-Tale/hotsheet2` with a
repository-scoped OS-keychain credential: create, read, comment, and close all passed.

**Note metadata on external trackers (HS2-5YNASC).** Tracker comments have no metadata
fields, so a Hot Sheet-authored comment is the note text, an optional
`Confidence: NN%` trailer paragraph, and the hidden `<!-- hotsheet-note-id:<ulid> -->`
idempotency marker (shared code: `hotsheet-extsync/src/note_trailer.rs`). On read, the
trailer becomes the note's `confidence` only when the comment carries that marker and the
trailer is the final paragraph before it, after non-empty text, matching exactly
`Confidence: <integer 0-100>%`. Ordinary human comments, prose on the same line,
out-of-range or non-integer values, and earlier paragraphs never become a score. Jira
writes the trailer as its own ADF paragraph.

`latest_confidence` is bounded by the current completion cycle on every provider
(HS2-N3RMTV). A completed or verified ticket reports its newest scored note written
strictly after the latest reopen, so an old score never survives a reopen unless the next
completion reports a new one. A completing note written just before the close still
counts. The git provider reads reopens from its activity notes. The external trackers
use their native history:

- **GitHub:** issue events with `event: "reopened"`, read page by page to the end.
- **GitLab:** `resource_state_events` with `state: "reopened"`, following `x-next-page`.
- **Jira:** changelog status items whose `from` status is in the `done` category and
  whose `to` status is not. The changelog carries status ids only, so the status
  catalogue (`GET /rest/api/3/status`) supplies the categories. It is requested only when
  the changelog has a status transition. A move between two done statuses is not a reopen.

The trade-off is request cost. History is read **only on detail reads**, and only when it
can change the answer: the ticket is completed or verified _and_ at least one comment
carries a score. List and page reads never fetch comments, so they report no
`latest_confidence` for external tickets and make no extra request per ticket. A scored
detail read pays one history request, or a few when the history spans several pages.
That history is capped at 50 pages of 100 entries. Jira may add one status-catalogue
request.

Comments are paged the same way (HS2-9GS5TS), on detail reads and on the `add_note`
idempotency check:

- **GitHub:** `per_page=100&page=N` until a short page.
- **GitLab:** `x-next-page`.
- **Jira:** `startAt` up to `total`, or until a short page when `total` is absent.

A ticket with more than 100 comments therefore costs one extra request per additional 100
comments, capped at 50 pages. In exchange, its newest notes, scores, and retry markers are
never dropped.

The reopen bound is covered by fake-transport tests on every provider. Opt-in live tests
also exist in the creds-gated live tier (HS2-7D9BPK), but they have not run yet because
they need live credentials:

- **GitHub:** the CRUD test walks score, close, reopen, re-close (no score), then
  re-score.
- **GitLab:** `gitlab_live_reopen_bounds_latest_confidence` does the same and writes
  only with `HOTSHEET_GITLAB_LIVE_WRITE=1`.
- **Jira:** the adapter cannot transition status, so
  `jira_live_reopen_bounds_latest_confidence` reads an operator-prepared reopened issue
  (`HOTSHEET_JIRA_LIVE_REOPENED_ISSUE`, `..._EXPECTED`).

| Note capability                | git                      | GitHub Issues                        | GitLab                                   | Jira                                           |
| ------------------------------ | ------------------------ | ------------------------------------ | ---------------------------------------- | ---------------------------------------------- |
| `note_confidence` (append)     | yes                      | yes, comment trailer                 | yes, trailer                             | yes, ADF paragraph                             |
| Note edit (text or confidence) | yes                      | no (`note_edit` capability off)      | no                                       | no                                             |
| Activity `summary`             | yes                      | no (falls back to the comment body)  | no                                       | no                                             |
| `latest_confidence` derivation | current completion cycle | since last `reopened` event (detail) | since last reopened state event (detail) | since last changelog exit from `done` (detail) |

| Attachment capability                                   | git              | GitHub Issues                                           | GitLab | Jira |
| ------------------------------------------------------- | ---------------- | ------------------------------------------------------- | ------ | ---- |
| `attachments` (add and read)                            | yes, store files | only with `attachment_repo`; assets repo + link comment | no     | no   |
| `attachment_edit` (rename, delete, labels, annotations) | yes              | no                                                      | no     | no   |

In the web dialog a new GitHub connection starts from sign-in (HS2-1JT25R): its other settings
stay hidden and **Connect** stays disabled until GitHub authorizes. **Sign in with GitHub**
opens a small GitHub window and copies the one-time code to the clipboard within the same
click (the popup and a promise-backed clipboard write start before any network wait, so
Safari still treats them as user-initiated); the dialog shows the code with **Copy code**,
**Reopen GitHub**, and **Cancel**, and closes the GitHub window itself once authorization
succeeds. GitHub Enterprise has an explicit **Use GitHub Enterprise…** path that asks for the
server address before sign-in and stores `{origin}/api/v3` as the connection's API base; the
dialog no longer has an API-base field for GitHub, and editing keeps the existing base.

GitHub App device authorization is the default interactive setup path. The browser starts
an authorization session and performs one blocking wait; only the user code, verification
URL, installed repository names, and an opaque credential reference cross the browser
boundary. The server owns protocol pacing (`authorization_pending` and `slow_down`), stores
access/refresh bundles in the OS credential store, refreshes expiring access tokens, and
reports expiry, denial, cancellation, revoked credentials, and SAML reauthorization
requirements. GitHub Enterprise derives its web origin from the configured `/api/v3` API
base. The web dialog has no manual credential-reference field for GitHub: sign-in supplies
the credential, and editing keeps the existing one (HS2-48GA17). A pre-registered key
(`hotsheet key set`) can still be named in `providers.json` as a headless/advanced path. New
connections need no user-chosen id: a create request with an empty `id` gets a readable
unique one (`github-small-tale-hotsheet2`, then `-2`, …), and a blank display name defaults to
the provider name ("GitHub Issues"). After sign-in the repository field is a searchable list of
every repository the app can reach, gathered across all of the user's app installations with
`Link: rel="next"` pagination (HS2-27T5WT). GitHub only exposes repositories the app is
installed on, so the list response also reports each installation's account, whether it grants
`all` or `selected` repositories, and its settings page; the dialog explains a missing
repository, links to **Change access** for limited installations and to **Add another account
or organization** (the app's `installations/new` page, derived from the installations'
`app_slug`), and offers **Refresh list** after the user changes access on GitHub. The first-party public
GitHub.com Client ID (`Iv23lialgSTESydkTreA`) is bundled into development and release server
builds; it is an identifier, not a secret. `HOTSHEET_GITHUB_APP_CLIENT_ID` may override it
for development builds. The server build fails clearly if neither the override nor the
tracked `crates/hotsheet-server/github-app-client-id.txt` contains a valid GitHub App Client
ID, so an unconfigured release cannot silently ship.

The first-party app is registered for any-account installation with Device Flow enabled,
webhooks disabled, Metadata read-only, Issues read/write, and all other permissions disabled.
GitHub Enterprise Server installations require a separately registered app and Client ID on
each host. Operators configure those public IDs on the server as a JSON origin map, for
example `HOTSHEET_GITHUB_ENTERPRISE_APP_CLIENT_IDS='{"https://github.example.com":"Iv…"}'`.
An attempted Enterprise sign-in without an exact host entry fails before contacting GitHub
and explains how to provision it. Windows credential storage is tracked by HS2-N3R18X.

Built in the GitLab/Jira increment: both adapters use the same provider-neutral
routes and qualified identity contract without creating git-store mirrors. GitLab
maps project issue IIDs, URLs, labels, usernames, and notes; uses project-issue
pagination and incremental `updated_after` reads; and explicitly declines assignment
because its write API requires numeric provider-native user IDs. Jira Cloud maps
issue keys, browse URLs, ADF descriptions/comments, status categories, priorities,
types, labels, and account IDs; uses enhanced JQL search with opaque page tokens; and
declines close/status transitions unless a future connection supplies an explicit
project-workflow mapping. Both expose opaque optimistic-concurrency tokens, typed
authentication/rate-limit failures, local date-bound filtering, server-owned
credential references, deterministic fake-transport coverage, and opt-in live drift
tests.

The first Kerf/Web Awesome client surface manages those non-secret connection
records through authenticated server CRUD routes, selects the default creation
target, aggregates or filters tickets by provider, links native qualified identity
to its provider URL, and derives enabled operations and copy/move destinations from
advertised capabilities. Provider secrets never appear in connection responses;
the UI names only a server-owned credential reference.

The headless `github-sign-in` command uses the same GitHub App Client ID selection,
device protocol, keychain bundle format, and refresh boundary as the server
(HS2-DJA052). `github-connect` creates or updates the non-secret connection
idempotently and can link a registered checkout; `providers`, `provider-ls`, and
`provider-remove` then cover listing, live reads, and permanent removal without a
server. A GitHub App credential is unwrapped before a CLI provider read and refreshed
in the keychain when near expiry. The server and CLI prepare connection registry
updates through the same validated workflow.

Removing a data source is permanent and user-initiated (HS2-724S9N, HS2-SM9PM8). Two
shared, idempotent workflows in `hotsheet_ticketing::connection_removal` back it:

- **Remove from this project** — `detach_source`, behind
  `DELETE /checkouts/{reference}/sources/{id}`, `hotsheet checkout remove-source`, and the
  web Ticket sources row action / edit dialog's **Remove from this project…** (confirmed
  inline). It unlinks the source from that checkout (clearing a default that named it).
  When no other checkout links the connection afterwards, its `providers.json` record is
  deleted too, because the source lived with the project. A source still shared with
  another project (attached headlessly) is kept for it, and the report lists who still uses
  it. A repeat with nothing left to do is an explicit not-found.
- **Remove everywhere** — `remove_provider_connection`, behind
  `DELETE /provider-connections/{id}` and `hotsheet provider-remove <id>`: it unlinks the
  connection from every registered checkout and drops its `providers.json` entry.
  Repeating it reports nothing left to remove and still cleans dangling checkout links
  left by older clients.

Neither deletes the credential: a sign-in is a machine-wide **account** that other projects
may reuse, signed out separately (see below). Ticket data stays in the provider — nothing
is mirrored locally — so no other local state names the connection. The response reports
what was removed and which account was kept.

Disabling a data source is temporary (HS2-SF6W34). A disabled connection keeps its
`providers.json` record (with `"disabled": true`; the flag is omitted when enabled), its
checkout links, and its credential, but Hot Sheet neither reads from nor writes to it:
the server refuses to build its provider before any credential or network use, merged
checkout views and counts leave it out (its tickets are simply not shown — nothing is
mirrored locally), unqualified lookups skip it, and a qualified read or any mutation fails
with an explicit "connection is disabled" error. Duplicate-backlink scans skip it rather
than reporting it inaccessible. Toggle it with `PUT /provider-connections/{id}/disabled`
(`{"disabled": bool}`), `hotsheet provider-disable|provider-enable <id>`, or the web edit
dialog's **Disable / Enable** action; the settings list badges it **Disabled**. An ordinary
connection edit preserves the flag.

A checkout link (`TicketSource` in `checkouts.json`) copies its connection's locator.
Editing a connection's repository or project with `PATCH /provider-connections/{id}` or
its checkout-scoped twin (a project's Ticket sources edit) rewrites that copy on
every checkout that links the connection, in one locked registry write, so no linked
project keeps the old value (HS2-RCBKA3). Git links are path-derived and never change
through a connection edit. The headless `github-connect` refuses to retarget an existing
connection to another repository, so the CLI cannot create a stale copy.

### Project-owned sources, machine-wide accounts (HS2-SM9PM8)

Ticket sources are never global. A provider connection is **owned by the checkouts that
link it** in `checkouts.json`; links stay many-to-many, and each checkout keeps its own
default source (HS2-3SCH1K). `providers.json` remains the non-secret record store for those
connections, but it is not a catalog a project browses: no project is offered another
project's sources.

- `GET /checkouts/{reference}/providers` lists only that checkout's linked sources, in link
  order, marked `default` by the checkout's own default source; the web client routes a
  project's `/providers` there, so the composer, capabilities, and **Project Settings →
  Ticket sources** see only the project's sources.
- `GET /checkouts/{reference}/provider-connections` returns only the connections that
  checkout links, each with `projects` (every checkout that owns it), so a source shared
  with another project says so. `POST` to the same path creates a source **owned by that
  project**: the connection record and the checkout link in one request (with
  `make_default`); a failed link removes the new record again. `PATCH …/{id}` and
  `PUT …/{id}/disabled` edit or disable it and answer 404 when the checkout does not link
  it, so a project can never touch another project's source through its own routes. The
  web bridge routes a project's `/provider-connections` calls here.
- **Project Settings → Ticket sources** offers the default-source choice, editing a
  source's details, **Disable / Enable**, and **Remove from this project**. It has no
  "Other connections on this machine" list. Attaching an existing connection to a second
  checkout is deliberately headless only: `hotsheet checkout add-source`.

What _is_ machine-wide is the **account** — the sign-in a connection uses, its credential
reference (`settings.credential.secret`) held in the OS keychain. `hotsheet_ticketing::accounts`
derives accounts from the connection records, the checkout links, and `keys.json`; they are
never stored, so there is nothing to migrate:

- `GET /accounts` / `hotsheet account list [--json]` list each account (`id` = credential
  reference, `provider`, `host`, Jira `identity`, `managed` for a Hot Sheet GitHub sign-in)
  with its sources and the projects using each. A Hot Sheet GitHub sign-in (`github-app-*`)
  that no source uses yet is listed too; other unused keys (AI-provider keys) are not
  accounts.
- `DELETE /accounts/{id}` / `hotsheet account sign-out <id>` delete the credential. Both
  refuse (409 / non-zero exit) while any source still uses the account, naming the projects.
- `GET /accounts/{id}/github-repositories` lists the repositories a signed-in GitHub account
  can reach (refreshing its token server-side), so adding a source in another project
  reuses the sign-in and picks that project's own repository. **App Settings → Accounts**
  shows the same listing.

**Existing installs.** Nothing is rewritten. Every pre-existing connection is owned by the
checkouts already linking it, and every credential a connection references becomes an
account. A connection left in `providers.json` with no linking checkout (a catalog-only
entry from before HS2-SM9PM8) is kept and reported under its account as used by no
project; remove it with `hotsheet provider-remove <id>` or keep it for a later
`checkout add-source`.

Headless parity: `hotsheet checkout add-source|remove-source|set-default` act on one
project's sources; `github-connect --checkout` creates and links one; `account list|sign-out`
manage accounts; `providers`, `provider-disable|provider-enable`, and `provider-remove`
address a connection directly.

## 16.12 Cross-references

- Git provider format and guarantees: [02-ticket-storage.md](02-ticket-storage.md)
- Index/cache behavior: [03-indexing-and-query.md](03-indexing-and-query.md)
- Server, CLI, settings, and secure keys: [04-core-server-cli.md](04-core-server-cli.md)
- Plugin loading/trust model: [05-ai-tool-plugins.md](05-ai-tool-plugins.md)
- Assignment/review semantics: [10-assignment-and-collaboration.md](10-assignment-and-collaboration.md)
