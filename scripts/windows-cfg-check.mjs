#!/usr/bin/env node
// HS2-V3R4JR: type-check and lint the Windows `cfg` branches from a macOS or Linux host.
//
// Test code gated `#[cfg(unix)]` twice left imports unused on Windows (HS2-QNZ60H, HS2-AZ6C66).
// The Windows build denies warnings, but the local gate and per-commit CI compile only their own
// host, so only the daily Platforms workflow saw it. This runs the `cargo lint` clippy pass for
// `x86_64-pc-windows-gnu` instead. `cargo clippy` never links, so the C dependencies' build
// scripts (ring, aws-lc-sys, libsqlite3-sys, libwebp-sys2, rav1e, dav1d, libavif) only need to
// succeed, not to produce real objects: stub compiler, archiver, nasm and cmake commands write
// empty outputs, the bundled dav1d build is overridden through its `links = "dav1d"` key, and the
// system dav1d probe is answered from the environment. Nothing built here can run or link.
//
//   node scripts/windows-cfg-check.mjs
//
// A Windows host skips: its own `cargo lint` already compiles these branches.
import { spawnSync } from 'node:child_process';
import { chmodSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { delimiter, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const TARGET = 'x86_64-pc-windows-gnu';
/** Its own target directory: the stubbed build-script outputs must never mix with real ones. */
export const TARGET_DIR = 'target/windows-cfg-check';
/** The deny set of the `cargo lint` alias in .cargo/config.toml (pinned by the unit test). */
export const LINT_FLAGS = [
  '-D',
  'warnings',
  '-D',
  'clippy::dbg_macro',
  '-D',
  'clippy::todo',
  '-D',
  'clippy::unimplemented',
];

const root = resolve(fileURLToPath(new URL('..', import.meta.url)));

/** POSIX shell stubs. `tools/` is named through environment variables; only `path/` joins PATH. */
export const STUBS = {
  // cc-rs: `--version` probes, `-E` detects the family (the gcc pragma), `-o`/`-Fo` name the object.
  'tools/cc': `#!/bin/sh
# HS2-V3R4JR stub C compiler: clippy never links, so an empty object satisfies the build script.
out=""; prev=""; pre=0
for a in "$@"; do
  case "$a" in --version|-v|-dumpversion) echo "stub 13.0.0"; exit 0;; -E) pre=1;; esac
  [ "$prev" = "-o" ] && out="$a"
  case "$a" in -o?*) out="\${a#-o}";; -Fo?*) out="\${a#-Fo}";; esac
  prev="$a"
done
if [ "$pre" = 1 ]; then echo '#pragma message "gcc"'; exit 0; fi
if [ -n "$out" ]; then : > "$out"; fi
exit 0
`,
  // `ar cq <archive> <objects…>` and `ar s <archive>`: the archive is the second argument.
  'tools/ar': `#!/bin/sh
# HS2-V3R4JR stub archiver.
if [ $# -ge 2 ]; then : > "$2"; fi
exit 0
`,
  // The cmake crate configures and builds libavif; neither step needs to produce anything.
  'tools/cmake': `#!/bin/sh
# HS2-V3R4JR stub cmake.
if [ "$1" = "--version" ]; then echo "cmake version 3.31.0"; fi
exit 0
`,
  // nasm-rs finds \`nasm\` on PATH and requires at least 2.15 (rav1e's x86_64 assembly).
  'path/nasm': `#!/bin/sh
# HS2-V3R4JR stub nasm.
out=""; prev=""
for a in "$@"; do
  case "$a" in -v|--version) echo "NASM version 2.16.03"; exit 0;; esac
  [ "$prev" = "-o" ] && out="$a"
  prev="$a"
done
if [ -n "$out" ]; then : > "$out"; fi
exit 0
`,
};

/** Write the stubs under `dir`, rewriting only a changed file so its mtime stays stable. */
export function writeStubs(dir) {
  for (const [name, text] of Object.entries(STUBS)) {
    const file = join(dir, name);
    mkdirSync(join(file, '..'), { recursive: true });
    if (!existsSync(file) || readFileSync(file, 'utf8') !== text) writeFileSync(file, text);
    chmodSync(file, 0o755);
  }
}

/** The environment of the stubbed clippy run, derived from `base` without changing it. */
export function checkEnv(base, stubs) {
  const env = { ...base };
  const triple = TARGET.replaceAll('-', '_');
  env[`CC_${triple}`] = join(stubs, 'tools/cc');
  env[`CXX_${triple}`] = join(stubs, 'tools/cc');
  env[`AR_${triple}`] = join(stubs, 'tools/ar');
  env.CMAKE = join(stubs, 'tools/cmake');
  // dav1d-sys (image's AVIF decoder) probes pkg-config through system-deps.
  env.SYSTEM_DEPS_DAV1D_NO_PKG_CONFIG = '1';
  env.SYSTEM_DEPS_DAV1D_LIB = 'dav1d';
  // cc-rs reuses an sccache RUSTC_WRAPPER for C, and sccache refuses an unknown compiler.
  delete env.RUSTC_WRAPPER;
  delete env.CARGO_BUILD_RUSTC_WRAPPER;
  env.PATH = [join(stubs, 'path'), base.PATH].filter(Boolean).join(delimiter);
  return env;
}

/** The cargo arguments: `cargo lint` for the Windows target, plus the bundled-dav1d override. */
export function cargoArgs(targetDir, stubs) {
  return [
    'clippy',
    '--workspace',
    '--all-targets',
    '--all-features',
    '--target',
    TARGET,
    '--target-dir',
    targetDir,
    // libdav1d-sys has `links = "dav1d"`, so this replaces its meson build; libavif-sys reads the
    // include path from it.
    '--config',
    `target.${TARGET}.dav1d.include=${JSON.stringify(join(stubs, 'include'))}`,
    '--',
    ...LINT_FLAGS,
  ];
}

const FAILURE_HINT =
  'HS2-V3R4JR: the Windows cfg check failed. A Rust error above is a real Windows build failure ' +
  '(for example an import or helper used only under #[cfg(unix)]); gate it the same way. ' +
  "If a dependency's build script failed instead, it needs another stub in scripts/windows-cfg-check.mjs.";

function defaultRun(command, args, options = {}) {
  return spawnSync(command, args, { stdio: 'inherit', ...options });
}

export function main(
  argv = process.argv.slice(2),
  { repo = root, platform = process.platform, env = process.env, run = defaultRun, log = console.log } = {},
) {
  if (argv.length) {
    log('usage: node scripts/windows-cfg-check.mjs');
    return 2;
  }
  if (platform === 'win32') {
    log('HS2-V3R4JR: Windows host; `cargo lint` already compiles the Windows cfg branches. Skipped.');
    return 0;
  }
  const installed = run('rustup', ['target', 'list', '--installed'], {
    cwd: repo,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'inherit'],
  });
  if (installed.status !== 0) {
    log('HS2-V3R4JR: `rustup target list --installed` failed; rustup is required.');
    return installed.status || 1;
  }
  if (!String(installed.stdout).split('\n').includes(TARGET)) {
    log(`HS2-V3R4JR: adding the ${TARGET} standard library (once per toolchain).`);
    const added = run('rustup', ['target', 'add', TARGET], { cwd: repo });
    if (added.status !== 0) return added.status || 1;
  }
  const stubs = join(repo, TARGET_DIR, 'stubs');
  writeStubs(stubs);
  const result = run('cargo', cargoArgs(TARGET_DIR, stubs), { cwd: repo, env: checkEnv(env, stubs) });
  const status = result.status ?? 1;
  if (status !== 0) log(FAILURE_HINT);
  return status;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) process.exitCode = main();
