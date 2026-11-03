#!/bin/bash
# HS2-FYXRRM: install a local-only code identity for stable Keychain access across rebuilds.
set -euo pipefail

if [[ "$(uname -s)" != Darwin ]]; then
  echo 'This setup is for macOS only.' >&2
  exit 1
fi
for command in /usr/bin/openssl security codesign mktemp; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "Missing required command: $command" >&2
    exit 1
  fi
done

umask 077
machine_home="${HOTSHEET_HOME:-$HOME/.hotsheet2}"
identity_file="$machine_home/dev-signing-identity"
if [[ -f "$identity_file" ]]; then
  identity="$(cat "$identity_file")"
  if [[ ! "$identity" =~ ^[0-9A-Fa-f]{40}$ ]]; then
    echo 'Existing development signing identity file is invalid.' >&2
    exit 1
  fi
  if security find-identity -v -p codesigning | grep -qi "$identity"; then
    echo 'Hot Sheet development signing identity is already configured.'
    exit 0
  fi
  echo 'Configured development signing identity is unavailable; restore it before rebuilding.' >&2
  exit 1
fi

temporary="$(mktemp -d "${TMPDIR:-/tmp}/hotsheet-dev-signing.XXXXXX")"
trap 'rm -rf "$temporary"' EXIT
identity="$(bash "$(dirname "$0")/macos-dev-signing-identity.sh" "$temporary")"

# The imported private key cannot be exported; only /usr/bin/codesign may use it.
# The throwaway passphrase protects only the temporary bundle removed on exit.
security import "$temporary/identity.p12" -f pkcs12 -P "$(cat "$temporary/passphrase")" -x \
  -T /usr/bin/codesign >/dev/null
# Trust is restricted to code signing in this user's trust store, not SSL or system trust.
security add-trusted-cert -r trustRoot -p codeSign "$temporary/cert.pem" >/dev/null
if ! security find-identity -v -p codesigning | grep -qi "$identity"; then
  echo 'The new identity is not available to codesign.' >&2
  exit 1
fi

cp /usr/bin/true "$temporary/signing-probe"
codesign --force --sign "$identity" --identifier com.smalltale.hotsheet2.dev.probe \
  "$temporary/signing-probe" >/dev/null 2>&1
codesign --verify --strict "$temporary/signing-probe"

mkdir -p "$machine_home"
printf '%s\n' "$identity" >"$temporary/identity-config"
mv "$temporary/identity-config" "$identity_file"
echo 'Hot Sheet development signing is configured. Relaunch the local host to use it.'
