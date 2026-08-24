---
name: hs-feature
description: Create a new feature ticket in Hot Sheet
---
<!-- hotsheet-skill-version: 28 -->

Create a new Hot Sheet **feature** ticket. New features to be implemented.

**Parsing the input:**
- If the input starts with "next", "up next", or "do next" (case-insensitive), set `up_next` to `true` and use the remaining text as the title
- Otherwise, use the entire input as the title

**Create the ticket — MCP tool (preferred when the channel is connected):**
Call the `hotsheet_create_ticket` tool with `{ "title": "<TITLE>", "category": "feature", "up_next": <true|false> }`. The tool is schema-validated and routes to the channel server's `--data-dir` so there's no chance of cross-project misrouting.

**Fallback (curl):**
```bash
curl -s -X POST "http://localhost:$HOTSHEET_PORT/api/tickets" \
  -H "Content-Type: application/json" \
  -H "X-Hotsheet-Secret: $HOTSHEET_SECRET" \
  -d '{"title": "<TITLE>", "defaults": {"category": "feature", "up_next": <true|false>}}'
```

Set these first. Both are machine-specific and deliberately not stored in this file (which is committed and shared with everyone on the repo):
```bash
export HOTSHEET_PORT=$(node -p "require('./.hotsheet/settings.local.json').port ?? 4174")
export HOTSHEET_SECRET=$(node -p "require('./.hotsheet/secret.json').secret")
```

If the request fails, distinguish generations before retrying credentials: `hotsheet-store.json`
(directly or via `.hotsheet/store`) is HS2; `.hotsheet/db/PG_VERSION` is HS1. A 401/403 from
an endpoint that does not identify as HS2 may be an HS1/wrong-project connector; use the
explicit HS2 CLI store instead. Only re-read secrets after confirming the endpoint is HS2.

Report the created ticket number and title to the user.
