# 17. Ticket File Format — canonical field schema

> **Status: Decided (the reference the parser is built against).** This consolidates
> the ticket-file schema previously scattered across [02](02-ticket-storage.md)
> (§2.5/§2.6a/§2.11/§2.13), [10](10-assignment-and-collaboration.md), and
> [16](16-external-sync-interface.md) into **one source of truth** for
> `hotsheet-model`. When a field's semantics need prose, this links back to the
> owning doc; the *field list + types + tiering* live here.

A ticket is one Markdown file (`<ulid>.md`) = **YAML frontmatter** (structured
fields) + a **Markdown body** (`details`) + an optional `## Notes` section. See
[02](02-ticket-storage.md) §2.5 for the narrative.

## 17.1 Data tiers (where each field lives)

- **Shared** — committed in the ticket file; synced to the team. (Most fields.)
- **Local** — per-user/per-machine, **never committed**; lives in a gitignored
  overlay ([02](02-ticket-storage.md) §2.11 Tier B), keyed by ticket ULID.
- **Derived** — not stored in the file at all; computed into the index
  ([03](03-indexing-and-query.md)) and rebuildable. Listed for completeness.

## 17.2 Frontmatter fields

| Field | Type | Req? | Tier | Notes |
|---|---|---|---|---|
| `id` | ULID (26-char) | yes | shared | The real key. [02](02-ticket-storage.md) §2.4 |
| `slug` | string, **ALL-CAPS** (`HS-7F3K9Q`) | yes | shared | Derived from `id` + store prefix; display handle |
| `title` | string | yes | shared | |
| `details` | (the Markdown **body**, not frontmatter) | no | shared | Long description |
| `category` | string (enum id) | yes | shared | Configurable categories |
| `priority` | `highest\|high\|default\|low\|lowest` | yes | shared | |
| `status` | `not_started\|started\|completed\|verified\|backlog\|archive\|deleted\|moved` | yes | shared | Set unchanged from HS1 + `moved` tombstone. HS2-24 |
| `up_next` | bool | no (def false) | shared | |
| `tags` | list<string> (normalized lowercase) | no (def `[]`) | shared | |
| `blocked_by` | list<ULID> | no (def `[]`) | shared | Flat dependency gate; cycle-checked |
| `blocked_reason` | string | no | shared | Free-text "waiting on" |
| `created_at` | RFC3339 | yes | shared | |
| `updated_at` | RFC3339 | yes | shared | Drives last-writer-wins merges (§2.7) |
| `completed_at` | RFC3339 \| null | no | shared | |
| `verified_at` | RFC3339 \| null | no | shared | |
| **Close outcome** ([02](02-ticket-storage.md) §2.6a) | | | | |
| `closed_at` | RFC3339 \| null | no | shared | |
| `close_reason` | `completed\|not_planned\|duplicate\|obsolete` \| null | no | shared | **Optional**, orthogonal to `status`. HS2-24/61 |
| `duplicate_of` | ULID \| null | cond. | shared | Required iff `close_reason == duplicate`; resolves globally |
| **Coordination** ([05](05-ai-tool-plugins.md) §5.7) — omitted when unclaimed | | | | |
| `claimed_by` | string (worker id) \| null | no | shared | Ephemeral; lease-expiring |
| `claim_lease_expires_at` | RFC3339 \| null | no | shared | |
| `worker_label` | string \| null | no | shared | Human worker name |
| `claim_count` | int (def 0) | no | shared | Poison-retry counter |
| **Assignment** ([10](10-assignment-and-collaboration.md) §10.2) | | | | |
| `assignees` | list<email> | no (def `[]`) | shared | git identities; roster maps to names |
| `review_requests` | list<{ who:email, kind:`work\|feedback\|review\|fyi`, by:ULID, at:RFC3339, requested_by:email? }> | no (def `[]`) | shared | Merge by set-union on `by`; new writes identify the requester |
| **External sync** ([16](16-external-sync-interface.md) §16.2) | | | | |
| `external` | list<{ system:`github\|gitlab\|jira`, repo:string, id:string, url:string, synced_at:RFC3339, remote_hash:string }> | no | shared | One entry per linked tracker |
| **Move tombstone** ([02](02-ticket-storage.md) §2.13) — only on a `status: moved` record | | | | |
| `moved_to_store` | store-id string | cond. | shared | Redirect target |
| **Migration** ([07](07-migration.md)) | | | | |
| `legacy_number` | string (`HS-1234`) | no | shared | Preserved HS1 number |
| **Provenance** | | | | |
| `copied_from` | ULID | no | shared | Set by a cross-store copy (§2.13) |
| **Schema** | | | | |
| `schema` | int | yes | shared | Frontmatter format version (forward migration) |

