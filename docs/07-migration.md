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
> with notes, dependency edges, and normalized close outcomes intact. HS1 ticket
> numbers are conversion-only inputs and are not retained in HS2.
> Headless `init`/`doctor` detect the exact `.hotsheet/db/PG_VERSION` marker and
> print the close-HS1 warning plus the explicit migrator command without running it
> (HS2-MNHGT3); the interactive prompt/progress experience remains client-owned.
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
> **Built (HS2-PWYTS8):** selecting an HS1 project in the local client detects the
> exact database marker and opens a destination-only import prompt that identifies
> the source folder, database path, and PostgreSQL version. Dismissing that modal is
> remembered for the detected source, with a non-blocking banner left available to
> reopen it. The bridge runs
> the bundled one-shot migrator, links the resulting source, carries applicable
> settings forward, and idempotently configures detected AI tools. Once the imported
> repository has a remote, a banner offers explicit removal of live HS1 artifacts
> while preserving every backup and the HS2 store link. (The `pglite-migrate` fetch
> for a newer-than-bundle datadir remains outside offline CI because it downloads an
> engine — HS2-82.)

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
  "project": { "name": "…", "ticketPrefix": "HS", "sourceRoot": "/code/project" },
  "settings": { /* shared settings + the effective HS1 custom-command tree */ },
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
exported). `sourceRoot` is optional for compatibility with older version-1 exports;
the live datadir exporter records it so the importer can reuse the code project's
settings directory. For `custom_commands` only, the exporter resolves the shared
`settings.json` value against `settings.local.json` using HS1's replacement/tree-delta
rules (including hidden items, overrides, child additions, and orphan-group survival).
Other machine-local HS1 settings remain excluded.

The imported shared settings and typed local commands are written back to the original
code project's `.hotsheet2/settings.json` and `settings.local.json`, carrying HS2's
`$hotsheetSchema` marker without sharing HS1's directory. Post-import cleanup still
recognizes schema-marked settings left by earlier HS2 releases and removes unmarked HS1
runtime settings. If an export lacks
an existing `sourceRoot` with an HS1 `.hotsheet` directory, the store-only import command
retains the old beside-store settings paths instead of inventing
`<ticket-store>/.hotsheet`; it never changes ticket-source discovery or ticket-store
locations.

### What the write step does (either shape)

For each ticket:
- **Derive a stable ULID** from the source project/ticket identity and creation time,
  then derive an all-caps slug ([02](02-ticket-storage.md) §2.4). The stable identity
  makes retries idempotent without retaining the HS1 number in HS2.
- Map fields → HS2 frontmatter; body ← `details`; notes → the `## Notes` section
  (each note gets a ULID id, §2.6); map HS1 `completed`/`verified` to a `completed`
  close outcome and `archive`/soft-delete to `obsolete`, carrying the best available
  close timestamp and clearing `up_next` for every inactive status ([02](02-ticket-storage.md) §2.6a).
- Write the file into the target store; copy attachments to `attachments/<id>/`.
- Rewrite `blocked_by` old-number refs to new ULIDs (**two-pass**: assign all IDs
  first, then resolve edges — including `duplicate_of` if present).
