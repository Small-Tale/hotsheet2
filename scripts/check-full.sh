#!/bin/bash
# HS2-RS0FFT: complete package-stage catalog for the affected gate.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo fmt --all --check
cargo lint
cargo nextest run --all-features --status-level leak
zsh -ic 'node --test scripts/instruction-policy.test.mjs scripts/sync-ai-playbook.test.mjs scripts/check-test-coverage.test.mjs scripts/check-test-quarantine.test.mjs scripts/test-quiet.test.mjs scripts/test-profile.test.mjs scripts/gate-timings.test.mjs scripts/playwright-retries.test.mjs scripts/ci-gate.test.mjs scripts/ci-completion-proposal.test.mjs scripts/integration-record.test.mjs scripts/gate-plan.test.mjs scripts/gate-budget.test.mjs scripts/gate-ticket-profile.test.mjs scripts/local-gate.test.mjs scripts/git-hooks.test.mjs scripts/windows-crt-directives.test.mjs scripts/windows-crt-link-correlation.test.mjs scripts/throughput-baseline.test.mjs scripts/gitgist-tooling.test.mjs scripts/check-rulesync-generation.test.mjs'
zsh -ic 'node scripts/check-test-coverage.mjs'
zsh -ic 'node scripts/check-test-quarantine.mjs'
zsh -ic 'node scripts/sync-role-skills.mjs --check'
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
