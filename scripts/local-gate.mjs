// HS2-RS0FFT: affected local verification and exact-input evidence.
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { classifyPath, collect, execute, parseCheckScript, plan } from './gate-plan.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const hash = (value) => createHash('sha256').update(JSON.stringify(value)).digest('hex');
const quote = (value) => `'${value.replaceAll("'", "'\\''")}'`;
const git = (repo, args) => {
  const result = spawnSync('git', args, { cwd: repo, encoding: 'utf8' });
  if (result.status !== 0) throw new Error(`Git evidence unavailable: ${args[0]}`);
  return result.stdout.trim();
};
const read = (path) => {
  try {
    return JSON.parse(readFileSync(path, 'utf8'));
  } catch {
    return null;
  }
};

export function browserStages(changes, full, providerTitles = [], testTitles = {}) {
  if (!full && !changes.some((path) => classifyPath(path).groups.includes?.('web'))) return [];
  const files = new Set(['tests/contracts.spec.ts', 'tests/shell-composition.spec.ts']);
  const reasons = ['real-server contracts and shipped shell composition'];
  const scoped = [];
  for (const path of changes) {
    if (/^clients\/web\/tests\/.*\.spec\.ts$/.test(path) && !path.endsWith('/providers.spec.ts')) {
      const file = path.replace('clients/web/', '');
      const titles = testTitles[path];
      if (titles?.length)
        scoped.push({
          cwd: 'clients/web',
          group: 'web',
          selected: true,
          command: `npx playwright test ${quote(file)} --grep ${quote(
            titles.map((title) => title.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('|'),
          )}`,
          reasons: ['changed browser test bodies with statically identified parents'],
        });
      else files.add(file);
    }
    if (/terminal|dashboard/.test(path)) files.add('tests/terminal-feedback.spec.ts');
    if (/timeline/.test(path)) files.add('tests/ticket-timeline-usage.spec.ts');
  }
  // Provider flows share one large file. A bounded grep preserves the affected shipped parents.
  const patterns = [];
  patterns.push(...providerTitles.map((title) => title.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')));
  if (changes.includes('clients/web/tests/providers.spec.ts') && !providerTitles.length) {
    files.add('tests/providers.spec.ts');
    reasons.push('shared or unclassified provider fixture change requires the full affected file');
  }
  if (changes.some((path) => /command/.test(path))) patterns.push('command.*(run|launch)|launch.*history');
  if (changes.some((path) => /attachment/.test(path))) patterns.push('attachment.*(batch|purpose|image|preview)');
  if (changes.some((path) => /code-review/.test(path))) patterns.push('Code Review|associated commits|change evidence');
  if (
    !patterns.length &&
    changes.some((path) => /^clients\/web\/src\/(app\/|state|routing|render|auth|credentials)/.test(path))
  ) {
    files.add('tests/providers.spec.ts');
    reasons.push('shared application infrastructure without a narrower feature boundary requires the provider suite');
  }
  const result = [
    {
      cwd: 'clients/web',
      group: 'web',
      selected: true,
      command: `npx playwright test ${[...files].sort().map(quote).join(' ')}`,
      reasons,
    },
  ];
  if (patterns.length)
    result.push({
      cwd: 'clients/web',
      group: 'web',
      selected: true,
      command: `npx playwright test tests/providers.spec.ts --grep ${quote(patterns.join('|'))}`,
      reasons: ['affected provider-backed parent flows'],
    });
  return [
    {
      cwd: 'clients/web',
      group: 'web',
      selected: true,
      command: 'npm run test:fixtures',
      reasons: ['build the real-server browser fixtures once before the selected flows'],
    },
    ...result,
    ...scoped,
  ];
}

export function prepare(repo, baseRef, full = false, versions) {
  const state = collect(repo, baseRef);
  const stages = parseCheckScript(readFileSync(join(repo, 'scripts/check-full.sh'), 'utf8'));
  const selected = plan({
    stages,
    changes: state.changes,
    fallback: [...state.fallback, ...(full ? ['explicit full local gate'] : [])],
  });
  const tools =
    versions ??
    ['node --version', 'npm --version', 'rustc --version', 'cargo --version'].map((command) => {
      const result = spawnSync('zsh', ['-ic', command], { cwd: repo, encoding: 'utf8' });
      if (result.status !== 0) throw new Error(`Toolchain evidence unavailable: ${command}`);
      return [command, result.stdout.trim()];
    });
  const entries = git(repo, ['ls-files', '--stage', '-z']).split('\0').filter(Boolean);
  const testTitles = {};
  for (const path of state.changes.filter((path) => /^clients\/web\/tests\/.*\.spec\.ts$/.test(path))) {
    if (!state.base) continue;
    const titles = [];
    const diff = git(repo, ['diff', '--unified=100000', state.base, '--', path]);
    let current = null;
    let shared = false;
    for (const line of diff.split('\n')) {
      if (/^(---|\+\+\+|@@|diff |index )/.test(line)) continue;
      const match = /^[ +]test\('((?:\\.|[^'\\])*)'/.exec(line);
      if (match) current = match[1].replace(/\\(.)/g, '$1');
      else if (/^[ +]test\(/.test(line)) current = null;
      else if (/^[ +](?:export )?(?:async )?(?:function |class |const |let )/.test(line)) current = null;
      if (/^[+-]/.test(line)) {
        if (current) titles.push(current);
        else shared = true;
      }
    }
    if (shared) titles.length = 0;
    testTitles[path] = [...new Set(titles)];
  }
  const chosen = [
    ...selected.stages.filter((stage) => stage.selected),
    ...browserStages(
      state.changes,
      selected.mode === 'full',
      testTitles['clients/web/tests/providers.spec.ts'] ?? [],
      testTitles,
    ),
  ];
  const environment = Object.fromEntries(
    [
      'CI',
      'NODE_OPTIONS',
      'RUSTFLAGS',
      'CARGO_ENCODED_RUSTFLAGS',
      'CARGO_TARGET_DIR',
      'PLAYWRIGHT_BROWSER',
      'PLAYWRIGHT_BROWSERS_PATH',
    ].map((name) => [name, process.env[name] ?? null]),
  );
  const context = {
    version: 1,
    base: state.base,
    tools,
    platform: process.platform,
    arch: process.arch,
    environment: hash(environment),
  };
  const checks = chosen.map((stage) => {
    const inputs = entries.filter((entry) => {
      if (stage.group === 'always') return true;
      const path = entry.slice(entry.indexOf('\t') + 1);
      const groups = classifyPath(path).groups;
      return groups === 'full' || groups.includes(stage.group);
    });
    return { ...stage, key: hash({ context, stage, inputs }) };
  });
  const key = hash({ context, tree: state.tree, checks: checks.map((stage) => stage.key) });
  const common = resolve(repo, git(repo, ['rev-parse', '--git-common-dir']));
  return { ...state, ...selected, checks, key, cache: join(common, 'hotsheet-local-gate'), tools };
}

export function runGate(options = {}) {
  const {
    repo = root,
    base,
    full = false,
    inspect = false,
    verify = false,
    runner,
    versions,
    log = console.log,
  } = options;
  const before = prepare(repo, base, full, versions);
  const receiptPath = join(before.cache, `${before.key}.json`);
  const compatibility = resolve(repo, git(repo, ['rev-parse', '--git-path', 'hotsheet-root-gate']));
  const marker = join(compatibility, before.tree);
  if (inspect) {
    log(JSON.stringify(before, null, 2));
    return 0;
  }
  if (before.dirty) {
    rmSync(marker, { force: true });
    throw new Error(
      'Local gate evidence requires a clean checkout; preserve local edits and verify the committed tree in a clean worktree.',
    );
  }
  const existing = read(receiptPath);
  if (existing?.key === before.key && existing.tree === before.tree) {
    mkdirSync(compatibility, { recursive: true });
    writeFileSync(marker, `${before.tree}\n`);
    log(`Local gate already passed for tree ${before.tree}`);
    return 0;
  }
  rmSync(marker, { force: true });
  if (verify) throw new Error('No valid exact-input local gate receipt; run scripts/check.sh before pushing.');
  const results = [];
  for (const stage of before.checks) {
    const stagePath = join(before.cache, `stage-${stage.key}.json`);
    if (read(stagePath)?.key === stage.key) {
      results.push({ command: stage.command, reused: true });
      continue;
    }
    log(`Run ${stage.cwd}: ${stage.command} (${stage.reasons.join('; ')})`);
    const result = execute([stage], { repo, runner });
    results.push(...result.results);
    if (result.status !== 0) return result.status;
    const current = prepare(repo, base, full, before.tools);
    if (current.dirty || current.key !== before.key)
      throw new Error('Inputs changed during the gate; evidence discarded.');
    mkdirSync(before.cache, { recursive: true });
    writeFileSync(stagePath, JSON.stringify({ key: stage.key }));
  }
  const after = prepare(repo, base, full, before.tools);
  if (after.dirty || after.key !== before.key) throw new Error('Inputs changed during the gate; evidence discarded.');
  mkdirSync(before.cache, { recursive: true });
  writeFileSync(receiptPath, JSON.stringify({ key: before.key, tree: before.tree, base: before.base, results }));
  // Preserve the coordination/completion contract after the stronger receipt has been recorded.
  mkdirSync(compatibility, { recursive: true });
  writeFileSync(marker, `${before.tree}\n`);
  log(`Local ${before.mode} gate passed for tree ${before.tree}`);
  return 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const args = process.argv.slice(2);
    const allowed = new Set(['--plan', '--full', '--verify-receipt', '--base']);
    for (let i = 0; i < args.length; i += 1) {
      if (!allowed.has(args[i])) throw new Error(`Unknown argument: ${args[i]}`);
      if (args[i] === '--base' && !args[++i]) throw new Error('--base requires a ref');
    }
    process.exitCode = runGate({
      inspect: args.includes('--plan'),
      full: args.includes('--full'),
      verify: args.includes('--verify-receipt'),
      base: args.includes('--base') ? args[args.indexOf('--base') + 1] : undefined,
    });
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
