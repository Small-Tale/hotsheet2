# 02. Ticket Storage — Git Repos

> **Status: Confirmed** (maintainer, 2026-08-19). Git-repo file-per-ticket storage,
> ULID IDs with an **all-caps** slug, **inline notes with timestamp-ordered UUID
> ids**, and **attachments support** are approved. The load-bearing addition from
> that review: conflict resolution must be **almost entirely automatic** — see the
> semantic merge driver in §2.7.

## 2.1 The bet

Tickets are **plain files in git repositories**. Git is the single source of
truth. Everything else — the SQLite index, the worklist markdown, the UI — is
derived and rebuildable.

**Every store is a git repo — always, even local-only ones** (maintainer,
2026-08-19). There is no "plain files, not a repo" mode. A store that isn't shared
is just a git repo with **no remote**: it still has commits, history, branches, and
automatic conflict resolution locally. This keeps one uniform storage model (and
one backup story — git history) across shared and private tickets alike.

Why this beats the HS1 PGLite approach:
- **Inspectable & diffable.** `git log`/`git diff` show ticket history for free.
  A user with a text editor can read and repair anything.
- **Selective sharing with real ACLs.** A store can be a GitHub repo with
  GitHub's own permissions; another store stays local-only. The app doesn't
  reimplement access control.
- **Versioned with the project.** Tickets can live beside the code they describe,
  under the same git the team already trusts.
- **No opaque-blob fragility.** HS1 needed tiered snapshots, backups, a repair
  subsystem, cluster eviction, and a `template1` pin to keep an embedded Postgres
  cluster alive across upgrades. Files + git eliminate that entire class of work —
  git *is* the backup and history.
- **Distribution-native.** Offline edits, clones, and merges are git's core
  competency. No central DB writer.

## 2.2 A "store" and a "project"

- A **store** is a git repository (or a designated root within one) that holds
  tickets. It has a small metadata file and its own visibility/sync policy.
- A **project** references **one or more stores**. A project is the unit the UI
  shows as a tab; a store is where a given ticket physically lives.

A project references multiple stores because (per the ticket):
- some tickets need **different access permissions**, enforced externally (e.g. a
  private GitHub repo vs. a public one);
- some tickets are **single-user and/or local-only** (never pushed anywhere).

**Project config** (`~/.hotsheet/projects/<project-id>.json`, or a
`.hotsheet/project.json` in the project root — see §2.8):

```jsonc
{
  "id": "01J9Z…",                 // ULID
  "name": "Hot Sheet 2",
  "stores": [
    {
      "id": "main",                 // stable machine key (referenced by config + index)
      "name": "Team backlog",       // human label for the UI (optional)
      "path": ".hotsheet/tickets",  // relative to project root, or absolute
      "visibility": "shared",       // "shared" | "local"
      "sync": { "mode": "git-remote", "remote": "origin", "branch": "main" },
      "default": true               // new tickets land here unless directed
    },
    {
      "id": "security",
      "name": "Security (private)",
      "path": "~/hotsheet-private/acme",
      "visibility": "shared",
      "sync": { "mode": "git-remote", "remote": "origin", "branch": "main" }
    },
    {
      "id": "scratch",
      "name": "My scratch",
      "path": ".hotsheet/local-tickets",
      "visibility": "local",
      "sync": { "mode": "local-only" }  // still a git repo — just no remote configured
    }
  ]
}
```

- **Every store is a git repo — no exceptions** (maintainer, 2026-08-19). A
  `local-only` store is simply a git repo with **no remote configured**: it still
  gets commits, history, branches, and the semantic merge driver locally — there is
  just nothing to fetch or push. So even purely local tickets keep full git history,
  and git is their backup too.
- `sync.mode: git-remote` stores have a remote and participate in push/pull;
  `local-only` stores have none. `visibility: local` additionally marks a store as
  private to this machine/user — gitignored from the *project* repo (or kept as its
  own separate repo) so it isn't shared even incidentally.
- A ticket's **store is part of its identity for routing** (which remote, which
  permissions) but not its global ID — IDs are globally unique across stores
  (§2.4), so a ticket can be referenced (e.g. `blocked_by`) across stores.

### 2.2.1 Store identity & membership — how you know which store a ticket is in

Three layers of "which store," from machine to human:

- **`id`** — a stable, project-unique machine key (`main`, `security`, `scratch`).
  The project config and the index reference stores by `id`; it never changes.
