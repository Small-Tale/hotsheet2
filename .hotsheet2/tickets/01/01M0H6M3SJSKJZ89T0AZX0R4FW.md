---
id: 01M0H6M3SJSKJZ89T0AZX0R4FW
slug: HS2-CER5M5
title: 'Build: usage/cost metrics — capability + JSONL/rollup storage + git sharing'
category: feature
priority: default
status: not_started
created_at: 2026-08-19T07:09:55.613Z
updated_at: 2026-08-19T07:09:55.613Z
legacy_number: HS2-75
schema: 1
---

Implement the metrics design (docs/14): the `metrics` plugin capability (map each tool's telemetry → the common usage-event shape) starting with the Claude mapper; the raw rotating-JSONL writer; the periodic rollup job (aggregate → rollup files, advance meta cursor); the read path (rollups + live-read raw tail newer than the cursor, no DB); a per-model price table (default + override; Anthropic prices from the claude-api skill); per-ticket attribution via the active-ticket tag; git team-sharing via per-contributor rollup files (conflict-free, opt-in, cost/tokens only — never prompt content). Feeds the dashboards (HS2-47). See docs/14.
