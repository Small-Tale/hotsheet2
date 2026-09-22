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
> (0.3.x _and_ 0.4.x). Only a datadir written by a PGLite _newer than the bundle_ —
> e.g. **PGLite 0.5.x = PG18** (a future Hot Sheet) — can't be opened; those fall back
> to `pglite-migrate`, which fetches a matching engine on demand. (Bundling the
> absolute-latest 0.5.x would be wrong — it can't read 0.3.x/0.4.x datadirs.) Verified
> with real on-disk 0.3.x _and_ 0.4.x clusters, and the 0.5.x/PG18 fetch validated
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
> settings forward, and idempotently configures detected AI tools. Import and first
> backup run as durable, project-owned background jobs with measured phase progress
> when the producer has real totals; unknown phases remain indeterminate
> (HS2-SF9ZZG). Only a verified backup enables the banner offering
> explicit removal of live HS1 artifacts
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
- **(B) Two CLIs: Node _export_ → portable JSON → _import_ into the store.** The
  Node exporter dumps `hotsheet-export.json` (below); a second importer writes the
  store. The importer can be the Rust `hotsheet import` (reusing the core's own
  format writer, so **zero drift**) _or_ also Node.

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
  "settings": {/* shared settings + the effective HS1 custom-command tree */},
  "tickets": [
    {
      "ticket_number": "HS-1234",
      "title": "…",
      "details": "…",
      "category": "bug",
      "priority": "high",
      "status": "started",
      "up_next": true,
      "tags": ["ui"],
      "notes": [{ "id": "n_…", "text": "…", "created_at": "…" }],
      "blocked_by": ["HS-1200"],
      "created_at": "…",
      "updated_at": "…",
      "completed_at": null,
      "verified_at": null,
      "deleted_at": null,
      "attachments": [{ "original_filename": "a.png", "stored_path": ".hotsheet/attachments/…" }],
    },
  ],
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
  makes retries idempotent. The HS1 number is also retained in the `legacy_number`
  frontmatter field so old `HS-N` references resolve/search against the new ticket
  (HS2-4H2ZR1); it is provenance, not the ticket's HS2 identity. `legacy_number` is
  carried on the wire ticket row and full ticket so clients can auto-link a bare
  `HS-N` reference in rendered ticket text to the imported ticket — one match opens
  directly, several offer the compact chooser — the same way HS2 slug references link
  (HS2-XB5R3Y). Single-digit legacy numbers (`HS-1`…`HS-9`) are not auto-linked because
  the shared reference pattern requires at least two trailing characters (HS2-T9TVYT).
- Map fields → HS2 frontmatter; body ← `details`; notes → the `## Notes` section
  (each note gets a ULID id, §2.6); map HS1 `completed`/`verified` to a `completed`
  close outcome and `archive`/soft-delete to `obsolete`, carrying the best available
  close timestamp and clearing `up_next` for every inactive status ([02](02-ticket-storage.md) §2.6a).
- Before writing a ticket with attachments, atomically record its expected attachment
  list in `hotsheet-hs1-import-pending/<ticket-ulid>.json` in the destination store.
  Write the ticket, copy attachments to `attachments/<id>/`, and checkpoint each
  verified attachment identity. A failed read or write leaves this checkpoint intact.
  Retrying the same export repairs unfinished copies while retaining current ticket
  fields, notes, timestamps, attachment names, annotations, and provenance. Readable
  destination payloads are retained without requiring their staged source files;
  already-verified attachments deliberately removed between attempts stay removed.
  An export omitting a pending ticket, a changed attachment list, an unsupported
  checkpoint version, or a ticket file disappearing after verified copies fails
  explicitly and retains the checkpoint.
  Remove the checkpoint only after the remaining destination payloads are verified,
  and commit the recovery even when it writes no new tickets (HS2-9ZW7B8).
- A completed ticket without a pending-copy checkpoint is skipped on reimport, so
  later user edits and deletions remain authoritative. This also conservatively skips
  partial imports made by older versions that never wrote checkpoints: the importer
  cannot distinguish their missing copies from intentional deletions. Explicit legacy
  diagnostics and selected recovery are tracked separately in HS2-94EB35.
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

### Explicit recovery of older partial attachment imports

Older imports can lack both a payload and its pending-copy checkpoint. Absence alone
cannot distinguish an interrupted import from an attachment intentionally deleted later.
Ordinary reimport preserves those deletions and existing ticket edits (HS2-94EB35).
Use a complete portable export to review the destination first:

```sh
hotsheet-cli -C <ticket-store> import <bundle>/hotsheet-export.json --diagnose-attachments
```

This command performs no writes, including Git maintenance or proof updates. Its JSON
lists every exported attachment with a stable `selection` (`TICKET_ULID/ATTACHMENT_ULID`),
source/destination names, source SHA-256 or read error, pending-checkpoint and omission
evidence, and one of
`present`, `missing_ticket`, `missing_metadata`, or `missing_payload`. Import missing
tickets normally before repairing their attachments. A selected ticket with a pending
import checkpoint must complete its ordinary retry before legacy recovery. Recovery
requires readable payloads within the selected portable bundle; absolute paths and links
outside it are rejected.

Restore only reviewed selections; both selection options may be repeated:

```sh
hotsheet-cli -C <ticket-store> import <bundle>/hotsheet-export.json \
  --restore-attachment <ticket-ulid>/<attachment-ulid>
```

The operation validates the full request before writing, leaves other attachments alone,
and preserves ticket fields, timestamps, edited attachment names, annotations, and
provenance. Existing readable payloads are never overwritten. Repeating an unchanged
selection is a no-op. Source changes, malformed evidence, conflicting selections, or
unknown identities fail explicitly.

If an exported attachment was intentionally deleted, confirm that specific omission:

```sh
hotsheet-cli -C <ticket-store> import <bundle>/hotsheet-export.json \
  --confirm-omission <ticket-ulid>/<attachment-ulid>
```

Confirmation is allowed only when destination attachment metadata is absent; it never
deletes existing metadata or files. A missing payload with retained metadata must be
restored, or explicitly deleted through normal attachment operations first. Confirmation
writes version-1 `hotsheet-hs1-attachment-omissions.json` evidence containing the ticket
and attachment IDs, source filename and SHA-256, an export-identity fingerprint, and the
confirmation time. The evidence follows the store's autocommit policy and must be
included in the proven import revision. Changed source content or attachment identity invalidates that approval.

An actual recovery or new confirmation invalidates previous import/backup proof. Rerun
migration, then push and verify backup again before cleanup. Matching explicit omissions
are accepted during completed-import verification; ordinary reimport still leaves them
absent. Restoring an omitted attachment removes its omission entry. Repeating an already
confirmed unchanged omission preserves its timestamp and existing proof.

## 7.3 The UI-prompted flow (per project, on demand)

Per the ticket, migration is offered automatically with a confirmation — **per
project**, when that project is opened (not a batch over all projects, which a user
may not have open at once):

1. On opening a directory that has a `.hotsheet/db/` cluster without matching
   verified import-completion evidence in its active HS2 store, Hot
   Sheet detects a **migratable HS1 project** and prompts: _"This project has Hot
   Sheet 1 data. Convert it to the new git-based format?"_ The prompt names the exact
   source and database paths plus the detected PostgreSQL version. Choosing Not now
   suppresses later automatic modal presentation for that checkout/source identity;
   a project banner keeps an explicit Import action available.
2. On confirm, Hot Sheet **runs the bundled migrator against this one project**.
   The bridge accepts the job with HTTP 202, then the dialog closes into a persistent
   project banner and tab indicator. Other projects remain usable. Measured bars
   describe the current phase; opening a database, committing, configuring tools, and
   waiting for remote acceptance remain indeterminate. The migrator remains a separate
   bundled executable (§7.2), spawned by the bridge rather than embedded in the core.
3. On success it shows a summary (N tickets, M attachments), links the new source,
   and leaves the old `.hotsheet/` data in place. The migrator verifies that its
   receipt, every ticket, and every referenced attachment payload are included in a
   clean Git revision, with no unfinished attachment checkpoint. Even ignored payloads
   must be present in that revision. It also checks every attachment expected by the
   actual export, so a legacy partial import with missing metadata cannot gain proof
   merely because its ticket already exists. Missing exported attachments refuse
   completion without recreating intentional deletions; selected legacy recovery is
   tracked in HS2-94EB35. It then atomically records that exact revision and
   source project in Git-local `hotsheet-hs1-import-completed.json`. A new migration
   attempt invalidates earlier import and backup proof before exporting. Older receipts
   without this proof require a repeat import before cleanup can become available.
4. The first remote connection/push is another project-owned background job. Git
   reports object counts and, when available, bytes sent and transfer rate. These are
   phase-local measurements, without an invented overall or byte-total percentage. Only a successful
   push followed by verification that the remote branch contains the intended import
   revision records `hotsheet-hs1-backup.json` in Git-local metadata. This proof binds
   the source project, import revision, origin URL, branch, and observed remote revision.
   Reopening uses this local proof and current local Git state without contacting the
   network; origin existence alone never enables cleanup. The destructive DELETE
   endpoint independently rechecks the proof, clean state, origin URL, and current
   remote ancestry before removing any source data. An unavailable, stale, or replaced
   backup refuses deletion. A matching existing origin can be retried; failed setup
   removes only the origin added by that attempt, provided its URL has not changed.
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

### Background ownership and progress protocol

`POST /__hotsheet/projects/migration-jobs` accepts canonical project, source, destination,
operation kind, and optional remote/retry attempt. Repeated starts rejoin the current
attempt; conflicting writers fail explicitly. Native advisory locks serialize the
canonical project and destination across bridge processes. The process-owned registry
persists atomic snapshots under `${HOTSHEET_HOME:-~/.hotsheet2}/migration-jobs`
(`HOTSHEET_MIGRATION_JOBS` overrides that directory). Lock files stay in place; their
existence alone is never treated as ownership.

`GET` on the same route returns a snapshot for `root`; `after=<revision>` blocks until
change or a 25-second idle timeout. Subscribers may disconnect without cancelling the
job. There is no fixed-interval state polling. Reload/reopen joins the saved attempt,
and stale revisions cannot overwrite a retry. Invalid/version-skewed or immediate idle
watch responses stop watching with an explicit Reconnect action rather than looping.
The stable project id, attempt id, and increasing revision keep background completion
from changing the currently selected project's data or dialogs.

Before launching actual work, a child gate waits for its PID checkpoint to be durable.
The bridge owns the process group, including termination on malformed progress. A
bridge restart reports unfinished work as interrupted; Retry refuses to launch while
the checkpointed child is alive. Retry resumes the existing idempotent import and
attachment checkpoints. Terminal success is persisted before watchers see it. A
persistence error becomes a failure, never a premature success announcement.
Source registration, checkout linking, tool setup, and backup validation belong to
the captured server job, so closing a tab cannot skip finalization. Tool-setup warnings
remain available in Details and expose Retry; completed import stays separate from
verified backup and source cleanup. A failed backup offers Retry for its captured remote
or Change backup to correct a URL before a new attempt. A successful historical backup
whose proof no longer matches the current repository offers the same recovery actions
while the old source remains; deleting the old source removes that prompt. Completion
refresh and later project reloads retain the linked custom ticket-store destination.

`hotsheet-migrate ... --progress-json` and the exporter's matching option emit version-1
NDJSON with `phase`, optional measured `completed`/`total`/`unit`, warnings, and a typed
final result/error. Human diagnostics and Git-hook output stay on stderr. Actual
producer boundaries are database bytes after each successful file copy, exported and
imported ticket items, staged attachment bytes, and verified/copied attachment items
within each ticket. Empty phases report 0 of 0; a phase reaching 100 percent does not
mean the whole job succeeded. Unknown engine/download and Git denominators stay unknown.

A referenced HS1 attachment that is missing or not a readable file **fails export**
before a portable manifest can discard its identity (HS2-FBVNJ6). It records no new
completion or cleanup proof. Restore the original payload or use a complete portable
export with the explicit reviewed recovery procedure above; source omissions are never
silently approved. Database copying and export leave the original source bytes intact.

The migration prompt uses Kerf's canonical spacing scale: it separates major regions by
24 px, its icon/copy pair by 16 px, and fields/actions by 8 px, with a 4 px connected
copy offset. The persistent import and cleanup notices are `StateBanner` compositions
with polite status semantics, info and success tones, and their existing 8 px block by
16 px inline padding, 16 px column separation, and 4 px connected-copy gap. Compact
actions retain their explicit 28.8 px minimum height and 8 px inline padding. The
cleanup notice supplies its Dismiss and Delete controls as one app-owned action group
inside the primitive's single action slot so the pair wraps together at narrow widths.

Browser regressions keep import/backup/delete and persisted dismissal/reopen as independently
initialized scenarios. After reload, they wait for the owning project and its loaded workspace
before asserting banner absence. Fixtures intercept only bridge API requests, keep idle
long polls pending, and give each test its own screenshot paths so parallel runs do not
intercept every development asset or overwrite each other's evidence (HS2-AMYY7Z).

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
and real commit failures retain their diagnostic warning and the standalone migrator
fails if the intended import cannot be proven fully committed; it records no completion
or cleanup proof. After fixing the commit failure, rerun migration.

For headless backup, push the imported ticket store normally, then run
`hotsheet-migrate -C <ticket-store> --verify-backup`. This verifies the recorded import
revision against the current origin branch and writes the same backup proof consumed
by the client and cleanup endpoint, without opening HS1 or rerunning export. A changed
origin, old remote revision, dirty store, or unfinished import fails verification.

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
