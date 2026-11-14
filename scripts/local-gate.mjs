// HS2-RS0FFT: affected local verification and exact-input evidence.
import { createHash, randomUUID } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { removeGitRepositoryEnv, withoutGitRepositoryEnv } from '../clients/web/scripts/repository-env.mjs';
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { classifyPath, collect, execute, parseCheckScript, plan } from './gate-plan.mjs';
import { exerciseIndexForRepo, withSpecimenInventory } from './gate-browser-exercise.mjs';
import { ciBrowserMatrix, gateBrowserCoverage, WORKFLOWS } from './browser-engines.mjs';
import { edgePreviewForRepo } from './gate-edge-plan.mjs';
import { platformCfgForRepo } from './platform-cfg.mjs';
import { contractInputsChanged, highImpactBrowserChange, parseBudget, selectBudget } from './gate-budget.mjs';
import { readTicketProfile } from './gate-ticket-profile.mjs';
import {
  planIdentity,
  planEstimate,
  rememberTestPlan,
  storedTestPlan,
  validateTestPlan,
  verifyTestPlanSelection,
} from './gate-test-plan.mjs';
import { judgeNarrowedRun, partialFromRun, readPartial, writePartial } from './gate-case-rerun.mjs';
import { sandboxFailureNotice } from './sandbox-notice.mjs';
import {
  importCiReports,
  importCiJobs,
  loadTimingHistory,
  recordCompletedGate,
  recordAttempt,
  reportCaseObservations,
  reportFileObservations,
  reportFileTimings,
  observationStageKey,
  summarizeObservations,
  summarizeTimings,
} from './gate-timings.mjs';
import { verifyInstalledDependencies } from './installed-dependencies.mjs';

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
/** A path's text at a revision, or null when it does not exist there (a new file). */
const gitText = (repo, args) => {
  const result = spawnSync('git', args, {
    cwd: repo,
    env: withoutGitRepositoryEnv(process.env),
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
  });
  return result.status === 0 ? result.stdout : null;
};
const git = (repo, args) => {
  const result = spawnSync('git', args, {
    cwd: repo,
    env: withoutGitRepositoryEnv(process.env),
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
  });
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
  const env = withoutGitRepositoryEnv(process.env);
  let command = stage.command;
  // A narrowed Vitest rerun already passes its own arguments after `--` (HS2-5340N1).
  if (report?.kind === 'vitest')
    command += `${/\s--\s/.test(` ${command} `) ? '' : ' --'} --reporter=default --reporter=json --outputFile.json=${quote(report.path)}`;
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
  const jobs = join(directory, 'jobs.json');
  if (!Object.keys(reports).length && !existsSync(jobs))
    throw new Error('No CI timing artifacts found in the named directory.');
  const roots = { nextest: repo, vitest: join(repo, 'clients/web'), playwright: join(repo, 'clients/web') };
  mkdirSync(dirname(timingPath(repo)), { recursive: true });
  const manifest = read(join(directory, 'manifest.json')) ?? {};
  if (Object.keys(reports).length) importCiReports(timingPath(repo), reports, roots, manifest);
  if (existsSync(jobs)) importCiJobs(timingPath(repo), readFileSync(jobs, 'utf8'), manifest);
  return loadTimingHistory(timingPath(repo));
}

export function inspectTimings(repo) {
  return summarizeTimings(loadTimingHistory(timingPath(repo)));
}

export function inspectObservations(repo) {
  return summarizeObservations(loadTimingHistory(timingPath(repo)));
}

/**
 * `exercised` is the HS2-N7EY18/HS2-R1D0S5 source index (scripts/gate-browser-exercise.mjs): the
 * specs and provider test titles that render or exercise a changed client module through its hooks.
 */
