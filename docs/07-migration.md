# 07. Migration — PGLite → Git Repos

> **Status: Partially built (shape B).** A **standalone, bundled, one-time,
> per-project** migrator — not a first-class core feature (maintainer, 2026-08-19).
> UI-prompted per project. Build: **HS2-14**.
>
> **Built + validated on real data:** the Node exporter (`migrator/src/export.mjs`,
> PGLite → `hotsheet-export.json`) and the Rust importer (`hotsheet import`, reusing
> the core writer for zero drift), with a cross-language conformance test (§7.2.1).
> We chose **shape (B)** over the shape-(A) default because the Rust importer doubles
> as the CLI's file writer and eliminates format drift. Proven end-to-end against a
> real HS1 cluster (this project's own snapshot): **81 tickets** exported + imported
> with notes, `legacy_number`, and close outcomes intact.
>
> **Version coverage:** targets the **5 most recent production releases + the current
> beta** (v0.17.2 … v0.20.0 + v0.21.0-beta). These use PGLite **0.3.x** (v0.17.x) and
> **0.4.x** (v0.18.0+) — all Postgres 17. The exporter bundles **one** engine, the
> PGLite line Hot Sheet ships (`@electric-sql/pglite` 0.4.x), and tries it first: a
> newer PGLite reads older datadirs, so 0.4.x opens **every** supported HS datadir
> (0.3.x *and* 0.4.x). Only a datadir written by a PGLite *newer than the bundle* —
> e.g. **PGLite 0.5.x = PG18** (a future Hot Sheet) — can't be opened; those fall back
> to `pglite-migrate`, which fetches a matching engine on demand. (Bundling the
> absolute-latest 0.5.x would be wrong — it can't read 0.3.x/0.4.x datadirs.) Verified
> with real on-disk 0.3.x *and* 0.4.x clusters, and the 0.5.x/PG18 fetch validated
> end-to-end. See [`migrator/README.md`](../migrator/README.md).
>
> **Real-cluster lessons baked into the exporter** (from the HS1 source): the newest
> PGLite reads older datadirs (bundle the line HS ships, not the absolute latest); the
> join column is **`blocks_on_ticket_id`**; a cluster written by PGLite < 0.4.0 keeps
> its tables in **`template1`**, not `postgres` (the opener probes both); and the
> column set is read **tolerantly** so schema drift across releases degrades instead
> of erroring.
>
> **Attachments** migrate: the exporter reads the `attachments` table (promoted
> only — `draft_id IS NULL`), stages the files next to the export JSON, and rewrites
> each `stored_path`; `hotsheet import` copies them into `attachments/<new-ulid>/`
> (basename-sanitized). Source files resolve by basename under
> `<.hotsheet>/attachments/` so a moved project still works.
>
> **Not yet:** the UI-prompted per-project flow (§7.3). (The `pglite-migrate` fetch for
> a newer-than-bundle datadir is validated against a real PG18 cluster; it just isn't
> in the offline CI suite because it downloads an engine — HS2-82.)

## 7.1 The problem

HS1 stores everything in an embedded Postgres cluster (PGLite/WASM) under
`.hotsheet/db/`. HS2 stores tickets as files in git. We need a reliable, idempotent
converter — and the source data is only readable through PGLite, a Node/WASM
artifact the Rust core won't (and shouldn't) link.

## 7.2 A disposable, bundled migrator — not part of the long-lived core

Migration runs **once per project and then never again**, so it should not be baked
into the Rust core we maintain forever. It is a **standalone tool bundled with Hot
Sheet** that we can retire once everyone's data is moved (maintainer decision,
2026-08-19). It is fine for it to be **Node end-to-end** — Node already speaks
PGLite, and the target file format (Markdown + YAML) is trivial to write from Node.

**Per-project + one-time.** A user may not have all their old projects open at once,
so there is **no "migrate everything" step** — the migrator runs **against one HS1
project's `.hotsheet/`**, invoked on demand (or auto-prompted, §7.3) when that
project is opened. Run it once per old project.

### 7.2.1 Two acceptable shapes (either is fine)

- **(A) One standalone Node CLI, PGLite → git store directly.** Reads the old
  cluster read-only and writes the HS2 ticket files + attachments + initial commit
  in one pass. Simplest to ship and run; the risk is it **re-implements the file
  format** in Node, which could drift from the core's parser.
- **(B) Two CLIs: Node *export* → portable JSON → *import* into the store.** The
  Node exporter dumps `hotsheet-export.json` (below); a second importer writes the
  store. The importer can be the Rust `hotsheet import` (reusing the core's own
  format writer, so **zero drift**) *or* also Node.

**Recommendation:** whichever shape, **the format is verified against the core's
parser** — a conformance test that the real `hotsheet-core` cleanly reads (and
round-trips) what the migrator wrote. That single test removes the only real risk
of a Node-side writer (drift), which frees us to pick shape (A) for simplicity. Use
the JSON interchange (B) if it's handy for debugging or staged runs; it's not
required.

