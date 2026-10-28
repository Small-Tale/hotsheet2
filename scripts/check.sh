#!/bin/bash
# HS2-RS0FFT: affected local gate; full regression remains in CI.
set -euo pipefail
cd "$(dirname "$0")/.."
exec zsh -ic 'node scripts/local-gate.mjs "$@"' hotsheet-local-gate "$@"
