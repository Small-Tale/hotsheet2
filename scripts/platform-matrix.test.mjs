import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

import { CORE_TARGETS, planOutputs, selectTargets, TAURI_TARGETS } from './platform-matrix.mjs';

const workflow = readFileSync(new URL('../.github/workflows/platforms.yml', import.meta.url), 'utf8');
const gasPreprocessor = readFileSync(new URL('./install-windows-arm64-gas-preprocessor.ps1', import.meta.url), 'utf8');
const awsLcCompiler = readFileSync(new URL('./configure-windows-arm64-aws-lc.ps1', import.meta.url), 'utf8');
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

test('HS2-BCCF8J installs a verified gas-preprocessor for both native Windows arm64 jobs', () => {
  for (const job of [
    workflow.slice(workflow.indexOf('\n  core:'), workflow.indexOf('\n  tauri:')),
    workflow.slice(workflow.indexOf('\n  tauri:')),
  ]) {
    const install = job.indexOf('Install gas-preprocessor for MSVC ARM assembly (Windows arm64)');
    assert.ok(install > job.indexOf('Install the AVIF decoder for the native build (Windows)'));
    assert.ok(install < job.indexOf('Set up Node'));
    assert.match(job.slice(install, job.indexOf('Set up Node')), /matrix\.os == 'windows' && matrix\.arch == 'arm64'/);
    assert.match(job.slice(install, job.indexOf('Set up Node')), /run: \.\/scripts\/install-windows-arm64-gas-preprocessor\.ps1/);
  }
  assert.match(gasPreprocessor, /d09971fad329d32df19f5bbafe88cf2f0ed04ed7/);
  assert.match(gasPreprocessor, /78ad06351ac8be513fb6e6291d9bb8f667899625c2649093e3b08f59fae950a2/);
  assert.match(gasPreprocessor, /Get-FileHash -Algorithm SHA256 -Path \$script/);
  assert.match(gasPreprocessor, /perl -c \$script/);
  assert.match(gasPreprocessor, /\$env:GITHUB_PATH/);
});

