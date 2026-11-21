#!/usr/bin/env node
// HS2-FCCRCK, HS2-RS0FFT: dependency plan for the local gate. Read-only by default: it reads the stages of
// scripts/check-full.sh, maps the changed paths since the last gated base to the stages they can
// affect, and lists every stage as selected or omitted with its reason. Anything it cannot classify
// selects the full gate. It never changes, replaces or caches the required gate.
//
//   node scripts/gate-plan.mjs [--base <ref>] [--json]       # plan only
//   node scripts/gate-plan.mjs [--base <ref>] --execute      # shadow-run only the selected stages
//
// The default base is the merge base of HEAD and origin/main, the last tree the full gate guarded.
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { withoutGitRepositoryEnv } from '../clients/web/scripts/repository-env.mjs';
import { fixtureReaders, readManifest } from './root-test-reads.mjs';

const root = resolve(fileURLToPath(new URL('..', import.meta.url)));

/** Stage groups, keyed by `<cwd>::<command>` exactly as scripts/check.sh runs them. */
export const STAGE_GROUPS = {
  '.::cargo fmt --all --check': 'always',
  '.::cargo lint': 'rust',
  '.::cargo nextest run --all-features --status-level leak': 'rust',
  '.::node --test scripts/verify-playwright-shards.test.mjs': 'tooling',
  '.::node scripts/check-test-coverage.mjs': 'always',
  '.::node scripts/check-test-quarantine.mjs': 'always',
  '.::node scripts/sync-role-skills.mjs --check': 'always',
  'clients/web::npm run format:sources:check': 'always',
  'clients/web::npm run lint:web': 'web_lint',
  'clients/web::npm run typecheck': 'web',
  'clients/web::npm run test:unit': 'web',
  'clients/web::npm run build': 'web',
  'migrator::npm run lint': 'migrator',
  'migrator::npm run test:coverage': 'migrator',
  'spikes/kerf-webawesome::npm run lint': 'spike',
  'spikes/kerf-webawesome::npm run typecheck': 'spike',
  'spikes/kerf-webawesome::npm run build': 'spike',
};
// HS2-3ZJBRM: exact lists keep a new or moved root test unclassified until its
// read inputs are reviewed. Policy tests read other packages or generated guidance and always run.
// Tooling tests read scripts/ and full-fallback inputs, plus the repository files that
// scripts/root-test-fixtures.json records for them (HS2-WC74CR).
export const ROOT_POLICY_TESTS = [
  'instruction-policy',
  'sync-ai-playbook',
  'check-test-coverage',
  'check-test-quarantine',
  'gitgist-tooling',
  'check-rulesync-generation',
  'repository-env-isolation',
  'windows-line-endings',
];
export const ROOT_TOOLING_TESTS = [
  'test-quiet',
  'test-profile',
  'gate-timings',
  'gate-review',
  'playwright-retries',
  'ci-gate',
  'ci-completion-proposal',
  'verified-worktree-cleanup',
  'ci-failure-parse',
  'ci-failure-attribution',
  'integration-record',
  'gate-plan',
  'gate-edge-plan',
  'gate-budget',
  'gate-test-plan',
  'gate-case-rerun',
  'gate-ticket-profile',
  'local-gate',
  'installed-dependencies',
  'git-hooks',
  'windows-crt-directives',
  'windows-crt-link-correlation',
  'throughput-baseline',
  'main-integration-options',
  'main-integration-authority',
  'main-integration-window',
  'integration-batch',
  'macos-dev-signing',
  'platform-matrix',
  'browser-engines',
  'root-test-reads',
];
/** HS2-WC74CR: the tooling stage records what its tests read and fails on an undeclared fixture. */
export const ROOT_TEST_READS_PRELOAD = './scripts/root-test-reads-preload.mjs';
const rootTestCommand = (names, preload) =>
  `node --test ${preload ? `--import ${preload} ` : ''}${names.map((name) => `scripts/${name}.test.mjs`).join(' ')}`;
