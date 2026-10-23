<!-- hotsheet:begin section=sandbox-recovery v=1 -->

## Launched-session sandbox recovery

When a Hot Sheet-launched session's `hotsheet-cli` command fails while
opening the ticket store with `Operation not permitted (os error 1)`, use
the configured `hotsheet_*` MCP tools for that store operation. A sibling
`<repo>.hs2` store may be outside the command sandbox. Keep the same claim,
actor, note, and completion workflow through MCP; do not infer store corruption
from this error alone.

If a Chromium or WebKit browser gate fails to launch with EPERM or a mach-port
denial, rerun that gate through approved unsandboxed execution. If approval or
a browser is unavailable, record the gate as blocked; do not count it as
passed (HS2-DVZXSD).

<!-- hotsheet:end section=sandbox-recovery -->
