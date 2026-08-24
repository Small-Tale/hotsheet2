---
name: hs-feature
description: Create a new feature ticket in Hot Sheet
---
<!-- hotsheet-skill-version: 29 -->

Create a new Hot Sheet **feature** ticket. New features to be implemented.

**Parsing the input:**
- If the input starts with "next", "up next", or "do next" (case-insensitive), set `up_next` to `true` and use the remaining text as the title
- Otherwise, use the entire input as the title

**Create the ticket — MCP tool (preferred when the channel is connected):**
Call the `hotsheet_create` tool with `{ "title": "<TITLE>", "category": "feature", "up_next": <true|false> }`. The HS2 shim is schema-validated and works serverless against the configured git store.

**Fallback (HS2 CLI):**
Use `hotsheet-cli new --title "<TITLE>" --category <CATEGORY>` and add `--up-next` when requested. The CLI resolves `-C`, `HOTSHEET_STORE`, or the project's `.hotsheet/store` link and writes through the same HS2 engine; it does not need a server or secret. Replace `<CATEGORY>` with the category named above.

If a connected MCP does not identify itself as HS2, do not retry HS1 `.hotsheet` credentials. A direct `hotsheet-store.json` or `.hotsheet/store` link identifies HS2; `.hotsheet/db/PG_VERSION` identifies HS1. Use the explicit HS2 CLI store until the connector is corrected.

Report the created ticket number and title to the user.
