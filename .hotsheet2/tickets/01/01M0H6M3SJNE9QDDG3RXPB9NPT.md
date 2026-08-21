---
id: 01M0H6M3SJNE9QDDG3RXPB9NPT
slug: HS2-79RXD1
title: 'Git store adapter: read/write/commit ticket files, multi-store resolution'
category: feature
priority: default
status: not_started
created_at: 2026-08-19T00:22:55.855Z
updated_at: 2026-08-19T11:16:46.884Z
legacy_number: HS2-4
schema: 1
---

Implement the store layer (Rust): read/write/commit ticket files in a git-backed store; hotsheet-store.json metadata; id-prefix sharding; attachments dir (attachments/<id>/); multiple stores per project with per-store visibility (shared/local) and sync policy (git-remote/local-only); ULID minting + derived ALL-CAPS display slug; note ids are ULIDs (for auto-merge). EVERY store is a git repo — no plain-files mode (maintainer 2026-08-19); a local-only store is just a repo with NO remote (still commits/history/branches/merge-driver locally). On init/registration, git-init a new store if needed, write the .gitattributes registering the semantic merge driver (HS2-18), and configure the repo to use it. See docs/02-ticket-storage.md §2.1, §2.8.

## Notes

<!-- note: 01M0H6M3SKHH31HWVNPWMP7SMY -->
2026-08-19T11:16:46.884Z — **Partial (2026-08-19):** a concrete filesystem store landed — `hotsheet-ticketing::FsStore` (`store.rs`): init/open, `hotsheet-store.json` metadata (StoreMetadata: schemaVersion/ticketPrefix/idStrategy/shard), 2-char id-prefix sharding, read/write/list ticket files through the core format writer, ULID + derived slug on write. Used by the CLI + importer; tempdir tests.

Still open for HS2-4: wrap this behind the injected FileSystem/GitLocal ports (fakeable I/O for the server, per ports.rs), git commit/read via gix (currently the CLI shells out to `git`), `.gitattributes` + merge-driver registration on init (HS2-18), multi-store resolution + per-store visibility/sync policy. The concrete FsStore is the direct path; the port abstraction is the remaining work.
