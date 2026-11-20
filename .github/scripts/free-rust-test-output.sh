#!/usr/bin/env bash
# Reclaim the Rust test build output a Linux Platforms job no longer needs (HS2-P9S2VT).
#
# The single Platforms Linux job builds clippy metadata and every workspace test executable,
# then runs the web, migrator, fixture and browser suites on the same runner disk. Run
# 44690376755 ran out of space there (ENOSPC, crashed Chromium renderers). The web steps
# rebuild only the server and CLI binaries, which reuse the dependency artifacts this keeps
# (`target/debug/deps/*.rlib`, `target/debug/build`). The nextest JUnit report under
# `target/nextest` stays for the profile and upload steps. Free space is printed before and
# after so the next exhaustion is attributable.
set -uo pipefail

echo "Free disk before reclaiming Rust test output:"
df -h /
du -sh target 2>/dev/null || true

# Clippy's separate target directory (`cargo lint`) and the incremental caches of the
# workspace crates. Neither is read by a later step.
for path in target/clippy target/debug/incremental; do
  if [ -e "$path" ]; then
    echo "Removing $path"
    rm -rf "$path"
  else
    echo "Skipping $path (absent)"
  fi
done

# Test executables are the extensionless files in deps (`<crate>-<hash>`); libraries keep
# their `.rlib`, `.rmeta`, `.so` or `.d` suffix and stay for the binary rebuild.
if [ -d target/debug/deps ]; then
  count=$(find target/debug/deps -maxdepth 1 -type f ! -name '*.*' | wc -l)
  echo "Removing $count test executables from target/debug/deps"
  find target/debug/deps -maxdepth 1 -type f ! -name '*.*' -delete
fi

echo "Free disk after reclaiming Rust test output:"
df -h /
du -sh target 2>/dev/null || true
