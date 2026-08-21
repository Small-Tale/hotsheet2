---
id: 01M0H6M3SJAE5N1GNVK9RSTX1D
slug: HS2-A9T87H
title: 'Server lifecycle: independent process, client auto-start, survives client exit'
category: feature
priority: high
status: not_started
created_at: 2026-08-19T01:55:57.130Z
updated_at: 2026-08-19T01:55:57.130Z
legacy_number: HS2-59
schema: 1
---

Maintainer decision (2026-08-19): the server is a separate process in ALL cases (local included); no client embeds the core. Build the lifecycle: (1) client discovers a running local server via ~/.hotsheet/instance.json; (2) if none, spawns hotsheet serve DETACHED (double-fork/setsid or a launchd/systemd user service) so it is NOT tied to the client's lifecycle; (3) server SURVIVES client exit (in-flight AI work, terminals, watcher keep running); (4) join-don't-collide via per-project index-writer lock + instance file so a second client/CLI/window attaches instead of duplicating; (5) client may SUPERVISE (restart-on-crash, health) but never OWNS; (6) explicit shutdown only (hotsheet serve --stop / menu / OS service stop), never implicit on client close; (7) iOS has no local server (background limits) — remote-first. Locally: loopback+secret default, mTLS optional. See docs/04 §4.3.1, docs/09 §9.1e, docs/06 §6.2.