test('HS2-XW91TJ selects an ARM64 assembler for aws-lc-sys without changing AVIF CC', () => {
  for (const job of [
    workflow.slice(workflow.indexOf('\n  core:'), workflow.indexOf('\n  tauri:')),
    workflow.slice(workflow.indexOf('\n  tauri:')),
  ]) {
    const avif = job.indexOf('Install the AVIF decoder for the native build (Windows)');
    const configure = job.indexOf('Select clang-cl for aws-lc-sys assembly (Windows arm64)');
    assert.ok(avif >= 0 && configure > avif && configure < job.indexOf('Set up Node'));
    assert.match(job.slice(avif, configure), /CC=cl/);
    assert.match(job.slice(configure, job.indexOf('Set up Node')), /matrix\.os == 'windows' && matrix\.arch == 'arm64'/);
    assert.match(job.slice(configure, job.indexOf('Set up Node')), /run: \.\/scripts\/configure-windows-arm64-aws-lc\.ps1/);
  }
  assert.match(awsLcCompiler, /VC\/Tools\/Llvm\/ARM64\/bin\/clang-cl\.exe/);
  assert.match(awsLcCompiler, /--target=aarch64-pc-windows-msvc \/c \$probe/);
  assert.match(awsLcCompiler, /\$LASTEXITCODE -eq 0/);
  assert.match(awsLcCompiler, /AWS_LC_SYS_CC=\$compiler/);
  assert.doesNotMatch(awsLcCompiler, /NO_ASM|PREBUILT_NASM/);
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

test('HS2-H37V5P formats and lints the desktop shell workspace on every Tauri target', () => {
  const tauri = workflow.slice(workflow.indexOf('\n  tauri:'));
  const at = (step) => {
    const index = tauri.indexOf(step);
    assert.notEqual(index, -1, `tauri target is missing ${step}`);
    return index;
  };
  at('run: cargo fmt --manifest-path clients/web/src-tauri/Cargo.toml --check');
  // Clippy needs the staged sidecars that the prepare step writes.
  assert.ok(at('npm run desktop:prepare') < at('run: cargo lint-desktop'));
  const config = readFileSync(new URL('../.cargo/config.toml', import.meta.url), 'utf8');
  const alias = config.match(/^lint-desktop = "(.*)"$/m)?.[1];
  assert.ok(alias, '.cargo/config.toml defines the lint-desktop alias');
  assert.match(alias, /^clippy --manifest-path clients\/web\/src-tauri\/Cargo\.toml --all-targets --all-features /);
  // Same deny set as the workspace `cargo lint` (Rust lint rule).
  const deny = (line) => line.slice(line.indexOf(' -- '));
  assert.equal(deny(alias), deny(config.match(/^lint = "(.*)"$/m)?.[1] ?? ''));
});

test('HS2-WTETE8 installs Playwright browsers before the web unit tests that launch Chromium', () => {
  const step = (name) => {
    const at = workflow.indexOf(`- name: ${name}`);
    assert.notEqual(at, -1, `missing step: ${name}`);
    return at;
  };
  const browsers = step('Install Playwright Chromium, WebKit, and their dependencies');
  // The binaries project's local-host and stable-dev e2e tests run inside this step.
  assert.ok(browsers < step('Web client lint, typecheck, unit tests, and production build'));
  assert.ok(step('Prefer the Ubuntu archive for Playwright dependencies') < browsers);
  assert.ok(browsers < step('Build isolated browser fixtures'));
});

test('HS2-MM68CT names the MSVC linker after activating MSVC in every Windows job', () => {
  const activate = '- name: Activate the MSVC compiler for native AVIF libraries (Windows)';
  const point = '- name: Point rustc at the MSVC linker (Windows)';
  const count = (needle) => workflow.split(needle).length - 1;
  assert.equal(count(point), count(activate));
  assert.ok(count(point) >= 2, 'both the core and Tauri Windows jobs set the linker');
  for (let at = workflow.indexOf(activate); at !== -1; at = workflow.indexOf(activate, at + 1)) {
    const next = workflow.indexOf('- name:', at + activate.length);
    assert.equal(workflow.indexOf(point, at), next, 'the linker step directly follows MSVC activation');
  }
  // It selects the VC tools link.exe and exports the host target's cargo linker variable.
  assert.match(workflow, /VC\\Tools\\MSVC/);
  assert.match(workflow, /'CARGO_TARGET_' \+ \(\$hostTriple\.ToUpper\(\) -replace '-', '_'\) \+ '_LINKER'/);
  // Native build scripts resolve link.exe from PATH, so Git's coreutils link.exe is moved aside.
  assert.equal(workflow.split("Where-Object { $_.Source -like '*\\Git\\usr\\bin\\*' }").length - 1, count(point));
});

test('HS2-CTC6BA bounds the per-commit Rust caches and frees runner disk before restoring them', () => {
  const ci = readFileSync(new URL('../.github/workflows/ci.yml', import.meta.url), 'utf8');
  const job = (id, next) => ci.slice(ci.indexOf(`\n  ${id}:`), ci.indexOf(`\n  ${next}:`));
  for (const [id, next] of [
    ['check', 'coverage'],
    ['coverage', 'browser-full'],
  ]) {
    const body = job(id, next);
    const at = (needle) => {
      const index = body.indexOf(needle);
      assert.notEqual(index, -1, `${id}: missing ${needle}`);
      return index;
    };
    // An unbounded actions/cache of `target` reached 8 GB compressed and ran the runner out of disk.
    assert.doesNotMatch(body, /actions\/cache@/, `${id}: cache Rust artifacts only through rust-cache`);
    assert.match(body, new RegExp(`uses: Swatinem/rust-cache@v2\\n\\s+with:\\n\\s+shared-key: ${id}\\n`));
    const free = at('- name: Free runner disk\n        run: bash .github/scripts/free-runner-disk.sh');
    assert.ok(free < at('- name: Install the pinned toolchain'), `${id}: free disk before any install`);
    assert.ok(
      at('- name: Install the pinned toolchain') < at('uses: Swatinem/rust-cache@v2'),
      'rust-cache keys on rustc',
    );
    const report = at('- name: Report free disk after the cache restore\n        run: df -h /');
    assert.ok(at('uses: Swatinem/rust-cache@v2') < report);
    assert.ok(report < at('- name: Install cargo-'), `${id}: report free disk before the build`);
  }
  // Browser jobs keep their own small cache and never fall back to another job's key.
  assert.doesNotMatch(ci, /^\s+\$\{\{ runner\.os \}\}-cargo-$/m);
  assert.equal(ci.match(/restore-keys: \$\{\{ runner\.os \}\}-cargo-browser-\n/g)?.length, 2);

  const script = readFileSync(new URL('../.github/scripts/free-runner-disk.sh', import.meta.url), 'utf8');
  assert.equal(script.match(/^df -h \/$/gm)?.length, 2, 'free space is logged before and after cleanup');
  for (const path of ['/usr/share/dotnet', '/usr/local/lib/android', '/opt/ghc', '/opt/hostedtoolcache/CodeQL'])
    assert.ok(script.includes(`  ${path}\n`), `removes ${path}`);
  assert.match(script, /if \[ -e "\$path" \]; then/, 'each removal is guarded');
});
