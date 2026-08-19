# 16. External-Sync Plugin Interface

> **Status: Design (HS2-71).** A **dedicated** interface for synchronizing HS2
> git-tickets with external ticketing systems — **not** HS1's broad general plugin
> loader (docs/18). Generalize across **GitHub Issues, GitLab Issues, and Jira**
> (maintainer, 2026-08-19); GitHub is the first + most important (user-facing
> tickets arrive through it). This document is the interface spec + a build plan.

## 16.1 Scope & what it is *not*

- **It is:** a first-class "sync HS2 tickets ↔ an external tracker" capability, with
  one interface that GitHub/GitLab/Jira providers implement.
- **It is not** HS1's general ESM plugin system (docs/18 `TicketingBackend` + loader,
  UI hooks) — that broad surface is dropped. We build the *specific* thing we need.
- **It is not** git-native store sharing. An HS2 store can *live* on GitHub as a
  **repo** ([02](02-ticket-storage.md)); this instead talks to GitHub's **Issues**
  API. The two are orthogonal — a project can do both.

## 16.2 Where the external identity lives (on the ticket)

A synced ticket records its external counterpart in **frontmatter** (shared, so the
whole team's sync agrees), under an `external` block:

```yaml
external:
  - system: github            # github | gitlab | jira
    repo: acme/app            # provider-specific locator
    id: 482                   # issue number / key (JIRA: ACME-482)
    url: https://github.com/acme/app/issues/482
    synced_at: 2026-08-19T14:03:11Z
    remote_hash: 9f2c…        # hash of the last-seen remote state (for change detection)
```

- A ticket may sync to **more than one** system (hence a list).
- The `external` block is the mapping anchor: pull matches remote→local by
  `(system, repo, id)`; push writes local→remote for the same key. A ticket with no
  `external` block is local-only.
- `remote_hash` is how we detect a remote change without a full diff (like the
  index's content-hash trick, [03](03-indexing-and-query.md) §3.4).

## 16.3 The interface (one trait, provider implementations)

> **Crate: `hotsheet-extsync`** (maintainer, 2026-08-19) — its own plugin-type crate,
> depending on `ticketing` + HTTP clients, **not** `terminals`. Separate from the
> AI-tool plugin crate (`hotsheet-aitools`); see [12](12-code-organization-and-testing.md)
> §12.2.1.

A provider is **declarative identity + a behavioral sync driver** (mirrors the
AI-tool plugin split, [05](05-ai-tool-plugins.md) §5.3). Sketch:

```rust
trait ExternalSyncProvider {
    fn id(&self) -> &str;                 // "github" | "gitlab" | "jira"
    fn display_name(&self) -> &str;

    // Auth — a token/credential resolved from secure storage (§32 keychain).
    fn auth(&self) -> AuthSpec;           // PAT | OAuth device-flow | (Jira) email+token

    // Field mapping — provider-specific ⇄ the HS2 model.
    fn map_in(&self, remote: RemoteIssue) -> TicketDraft;   // remote → HS2 fields
    fn map_out(&self, ticket: &Ticket) -> RemotePatch;      // HS2 → remote fields

    // Incremental pull: everything changed since a cursor.
    fn pull(&self, since: Cursor) -> Result<(Vec<RemoteIssue>, Cursor)>;
    // Push one ticket's changes to the remote (create or update).
    fn push(&self, ticket: &Ticket, link: Option<&ExternalLink>) -> Result<ExternalLink>;

    // Comments ⇄ notes, attachments — optional capabilities (absence = unsupported).
    fn pull_comments(&self, issue: &RemoteRef) -> Result<Vec<RemoteComment>> { … }
    fn push_note(&self, issue: &RemoteRef, note: &Note) -> Result<RemoteCommentId> { … }
}
```

Everything provider-specific is behind `map_in`/`map_out`/`pull`/`push`; the **host
owns the sync engine** (scheduling, cursors, conflict resolution, writing files,
reindex) so a new provider is mostly field-mapping + API calls.

## 16.4 Field mapping (the part that actually differs)

| HS2 field | GitHub | GitLab | Jira |
|---|---|---|---|
| open/closed (status) | issue `state` | issue `state` | status category |
| `status` (granular) | labels (`status:started`) | labels | workflow status |
| `category` | label | label | issue type |
| `priority` | label (`priority:high`) | label | priority field |
| `tags` | labels | labels | labels |
| `assignees` | assignees | assignees | assignee |
| `details` (body) | issue body | description | description |
| notes | comments | comments | comments |
| `title` | title | title | summary |

Providers declare their mapping (a config, not hardcoded branches), and unmapped
labels pass through as `tags` so nothing is lost. The mapping is **configurable per
project** (a team's label conventions vary) — reuses the settings model
([02](02-ticket-storage.md) §2.11).

## 16.5 The sync loop (host-owned)

1. **Pull** on a cadence + on demand: `provider.pull(since_cursor)` → remote issues
   changed since last sync; `map_in` → ticket drafts.
2. **Reconcile** each against the local ticket (matched by the `external` block):
   - New remote, no local → **create** a local ticket file in the project's
     configured **import store** (§16.7), stamping the `external` block.
   - Both changed since last sync → **conflict** (§16.6).
   - Only remote changed → apply remote → local.
3. **Push** local changes (where the ticket has an `external` link and local is
   newer) via `provider.push` → update the remote + refresh `synced_at`/`remote_hash`.
4. **Commit** the resulting file changes to the store (they flow through the normal
   git commit + merge machinery). The **watcher/index reconcile** picks them up so
   the UI updates live.

Both **scheduled** (interval) and **event-driven** (webhooks where available — GitHub
issue events; a manual "sync now" always) — mirrors HS1 docs/88 scheduled sync.

## 16.6 Conflict handling (external vs. local)

Distinct from the *git* merge driver ([02](02-ticket-storage.md) §2.7, which handles
git-branch conflicts). Here a field changed on *both* the remote and locally since
`synced_at`:

- **Field-level, last-writer-wins by timestamp** where each side carries one
  (updated_at vs the remote's updated timestamp) — same spirit as the merge driver.
- **Notes/comments never conflict** — union by origin id (a comment carries its
  remote id; a note its ULID), so both sides' additions are kept.
- **Irreconcilable field clash** (rare) → keep local, record a `status`-kind note
  ("sync conflict on `priority`: local=high, github=low — kept local") so it's
  visible, never silently dropped. Optionally a UI conflict affordance later.

## 16.7 Where imported tickets land

- A project configures, per external connection, an **import store + defaults**
  (which store new external tickets are created in, default category/priority). This
  reuses the multi-store model ([02](02-ticket-storage.md) §2.2) — e.g. GitHub issues
  import into a `github` store that syncs to that repo.
- Because a ticket's store determines its permissions/remote, importing into a
  specific store is how "these are the public GitHub-sourced tickets" stays separable
  from private ones.

## 16.8 Auth & security

- Credentials (PAT / OAuth tokens / Jira email+token) live in **OS keychain secure
  storage** (area 32 / HS1 docs/20), never committed.
- Tokens are **machine-local** — the `external` *link* (system/id/url) is shared in
  frontmatter, but the *credential* to reach it is per-device.

## 16.9 Open questions
- **Webhook delivery** needs a reachable endpoint — fine when the server is exposed
  (mTLS tier, [08](08-distributed-and-remote.md)); otherwise poll. Decide the default.
- **Two-way vs. import-only** per connection (some teams only want to pull) — make it
  a per-connection mode.
- **Jira's richer model** (epics, sprints, custom fields) — map a sensible subset;
  don't try to mirror everything.
- **De-dup on first import** if the same issue was already imported into two stores.

## 16.10 Build plan (follow-up tickets)
- **HS2-71 (this)** delivers the spec.
- Follow-ups to file: the host sync-engine (cursors/scheduling/reconcile/conflict);
  the provider trait + config; the **GitHub Issues provider** (first); then GitLab
  and Jira providers; the settings UI for connections + field mapping.

## 16.11 Cross-references
- Ticket storage + the `external` frontmatter block: [02-ticket-storage.md](02-ticket-storage.md)
- Automatic git sync (separate concern): [02-ticket-storage.md](02-ticket-storage.md) §2.12
- Secure storage for tokens: area 32 (HS2-54)
- Plugin-split pattern this mirrors: [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.3
