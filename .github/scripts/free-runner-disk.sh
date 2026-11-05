#!/usr/bin/env bash
# Free preinstalled runner software the Rust CI jobs never use (HS2-CTC6BA).
#
# The ubuntu-24.04 image 20261101 left too little free space for the Rust jobs' cargo + target
# cache to extract, and the runner died with "No space left on device" before any test ran.
# Each removal is guarded and logged; free space is printed before and after so the next
# disk failure is diagnosable from the job log.
set -uo pipefail

echo "Free disk before cleanup:"
df -h /

# Toolchains and SDKs no job in ci.yml uses. The removals run in parallel; each is independent.
paths=(
  /usr/share/dotnet
  /usr/local/lib/android
  /opt/ghc
  /usr/local/.ghcup
  /opt/hostedtoolcache/CodeQL
  /usr/share/swift
  /usr/local/share/powershell
)
for path in "${paths[@]}"; do
  if [ -e "$path" ]; then
    echo "Removing $path"
    sudo rm -rf "$path" &
  else
    echo "Skipping $path (absent on this image)"
  fi
done
wait

echo "Free disk after cleanup:"
df -h /
