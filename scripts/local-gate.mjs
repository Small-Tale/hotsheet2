// HS2-RS0FFT: affected local verification and exact-input evidence.
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { classifyPath, collect, execute, parseCheckScript, plan } from './gate-plan.mjs';
import { parseBudget, selectBudget } from './gate-budget.mjs';
import {
  importCiReports,
  loadTimingHistory,
  recordCompletedGate,
  reportFileTimings,
  summarizeTimings,
} from './gate-timings.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const hash = (value) => createHash('sha256').update(JSON.stringify(value)).digest('hex');
const quote = (value) => `'${value.replaceAll("'", "'\\''")}'`;
export function parseToolVersion(command, output) {
  const prefix = command.startsWith('node ')
    ? /^v\d+\./
    : command.startsWith('npm ')
      ? /^\d+\./
      : command.startsWith('rustc ')
        ? /^rustc \d+\./
        : /^cargo \d+\./;
  const version = output.split('\n').find((line) => prefix.test(line));
  if (!version) throw new Error(`Toolchain version unavailable: ${command}`);
  return version;
}
const git = (repo, args) => {
  const result = spawnSync('git', args, { cwd: repo, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  if (result.status !== 0)
    throw new Error(`Git evidence unavailable: ${args[0]} (${result.error?.code ?? result.status})`);
  return result.stdout.trim();
};
const read = (path) => {
  try {
    return JSON.parse(readFileSync(path, 'utf8'));
  } catch {
    return null;
  }
};

function timingPath(repo) {
  const common = resolve(repo, git(repo, ['rev-parse', '--git-common-dir']));
  return join(common, 'hotsheet-gate-timings.json');
}

function reportFor(stage, repo) {
  if (stage.command.startsWith('cargo nextest run '))
    return { kind: 'nextest', path: join(repo, 'target/nextest/default/junit.xml'), root: repo };
  if (stage.command === 'npm run test:unit')
    return {
      kind: 'vitest',
      path: join(repo, 'clients/web/target', `gate-vitest-${stage.key}.json`),
      root: join(repo, 'clients/web'),
    };
  if (stage.command.startsWith('npx playwright test '))
    return {
      kind: 'playwright',
      path: join(repo, 'clients/web/target', `gate-playwright-${stage.key}.json`),
      root: join(repo, 'clients/web'),
    };
  return null;
}

function instrumentedRunner(stage, repo, report) {
  const env = { ...process.env };
  let command = stage.command;
  if (report?.kind === 'vitest')
    command += ` -- --reporter=default --reporter=json --outputFile.json=${quote(report.path)}`;
  if (report?.kind === 'playwright') {
    command += ' --reporter=line,json';
    env.PLAYWRIGHT_JSON_OUTPUT_NAME = report.path;
  }
  return spawnSync('zsh', ['-ic', command], { cwd: join(repo, stage.cwd), stdio: 'inherit', env });
}

function observedFiles(report, started) {
  if (!report) return [];
  try {
    if (statSync(report.path).mtimeMs < started) return [];
    return reportFileTimings(report.kind, readFileSync(report.path, 'utf8'), report.root);
  } catch {
    return [];
  }
}

export function importCiTimings(repo, directory) {
  const paths = {
    nextest: join(directory, 'nextest-junit.xml'),
    vitest: join(directory, 'vitest-report.json'),
    playwright: join(directory, 'playwright-report.json'),
  };
  const reports = Object.fromEntries(Object.entries(paths).filter(([, path]) => existsSync(path)));
  if (!Object.keys(reports).length) throw new Error('No CI timing artifacts found in the named directory.');
  const roots = { nextest: repo, vitest: join(repo, 'clients/web'), playwright: join(repo, 'clients/web') };
  mkdirSync(dirname(timingPath(repo)), { recursive: true });
  return importCiReports(timingPath(repo), reports, roots);
}

export function inspectTimings(repo) {
  return summarizeTimings(loadTimingHistory(timingPath(repo)));
}

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
    ...scoped.filter(
      (stage) => ![...files].some((file) => stage.command.startsWith(`npx playwright test ${quote(file)} --grep `)),
    ),
  ];
}

function budgetConfig(repo) {
  const config = read(join(repo, 'scripts/gate-budget.json'));
  if (!Number.isSafeInteger(config?.defaultBudgetMs) || config.defaultBudgetMs <= 0 || Object.keys(config).length !== 1)
    throw new Error('Checked-in gate budget config needs a positive defaultBudgetMs.');
  return config;
}

