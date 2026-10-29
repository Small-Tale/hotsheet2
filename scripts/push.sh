#!/bin/bash
# HS2-BND42N: reserve main before preflight, gate, push, and confirmation.
set -euo pipefail
cd "$(dirname "$0")/.."
exec zsh -ic 'node scripts/main-integration-window.mjs push "$@"' hotsheet-main-window "$@"
