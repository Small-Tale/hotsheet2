# CLI and service parity audit

This audit compares the public command enum in `crates/hotsheet-cli/src/main.rs`,
the MCP tool registry in `crates/hotsheet-mcp/src/lib.rs`, and the HTTP route table
in `crates/hotsheet-server/src/lib.rs` as of 2026-10-08. It covers high-level
user workflows, rather than requiring a one-to-one command for each transport
detail. A generic `settings set` is counted only when it addresses the same
project and applies the same validation as the dedicated service route.

| Workflow | CLI | Service | MCP | Finding |
| --- | --- | --- | --- | --- |
| Git-backed ticket list, read, create, edit, close, restore, assign, claim, copy and move | Yes | Yes | Yes | Core workflow covered. |
| Batch ticket update | No | `POST /batch` and checkout batch | `hotsheet_batch` | Add CLI parity in `HS2-HQ5YBR`. |
| Delete a Git-backed ticket note | No | Ticket note DELETE | No | Add a headless path in `HS2-HQ5YBR`. |
| Provider-native assignment, restore, note deletion and not-working report | No | Provider ticket routes | No | Existing `provider-*` commands do not address these operations; `HS2-CW56C9`. |
| Attach files and correct attachment actor | Yes | Yes | No | The remaining attachment lifecycle operations are in `HS2-31NJFQ`. |
| Rename/delete an attachment; replace batch, actor and purpose metadata; copy a provider attachment | No, except actor correction | Yes | No | `HS2-31NJFQ`. |
| Replace media annotations | No | Checkout attachment PUT | No | Already tracked by `HS2-3GA0WK`, including shared validation. |
| Provider connection creation, linking, enable/disable, removal, sign-in and account sign-out | Yes for GitHub; generic provider support is capability-dependent | Yes | Provider inventory and transfers only | Generic GitLab/Jira CLI creation is already tracked by `HS2-65KYVP`. |
| Project-scoped ticket source color | No | Checkout source color PATCH | No | `HS2-G9QYZC`. |
| Saved command definitions and groups | Generic project settings read/write | Dedicated checkout and global routes | No | Definitions can be edited headlessly, but the CLI has no saved-command runner or run history; `HS2-YEMAVH`. |
| Saved command run, inspect, cancel | No | Checkout and global command-run routes | No | `HS2-YEMAVH`. |
| Custom views and shared project settings | Generic project settings read/write | Dedicated views and settings routes | No | No demonstrated CLI gap for stored definitions; keep validation parity under normal settings tests. |
| AI setup, defaults, tool discovery and driven turns | Yes | Setup, AI settings, tool inventory and Drive routes | Limited | The CLI has headless setup and `trigger`/`work`; live Drive connections are server session resources rather than durable configuration. |
| Ticket-flow analytics, usage metrics, activity and notifications | Usage metrics only | Analytics, activity and notification routes | No | `HS2-JCNSXC` covers headless ticket-flow reads and activity/notification state. |
| Repository browser, terminal PTY, permission bridge and WebSocket replay | Local git/launch/permission-hook equivalents where appropriate | Live server resources | No | These operate on a running server or browser session. Command parity would mean a client to the server, not a second direct-to-disk implementation; no missing durable operation established. |
| Store bootstrap, import, format activation, sync and reindex | Yes | Partial setup and repository routes | No | Local administration is intentionally headless in the CLI. The server's live lifecycle does not need to duplicate destructive store maintenance. |

The audit found concrete gaps in eight follow-up tickets above, including the
pre-existing media-annotation and GitLab/Jira setup tickets. It did not treat
transport-only endpoints (health, compatibility, polling, streaming, process
restart, media byte ranges and thumbnails) as separate product functions. The
service and CLI should continue to use shared core operations where a gap is
closed, with provider capability errors where a source cannot support it.
