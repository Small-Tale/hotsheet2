#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
git config --local core.hooksPath .githooks
echo 'Hot Sheet Git hooks installed for this checkout.'