export function prepare(repo, baseRef, full = false, versions, options = {}) {
  if (!options.tier && options.budgetMs !== undefined)
    throw new Error('Specify --tier T0 or T1 with --budget until ticket-tier resolution ships.');
  if (options.tier === 'T2' && options.budgetMs !== undefined)
    throw new Error('T2 analyzed selection has no time budget; omit --budget.');
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
      return [command, parseToolVersion(command, result.stdout)];
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
  const affected = [
    ...selected.stages.filter((stage) => stage.selected),
    ...browserStages(
      state.changes,
      selected.mode === 'full',
      testTitles['clients/web/tests/providers.spec.ts'] ?? [],
      testTitles,
    ),
  ];
  const config = budgetConfig(repo);
  const tier = options.tier ?? 'unresolved';
  const budgetMs = ['T2', 'unresolved'].includes(tier) ? null : (options.budgetMs ?? config.defaultBudgetMs);
  const budgeted = selectBudget({
    stages: [...selected.stages.filter((stage) => !stage.selected), ...affected],
    changes: state.changes,
    history: loadTimingHistory(timingPath(repo)),
    budgetMs,
    tier,
    analyzed: options.analyzed ?? [],
    full: selected.mode === 'full' || tier === 'unresolved',
  });
  const chosen = budgeted.selected;
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
    version: 2,
    base: state.base,
    budgetMs,
    tier: budgeted.tier,
    analyzed: options.analyzed ?? [],
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
    return {
      ...stage,
      key: hash({ context, stage: { cwd: stage.cwd, command: stage.command, group: stage.group }, inputs }),
    };
  });
  const key = hash({ context, tree: state.tree, checks: checks.map((stage) => stage.key) });
  const common = resolve(repo, git(repo, ['rev-parse', '--git-common-dir']));
  return { ...state, ...selected, ...budgeted, checks, key, cache: join(common, 'hotsheet-local-gate'), tools };
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
    budgetMs,
    tier,
    analyzed,
    log = console.log,
  } = options;
  const selection = { budgetMs, tier, analyzed };
  const before = prepare(repo, base, full, versions, selection);
  const gateStarted = Date.now();
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
    log(
      `Local gate already passed for tree ${before.tree}; reused exact receipt for budget ${before.budgetMs ?? 'none'}ms`,
    );
    return 0;
  }
  rmSync(marker, { force: true });
  if (verify) throw new Error('No valid exact-input local gate receipt; run scripts/check.sh before pushing.');
  const results = [];
  const observations = [];
  for (const stage of before.checks) {
    const stagePath = join(before.cache, `stage-${stage.key}.json`);
    if (read(stagePath)?.key === stage.key) {
      results.push({ command: stage.command, reused: true });
      continue;
    }
    log(`Run ${stage.cwd}: ${stage.command} (${stage.budgetReason}; ${stage.reasons.join('; ')})`);
    const report = runner ? null : reportFor(stage, repo);
    if (report && report.kind !== 'nextest') {
      mkdirSync(dirname(report.path), { recursive: true });
      rmSync(report.path, { force: true });
    }
    const started = Date.now();
    const result = execute([stage], {
      repo,
      runner: runner ?? ((current) => instrumentedRunner(current, repo, report)),
    });
    results.push(...result.results);
    if (result.status !== 0) {
      log(
        `Local gate failed after ${Date.now() - gateStarted}ms; budget ${before.budgetMs ?? 'none'}ms. No receipt recorded.`,
      );
      return result.status;
    }
    const current = prepare(repo, base, full, before.tools, selection);
    if (current.dirty || current.key !== before.key)
      throw new Error('Inputs changed during the gate; evidence discarded.');
    mkdirSync(before.cache, { recursive: true });
    writeFileSync(stagePath, JSON.stringify({ key: stage.key }));
    observations.push({ ...result.results[0], files: observedFiles(report, started) });
  }
  const after = prepare(repo, base, full, before.tools, selection);
  if (after.dirty || after.key !== before.key) throw new Error('Inputs changed during the gate; evidence discarded.');
  mkdirSync(before.cache, { recursive: true });
  writeFileSync(
    receiptPath,
    JSON.stringify({
      key: before.key,
      tree: before.tree,
      base: before.base,
      budgetMs: before.budgetMs,
      tier: before.tier,
      selected: before.checks.map((stage) => stage.command),
      deferred: before.deferred.map((stage) => stage.command),
      results,
    }),
  );
  // Preserve the coordination/completion contract before recording an observation.
  mkdirSync(compatibility, { recursive: true });
  writeFileSync(marker, `${before.tree}\n`);
  try {
    recordCompletedGate(timingPath(repo), observations);
  } catch (error) {
    console.warn(`Gate passed, but timing history was unavailable: ${error.message}`);
  }
  log(
    `Local ${before.mode} gate passed for tree ${before.tree}; actual ${Date.now() - gateStarted}ms, budget ${before.budgetMs ?? 'none'}ms${before.floorOverBudget ? ' (mandatory floor exceeded budget)' : ''}`,
  );
  return 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const args = process.argv.slice(2);
    const allowed = new Set([
      '--plan',
      '--full',
      '--verify-receipt',
      '--base',
      '--timings',
      '--import-ci-timings',
      '--budget',
      '--tier',
      '--t2-stage',
    ]);
    for (let i = 0; i < args.length; i += 1) {
      if (!allowed.has(args[i])) throw new Error(`Unknown argument: ${args[i]}`);
      if (['--base', '--import-ci-timings', '--budget', '--tier', '--t2-stage'].includes(args[i]) && !args[++i])
        throw new Error(`${args[i - 1]} requires a value`);
    }
    if (args.includes('--timings') || args.includes('--import-ci-timings')) {
      if (args.includes('--import-ci-timings')) importCiTimings(root, args[args.indexOf('--import-ci-timings') + 1]);
      console.log(JSON.stringify(inspectTimings(root), null, 2));
    } else {
      process.exitCode = runGate({
        inspect: args.includes('--plan'),
        full: args.includes('--full'),
        verify: args.includes('--verify-receipt'),
        base: args.includes('--base') ? args[args.indexOf('--base') + 1] : undefined,
        budgetMs: args.includes('--budget') ? parseBudget(args[args.indexOf('--budget') + 1]) : undefined,
        tier: args.includes('--tier') ? args[args.indexOf('--tier') + 1] : undefined,
        analyzed: args.flatMap((arg, index) => (arg === '--t2-stage' ? [args[index + 1]] : [])),
      });
    }
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
