---
id: 01M0H6M3SJW1BZPSGV2QXJJWWK
slug: HS2-0QGW07
title: Wire Codex app-server approval ServerRequests to the real permission bridge (§5.7)
category: feature
priority: default
status: not_started
created_at: 2026-08-21T01:17:14.863Z
updated_at: 2026-08-21T01:17:14.863Z
legacy_number: HS2-113
schema: 1
---

Follow-up from HS2-112. The live CodexAppServer (crates/hotsheet-aitools/src/codex.rs) currently opens threads with approvalPolicy="never" and, defensively, auto-approves any approval ServerRequest that still arrives (execCommandApproval/applyPatchApproval -> "approved"; item/commandExecution|fileChange/requestApproval -> "accept"; unknown -> {}).

That is headless-permissive by design for the first slice. To match docs/05 §5.7 / docs/13 §13.4, route these ServerRequests to a real PermissionSink/permission bridge so a host/policy decides (approve/deny/ask), rather than blanket-approving. Also handle item/tool/requestUserInput and mcpServer/elicitation/request meaningfully.

Depends on the permission-bridge abstraction existing. Relates to: HS2-112, HS2-9 (Claude channel async TurnEvent + permission-as-response).
