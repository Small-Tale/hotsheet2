import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

import { CORE_TARGETS, planOutputs, selectTargets, TAURI_TARGETS } from './platform-matrix.mjs';

const workflow = readFileSync(new URL('../.github/workflows/platforms.yml', import.meta.url), 'utf8');
const ids = (entries) => entries.map(({ target }) => target);

test('HS2-31RGRK covers the maintainer platform set with stable target ids and runners', () => {
  assert.deepEqual(
    CORE_TARGETS.map(({ target, runner }) => [target, runner]),
    [
      ['linux-x86_64', 'ubuntu-latest'],
      ['linux-arm64', 'ubuntu-24.04-arm'],
      ['windows-x86_64', 'windows-latest'],
      ['windows-arm64', 'windows-11-arm'],
      ['macos-arm64', 'macos-latest'],
    ],
  );
  assert.deepEqual(
    ids(TAURI_TARGETS),
    ids(CORE_TARGETS).map((id) => `tauri-${id}`),
  );
  for (const entry of CORE_TARGETS.filter(({ os }) => os === 'windows'))
    assert.equal(entry.vcpkg_triplet, `${entry.arch === 'arm64' ? 'arm64' : 'x64'}-windows-static-md`);
  for (const entry of TAURI_TARGETS)
    assert.equal(entry.bundle_args, entry.os === 'macos' ? '--bundles app' : '--no-bundle');
});

test('HS2-31RGRK selects all, groups, and single targets, and rejects unknown ids', () => {
  const all = selectTargets('all');
  assert.equal(all.core.length, 5);
  assert.equal(all.tauri.length, 5);
  assert.deepEqual(selectTargets(''), all, 'a scheduled run has no input and selects everything');
  assert.deepEqual(selectTargets(undefined), all);
  assert.deepEqual(selectTargets('core').tauri, []);
  assert.deepEqual(ids(selectTargets('tauri').tauri), ids(TAURI_TARGETS));
  assert.deepEqual(selectTargets('tauri').core, []);
  const some = selectTargets(' windows-arm64, tauri-macos-arm64 windows-arm64 ');
  assert.deepEqual(ids(some.core), ['windows-arm64']);
  assert.deepEqual(ids(some.tauri), ['tauri-macos-arm64']);
  assert.throws(() => selectTargets('linux-x86_64,solaris'), /Unknown platform target "solaris"/);
});

test('HS2-31RGRK never emits an empty matrix', () => {
  const lines = planOutputs('linux-arm64');
  assert.ok(lines.includes('has_core=true'));
  assert.ok(lines.includes('has_tauri=false'));
  assert.ok(lines.includes(`tauri=${JSON.stringify({ include: [] })}`));
  assert.match(workflow, /if: needs\.plan\.outputs\.has_core == 'true'/);
  assert.match(workflow, /if: needs\.plan\.outputs\.has_tauri == 'true'/);
});

test('HS2-31RGRK keeps the platform workflow out of the per-commit CI gate', () => {
  assert.match(workflow, /^name: Platforms$/m, 'scripts/ci-gate.mjs counts only the CI workflow');
  const triggers = workflow.slice(workflow.indexOf('\non:'), workflow.indexOf('\npermissions:'));
  assert.match(triggers, /\n {2}schedule:/);
  assert.match(triggers, /\n {2}workflow_dispatch:/);
  assert.match(triggers, /\n {2}workflow_call:/);
  assert.doesNotMatch(triggers, /\n {2}(push|pull_request):/);
});

test('HS2-31RGRK reports every target separately without masking failures', () => {
  assert.equal(workflow.match(/fail-fast: false/g)?.length, 2);
  assert.equal(workflow.match(/name: \$\{\{ matrix\.target \}\}/g)?.length, 2, 'stable job names for HS2-C6P32J');
  assert.doesNotMatch(workflow, /^\s*continue-on-error:/m, 'a known-failing target reports red');
  assert.match(workflow, /node scripts\/platform-matrix\.mjs --targets "\$TARGETS"/);
});

test('HS2-31RGRK runs the full suite on every core target', () => {
  const core = workflow.slice(workflow.indexOf('\n  core:'), workflow.indexOf('\n  tauri:'));
  for (const step of [
    'cargo fmt --all --check',
    'run: cargo lint',
    'cargo nextest run --all-features',
    'npm run lint',
    'npm run typecheck',
    'npm run test:unit',
    'npm run build',
    'npm run test:coverage',
    'npm run test:fixtures',
    'npx playwright test --fully-parallel',
    'name: platform-reports-${{ matrix.target }}',
  ])
    assert.ok(core.includes(step), `core target is missing ${step}`);
  assert.match(core, /arch: \$\{\{ matrix\.msvc_arch \}\}/);
  assert.match(core, /dav1d:\$env:TRIPLET/);
});

test('HS2-31RGRK keeps the Windows CRT diagnostics after the native build', () => {
  const core = workflow.slice(workflow.indexOf('\n  core:'), workflow.indexOf('\n  tauri:'));
  const order = [
    'cargo nextest run --all-features',
    'node scripts/windows-crt-directives.mjs --target target --output target/crt-directives/report.json',
    'node scripts/windows-crt-link-correlation.mjs --target target --directives target/crt-directives/report.json --output target/crt-directives/link-correlation.json',
    'name: windows-crt-link-correlation-${{ matrix.target }}',
  ].map((step) => core.indexOf(step));
  assert.ok(
    order.every((at) => at > 0),
    'both reports and their upload stay in the Windows targets',
  );
  assert.deepEqual(
    [...order].sort((a, b) => a - b),
    order,
  );
  assert.doesNotMatch(core, /NODEFAULTLIB|crt-static|\/MDd?\b/i);
});

test('HS2-WSE3FJ opts the Windows targets into the ignored native Credential Manager round trip', () => {
  const core = workflow.slice(workflow.indexOf('\n  core:'), workflow.indexOf('\n  tauri:'));
  const step = core.slice(core.indexOf('Exercise the native Credential Manager round trip (Windows)'));
  assert.match(step, /matrix\.os == 'windows'/);
  assert.match(step, /run: cargo test -p hotsheet-ticketing windows_credential_manager_round_trip -- --ignored\n/);
  assert.ok(
    core.indexOf('windows_credential_manager_round_trip') < core.indexOf('node scripts/windows-crt-directives.mjs'),
    'the CRT diagnostic follows every native build',
  );
  const ci = readFileSync(new URL('../.github/workflows/ci.yml', import.meta.url), 'utf8');
  assert.doesNotMatch(ci, /runs-on: (windows|macos)|ubuntu-24\.04-arm/, 'per-commit CI runs Linux x86_64 only');
});

test('HS2-31RGRK builds and tests the Tauri desktop shell as its own targets', () => {
  const tauri = workflow.slice(workflow.indexOf('\n  tauri:'));
  assert.match(tauri, /npm run desktop:prepare/);
  assert.match(tauri, /cargo nextest run --manifest-path clients\/web\/src-tauri\/Cargo\.toml/);
  assert.match(tauri, /npx tauri build --ci \$\{\{ matrix\.bundle_args \}\}/);
  assert.match(tauri, /libwebkit2gtk-4\.1-dev/);
  assert.doesNotMatch(tauri, /APPLE_SIGNING_IDENTITY|TAURI_SIGNING_PRIVATE_KEY/, 'the daily build is unsigned');
});
