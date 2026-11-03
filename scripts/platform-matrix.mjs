#!/usr/bin/env node
// HS2-31RGRK: the single source of truth for the full-platform workflow's targets.
// `.github/workflows/platforms.yml` runs this in its plan job and expands the two
// matrices it prints. Each target id is a stable job name that HS2-C6P32J keys its
// failure tickets on. Renaming an id orphans those tickets, so add rather than rename.
import { appendFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

// Expanded by bash at run time, so the wrapper works from any working directory.
const linuxTestWrapper = 'dbus-run-session -- bash "$GITHUB_WORKSPACE/.github/scripts/with-secret-service.sh"';

/** The core suite targets, in the order the workflow lists them. */
export const CORE_TARGETS = Object.freeze([
  { target: 'linux-x86_64', os: 'linux', arch: 'x86_64', runner: 'ubuntu-latest', test_wrapper: linuxTestWrapper },
  { target: 'linux-arm64', os: 'linux', arch: 'arm64', runner: 'ubuntu-24.04-arm', test_wrapper: linuxTestWrapper },
  {
    target: 'windows-x86_64',
    os: 'windows',
    arch: 'x86_64',
    runner: 'windows-latest',
    msvc_arch: 'x64',
    vcpkg_triplet: 'x64-windows-static-md',
    vcpkg_host_triplet: 'x64-windows',
    test_wrapper: '',
  },
  {
    target: 'windows-arm64',
    os: 'windows',
    arch: 'arm64',
    runner: 'windows-11-arm',
    msvc_arch: 'arm64',
    vcpkg_triplet: 'arm64-windows-static-md',
    vcpkg_host_triplet: 'arm64-windows',
    test_wrapper: '',
  },
  { target: 'macos-arm64', os: 'macos', arch: 'arm64', runner: 'macos-latest', test_wrapper: '' },
]);

/**
 * The Tauri desktop shell runs as its own target per desktop OS and architecture
 * (maintainer, 2026-11-03: "only done daily"), so a desktop failure stays separate from
 * that OS's core suite. macOS bundles the `.app`; other targets build the shell without an
 * installer until HS2-BYXA4V owns per-architecture installers.
 */
export const TAURI_TARGETS = Object.freeze(
  CORE_TARGETS.map((core) => ({
    ...core,
    target: `tauri-${core.target}`,
    core_target: core.target,
    bundle_args: core.os === 'macos' ? '--bundles app' : '--no-bundle',
  })),
);

const ALL_IDS = [...CORE_TARGETS, ...TAURI_TARGETS].map((entry) => entry.target);

/**
 * Selects targets from a comma or whitespace separated list. `all` (or an empty selection,
 * as on a schedule) selects every target; `core` and `tauri` select a whole group.
 */
export function selectTargets(selection = 'all') {
  const tokens = String(selection ?? '')
    .split(/[\s,]+/)
    .map((token) => token.trim())
    .filter(Boolean);
  const wanted = new Set();
  for (const token of tokens.length === 0 ? ['all'] : tokens) {
    if (token === 'all') ALL_IDS.forEach((id) => wanted.add(id));
    else if (token === 'core') CORE_TARGETS.forEach(({ target }) => wanted.add(target));
    else if (token === 'tauri') TAURI_TARGETS.forEach(({ target }) => wanted.add(target));
    else if (ALL_IDS.includes(token)) wanted.add(token);
    else throw new Error(`Unknown platform target "${token}". Use all, core, tauri, or one of: ${ALL_IDS.join(', ')}`);
  }
  const core = CORE_TARGETS.filter(({ target }) => wanted.has(target));
  const tauri = TAURI_TARGETS.filter(({ target }) => wanted.has(target));
  return { core, tauri };
}

/** The `GITHUB_OUTPUT` lines for the plan job. An empty group is skipped, never an empty matrix. */
export function planOutputs(selection) {
  const { core, tauri } = selectTargets(selection);
  return [
    `core=${JSON.stringify({ include: core })}`,
    `has_core=${core.length > 0}`,
    `tauri=${JSON.stringify({ include: tauri })}`,
    `has_tauri=${tauri.length > 0}`,
  ];
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const args = process.argv.slice(2);
  const index = args.indexOf('--targets');
  const selection = index >= 0 ? (args[index + 1] ?? '') : 'all';
  try {
    const lines = planOutputs(selection);
    const output = process.env.GITHUB_OUTPUT;
    if (output) appendFileSync(output, `${lines.join('\n')}\n`);
    for (const line of lines) console.log(line);
  } catch (error) {
    console.error(`::error::${error.message}`);
    process.exit(1);
  }
}