- **`name`** — an optional human label for the UI ("Security (private)").
- **`ticketPrefix`** (per store, in `hotsheet-store.json` — §2.3) — the **at-a-glance
  human signal.** Give each store its own prefix and a slug *shows* its store:
  `HS-7F3K9Q` lives in the team backlog, `SEC-2M8XQ1` in the security store. This is
  the cheapest "which repo is this?" cue there is.

**Store membership is *positional*, not a frontmatter field.** The store a ticket
belongs to is simply *the store whose `tickets/` directory physically contains its
file* — nothing in the ticket file records its store. This is deliberate: a copy or
move is then just a file in a different store root, with **no stale `store:` field
to reconcile** and no way for the file to disagree with its location. The **index**
records the derived `store_id` (recomputed from location on every reindex) so the UI
can filter/group by store fast, and it keys a ticket by **`(store_id, id)`** — a
ticket's full identity is *store + ULID*. A global ULID still resolves to exactly
one **live** ticket (see the move tombstones in §2.13).

## 2.3 On-disk layout of a store

```
<store-root>/
  hotsheet-store.json         # store metadata (schema version, prefix, id strategy)
  tickets/
    01/                       # 2-char shard by id prefix (see §2.5)
      01J9ZK3M7Q8F2N4V6X.md
    7f/
      7f2a…​.md
  attachments/
    01J9ZK3M7Q8F2N4V6X/
      screenshot.png
```

`hotsheet-store.json`:
```jsonc
{
  "schemaVersion": 1,
  "ticketPrefix": "HS",       // display prefix; the dash is added automatically
  "idStrategy": "ulid",
  "shard": "id-prefix-2"      // 2-char id-prefix sharding (confirmed 2026-08-19)
}
```

