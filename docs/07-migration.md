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
> **Real-cluster lessons baked into the exporter** (from the HS1 source): the engine
> must match the datadir's PG major — PGLite **0.4.x = PG17**, 0.3.x = PG16 (a
> mismatch is an opaque WASM abort, now caught with a clear message); the join table
> column is **`blocks_on_ticket_id`**; and a cluster predating PGLite 0.4.0 keeps its
> tables in **`template1`**, not `postgres`, so the opener probes both.
>
> **Not yet:** attachments (copied + written, HS2-78); the UI-prompted flow (§7.3);
> and cross-**major** datadirs (PG16/PG18) — bridged by `pglite-migrate` (HS2-82).

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

The same migrator is runnable **by hand** from a terminal for anyone who prefers it
(`hotsheet-migrate <path-to-old-project>`), independent of the UI prompt.

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
