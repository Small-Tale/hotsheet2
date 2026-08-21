---
id: 01M0H6M3SJXARC45XFJHZDG1R4
slug: HS2-V5Z2EY
title: Fold ensure_codex_daemon behind a Drive::service() BackingService accessor
category: feature
priority: default
status: not_started
created_at: 2026-08-21T01:17:18.165Z
updated_at: 2026-08-21T01:54:35.252Z
legacy_number: HS2-114
schema: 1
---

Follow-up from HS2-112 / docs/13 §13.5. ensure_codex_daemon(program) exists as the concrete Codex daemon prestart, but a generic caller must import codex.rs to call it. Add the Drive::service() -> Option<&dyn BackingService> accessor (is-on / health / prestart / note_terminal_launch) so a tool-id-free caller can warm the backing service without importing a tool's daemon module (closes the §132.11.2 leak by construction). AppServerDrive returns Some(codex service); spawn/channel drives return None.

Relates to: HS2-112, HS2-110, docs/13 §13.5.