**Sharding is id-prefix (2-char)** — decided (maintainer, 2026-08-19) — so a store
scales to tens of thousands of tickets with bounded, evenly-filled directories (a
ULID's trailing random bits distribute uniformly across the 256 shard buckets),
without a flat `tickets/` directory growing unwieldy.

## 2.4 Ticket IDs — ULID, no central sequence

The ticket requires IDs that are **UUID/random-based instead of linearly
increasing**. We use **ULIDs** (128-bit, Crockford base32, 26 chars):

- **No central writer.** Any client, CLI, or offline device mints an ID with no
  coordination. This is the whole point — a linear Postgres sequence can't exist
  in a distributed, git-merged world.
- **Collision-free** in practice (80 bits of randomness per millisecond).
- **k-sortable.** A ULID's leading 48 bits are a timestamp, so lexicographic sort
  ≈ creation order — we get a sensible default ordering *for free*, without a
  counter, and IDs shard evenly by their trailing random bits.

**Human-facing form.** Users don't want to read a 26-char ULID. Each ticket also
carries a **short display slug**, rendered in **ALL CAPS** (maintainer preference,
2026-08-19): `<PREFIX>-<BASE32 of a hash of the ULID, truncated>`, e.g.
`HS-7F3K9Q`. Crockford base32 is uppercase-canonical, so this is natural. The slug
is:
- derived deterministically from the ULID (so it needs no allocation),
- checked for collision *within the index* at display time; on the astronomically
  rare clash, we lengthen the truncation for the newer ticket. The full ULID is
  always the real key; the slug is a convenience.

> Trade-off vs. HS1's `HS-1234`: we lose the friendly monotonic number. We gain
> offline creation, zero merge conflicts on an ID counter, and even sharding. The
> slug keeps a short human handle. **Decision (confirmed): ULID + derived all-caps
> slug.**

## 2.5 File format — Markdown + YAML frontmatter

One file per ticket, `<ulid>.md`:

```markdown
---
id: 01J9ZK3M7Q8F2N4V6X8Y0A1B2C
slug: HS-7F3K9Q
title: Fix the dashboard flicker on project switch
category: bug
priority: high
status: started
up_next: true
tags: [dashboard, ui]
blocked_by: [01J9ZK…another-ulid]
# human assignment (shared; see 10-assignment-and-collaboration.md)
assignees: [alex@example.com]
review_requests: []          # e.g. [{ who: dana@example.com, kind: feedback }]
created_at: 2026-08-19T14:03:11Z
updated_at: 2026-08-19T15:20:44Z
completed_at: null
verified_at: null
# close outcome (set when the ticket is closed; see §2.6a)
closed_at: null
close_reason: null            # completed | not_planned | duplicate | obsolete
duplicate_of: null            # a ticket ULID, required when close_reason == duplicate
# coordination (optional; omitted when unclaimed)
claimed_by: worker-1
claim_lease_expires_at: 2026-08-19T15:50:44Z
worker_label: worktree-2
schema: 1
---

The dashboard flashes white for one frame when switching projects because the
terminal gutter paints `var(--bg)` before the theme is applied.

## Notes

<!-- note: 01J9ZK4A0R… -->
2026-08-19T15:20:44Z — Reproduced on macOS; root cause is the pre-theme paint.

<!-- note: 01J9ZK5B1S… kind: feedback_needed -->
2026-08-19T15:31:02Z — should the fix also cover the dashboard dedicated view?
```

- **Frontmatter = structured fields** (validated by a schema; `schema:` versions
  the format for forward migration).
- **Body = the ticket `details`** as Markdown.
- **Notes** are a `## Notes` section where **each note carries its own
  timestamp-ordered UUID (a ULID)** in a leading HTML comment. The ULID id is what
  makes note merges automatic (§2.7): two branches that each append a note produce
  two distinct, uniquely-identified blocks that union cleanly and sort by id
  (= chronologically). The comment keeps the id machine-readable without cluttering
  the rendered Markdown.
- **A note has a `kind`** (default `regular`), carried in the same comment
  (`kind: …`), one of **four** (HS2-26, maintainer 2026-08-19):
  - `regular` — an ordinary note. **Shared** (committed).
  - `feedback_needed` — a request for a human decision (HS1's `FEEDBACK NEEDED:`
    prefix, promoted to a first-class kind). **Shared** (committed).
  - `feedback_draft` — a user's half-written response to a `feedback_needed` ask.
    **Local / per-user** — it lives in the gitignored local overlay (§2.11 Tier B),
    *not* the committed ticket file; on submit it becomes a `regular` shared note.
  - `status` — a system-generated event note (e.g. "claim expired — reclaimed",
    "QUARANTINED"). **Shared** (committed), informational for the team.
  The `kind` drives how the UI renders a note (feedback kinds get an editor; the rest
  get the reader) — [06-clients.md](06-clients.md) §6.8.
- **Attachments** are files under `attachments/<id>/`, referenced by relative path.
  They merge trivially — a new attachment is a new file, so two branches adding
  attachments to the same ticket never conflict.

Why Markdown + YAML: it renders natively in every editor and on GitHub, is
trivially diffable, and separates machine fields (frontmatter) from human prose
(body). It is the format the ticket asks for in spirit — "search the text of
tickets," "draw tickets" — while staying git-merge-friendly.

## 2.6 Notes storage — the one real design fork

**Notes** (timestamped entries, appended heavily by AI tools) can be stored two
ways:

- **(A) Inline** in the ticket file, under a `## Notes` section (as above).
  *Pro:* one file per ticket, simple, everything in one place. *Con:* two agents
  appending notes to the same ticket concurrently can conflict on the same file
  region (though appends usually auto-merge).
- **(B) Per-note files** under `tickets/<id>/notes/<note-ulid>.md`, with the
  ticket body in `tickets/<id>/ticket.md`. *Pro:* concurrent appends never
  conflict (each note is a new file). *Con:* many small files, more complex.

**Decision (confirmed, 2026-08-19): (A) inline**, because the common case is one
agent per ticket at a time (the claim/lease primitive enforces this — see §2.7),
and inline keeps the store human-readable. **Each note carries a timestamp-ordered
UUID (ULID)** so appends merge automatically and order is intrinsic to the id — the
maintainer's explicit ask, and the mechanism §2.7 relies on. Revisit (B) only if
inline-append conflicts survive the merge driver in practice (they shouldn't).

## 2.6a Close reasons — why a ticket was closed

> **New for HS2** (maintainer, 2026-08-19), motivated by the collaborative nature
> of shared tickets: when someone closes a ticket, others need to know *why* — not
> just that it's closed. Modeled on GitHub's close reasons. Build: **HS2-61**.

Three frontmatter fields record the **outcome** of a close, distinct from the
workflow `status`:

- **`close_reason`** — `completed | not_planned | duplicate | obsolete`
  (extensible). `completed` = the work was done; `not_planned` = deliberately won't
  do it; `duplicate` = the same as another ticket; `obsolete` = no longer relevant.
- **`duplicate_of`** — a ticket **ULID** (may live in another store — resolved
  globally, §2.2.1), **required when** `close_reason == duplicate`. This is a real
  reference, so the UI links to the canonical ticket and the two show their
  relationship both ways.
- **`closed_at`** — timestamp of the close.

**Relationship to `status` (and to the status decision, HS2-24).** `close_reason`
is *orthogonal metadata on a closed ticket*, not a replacement for the status:

- The **done path** — `completed` / `verified` — carries `close_reason: completed`
  (implied/defaulted; `verified` adds the human-checked bit on top).
**`close_reason` is a separate OPTIONAL field, orthogonal to `status` — the statuses
are unchanged (maintainer, 2026-08-19, HS2-24).** We do **not** collapse the status
set into an open/closed axis and do **not** add a `closed` status. HS1's status set
stays as-is (`not_started` / `started` / `completed` / `verified` / `backlog` /
`archive` / `deleted`, plus our `moved` tombstone). When a ticket is closed out
(typically moved to `completed`), you may **optionally** set `close_reason` to record
*why* — `completed` vs `duplicate` (+ `duplicate_of`) vs `not_planned` vs `obsolete`
— purely for tracking and filtering. It annotates the closure; it does not change or
replace the status. `verified` remains the human-checked flag on top of a completed
ticket. A ticket with no `close_reason` set is simply untracked in that dimension.

**Freeform vs. structured.** `close_reason` is the *structured* tag (filterable,
reportable). A **note** still carries any freeform explanation ("closing — we chose
the other approach in HS-8842"), so the two compose: reason for the machine, note
for the human.

**Merge behavior (automatic).** All three fields are scalar frontmatter, so they
merge by the §2.7 rule — **newest `updated_at` wins**. If two people close the same
ticket concurrently with different reasons, the later close wins and nothing
conflicts; `duplicate_of` is a plain ULID that survives a move (§2.13).

**Reopening** clears `close_reason` / `closed_at` / `duplicate_of` (and returns the
ticket to an open status), the same way HS1 clears completion metadata on reopen.

## 2.7 Concurrency & automatic conflict resolution

> **Maintainer requirement (2026-08-19): conflict resolution must be "almost
> entirely automatic."** This section is the design for that. It is a
> first-class goal, not a fallback — a git-backed ticket store that dumps
> `<<<<<<<` markers on the user is a failed design.

Four layers, each removing conflicts before the next has to act:

**1 — File-per-ticket isolates by construction.** Different tickets are different
files and *never* conflict. Attachments are separate files and never conflict.
This alone removes the overwhelming majority of potential conflicts.

**2 — Claim/lease keeps a single active writer per ticket.** The coordination
primitive (carried from HS1 `src/db/claims.ts`) means at most one worker actively
edits a given ticket at a time; the write chokepoint rejects a write to a ticket
another actor holds a live lease on. So two *simultaneous* edits to one ticket are
already the exception. See [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.7.

**3 — A semantic git merge driver makes same-ticket merges automatic.** When two
branches *do* touch the same ticket file (e.g. an offline edit merged against an
upstream edit), we do **not** rely on git's line-based merge. Each store installs
a custom merge driver, registered via `.gitattributes`:

```gitattributes
# in each store, written by `hotsheet init` / store registration
tickets/**/*.md merge=hotsheet-ticket
```

The driver is a **`hotsheet merge-driver` CLI subcommand** (so the merge logic is
the same core code, tested once — [04-core-server-cli.md](04-core-server-cli.md)
§4.4). Git invokes it with the base/ours/theirs versions; it performs a
**format-aware 3-way merge**:

- **Frontmatter (structured fields):** merged **field-by-field**, not line-by-line.
  - Scalars (title, status, priority, category, up_next, timestamps): if only one
    side changed a field vs. base, take that side. If both changed it, **newer
    `updated_at` wins** (last-writer-wins per field). No conflict emitted.
  - Sets (tags, `blocked_by`): **set union** of both sides (with base-relative
    delete honored — a tag removed on one side and untouched on the other stays
    removed). No conflict.
  - Claim fields: **newest lease wins**, and a stale lease is reclaimable anyway
    (§2.7-note below), so these never wedge a merge.
- **Notes:** **union by note-ULID.** Each note is uniquely identified, so appends
  from both sides are simply combined and **sorted by ULID (= chronological)**.
  A note edited on both sides resolves by the note's own newest timestamp; a note
  present on only one side is kept. Because notes are mostly append-only with
  unique ids, this is a clean union essentially always — which is exactly why the
  maintainer asked for timestamp-ordered UUIDs on notes.
- **Body (`details` prose):** if only one side changed it, take that side. If both
  changed it, run a standard 3-way text merge on *just the body*. This is the
  **only** place a human-visible conflict can still occur — two people rewriting
  the same paragraph of one ticket's description — and it's scoped to that
  paragraph, never the structured fields or notes.

Result: the realistic conflict surface shrinks to "two humans edited the same
prose paragraph of the same ticket while both offline" — rare, and even then the
frontmatter and notes still merge cleanly around it.

**4 — The index never participates.** Whatever the merge produces, the index is
rebuilt from the merged files ([03-indexing-and-query.md](03-indexing-and-query.md)),
so it can never be a source of conflict or drift.

> **Claim-field note:** claim fields live in frontmatter but are lease-based and
> expiring, so a stale claim arriving from a merged branch is simply past its lease
> and reclaimable — it can't wedge anything even before the "newest lease wins"
> rule applies.

**Rebase/merge policy.** Stores that sync to a remote pull with rebase (or a merge)
through this driver; a worker rebases at ticket boundaries (never mid-work), the
same discipline HS1's `/hotsheet-worker` uses. The driver makes those rebases
land automatically in the common case.

**Fallback + safety.** If the driver itself errors (malformed input, an
unparseable file), it falls back to git's default text merge rather than losing
data — a conflict is surfaced, never a silent overwrite. `hotsheet doctor` flags
any store missing the `.gitattributes` registration so the automatic path can't be
silently bypassed.

## 2.8 Where a project's default store lives

Two supported shapes — **both are git repos** (§2.1):
1. **In-repo:** `.hotsheet/tickets/` inside the project's existing git repo —
   tickets versioned alongside code. The default store is committed; `local`
   stores are gitignored.
2. **Standalone:** a dedicated tickets repo at any path, referenced by absolute
   path. Good for a tickets-only repo shared across several code repos, or a
   private tickets repo separate from public code.

`hotsheet init` offers both; default is in-repo when the cwd is a git repo, else a
standalone store under `~/.hotsheet/stores/<project>/`. When it creates a standalone
store (or any store that isn't already inside a repo) it **runs `git init`** and
installs the merge driver (§2.7) — a local-only store is a perfectly normal repo
that simply has no remote added.

## 2.9 What we drop from HS1 storage

- The embedded Postgres cluster, the `template1` pin, tiered snapshots, backups,
  the repair/restore subsystem, cluster eviction, and DB lock recycling-PID
  guards — **all gone.** Git history + rebuildable index replaces every one of
  them.
- The central `ticket_seq` sequence — replaced by ULIDs (§2.4).
- Settings-in-DB — settings are JSON files (as HS1 already largely moved to).

## 2.11 Shared vs. local data — what's committed, what stays per-user

> **Maintainer question (2026-08-19):** *"what goes into stored tickets and if
> anything is local-only — and if it is, is it still stored on disk (a gitignored
> file) or only in the db?"* This section answers it. Tracked: HS2-21.

Not everything about a ticket is shared. A shared git store is seen by every
teammate, so **per-user or per-machine** facts must not be committed into the
ticket file (my read state is not your read state). Three tiers:

**Tier A — Shared (committed in the ticket file).** The ticket itself:
`title`, `details` (body), `category`, `priority`, `status`, `up_next`, `tags`,
`notes`, `blocked_by`, **`assignees` + assignment requests**
([10-assignment-and-collaboration.md](10-assignment-and-collaboration.md)),
`attachments`, and the shared timestamps (`created_at`, `updated_at`,
`completed_at`, `verified_at`). Coordination `claim_*` fields are shared too — they
*are* the distributed-work signal — but expiring, so they never wedge anything.

**Tier B — Local (per-user / per-machine, NOT committed, but ON DISK).** Facts that
differ per person or per device:
- **Read tracking** (`last_read_at` / unread state) — inherently per-user.
- **Feedback drafts** (HS1's `feedback_drafts`) — a half-written response is yours.
- **UI / view state** that is per-machine (last view, scroll, drawer state).
- **Machine-local preferences** (per-device settings — HS1's `settings.local.json`).

**Where Tier B lives — the rule: on disk, gitignored, index is only a cache.**
Local durable data is stored in **gitignored overlay files inside the store**, e.g.
`.hotsheet/local/reads.json`, `.hotsheet/local/drafts/…`, keyed by ticket ULID.
The `.gitignore` block ignores `.hotsheet/local/**` while the ticket files stay
committed. This follows the cardinal principle
([00-vision-and-principles.md](00-vision-and-principles.md) §0.4): **everything
reconstructs from disk.** If local data lived *only* in the SQLite index and the
index is disposable, a rebuild would lose it — so local data is on disk, and the
index caches it exactly as it caches the shared ticket fields. **The DB/index is
never the sole home of any durable data**, shared or local. (Truly ephemeral,
recomputable state — an in-flight busy timer — may stay in memory; that's not
durable data.)

**Tier C — Local-only *stores*.** A whole store with `visibility: local` (§2.2) is
the "single-user, local-only tickets" case. It is **still a git repo, just with no
remote** (§2.1) — its ticket files are committed and versioned locally, and are
gitignored from the *project* repo (or kept as a separate repo) so they're never
shared. That's different from Tier B: Tier C is entire tickets that are private
(but fully git-versioned); Tier B is per-user *slivers* of otherwise-shared tickets
that are deliberately kept out of git entirely.

**The precise field-by-field classification (HS2-21).** Every ticket-related datum
maps to exactly one tier:

| Datum | Tier | Home |
|---|---|---|
| `title` · `details` · `category` · `priority` · `status` · `up_next` · `tags` · `blocked_by` · `blocked_reason` | A (shared) | committed ticket frontmatter/body |
| `notes` (kind `regular` / `feedback_needed` / `status`) | A (shared) | committed `## Notes` |
| `assignees` · `review_requests` · `external` | A (shared) | committed frontmatter |
| `attachments` | A (shared) | committed `attachments/<ulid>/` |
| `created_at` · `updated_at` · `completed_at` · `verified_at` · close/move fields (`closed_at` · `close_reason` · `duplicate_of` · `moved_to_store` · `moved_at`) | A (shared) | committed frontmatter |
| `claimed_by` · `claim_lease_expires_at` · `worker_label` · `claim_count` | A (shared, but expiring) | committed frontmatter — a stale lease is reclaimable, never wedges |
| **read state** (`last_read_at` / unread) | **B (local)** | `local/reads.json` (gitignored), keyed by ULID |
| **feedback drafts** (notes of kind `feedback_draft`) | **B (local)** | dropped from the committed file today; overlay persistence is HS2-AWTHJE |
| **UI / view state** (last view, scroll, drawer) | **B (local)** | overlay `local/…` — HS2-AWTHJE |
| **machine preferences** | **B (local)** | `hotsheet-settings.local.json` (gitignored, `Scope::Local`) / overlay |
| a whole `visibility: local` store | C | its own git repo, no remote, gitignored from the project |

**Built (HS2-21):** the Tier B **overlay mechanism** — `ticketing::LocalOverlay`
reads/writes gitignored files under `<store>/local/` (adding `local/` to
`.gitignore` on first write), durable on disk so an index rebuild reconstructs it.
Its first consumer is **read tracking** (`local/reads.json`; `hotsheet read <slug>`
marks read, `hotsheet ls` shows an unread `●`). Feedback-draft persistence, UI/view
state, and machine-pref reconciliation slot into the same overlay next
(**HS2-AWTHJE**).

## 2.12 Automatic repo syncing — aggressive, hands-off

> **Maintainer requirement (2026-08-19):** *"syncing of the tickets repo should be
> entirely / almost entirely automatic. Fetching, pushing, rebasing/merging should
> all be done by Hot Sheet fairly aggressively. Users CAN do these things
> themselves, but that should almost never be required."* Tracked: HS2-19.

For every store with `sync.mode: git-remote`, Hot Sheet runs a **background sync
engine** so the user effectively never runs git by hand:

- **Fetch** aggressively — on a short interval *and* event-driven (on focus, after
  a local change, on reconnect).
- **Integrate** incoming changes by rebase/merge **through the semantic merge
  driver** (§2.7), so pulls land automatically. Because merges are automatic
  (§2.7) this is safe to do without asking.
- **Commit** local ticket edits automatically (a Hot Sheet edit writes the file and
  commits it — the user doesn't stage/commit tickets manually).
- **Push** automatically after local commits, with backoff + retry on failure.
- **Offline-tolerant:** when the remote is unreachable, keep working locally and
  reconcile on reconnect; never block a local edit on the network.
- **Surface a conflict only when the driver genuinely cannot resolve** (the rare
  same-paragraph body case, §2.7) — otherwise sync is invisible.
- **Coordinates with the watcher/index** ([03](03-indexing-and-query.md) §3.4): a
  sync that moves HEAD triggers the git-diff-aware incremental reindex, so the UI
  reflects pulled changes live.

Manual `git`/`hotsheet sync` remains available (power users, debugging, or an
explicit "sync now"), but the default posture is **hands-off**. A `local-only`
store skips only the *remote* half — there's no remote to fetch/push — but Hot
Sheet **still auto-commits its edits locally**, so it keeps full git history and
the merge driver still governs any local branch merges. Detailed design +
cadence/backoff: HS2-19.

## 2.13 Copy & move between stores

Tickets need to move between stores — e.g. promote a scratch idea into the team
backlog, or pull a ticket into the private security store. Two operations, and one
hard git truth shaping both.

**Copy.** Create a **new ticket (new ULID)** in the destination store with the same
content (title, details, category, priority, tags, notes) and copied attachments;
the original is untouched. The new ticket takes the destination store's prefix, and
records `copied_from: <source-ulid>` provenance in its frontmatter. (This mirrors
HS1's copy/paste = a new ticket number.)

**Move — there is no true git "move" across repos.** Once content is committed to a
git repo it is in that repo's history *permanently*, barring a force history
rewrite (which Hot Sheet **never** does automatically). So "move" is implemented as
**copy-to-destination (keeping the same ULID) + a tombstone left in the source**
(approach confirmed by the maintainer, 2026-08-19 — keeping the ULID is what lets
references survive a move):

- The **destination** store gets the ticket file with the **same ULID** — so every
  reference to it (`blocked_by`, mentions) keeps resolving — and the destination
  store's prefix (its *slug* changes, e.g. `HS-7F3K9Q` → `SEC-7F3K9Q`; the ULID is
  the stable key, the slug is cosmetic). Attachments move with it.
- The **source** store keeps a small **tombstone / redirect** for that ULID —
  `status: moved`, `moved_to_store: <dest-id>`, `moved_at` — which the UI hides from
  normal views. It exists so the source store's final committed state cleanly says
  *"this went to `<store>`"* rather than showing a bare deletion, and so anyone
  browsing the source (or its history) can follow the pointer.
- The **index** resolves a ULID to its single **live** instance (the non-tombstone),
  keying rows by `(store_id, id)` and excluding `moved` rows from ticket lists.

**The retention caveat — say it out loud (security-relevant).** Because git never
forgets, **moving a ticket *out* of a shared/remote store does NOT remove it from
that store's history or its remote.** The tombstone marks it moved, but every past
commit — and the copy already pushed to the remote — still holds the full content.
So *"move it to my private store"* is **not** a way to un-expose something already
committed to a shared repo. Genuinely purging it needs a manual history rewrite
(`git filter-repo` / BFG) + force-push, which Hot Sheet leaves entirely to the user.
Symmetrically, moving a ticket *into* a shared store **exposes** it and the sync
engine (§2.12) will push it. **Hot Sheet warns before any move that changes a
ticket's exposure** (private → shared, or shared → "hidden" that history retains).

**Surfaces.** `hotsheet copy <slug> --to <store>` / `hotsheet move <slug> --to
<store>` (CLI), matching MCP tools, and a UI affordance (drag a ticket onto a store,
or a "Move to store…" menu — mirroring HS1's cross-project drag), with the exposure
warning shown before confirming. Build: **HS2-60**.

## 2.14 Cross-references
- IDs and slugs feed the index: [03-indexing-and-query.md](03-indexing-and-query.md).
- Migration from the PGLite schema: [07-migration.md](07-migration.md).
- Claim/lease coordination: [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.7.
- Human assignment & the `assignees` field: [10-assignment-and-collaboration.md](10-assignment-and-collaboration.md).
- Automatic sync engine + distributed model: [08-distributed-and-remote.md](08-distributed-and-remote.md).
