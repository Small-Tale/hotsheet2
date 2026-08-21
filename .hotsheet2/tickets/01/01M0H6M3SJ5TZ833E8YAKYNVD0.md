---
id: 01M0H6M3SJ5TZ833E8YAKYNVD0
slug: HS2-QTDS2X
title: Live Codex app-server client (real AppServerClient over codex app-server proxy JSON-RPC)
category: feature
priority: default
status: completed
created_at: 2026-08-20T22:41:29.861Z
updated_at: 2026-08-21T01:28:01.609Z
completed_at: 2026-08-21T01:28:01.609Z
closed_at: 2026-08-21T01:28:01.609Z
close_reason: completed
legacy_number: HS2-112
schema: 1
---

Follow-up from HS2-110 (which built AppServerDrive + the AppServerClient port, fake-tested). Implement the REAL AppServerClient that drives the running codex app-server daemon:
- Ensure the daemon is up (BackingService): `codex app-server daemon` / `codex remote-control start`; is-it-up check.
- Connect: `codex app-server proxy` (proxy stdio to the control socket) and speak JSON-RPC: initialize → thread/start (or thread/resume for a Target thread id) → turn/start with the prompt items → observe turn/started..turn/completed (busy→done), item/agentMessage/delta for output.
- Map approval ServerRequests (execCommandApproval, item/*/requestApproval, item/tool/requestUserInput) to the permission bridge (§5.7).
- turn/interrupt for interrupt.

Acceptance: a real codex app-server turn runs against the daemon (reuses the instance, no new process per play) and reports Completed/Failed; interrupt works; a resumed thread continues. Gated/creds as needed. Apply HS2-103 launch safety.

Relates to: HS2-110, HS2-109 (real trigger), docs/13 §13.5.