- Translate every runnable HS1 custom-command leaf into a native HS2 command kind in
  machine-local `commands` settings. Modern nested groups and the older per-command
  `group` format retain every command and group label. Ordinary buttons retain their
  command text and resolve the current platform shell and project root only at run time,
  so settings contain no fixed local executable or checkout path. AI buttons retain a
  dedicated `ai` kind, prompt, and selected drivable tool (with HS1's Claude fallback),
  without serializing `hotsheet-cli` or its argv. HS1 icon and color metadata carry over;
  retired worker-target and group-collapse state do not.
- `git init` the store if needed, install the merge driver (§2.7), make the initial
  commit ("Import N tickets from Hot Sheet 1"). The HS2 server (re)builds the index
  from the files afterward.

**Idempotent & safe:** re-running derives the same HS2 ULIDs and command ids and skips
equivalent existing values. Existing HS2 command definitions always win; a deterministic
suffix preserves both definitions if a migrated id conflicts, rather than overwriting
local customization. The source cluster is opened **read-only** and never modified.
Shell buttons work immediately. AI buttons also work before the remote-gated cleanup
removes the retained `.hotsheet/db/PG_VERSION` marker, but only when the selected HS2
store has the schema-valid durable `hotsheet-hs1-import.json` receipt whose canonical
`sourceProject` matches this exact checkout. Missing, malformed, or unrelated receipts
leave the strict HS1 launch refusal in place.

## 7.3 The UI-prompted flow (per project, on demand)

Per the ticket, migration is offered automatically with a confirmation — **per
project**, when that project is opened (not a batch over all projects, which a user
may not have open at once):
1. On opening a directory that has a `.hotsheet/db/` cluster without a matching
   import receipt in its active HS2 store, Hot
   Sheet detects a **migratable HS1 project** and prompts: *"This project has Hot
   Sheet 1 data. Convert it to the new git-based format?"* The prompt names the exact
   source and database paths plus the detected PostgreSQL version. Choosing Not now
   suppresses later automatic modal presentation for that checkout/source identity;
   a project banner keeps an explicit Import action available.
2. On confirm, Hot Sheet **runs the bundled migrator against this one project**,
   streaming progress to the UI. (The migrator is a separate bundled executable —
   §7.2 — the server just spawns it; it does not live in the core.)
3. On success it shows a summary (N tickets, M attachments), links the new source,
   and leaves the old `.hotsheet/` data in place. A durable receipt in the new store
   distinguishes a completed import from an unrelated HS2 repository.
4. Only after an `origin` remote exists does the project show its cleanup banner.
   Cleanup requires confirmation and refuses to start while a registered HS1 channel
   process owned by this checkout is still live, because that process can recreate its
   database after removal. Project-tagged entries owned by another checkout are ignored;
   matching and identity-less legacy entries remain conservatively blocking.
   It removes only the explicit HS1 live-data/runtime allowlist (database, attachments,
   migrated settings, and generated runtime files); backups, snapshots, the HS2
   legacy `.hotsheet/store` link, and unknown files are preserved. New HS2 links live at
   `.hotsheet2/store`. Dismiss hides the reminder
   for that checkout and detected HS1 source identity across refreshes. Successful
   cleanup records the same dismissal so a stale or concurrently recreated marker does
   not make the banner recur.

The same migration is runnable **by hand** in one command, independent of the UI
prompt: **`hotsheet-migrate <old-project>/.hotsheet -C <new-store>`** spawns the Node
exporter against a copy of the old database and imports the result. It is a
**separate binary** from the live `hotsheet` CLI on purpose — migration is rarely
used, one-time, and needs Node + the bundled exporter, which the always-on ticket
commands should not carry. (Under the hood it is the two-step `node
migrator/src/export.mjs …` + `hotsheet import …`, which remain available separately.)
Re-running the standalone migrator is a clean success: deterministic tickets and the
completed-import receipt remain unchanged, no extra Git commit is created, and a clean
index does not emit a failure-shaped `git commit` warning. Staging, index inspection,
and real commit failures still retain the migrator's best-effort warning.

## 7.4 What is and isn't migrated

- **Migrated:** tickets (all fields), notes, tags, attachments, blocked-by edges,
  category/priority/status, up_next, timestamps, project settings that still apply,
  effective shared/local custom commands, and detected AI-tool instructions/MCP setup.
- **Not migrated (runtime/derived):** claim/lease state (transient), the index
  (rebuilt), generated `worklist.md`/`open-tickets.md` (regenerated), telemetry
  rollups and the Announcer history (HS1-specific; a later, optional export if
  wanted), backups/snapshots (obsolete — git is the new history).
- **Ticket numbers:** the linear `HS-N` is used only while remapping references and
  deriving stable HS2 identities. It is not persisted; the new slug is the sole live
  handle. This discontinuity is inherent to dropping the central sequence.

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
