#!/bin/bash
# HS2-CD3D2T: local root gate for verified pushes. CI runs platform/browser shards separately.
set -euo pipefail
cd "$(dirname "$0")/.."

tree="$(git rev-parse 'HEAD^{tree}')"
cache_dir="$(git rev-parse --git-path hotsheet-root-gate)"
clean_tree() {
  git diff --quiet && git diff --cached --quiet &&
    [[ -z "$(git ls-files --others --exclude-standard)" ]]
}
if clean_tree && [[ -f "$cache_dir/$tree" ]]; then
  echo "Root gate already passed for tree $tree"
  exit 0
fi

if ! command -v git-filter-repo >/dev/null 2>&1; then
  echo 'git-filter-repo is required for the Rust compaction tests (CI pins 2.47.0).' >&2
  exit 1
fi
if ! command -v cargo-nextest >/dev/null 2>&1; then
  echo 'cargo-nextest is required for the root gate.' >&2
  exit 1
fi

cargo fmt --all --check
cargo lint
cargo nextest run --all-features --status-level fail
zsh -ic 'node --test scripts/instruction-policy.test.mjs scripts/sync-ai-playbook.test.mjs scripts/check-test-coverage.test.mjs scripts/check-test-quarantine.test.mjs scripts/test-quiet.test.mjs scripts/test-profile.test.mjs scripts/playwright-retries.test.mjs scripts/ci-gate.test.mjs scripts/git-hooks.test.mjs'
zsh -ic 'node scripts/check-test-coverage.mjs'
zsh -ic 'node scripts/check-test-quarantine.mjs'
(
  cd clients/web
  zsh -ic 'npm run lint && npm run typecheck && npm run test:unit && npm run build'
)
(
  cd migrator
  zsh -ic 'npm run lint && npm run test:coverage'
)
(
  cd spikes/kerf-webawesome
  zsh -ic 'npm run lint && npm run typecheck && npm run build'
)

if clean_tree; then
  mkdir -p "$cache_dir"
  printf '%s\n' "$tree" >"$cache_dir/$tree"
fi
echo "Root gate passed for tree $tree"
