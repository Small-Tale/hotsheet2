#!/bin/bash
# HS2-RS0FFT: affected local gate; full regression remains in CI.
set -euo pipefail
cd "$(dirname "$0")/.."
# HS2-KDVHN5: a gate run from a Hot Sheet terminal must not hand its live server route or its
# terminal and actor identity to the tests and binaries it starts.
unset HOTSHEET_SERVER HOTSHEET_SECRET HOTSHEET_TERMINAL_ID HOTSHEET_PROJECT \
  HOTSHEET_PERMISSION_PROJECT HOTSHEET_WORKER_ID HOTSHEET_ACTOR_ROLE HOTSHEET_ACTOR_ID HOTSHEET_AGENT
# HS2-EBZ0CS: nothing this script starts may read the OS keychain or show a keychain prompt.
export HOTSHEET_NATIVE_SECRETS=deny
exec zsh -ic 'node scripts/local-gate.mjs "$@"' hotsheet-local-gate "$@"