/** Repository files and directories outside scripts/ that root tooling tests read (HS2-WC74CR). */
export const ROOT_TEST_FIXTURES = readManifest(root);

/** Why each group can never be omitted: these checks read the whole repository. */
const ALWAYS_REASON =
  'always runs: repository-wide formatting, cross-package policy, coverage-matrix and quarantine checks read every area';

/**
 * Path rules, first match wins. `full` forces the whole gate. Edges come from source (HS2-FCCRCK):
 * Rust embeds or reads plugins/, the .claude and .agents skills, migrator/src, clients/web/public and
 * the CI workflow; web tests spawn the Rust binaries and read docs, crates, migrator and the spike;
 * migrator tests spawn the Rust binaries.
 */
export const PATH_RULES = [
  [/^scripts\//, 'full', 'root scripts include the gate, its tooling and shared test helpers'],
  [/^\.githooks\//, 'full', 'git hooks run the gate'],
  [/^\.github\//, 'full', 'CI configuration; Rust tests also read the workflow'],
  [/(^|\/)package(-lock)?\.json$/, 'full', 'package manifest or lockfile'],
  [
    /^(Cargo\.(toml|lock)|rust-toolchain\.toml|clippy\.toml|rustfmt\.toml|\.cargo\/)/,
    'full',
    'Rust workspace, lockfile or toolchain',
  ],
  [/^(AGENTS\.md|CLAUDE\.md|\.rulesync\/|guidance\/)/, 'full', 'shared guidance and its generators'],
  [/^clients\/web\/src-tauri\//, 'full', 'desktop shell outside the Cargo workspace; not classified'],
  [/^clients\/web\/scripts\/format-sources\.mjs$/, 'full', 'shared formatter and gate input'],
  [/^crates\//, ['rust', 'web', 'migrator'], 'Rust source; web and migrator tests spawn the built binaries'],
  [/^(plugins|\.claude|\.agents)\//, ['rust'], 'embedded in or read by the Rust build and tests'],
  [/^clients\/web\/public\//, ['rust', 'web', 'web_lint'], 'web assets; Rust tests read clients/web/public'],
  [/^clients\/web\//, ['web', 'web_lint'], 'web client source, tests or configuration'],
  [/^migrator\//, ['migrator', 'rust', 'web'], 'migrator; the Rust CLI runs migrator/src and web tests read it'],
  [/^spikes\/kerf-webawesome\//, ['spike', 'web'], 'Kerf/Web Awesome spike; web tests read it'],
  [/^docs\/ux-components\.md$/, ['web', 'web_lint'], 'web component catalog links this documentation'],
  [/^docs\//, ['web'], 'documentation; web tests read docs/contracts and other docs'],
];

export function classifyPath(path, fixtures = ROOT_TEST_FIXTURES) {
  const rule = PATH_RULES.find(([pattern]) => pattern.test(path));
  const match = rule ? { groups: rule[1], reason: rule[2] } : { groups: 'full', reason: 'unclassified path' };
  // HS2-WC74CR: a root tooling test reads this file as a fixture, so a change to it reaches that test.
  const readers = fixtureReaders(fixtures, path);
  if (!readers.length || match.groups === 'full' || match.groups.includes('tooling')) return match;
  return {
    groups: [...match.groups, 'tooling'],
    reason: `${match.reason}; ${readers.map((name) => `scripts/${name}.test.mjs`).join(', ')} read it (scripts/root-test-fixtures.json)`,
    readers,
  };
}

/** The tooling stage narrowed to the test files that read a changed fixture, in gate order (HS2-WC74CR). */
export function narrowedToolingCommand(readers) {
  const names = ROOT_TOOLING_TESTS.filter((name) => readers.has(name));
  return names.length ? rootTestCommand(names, ROOT_TEST_READS_PRELOAD) : null;
}

/** The gate stages, read from scripts/check.sh so the plan can never drift from the real gate. */
export function parseCheckScript(text) {
  const stages = [];
  const lines = text.split('\n');
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index].trim();
    if (line === '(') {
      const cd = /^cd (\S+)$/.exec(lines[index + 1]?.trim() ?? '');
      const run = /^zsh -ic '(.+)'$/.exec(lines[index + 2]?.trim() ?? '');
      if (!cd || !run || lines[index + 3]?.trim() !== ')')
        throw new Error(`Unrecognized gate subshell at line ${index + 1}`);
      for (const command of run[1].split(' && ')) stages.push({ cwd: cd[1], command });
      index += 3;
    } else if (/^cargo /.test(line)) stages.push({ cwd: '.', command: line });
    else if (/^zsh -ic '/.test(line)) {
      const run = /^zsh -ic '(.+)'$/.exec(line);
      if (!run) throw new Error(`Unrecognized gate command at line ${index + 1}`);
      stages.push({ cwd: '.', command: run[1] });
    } else if (line && !line.startsWith('#') && line !== 'set -euo pipefail' && line !== 'cd "$(dirname "$0")/.."')
      throw new Error(`Unrecognized gate statement at line ${index + 1}: ${line}`);
  }
  return stages;
}

export function stageGroup(stage) {
  if (stage.cwd === '.' && stage.command === rootTestCommand(ROOT_POLICY_TESTS)) return 'always';
  if (stage.cwd === '.' && stage.command === rootTestCommand(ROOT_TOOLING_TESTS, ROOT_TEST_READS_PRELOAD))
    return 'tooling';
  return STAGE_GROUPS[`${stage.cwd}::${stage.command}`];
}

/** Build the plan from the gate's stages and the changed paths. Pure: no Git or process access. */
export function plan({ stages, changes, fallback = [], fixtures = ROOT_TEST_FIXTURES }) {
  const reasons = new Map();
  const add = (group, reason) => reasons.set(group, [...(reasons.get(group) ?? []), reason]);
  const full = [...fallback];
  // Root tooling tests that read a changed fixture; null once a change reaches the whole tooling group.
  let readers = new Set();
  if (changes.length === 0) full.push('no changes since the base; nothing narrower can be justified');
  for (const path of changes) {
    const classified = classifyPath(path, fixtures);
    const { groups, reason } = classified;
    if (groups === 'full') full.push(`${path}: ${reason}`);
    else for (const group of groups) add(group, `${path}: ${reason}`);
    if (groups !== 'full' && groups.includes('tooling'))
      if (classified.readers && readers) for (const name of classified.readers) readers.add(name);
      else readers = null;
  }
  const unknown = stages.filter((stage) => stageGroup(stage) === undefined);
  for (const stage of unknown) full.push(`unclassified gate stage: ${stage.cwd} ${stage.command}`);
  const mode = full.length ? 'full' : 'selected';
  return {
    mode,
    fallback: full,
    stages: stages.map((stage) => {
      const group = stageGroup(stage);
      if (mode === 'full') return { ...stage, group, selected: true, reasons: ['full gate'] };
      if (group === 'always') return { ...stage, group, selected: true, reasons: [ALWAYS_REASON] };
      const why = reasons.get(group);
      if (!why) return { ...stage, group, selected: false, reasons: [`no changed path reaches the ${group} group`] };
      // A fixture change runs only the root tests that read it, not every tooling test (HS2-WC74CR).
      const narrowed =
        group === 'tooling' && readers && stage.command === rootTestCommand(ROOT_TOOLING_TESTS, ROOT_TEST_READS_PRELOAD)
          ? narrowedToolingCommand(readers)
          : null;
      return narrowed
        ? { ...stage, command: narrowed, group, selected: true, reasons: why }
        : { ...stage, group, selected: true, reasons: why };
    }),
  };
}

function git(args, cwd) {
  const result = spawnSync('git', args, { cwd, env: withoutGitRepositoryEnv(process.env), encoding: 'utf8' });
  return { ok: result.status === 0, out: (result.stdout ?? '').trim() };
}

/**
 * Changed paths between the base and the working tree: committed, staged, unstaged, deleted and
 * untracked. Renames list both sides. Any Git failure becomes a full-gate fallback reason.
 */
export function collect(repo, baseRef = 'origin/main') {
  const fallback = [];
  const head = git(['rev-parse', 'HEAD'], repo);
  const base = git(['merge-base', 'HEAD', baseRef], repo);
  if (!head.ok) fallback.push('HEAD cannot be resolved');
  if (!base.ok || !base.out) fallback.push(`base ${baseRef} cannot be resolved or shares no history with HEAD`);
  const diff = base.ok ? git(['diff', '--name-only', '--no-renames', base.out], repo) : { ok: false, out: '' };
  if (base.ok && !diff.ok) fallback.push('the diff against the base failed');
  const untracked = git(['ls-files', '--others', '--exclude-standard'], repo);
  if (!untracked.ok) fallback.push('untracked files cannot be listed');
  const dirtyTracked = git(['status', '--porcelain', '--untracked-files=no'], repo);
  const changes = [...new Set([...diff.out.split('\n'), ...untracked.out.split('\n')].filter(Boolean))].sort();
  return {
    base: base.ok ? base.out : null,
    head: head.ok ? head.out : null,
    tree: git(['rev-parse', 'HEAD^{tree}'], repo).out || null,
    dirty: !dirtyTracked.ok || !untracked.ok || Boolean(dirtyTracked.out) || Boolean(untracked.out),
    changes,
    fallback,
  };
}

/** Shadow-run the selected stages in gate order; stop at the first failure and return its status. */
export function execute(selected, { repo = root, runner } = {}) {
  const run =
    runner ?? ((stage) => spawnSync('zsh', ['-ic', stage.command], { cwd: join(repo, stage.cwd), stdio: 'inherit' }));
  const results = [];
  for (const stage of selected) {
    const started = Date.now();
    const outcome = run(stage);
    const status = outcome.status ?? (outcome.signal ? 128 : 1);
    results.push({
      cwd: stage.cwd,
      command: stage.command,
      status,
      signal: outcome.signal ?? null,
      ms: Date.now() - started,
    });
    if (status !== 0) return { status, results };
  }
  return { status: 0, results };
}

export function main(argv = process.argv.slice(2), { repo = root, log = console.log, runner } = {}) {
  const at = argv.indexOf('--base');
  const state = collect(repo, at === -1 ? undefined : argv[at + 1]);
  const stages = parseCheckScript(readFileSync(join(repo, 'scripts/check-full.sh'), 'utf8'));
  const result = { ...state, ...plan({ stages, changes: state.changes, fallback: state.fallback }) };
  // A dirty plan describes the working tree, never HEAD^{tree}; nothing here may be cached as HEAD.
  result.verifiedTree = result.dirty ? null : result.tree;
  if (argv.includes('--json')) log(JSON.stringify(result, null, 2));
  else {
    log(
      `HS2-FCCRCK shadow plan: ${result.mode} gate (base ${result.base ?? 'unknown'}, ${result.changes.length} changed paths${result.dirty ? ', dirty working tree' : ''})`,
    );
    for (const reason of result.fallback) log(`  full because ${reason}`);
    for (const stage of result.stages)
      log(
        `  ${stage.selected ? 'run ' : 'omit'} ${stage.cwd} ${stage.command} — ${stage.reasons.slice(0, 3).join('; ')}${stage.reasons.length > 3 ? ` (+${stage.reasons.length - 3})` : ''}`,
      );
    log('Diagnostic plan only: scripts/check.sh runs the required gate and records validated receipts.');
  }
  if (!argv.includes('--execute')) return 0;
  const run = execute(
    result.stages.filter((stage) => stage.selected),
    { repo, runner },
  );
  for (const entry of run.results)
    log(`  ${entry.status === 0 ? 'passed' : 'FAILED'} ${(entry.ms / 1000).toFixed(1)}s ${entry.cwd} ${entry.command}`);
  return run.status;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) process.exitCode = main();