export function browserStages(changes, full, providerTitles = [], testTitles = {}, exercised = undefined) {
  const webChanged = changes.some(
    (path) =>
      path !== 'crates/hotsheet-server/src/lib.rs' &&
      path !== 'docs/contracts/v1/openapi.yaml' &&
      classifyPath(path).groups.includes?.('web'),
  );
  if (!full && !webChanged && !contractInputsChanged(changes)) return [];
  const files = new Set(['tests/contracts.spec.ts']);
  if (full || webChanged) files.add('tests/shell-composition.spec.ts');
  const reasons = [
    full || webChanged
      ? 'real-server contracts and shipped shell composition'
      : 'HS2-PDNJA0: router or OpenAPI contract changed',
  ];
  for (const file of exercised?.files ?? []) files.add(file);
  if (exercised?.providerWhole) files.add('tests/providers.spec.ts');
  reasons.push(...(exercised?.reasons ?? []));
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
  // HS2-WWZS75: startup code must still boot in WebKit on an insecure LAN origin whose random
  // source fails. HS2-2MZWZT escaped to CI because only the provider suite was selected here.
  if (changes.some((path) => /^clients\/web\/src\/(?:app\/|main\.tsx$|browser-id\.ts$)/.test(path))) {
    files.add('tests/terminal-safari.spec.ts');
    reasons.push('HS2-WWZS75: app startup must boot under degraded WebKit browser APIs');
  }
  const workspaceChromeChanged = changes.some((path) =>
    /^clients\/web\/src\/(?:components\/(?:workspace-controls|app-shell|workspace-header|workspace-composition-surfaces)\.(?:tsx|css)|workspace-toolbar-visibility\.ts|features\/hot-sheet-shell\.tsx)$/.test(
      path,
    ),
  );
  if (workspaceChromeChanged) {
    scoped.push({
      cwd: 'clients/web',
      group: 'web',
      selected: true,
      command:
        "npx playwright test 'tests/mobile-layout.spec.ts' --grep 'mobile viewport uses a single-column layout|resizing from mobile back to desktop'",
      reasons: ['HS2-P9A21R: workspace navigation must retain reachable phone toggles through resize'],
    });
    scoped.push({
      cwd: 'clients/web',
      group: 'web',
      selected: true,
      command:
        "npx playwright test 'tests/search-scope.spec.ts' --grep 'keeps the workspace title whole when a search changes it to a longer title'",
      reasons: ['HS2-P9A21R: title-only search transitions must remeasure workspace action overflow'],
    });
    // HS2-4S22C4: HS2-6PZ35D narrowed the phone workspace search from app-shell.tsx and escaped to
    // CI twice, once through these warm-switch width cases and once through the GRAQ2K focus case.
    scoped.push({
      cwd: 'clients/web',
      group: 'web',
      selected: true,
      command: "npx playwright test 'tests/project-tab-warm-cache.spec.ts' --grep 'HS2-RDFY4A|HS2-C5Y899'",
      reasons: ['HS2-4S22C4: workspace chrome changes must keep the measured workspace search width across switches'],
    });
  }
  // Provider flows share one large file. A bounded grep preserves the affected shipped parents.
  const patterns = [];
  if (workspaceChromeChanged)
    patterns.push(
      'projects Kerf workspace segments through mode transitions',
      // HS2-4S22C4: the second HS2-6PZ35D escape; workspace search focus lives in the same chrome.
      'keeps workspace search focused after Select All deletion',
    );
  const escapeTitle = (title) => title.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  // HS2-EWF0FT escape: the demo spec is large too; a demo change runs only the tests that open it.
  if (exercised?.demoTitles?.length)
    scoped.push({
      cwd: 'clients/web',
      group: 'web',
      selected: true,
      command: `npx playwright test ${quote('tests/ux-demo.spec.ts')} --grep ${quote(exercised.demoTitles.map(escapeTitle).join('|'))}`,
      reasons: ['HS2-N7EY18/HS2-R1D0S5: ux-demo tests that open or render the changed module'],
    });
  // HS2-1DF5PQ: tests in other specs that walk the whole demo catalog, by title.
  for (const [file, titles] of Object.entries(exercised?.scopedTitles ?? {}))
    if (!files.has(file))
      scoped.push({
        cwd: 'clients/web',
        group: 'web',
        selected: true,
        command: `npx playwright test ${quote(file)} --grep ${quote(titles.map(escapeTitle).join('|'))}`,
        reasons: ['HS2-1DF5PQ: catalog-walking tests render every ux-demo entry, including a changed or new one'],
      });
  patterns.push(...providerTitles.map(escapeTitle));
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
  // HS2-R1D0S5: provider tests that render or exercise a changed module through its shipped parent.
  patterns.push(...(exercised?.providerTitles ?? []).map(escapeTitle));
  const result = [
    {
      cwd: 'clients/web',
      group: 'web',
      selected: true,
      command: `npx playwright test ${[...files].sort().map(quote).join(' ')}`,
      reasons,
    },
  ];
  if (patterns.length && !files.has('tests/providers.spec.ts'))
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

/**
 * HS2-HW6VED: the engines the selected and deferred Playwright stages actually run, and the CI
 * browser matrix that covers what is deferred. A handoff quotes these facts instead of
 * describing a focused WebKit group, or a Chromium full suite, as a full engine matrix. Evidence
 * only: it never changes a check's key or selection. A repository without the web client or
 * workflows reports `null`.
 */
export function browserEvidence(repo, selected, deferred) {
  if (![join('clients/web', 'playwright.config.ts'), ...WORKFLOWS].every((path) => existsSync(join(repo, path))))
    return null;
  const local = gateBrowserCoverage(selected, repo);
  const ci = ciBrowserMatrix(repo).map(({ workflow, job, defaultEngine, engines, files, grep, specLaunched }) => ({
    workflow,
    job,
    defaultEngine,
    engines,
    files,
    grep,
    specLaunched,
  }));
  return { local, deferred: gateBrowserCoverage(deferred, repo), ci };
}

/** HS2-A4HD9X: only directly changed packages receive their measured baseline. */
export function performanceStages(changes) {
  const stages = [];
  if (changes.some((path) => /^crates\/|^Cargo\.(toml|lock)$|^scripts\/perf-ticketing\.mjs$/.test(path)))
    stages.push({
      cwd: '.',
      group: 'rust',
      selected: true,
      force: true,
      command: 'node scripts/perf-ticketing.mjs',
      reasons: ['testing:perf selects the synthetic local ticketing baseline for Rust changes'],
    });
  if (changes.some((path) => path.startsWith('migrator/')))
    stages.push({
      cwd: 'migrator',
      group: 'migrator',
      selected: true,
      force: true,
      command: 'npm run test:performance',
      reasons: ['testing:perf selects the synthetic PGLite export baseline for migrator changes'],
    });
  if (changes.some((path) => path.startsWith('clients/web/')))
    stages.push({
      cwd: 'clients/web',
      group: 'web',
      selected: true,
      force: true,
      command: 'npm run test:performance',
      reasons: ['testing:perf selects the web performance baseline'],
    });
  return stages;
}

function budgetConfig(repo) {
  const config = read(join(repo, 'scripts/gate-budget.json'));
  if (!Number.isSafeInteger(config?.defaultBudgetMs) || config.defaultBudgetMs <= 0 || Object.keys(config).length !== 1)
    throw new Error('Checked-in gate budget config needs a positive defaultBudgetMs.');
  return config;
}

function toolVersions(repo) {
  return ['node --version', 'npm --version', 'rustc --version', 'cargo --version'].map((command) => {
    const result = spawnSync('zsh', ['-ic', command], { cwd: repo, encoding: 'utf8' });
    if (result.status !== 0) throw new Error(`Toolchain evidence unavailable: ${command}`);
    return [command, parseToolVersion(command, result.stdout)];
  });
}

function environmentHash() {
  return hash(
    Object.fromEntries(
      [
        'CI',
        'NODE_OPTIONS',
        'RUSTFLAGS',
        'CARGO_ENCODED_RUSTFLAGS',
        'CARGO_TARGET_DIR',
        'PLAYWRIGHT_BROWSER',
        'PLAYWRIGHT_BROWSERS_PATH',
        // HS2-XW38CB: clients/web/playwright.config.ts reads these; a different worker count,
        // retry allowance or quarantine opt-in is a different browser run, not the same evidence.
        'HOTSHEET_PLAYWRIGHT_WORKERS',
        'HOTSHEET_PLAYWRIGHT_RETRIES',
        'HOTSHEET_RUN_QUARANTINED',
      ].map((name) => [name, process.env[name] ?? null]),
    ),
  );
}

/**
 * HS2-9G0M3V: the project's declared smoke checks for a batch push: build or compile, lint and a
 * fast sanity set. They never expand to affected suites, and browser suites are refused.
 */
export function smokeStages(repo) {
  const config = read(join(repo, 'scripts/gate-smoke.json'));
  if (
    !config ||
    Object.keys(config).some((key) => key !== 'stages') ||
    !Array.isArray(config.stages) ||
    !config.stages.length
  )
    throw new Error('scripts/gate-smoke.json must declare a non-empty "stages" list and nothing else.');
  const seen = new Set();
  return config.stages.map((stage) => {
    if (
      !stage ||
      Object.keys(stage).some((key) => !['cwd', 'command', 'reason'].includes(key)) ||
      ![stage.cwd, stage.command, stage.reason].every((value) => typeof value === 'string' && value.trim())
    )
      throw new Error('Each smoke stage declares exactly a cwd, a command and a reason.');
    if (/\bplaywright\b|test:browser|test:e2e/.test(stage.command))
      throw new Error(`Smoke stages never run browser suites: ${stage.command}`);
    const id = `${stage.cwd}::${stage.command}`;
    if (seen.has(id)) throw new Error(`Duplicate smoke stage: ${id}`);
    seen.add(id);
    return {
      cwd: stage.cwd,
      command: stage.command,
      group: 'always',
      selected: true,
      force: true,
      budgetReason: 'smoke profile',
      reasons: [stage.reason],
    };
  });
}

function prepareSmoke(repo, baseRef, versions, options) {
  if (
    options.tier ||
    options.budgetMs !== undefined ||
    options.ticket ||
    options.analyzed?.length ||
    options.proposed?.length
  )
    throw new Error(
      '--smoke cannot be combined with --full, --ticket, --tier, --t2-stage, --propose-stage or --budget.',
    );
  const state = collect(repo, baseRef);
  const checks = smokeStages(repo);
  const tools = versions ?? toolVersions(repo);
  const dependencies = verifyInstalledDependencies(
    repo,
    checks.map((stage) => stage.cwd),
  );
  const profile = { smoke: true };
  const context = {
    version: 2,
    base: state.base,
    budgetMs: null,
    tier: 'smoke',
    analyzed: [],
    profile,
    tools,
    dependencies,
    platform: process.platform,
    arch: process.arch,
    environment: environmentHash(),
  };
  // Every smoke check reads the whole tree: the receipt is exact for the batch commit.
  const entries = git(repo, ['ls-files', '--stage', '-z']).split('\0').filter(Boolean);
  const keyed = checks.map((stage) => ({
    ...stage,
    key: hash({ context, stage: { cwd: stage.cwd, command: stage.command, group: stage.group }, inputs: entries }),
  }));
  const smoke = new Set(checks.map((stage) => `${stage.cwd}::${stage.command}`));
  const deferred = parseCheckScript(readFileSync(join(repo, 'scripts/check-full.sh'), 'utf8'))
    .filter((stage) => !smoke.has(`${stage.cwd}::${stage.command}`))
    .map((stage) => ({ ...stage, reasons: ['HS2-9G0M3V smoke profile: the batch CI run tests this'] }));
  const common = resolve(repo, git(repo, ['rev-parse', '--git-common-dir']));
  return {
    ...state,
    mode: 'smoke',
    smoke: true,
    stages: [],
    profile,
    tier: 'smoke',
    budgetMs: null,
    floorOverBudget: false,
    checks: keyed,
    deferred,
    platformCfg: platformCfgForRepo(repo, state.base, state.changes),
    key: hash({ context, tree: state.tree, checks: keyed.map((stage) => stage.key) }),
    cache: join(common, 'hotsheet-local-gate'),
    tools,
  };
}

export function prepare(repo, baseRef, full = false, versions, options = {}) {
  if (options.smoke) {
    if (full)
      throw new Error(
        '--smoke cannot be combined with --full, --ticket, --tier, --t2-stage, --propose-stage or --budget.',
      );
    return prepareSmoke(repo, baseRef, versions, options);
  }
  const state = collect(repo, baseRef);
  const edgePreview = edgePreviewForRepo(repo, state.base, state.changes);
  const profile = options.profile ?? readTicketProfile(repo, { ticket: options.ticket, dirty: state.dirty });
  if (options.tier && profile.ticket && options.tier !== profile.tier)
    throw new Error(`--tier ${options.tier} conflicts with ${profile.ticket} tag tier:${profile.tier}.`);
  const tier = options.tier ?? profile?.tier ?? 'T1';
  if (tier === 'T2' && (options.budgetMs !== undefined || profile?.budgetMs !== undefined))
    throw new Error('T2 analyzed selection has no time budget; omit --budget.');
  const stages = parseCheckScript(readFileSync(join(repo, 'scripts/check-full.sh'), 'utf8'));
  const selected = plan({
    stages,
    changes: state.changes,
    fallback: [...state.fallback, ...(full ? ['explicit full local gate'] : [])],
  });
  const tools = versions ?? toolVersions(repo);
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
      const match = /^[ +]\s*test\('((?:\\.|[^'\\])*)'/.exec(line);
      const template = /^[ +]\s*test\(`((?:\\.|[^`\\$]|\$(?!\{))*)/.exec(line);
      if (match) current = match[1].replace(/\\(.)/g, '$1');
      // HS2-P9A21R: a stable template prefix covers every generated theme/size case.
      else if (template && template[1].length >= 12) current = template[1].replace(/\\(.)/g, '$1');
      else if (/^[ +]\s*test\(/.test(line)) current = null;
      // HS2-P9A21R: after a closing scope, uncertain edits require the whole file.
      else if (/^[ +]\s*}[);,\s]*$/.test(line)) current = null;
      else if (/^[ +](?:export )?(?:async )?(?:function |class |const |let )/.test(line)) current = null;
      if (/^[+-]/.test(line)) {
        if (current) titles.push(current);
        else shared = true;
      }
    }
    if (shared) titles.length = 0;
    testTitles[path] = [...new Set(titles)];
  }
  const highImpactBrowser = highImpactBrowserChange(state.changes);
  let affected = [
    ...selected.stages.filter((stage) => stage.selected),
    ...browserStages(
      state.changes,
      selected.mode === 'full' || profile?.browser === true || highImpactBrowser,
      testTitles['clients/web/tests/providers.spec.ts'] ?? [],
      testTitles,
      withSpecimenInventory(exerciseIndexForRepo(repo, state.changes), {
        changes: state.changes,
        readBase: (path) => (state.base ? gitText(repo, ['show', `${state.base}:${path}`]) : null),
        readWorking: (path) => (existsSync(join(repo, path)) ? readFileSync(join(repo, path), 'utf8') : null),
        demoSpec: existsSync(join(repo, 'clients/web/tests/ux-demo.spec.ts'))
          ? readFileSync(join(repo, 'clients/web/tests/ux-demo.spec.ts'), 'utf8')
          : null,
      }),
    ).map((stage) =>
      highImpactBrowser
        ? {
            ...stage,
            force: true,
            reasons: [...stage.reasons, 'HS2-RRNZYN: high-impact shared state requires a shipped-flow browser check'],
          }
        : stage,
    ),
  ];
  if (profile?.browser && affected.some((stage) => stage.command.startsWith('npx playwright test '))) {
    const files = [
      ...new Set(
        affected
          .filter((stage) => stage.command.startsWith('npx playwright test '))
          .flatMap((stage) => [...stage.command.matchAll(/'?(tests\/[^' ]+\.spec\.ts)'?/g)].map((match) => match[1])),
      ),
    ].sort();
    affected = [
      ...affected.filter((stage) => !stage.command.startsWith('npx playwright test ')),
      {
        cwd: 'clients/web',
        group: 'web',
        selected: true,
        force: true,
        command: `npx playwright test ${files.map(quote).join(' ')}`,
        reasons: ['testing:browser selects full affected Playwright files'],
      },
    ];
  }
  if (profile?.perf) {
    const performance = performanceStages(state.changes);
    if (!performance.length) throw new Error('testing:perf has no configured baseline for these changed paths.');
    affected.push(...performance);
  }
  const config = budgetConfig(repo);
  const budgetMs =
    tier === 'T2' || profile?.deep ? null : (options.budgetMs ?? profile?.budgetMs ?? config.defaultBudgetMs);
  const budgeted = selectBudget({
    stages: [...selected.stages.filter((stage) => !stage.selected), ...affected],
    changes: state.changes,
    history: loadTimingHistory(timingPath(repo)),
    budgetMs,
    tier,
    analyzed: options.analyzed ?? [],
    proposed: options.proposed ?? [],
    full: selected.mode === 'full' || profile?.deep === true,
    fullReason: profile?.deep ? 'deep-testing tag' : 'full fallback',
  });
  const chosen = budgeted.selected;
  // HS2-PNBH4X: refuse a stale or missing install in any selected Node package, and bind the exact
  // installed tree into every receipt so evidence from one install is never reused for another.
  const dependencies = verifyInstalledDependencies(
    repo,
    chosen.map((stage) => stage.cwd),
  );
  // HS2-Q5XAAK: discovery explains the plan but does not change the verified inputs.
  const profileIdentity = { ...profile };
  delete profileIdentity.reason;
  const context = {
    version: 2,
    base: state.base,
    budgetMs,
    tier: budgeted.tier,
    analyzed: options.analyzed ?? [],
    proposed: options.proposed ?? [],
    profile: profileIdentity,
    testPlan: options.testPlan ? planIdentity(options.testPlan) : null,
    tools,
    dependencies,
    platform: process.platform,
    arch: process.arch,
    environment: environmentHash(),
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
  return {
    ...state,
    edgePreview,
    ...selected,
    ...budgeted,
    profile,
    checks,
    testPlan: options.testPlan ?? null,
    testPlanEstimate: options.testPlan ? planEstimate(options.testPlan) : null,
    browserCoverage: browserEvidence(repo, checks, budgeted.deferred),
    // HS2-A7X8F5: Rust cfg branches this host never compiles; derived from the tree, not a key input.
    platformCfg: platformCfgForRepo(repo, state.base, state.changes),
    key,
    cache: join(common, 'hotsheet-local-gate'),
    tools,
  };
}

/** A passing T2 receipt for the current tree whose recorded selection reproduces its exact key. */
function recordedT2Selection(repo, base, full, versions, selection) {
  const cache = join(resolve(repo, git(repo, ['rev-parse', '--git-common-dir'])), 'hotsheet-local-gate');
  if (!existsSync(cache)) return undefined;
  const { tree } = collect(repo, base);
  for (const name of readdirSync(cache)) {
    if (!name.endsWith('.json') || name.startsWith('stage-')) continue;
    const receipt = read(join(cache, name));
    if (
      receipt?.tree !== tree ||
      receipt.tier !== 'T2' ||
      !Array.isArray(receipt.analyzed) ||
      !(receipt.analyzed.length || receipt.proposed?.length)
    )
      continue;
    const candidate = { ...selection, analyzed: receipt.analyzed, proposed: receipt.proposed ?? [] };
    let before;
    try {
      before = prepare(repo, base, full, versions, candidate);
    } catch {
      continue;
    }
    if (before.key === receipt.key) return { selection: candidate, before };
  }
  return undefined;
}

/** HS2-A7X8F5: a handoff quotes this so nobody reads a pass on this host as other-platform evidence. */
function logPlatformCfg(prepared, log) {
  if (prepared.platformCfg?.files.length) log(`Platform cfg code deferred to CI: ${prepared.platformCfg.summary}`);
}

export function runGate(options = {}) {
  const {
    repo = root,
    base,
    full = false,
    inspect = false,
    verify = false,
    runner,
    // Test seam: the per-case report a stage writes; production derives it from the command.
    reportFor: stageReport = runner ? () => null : reportFor,
    versions,
    budgetMs,
    tier,
    analyzed,
    proposed,
    ticket,
    profile,
    testPlan,
    smoke = false,
    log = console.log,
    env = withoutGitRepositoryEnv(process.env),
  } = options;
  if (smoke && testPlan) throw new Error('--smoke cannot be combined with --test-plan.');
  let selection = { budgetMs, tier, analyzed, proposed, ticket, profile, smoke };
  // HS2-RRNZYN: persist a reviewed plan outside the source tree, bound to the tree
  // it describes. Bare push hooks may reuse it only for the same exact tree.
  if (!smoke) {
    const initial = collect(repo, base);
    const common = resolve(repo, git(repo, ['rev-parse', '--git-common-dir']));
    const cache = join(common, 'hotsheet-local-gate');
    const resolved = profile ?? readTicketProfile(repo, { ticket, dirty: initial.dirty });
    const manifest = testPlan
      ? JSON.parse(readFileSync(resolve(repo, testPlan), 'utf8'))
      : storedTestPlan(cache, { tree: initial.tree, base: initial.base, ticket: resolved.ticket });
    if (manifest) {
      validateTestPlan(manifest, { ...initial, profile: resolved });
      if (analyzed?.length && JSON.stringify(analyzed) !== JSON.stringify(manifest.analyzed))
        throw new Error('HS2-RRNZYN: --t2-stage conflicts with the reviewed test plan.');
      if (proposed?.length && JSON.stringify(proposed) !== JSON.stringify(manifest.proposed))
        throw new Error('HS2-RRNZYN: --propose-stage conflicts with the reviewed test plan.');
      selection = {
        ...selection,
        profile: resolved,
        ticket: manifest.ticket,
        analyzed: manifest.analyzed,
        proposed: manifest.proposed,
        testPlan: manifest,
      };
    } else if (testPlan) {
      throw new Error('HS2-RRNZYN: reviewed test plan unavailable.');
    } else if (!inspect && resolved.developmentStrategy) {
      throw new Error('HS2-RRNZYN: configured projects require --test-plan <reviewed.json> for this tree.');
    }
  }
  let before;
  try {
    before = prepare(repo, base, full, versions, selection);
  } catch (error) {
    if (analyzed?.length || proposed?.length || !/T2 needs an explicit analyzed/.test(error.message)) throw error;
    // HS2-BNAYKG: push.sh and the pre-push hook run a bare check; reuse the exact receipt
    // of the passing --t2-stage run for this tree instead of refusing every T2 push.
    const reused = recordedT2Selection(repo, base, full, versions, selection);
    if (!reused) throw new Error(`${error.message} Pass --t2-stage <cwd::command> for each analyzed check.`);
    ({ selection, before } = reused);
  }
  if (selection.testPlan) verifyTestPlanSelection(selection.testPlan, before);
  const gateStarted = Date.now();
  const budgetLabel = before.budgetMs === null ? 'none' : `${before.budgetMs}ms`;
  const receiptPath = join(before.cache, `${before.key}.json`);
  const compatibility = resolve(repo, git(repo, ['rev-parse', '--git-path', 'hotsheet-root-gate']));
  // HS2-9G0M3V: a smoke receipt is not root-gate evidence, so it never writes or clears the
  // root-gate marker that serial prerequisite resumption reads.
  const marker = before.smoke ? null : join(compatibility, before.tree);
  const writeMarker = () => {
    if (!marker) return;
    mkdirSync(compatibility, { recursive: true });
    writeFileSync(marker, `${before.tree}\n`);
  };
  const clearMarker = () => {
    if (marker) rmSync(marker, { force: true });
  };
  if (inspect) {
    log(JSON.stringify(before, null, 2));
    return 0;
  }
  if (before.dirty) {
    clearMarker();
    throw new Error(
      'Local gate evidence requires a clean checkout; preserve local edits and verify the committed tree in a clean worktree.',
    );
  }
  if (selection.testPlan && !verify) rememberTestPlan(before.cache, selection.testPlan);
  const existing = read(receiptPath);
  const selectedChecks = before.checks.map(observationStageKey);
  const omittedStages = selection.testPlan ? before.stages.filter((stage) => !stage.selected) : before.deferred;
  const deferredChecks = omittedStages.map(observationStageKey);
  const attempt = {
    id: `local:${randomUUID()}`,
    source: 'local',
    cohort: `local-${before.mode}-${before.tier ?? 'T1'}`,
    startedAt: gateStarted,
    selected: selectedChecks,
    deferred: deferredChecks,
    checks: [],
    cases: [],
  };
  const remember = () => {
    try {
      recordAttempt(timingPath(repo), attempt);
    } catch (error) {
      console.warn(`Gate observation history unavailable: ${error.message}`);
    }
  };
  if (existing?.key === before.key && existing.tree === before.tree) {
    attempt.endedAt = Date.now();
    attempt.checks = selectedChecks.map((id) => ({ id, outcome: 'reused' }));
    remember();
    writeMarker();
    log(`Local gate already passed for tree ${before.tree}; reused exact receipt for budget ${budgetLabel}`);
    logPlatformCfg(before, log);
    return 0;
  }
  clearMarker();
  if (verify) throw new Error('No valid exact-input local gate receipt; run scripts/check.sh before pushing.');
  remember();
  const results = [];
  const observations = [];
  const flaky = [];
  for (const stage of before.checks) {
    const stagePath = join(before.cache, `stage-${stage.key}.json`);
    const recorded = read(stagePath);
    if (recorded?.key === stage.key) {
      results.push({ command: stage.command, reused: true });
      flaky.push(...(recorded.flaky ?? []));
      attempt.checks.push({ id: observationStageKey(stage), outcome: 'reused' });
      remember();
      continue;
    }
    const report = stageReport(stage, repo);
    // HS2-5340N1: a failed suite stage on these exact inputs reruns only its failed cases.
    const partialPath = join(before.cache, `stage-${stage.key}.partial.json`);
    const partial = report ? readPartial(partialPath, stage.key, stage.command) : null;
    if (partial)
      log(
        `Rerun only ${partial.failed.length} failed of ${partial.total} case(s) from the previous attempt on these exact inputs: ${stage.cwd}: ${stage.command}`,
      );
    else log(`Run ${stage.cwd}: ${stage.command} (${stage.budgetReason}; ${stage.reasons.join('; ')})`);
    const running = partial ? { ...stage, command: partial.narrowed } : stage;
    if (report && report.kind !== 'nextest') {
      mkdirSync(dirname(report.path), { recursive: true });
      rmSync(report.path, { force: true });
    }
    const started = Date.now();
    const check = { id: observationStageKey(stage), outcome: 'interrupted', startedAt: started };
    attempt.checks.push(check);
    remember();
    const result = execute([running], {
      repo,
      runner: runner ? (current) => runner(current, report) : (current) => instrumentedRunner(current, repo, report),
    });
    result.results[0].command = stage.command;
    if (partial) result.results[0].narrowed = partial.narrowed;
    results.push(...result.results);
    const finished = Date.now();
    const outcome = result.results[0];
    check.endedAt = finished;
    check.ms = finished - started;
    check.outcome = outcome.signal ? 'interrupted' : result.status === 0 ? 'pass' : 'fail';
    check.category = outcome.signal ? `signal:${outcome.signal}` : result.status === 0 ? null : `exit:${result.status}`;
    let body = null;
    if (report) {
      try {
        // Playwright and Vitest reports were removed before the run, so any report is fresh;
        // nextest's fixed JUnit path is judged by time, allowing for coarse file timestamps.
        if (report.kind !== 'nextest' || statSync(report.path).mtimeMs >= started - 1000)
          body = readFileSync(report.path, 'utf8');
      } catch {
        /* A missing runner report leaves case observations unknown. */
      }
      try {
        if (body !== null) {
          attempt.cases.push(...reportCaseObservations(report.kind, body, report.root));
          attempt.checks.push(...reportFileObservations(report.kind, body, report.root));
        }
      } catch {
        /* An unparseable report leaves case observations unknown. */
      }
    }
    remember();
    let stageFlaky = [];
    if (partial) {
      const judged = judgeNarrowedRun(partial, { ...outcome, status: result.status, body, root: report.root });
      if (judged.result === 'passed') {
        rmSync(partialPath, { force: true });
        stageFlaky = judged.flaky;
        log(
          `All ${partial.failed.length} previously failed case(s) passed on the targeted rerun. They are flaky; file a ticket for each per the flake policy:\n${stageFlaky.map((name) => `  ${name}`).join('\n')}`,
        );
      } else if (judged.result === 'retry') {
        writePartial(partialPath, judged.partial);
        log(
          `${judged.partial.failed.length} case(s) still fail; the next attempt on these inputs reruns only them:\n${judged.partial.failed.map((entry) => `  ${entry.name}`).join('\n')}`,
        );
      } else {
        rmSync(partialPath, { force: true });
        log(`Cannot trust the targeted rerun (${judged.reason}); the next attempt reruns the whole stage.`);
      }
      if (judged.result !== 'passed' && result.status === 0) result.status = 1;
    } else if (report) {
      const next = partialFromRun({
        key: stage.key,
        command: stage.command,
        kind: report.kind,
        status: result.status,
        signal: outcome.signal,
        body,
        root: report.root,
      });
      if (next) {
        writePartial(partialPath, next);
        log(
          `${next.failed.length} of ${next.total} case(s) failed; the next attempt on these inputs reruns only them:\n${next.failed.map((entry) => `  ${entry.name}`).join('\n')}`,
        );
      } else rmSync(partialPath, { force: true });
    }
    if (result.status !== 0) {
      log(`Local gate failed after ${Date.now() - gateStarted}ms; budget ${budgetLabel}. No receipt recorded.`);
      const notice = sandboxFailureNotice(env);
      if (notice) log(notice.trim());
      return result.status;
    }
    const current = prepare(repo, base, full, before.tools, { ...selection, profile: before.profile });
    if (current.dirty || current.key !== before.key)
      throw new Error('Inputs changed during the gate; evidence discarded.');
    mkdirSync(before.cache, { recursive: true });
    writeFileSync(
      stagePath,
      JSON.stringify(stageFlaky.length ? { key: stage.key, flaky: stageFlaky } : { key: stage.key }),
    );
    flaky.push(...stageFlaky);
    // A narrowed rerun's duration is not a whole-stage timing sample.
    if (!partial) observations.push({ ...result.results[0], files: observedFiles(report, started) });
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
      analyzed: selection.analyzed ?? [],
      proposed: selection.proposed ?? [],
      profile: before.profile,
      testPlan: selection.testPlan
        ? {
            id: planIdentity(selection.testPlan),
            ticket: selection.testPlan.ticket,
            selected: selection.testPlan.selected,
            deferred: selection.testPlan.deferred,
            behavior: selection.testPlan.behavior,
            risks: selection.testPlan.risks,
            reviewed_by: selection.testPlan.reviewed_by,
            estimate: planEstimate(selection.testPlan),
            strategy: selection.testPlan.strategy,
            strategy_source: selection.testPlan.strategy_source,
            override: selection.testPlan.override ?? null,
          }
        : null,
      selected: before.checks.map((stage) => stage.command),
      deferred: omittedStages.map((stage) => stage.command),
      browserCoverage: before.browserCoverage,
      platformCfg: before.platformCfg,
      results,
      flaky,
    }),
  );
  // Preserve the coordination/completion contract before recording an observation.
  writeMarker();
  attempt.endedAt = Date.now();
  remember();
  try {
    recordCompletedGate(timingPath(repo), observations);
  } catch (error) {
    console.warn(`Gate passed, but timing history was unavailable: ${error.message}`);
  }
  log(
    `Local ${before.mode} gate passed for tree ${before.tree}; actual ${Date.now() - gateStarted}ms, budget ${budgetLabel}${before.floorOverBudget ? ' (mandatory floor exceeded budget)' : ''}`,
  );
  // HS2-HW6VED: the engine facts a handoff quotes; the full CI matrix is in the receipt and --plan.
  if (before.browserCoverage) {
    log(`Browser engines run locally: ${before.browserCoverage.local.summary}`);
    if (before.browserCoverage.deferred.stages.length)
      log(`Browser engines deferred to CI: ${before.browserCoverage.deferred.summary}`);
  }
  logPlatformCfg(before, log);
  if (flaky.length)
    log(`Passed with ${flaky.length} flaky case(s) recorded in the receipt; each needs a flake ticket.`);
  return 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  // HS2-KKDZF4: under a git hook, GIT_DIR and GIT_INDEX_FILE name the calling worktree. Remove
  // them before any stage starts so no test fixture's git can commit into, or re-initialize, it.
  removeGitRepositoryEnv(process.env);
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
      '--propose-stage',
      '--ticket',
      '--smoke',
      '--test-plan',
    ]);
    for (let i = 0; i < args.length; i += 1) {
      if (!allowed.has(args[i])) throw new Error(`Unknown argument: ${args[i]}`);
      if (
        [
          '--base',
          '--import-ci-timings',
          '--budget',
          '--tier',
          '--t2-stage',
          '--propose-stage',
          '--ticket',
          '--test-plan',
        ].includes(args[i]) &&
        !args[++i]
      )
        throw new Error(`${args[i - 1]} requires a value`);
    }
    if (args.includes('--timings') || args.includes('--import-ci-timings')) {
      if (args.includes('--import-ci-timings')) importCiTimings(root, args[args.indexOf('--import-ci-timings') + 1]);
      console.log(JSON.stringify({ ...inspectTimings(root), observations: inspectObservations(root) }, null, 2));
    } else {
      process.exitCode = runGate({
        inspect: args.includes('--plan'),
        full: args.includes('--full'),
        verify: args.includes('--verify-receipt'),
        base: args.includes('--base') ? args[args.indexOf('--base') + 1] : undefined,
        budgetMs: args.includes('--budget') ? parseBudget(args[args.indexOf('--budget') + 1]) : undefined,
        tier: args.includes('--tier') ? args[args.indexOf('--tier') + 1] : undefined,
        analyzed: args.flatMap((arg, index) => (arg === '--t2-stage' ? [args[index + 1]] : [])),
        proposed: args.flatMap((arg, index) => (arg === '--propose-stage' ? [args[index + 1]] : [])),
        ticket: args.includes('--ticket') ? args[args.indexOf('--ticket') + 1] : undefined,
        testPlan: args.includes('--test-plan') ? args[args.indexOf('--test-plan') + 1] : undefined,
        smoke: args.includes('--smoke'),
      });
    }
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
