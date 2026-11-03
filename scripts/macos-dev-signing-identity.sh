#!/bin/bash
# HS2-FYXRRM: create a local code-signing certificate and a PKCS#12 bundle that macOS
# `security import` accepts. Usage: macos-dev-signing-identity.sh <empty private directory>
# Writes cert.pem, identity.p12 and passphrase into the directory and prints the SHA-1
# fingerprint. The caller imports the bundle and removes the directory.
#
# macOS cannot import every bundle OpenSSL writes. On macOS 27, `security import` rejects
# an empty PKCS#12 passphrase ("The user name or passphrase you entered is not correct")
# and crashes reading an EC private key, and OpenSSL 3+/4 (often first on PATH through
# Homebrew) defaults to PBES2/AES encryption that it cannot read. So this always uses the
# system LibreSSL, an RSA key, and a random throwaway passphrase.
set -euo pipefail

directory="${1:?usage: macos-dev-signing-identity.sh <directory>}"
openssl=/usr/bin/openssl
umask 077

"$openssl" req -x509 -newkey rsa:2048 -nodes -days 3650 \
  -subj '/CN=Hot Sheet Local Development Code Signing' \
  -addext 'basicConstraints=critical,CA:TRUE' \
  -addext 'keyUsage=digitalSignature' -addext 'extendedKeyUsage=codeSigning' \
  -keyout "$directory/key.pem" -out "$directory/cert.pem" >/dev/null 2>&1
"$openssl" rand -hex 24 >"$directory/passphrase"
"$openssl" pkcs12 -export -inkey "$directory/key.pem" -in "$directory/cert.pem" \
  -out "$directory/identity.p12" -passout "file:$directory/passphrase" >/dev/null 2>&1
rm -f "$directory/key.pem"
fingerprint="$("$openssl" x509 -in "$directory/cert.pem" -noout -fingerprint -sha1 | cut -d= -f2 | tr -d ':')"
if [[ ! "$fingerprint" =~ ^[0-9A-Fa-f]{40}$ ]]; then
  echo 'Could not derive the development certificate fingerprint.' >&2
  exit 1
fi
printf '%s\n' "$fingerprint"