**Not in the file (Local / Derived):**

| Field | Tier | Where |
|---|---|---|
| `last_read_at` / unread | **local** | gitignored overlay (per-user read tracking) |
| feedback **drafts** | **local** | overlay; a `feedback_draft` note becomes a shared `regular` note on submit (§17.3) |
| UI/view state | **local** | overlay / machine settings |
| `store_id` | **derived** | the store the file physically lives in (positional, §2.2.1); index only |
| computed slug-collision suffix, FTS content, tag facets | **derived** | index only |

## 17.3 Notes (the `## Notes` section)

Canonical files explicitly bound the Markdown body, the Notes container, and every
note. This makes headings inside notes unambiguous and leaves a safe boundary for
future top-level sections:

```markdown
<!-- hotsheet:body:begin -->
Ticket details can contain arbitrary Markdown, including a `## Notes` heading.
<!-- hotsheet:body:end -->

<!-- hotsheet:notes:begin -->
## Notes

<!-- hotsheet:note:begin 01J9ZK4A0R… kind: regular -->
2026-08-19T15:20:44Z — Reproduced on macOS; root cause is the pre-theme paint.
<!-- hotsheet:note:end -->

<!-- hotsheet:note:begin 01J9ZK5B1S… kind: feedback_needed -->
2026-08-19T15:31:02Z — should the fix also cover the dashboard dedicated view?
<!-- hotsheet:note:end -->
<!-- hotsheet:notes:end -->
```

The reader remains backward-compatible with schema-1 files that use `## Notes`
followed by one-sided `<!-- note: … -->` markers through EOF. Every canonical write
upgrades that layout to bounded blocks. A line of user-authored Markdown that looks
like a reserved `<!-- hotsheet:… -->` marker is backslash-escaped on disk and restored
on read. Content after `hotsheet:notes:end` is rejected by current readers rather
than swallowed or discarded; a future schema can define such a section explicitly.

| Note field | Type | Notes |
|---|---|---|
| `id` | ULID | timestamp-ordered → union-merge + chronological sort (§2.6/§2.7) |
| `kind` | `regular\|feedback_needed\|feedback_draft\|status` | default `regular`. **`feedback_draft` is LOCAL** (overlay), the others shared. §2.6 |
| timestamp | RFC3339 | leads the note text |
| text | Markdown | rendered; raw HTML escaped |

## 17.4 Rules the parser/serializer enforce

- **Round-trip stable:** parse → serialize is byte-idempotent for a canonical file
  (the migrator conformance test, [07](07-migration.md) §7.2.1, depends on this).
- **Unknown frontmatter keys are preserved** (forward-compat: a newer `schema`
  writes a key an older reader doesn't know — don't drop it).
- **Field ordering** is canonical (stable key order) so diffs are clean and merges
  are predictable.
- **Markdown boundaries are explicit and collision-safe:** details and shared note
  text round-trip arbitrary Markdown; all other ticket strings are YAML scalars.
  JSON API/MCP representations use normal JSON string escaping. Renderers must escape
  raw HTML; the derived `worklist.md` also escapes Markdown syntax in ticket titles.
- **Enums validated**; an invalid value degrades (kept as-is + a `status`-kind note)
  rather than panicking (fuzz target, [12](12-code-organization-and-testing.md) §12.7.2).
- **`duplicate_of` presence** is tied to `close_reason == duplicate` (validation).

## 17.5 Cross-references
- Storage narrative + tiering: [02-ticket-storage.md](02-ticket-storage.md)
- Index columns derived from these: [03-indexing-and-query.md](03-indexing-and-query.md) §3.3
- Assignment fields: [10-assignment-and-collaboration.md](10-assignment-and-collaboration.md)
- External block: [16-external-sync-interface.md](16-external-sync-interface.md) §16.2
- Implemented by `hotsheet-model`: [12-code-organization-and-testing.md](12-code-organization-and-testing.md) §12.2
