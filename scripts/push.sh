#!/bin/bash
# HS2-BND42N: reserve main before preflight, gate, push, and confirmation.
set -euo pipefail
cd "$(dirname "$0")/.."
# HS2-EBZ0CS: nothing this script starts may read the OS keychain or show a keychain prompt.
export HOTSHEET_NATIVE_SECRETS=deny
exec zsh -ic 'node scripts/main-integration-window.mjs push "$@"' hotsheet-main-window "$@"