### The export JSON (used by shape B; also a useful debug artifact for A)

```jsonc
// hotsheet-export.json  — one file per HS1 project
{
  "exportVersion": 1,
  "project": { "name": "…", "ticketPrefix": "HS" },
  "settings": { /* .hotsheet/settings.json merged */ },
  "tickets": [
    {
      "ticket_number": "HS-1234",
      "title": "…", "details": "…",
      "category": "bug", "priority": "high", "status": "started",
      "up_next": true, "tags": ["ui"],
      "notes": [ { "id": "n_…", "text": "…", "created_at": "…" } ],
      "blocked_by": ["HS-1200"],
      "created_at": "…", "updated_at": "…", "completed_at": null,
      "verified_at": null, "deleted_at": null,
      "attachments": [ { "original_filename": "a.png", "stored_path": ".hotsheet/attachments/…" } ]
    }
  ]
}
```
Attachment files are copied to a staging dir alongside the JSON. This reads the full
HS1 schema the exploration confirmed (`tickets`, `attachments`, `ticket_blocked_by`,
notes-as-JSON, tags-as-JSON; the claim columns are runtime-only and **not**
exported).

### What the write step does (either shape)

For each ticket:
- **Mint a fresh ULID** and derive an all-caps slug ([02](02-ticket-storage.md)
  §2.4). Preserve the old **`HS-1234`** number in frontmatter as `legacy_number` so
  history and external references stay resolvable and searchable.
- Map fields → HS2 frontmatter; body ← `details`; notes → the `## Notes` section
  (each note gets a ULID id, §2.6); map an HS1 `completed`/`verified` outcome to the
  new close fields where sensible ([02](02-ticket-storage.md) §2.6a).
- Write the file into the target store; copy attachments to `attachments/<id>/`.
- Rewrite `blocked_by` old-number refs to new ULIDs (**two-pass**: assign all IDs
  first, then resolve edges — including `duplicate_of` if present).
- `git init` the store if needed, install the merge driver (§2.7), make the initial
  commit ("Import N tickets from Hot Sheet 1"). The HS2 server (re)builds the index
  from the files afterward.

**Idempotent & safe:** re-running detects already-imported tickets by
`legacy_number` and skips them (never duplicates); the source cluster is opened
**read-only** and never modified.

## 7.3 The UI-prompted flow (per project, on demand)

Per the ticket, migration is offered automatically with a confirmation — **per
project**, when that project is opened (not a batch over all projects, which a user
may not have open at once):
1. On opening a directory that has a `.hotsheet/db/` cluster but no HS2 store, Hot
   Sheet detects a **migratable HS1 project** and prompts: *"This project has Hot
   Sheet 1 data (N tickets). Convert it to the new git-based format?"*
2. On confirm, Hot Sheet **runs the bundled migrator against this one project**,
   streaming progress to the UI. (The migrator is a separate bundled executable —
   §7.2 — the server just spawns it; it does not live in the core.)
3. On success it shows a summary (N tickets, M attachments) and leaves the old
   `.hotsheet/db/` **in place** (renamed to `.hotsheet/db.hs1-backup/`) so nothing
   is destroyed — the user deletes it when satisfied.

The same migration is runnable **by hand** in one command, independent of the UI
prompt: **`hotsheet -C <new-store> migrate <old-project>/.hotsheet`** spawns the Node
exporter against a copy of the old database and imports the result. (Under the hood
that is the two-step `node migrator/src/export.mjs …` + `hotsheet import …`, which
remain available separately.)

## 7.4 What is and isn't migrated

- **Migrated:** tickets (all fields), notes, tags, attachments, blocked-by edges,
  category/priority/status, up_next, timestamps, project settings that still apply.
- **Not migrated (runtime/derived):** claim/lease state (transient), the index
  (rebuilt), generated `worklist.md`/`open-tickets.md` (regenerated), telemetry
  rollups and the Announcer history (HS1-specific; a later, optional export if
  wanted), backups/snapshots (obsolete — git is the new history).
- **Ticket numbers:** the linear `HS-N` becomes `legacy_number`; the live handle
  is the new slug. This is the one visible discontinuity, and it's inherent to
  dropping the central sequence.

## 7.5 Bidirectional / rollback

One-way by design (HS2 is a fresh implementation). Safety is the read-only source +
the renamed backup cluster + the idempotent import — a user can re-run HS1 against
`db.hs1-backup/` if they abort. No live two-way sync between formats.

## 7.6 Cross-references
- Target format the migrator writes (and is conformance-tested against):
  [02-ticket-storage.md](02-ticket-storage.md) §2.5, §2.6a
- The migrator is a **standalone bundled tool**, not part of the long-lived core
  ([04-core-server-cli.md](04-core-server-cli.md)); if shape (B)'s importer is Rust,
  it reuses the core's format writer, but the migrator itself is disposable.
