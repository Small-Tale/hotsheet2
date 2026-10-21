#!/usr/bin/env bash
# Give headless Linux CI a disposable Secret Service for real permission-vault tests.
set -euo pipefail

printf 'ci-ephemeral-keyring\n' | gnome-keyring-daemon --unlock --components=secrets >/dev/null
for attempt in 1 2 3; do
  if printf 'ready' | timeout 10s secret-tool store --label 'HS2 CI keyring probe' service hs2-ci account probe; then
    if [[ "$(timeout 10s secret-tool lookup service hs2-ci account probe)" == 'ready' ]]; then
      exec "$@"
    fi
  fi
  sleep 1
done
echo 'Headless CI Secret Service did not become available' >&2
exit 1
