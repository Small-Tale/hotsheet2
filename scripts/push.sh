#!/bin/bash
# HS2-CD3D2T: complete the long gate before opening a remote Git connection.
set -euo pipefail
cd "$(dirname "$0")/.."
scripts/check.sh
export HOTSHEET_GATE_PASSED_TREE="$(git rev-parse 'HEAD^{tree}')"
if [[ $# -eq 0 ]]; then
  git push origin main
else
  git push "$@"
fi
