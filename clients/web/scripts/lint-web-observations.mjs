#!/usr/bin/env node
// HS2-5RDMEV: record the three lint:web commands in their first-failure order.
// HS2-N3XBS3: skip an ESLint pass whose exact inputs already passed, and leave TypeScript to
// `npm run typecheck`, which runs wherever lint:web runs.
import { spawnSync } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, extname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { recordAttempt } from '../../../scripts/gate-timings.mjs';
import { withoutGitRepositoryEnv } from './repository-env.mjs';

const cwd = resolve(fileURLToPath(new URL('..', import.meta.url)));
const execute = (command, dir) =>
  spawnSync(command.file, command.args, { cwd: dir, stdio: 'inherit', env: withoutGitRepositoryEnv(process.env) });

// HS2-N3XBS3: the doctor stages lint:web reads from the report. TypeScript is disabled in the
// lint:web doctor configuration because `tsc --noEmit` (npm run typecheck) checks a superset of the
// same program in every place that runs lint:web: scripts/check.sh, check-full.sh and CI.
export const DOCTOR_STAGES = ['catalog', 'eslint', 'analyzer', 'browser'];

// HS2-N3XBS3: directories the ESLint pass never reads. The ESLint config's own ignores are added
// at run time; `.kerf-cache` is the UI doctor's result cache and `.git` is repository metadata.
const UNREAD_DIRECTORIES = ['.git', '.kerf-cache'];
// HS2-N3XBS3: file types no ESLint rule or TypeScript program in this package reads. TypeScript
// types `*.css` and asset imports through `vite/client` ambient modules, so their bytes cannot
// change a type. Everything else, including Markdown skills the kerfjs/ai-assistant-configs rule
// reads, stays in the key, so an unknown input type invalidates the reuse instead of escaping it.
export const ESLINT_UNREAD_EXTENSIONS = new Set([
  '.css',
  '.png',
  '.jpg',
  '.jpeg',
  '.gif',
  '.webp',
  '.ico',
  '.icns',
  '.mp4',
  '.woff',
  '.woff2',
  '.ttf',
  '.otf',
]);

// HS2-5RDMEV: invoke the same package CLIs with the active Node executable on every platform.
export function lintWebCommands(root, reportPath, node = process.execPath, doctorConfigPath = null) {
  return [
    {
      id: 'web-lint:eslint',
      command: {
        file: node,
        args: [
          '--max-old-space-size=4096',
          join(root, 'node_modules/eslint/bin/eslint.js'),
          '.',
          '--max-warnings',
          '0',
        ],
      },
    },
    {
      id: 'web-lint:catalog',
      command: { file: node, args: [join(root, 'node_modules/create-kerf-component/catalog.js'), '--check'] },
    },
    {
      id: 'web-lint:doctor',
      command: {
        file: node,
        args: [
          '--max-old-space-size=4096',
          join(root, 'node_modules/@kerfjs/ui/doctor/cli.mjs'),
          '--full',
          ...(doctorConfigPath ? ['--config', doctorConfigPath] : []),
          '--format',
          'json',
          '--output',
          reportPath,
        ],
      },
    },
  ];
}

/**
 * HS2-N3XBS3: the lint:web doctor configuration is the package's `.kerf-ui-doctor.json` with only
 * the TypeScript stage turned off. Every other stage, threshold, suppression and ownership option
 * is the package's own.
 */
export function lintWebDoctorConfig(config) {
  return { ...config, stages: { ...config.stages, typescript: false } };
}

function ignoredDirectories(ignores) {
  // ESLint flat-config ignores in eslint.config.mjs are `<dir>/**` patterns.
  return ignores.map((pattern) => pattern.replace(/\/\*\*$/, '')).filter((dir) => !/[*?[{]/.test(dir));
}

/**
 * HS2-N3XBS3: a content key for one ESLint pass. It covers every file under the package root that
 * the pass can read (sources, JSON, configuration, guidance), the installed dependency tree through
 * `node_modules/.package-lock.json`, the ESLint arguments and the Node version. Equal keys mean
 * byte-identical inputs, so a recorded pass for the key is the pass ESLint would report again.
 */
export function eslintInputKey(root, { ignores, args, nodeVersion = process.version }) {
  const skipped = new Set([...UNREAD_DIRECTORIES, ...ignoredDirectories(ignores)]);
  const hash = createHash('sha256');
  hash.update(JSON.stringify({ schema: 1, args, nodeVersion }));
  const visit = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true }).sort((a, b) => (a.name < b.name ? -1 : 1))) {
      const path = join(dir, entry.name);
      const rel = relative(root, path).split(sep).join('/');
      if (entry.isDirectory()) {
        if (!skipped.has(rel)) visit(path);
      } else if (entry.isFile() && !ESLINT_UNREAD_EXTENSIONS.has(extname(entry.name).toLowerCase())) {
        hash.update(`${rel}\0`);
        hash.update(createHash('sha256').update(readFileSync(path)).digest('hex'));
        hash.update('\0');
      }
    }
  };
  visit(root);
  // Node resolves packages from ancestor node_modules too, so every installed tree on the way up counts.
  for (let dir = resolve(root); ; dir = dirname(dir)) {
    let installed = 'missing';
    try {
      installed = createHash('sha256')
        .update(readFileSync(join(dir, 'node_modules/.package-lock.json')))
        .digest('hex');
    } catch {
      /* Without npm's installed-tree record the key still changes with package-lock.json. */
    }
    hash.update(`installed\0${dir === resolve(root) ? '.' : relative(root, dir)}\0${installed}\0`);
    if (dirname(dir) === dir) break;
  }
  return hash.digest('hex');
}

export function doctorStageObservations(report) {
  return (report?.stages ?? []).flatMap((stage) => {
    if (!DOCTOR_STAGES.includes(stage.id)) return [];
    const hasError = report.diagnostics?.some((item) => item.stage === stage.id && item.severity === 'error');
    const outcome =
      stage.status === 'skipped'
        ? 'skip'
        : stage.status === 'cancelled'
          ? 'interrupted'
          : stage.status === 'unavailable' || stage.status === 'failed' || hasError
            ? 'fail'
            : stage.status === 'cached'
              ? 'reused'
              : stage.status === 'ran'
                ? 'pass'
                : 'fail';
    return [{ id: `web-lint:doctor:${stage.id}`, outcome, category: outcome === 'fail' ? `doctor:${stage.id}` : null }];
  });
}

/**
 * HS2-52W4TK: one line per diagnostic that can fail the doctor, as
 * `<severity> <id> <file>:<line>:<column> <message>`. lint:web prints these only when the doctor step
 * fails, so a failing run names what failed instead of leaving it in the JSON report alone.
 */
export function doctorFailureLines(report) {
  // The report's `diagnostics` are the active (unsuppressed) error, warning and review entries.
  return (report?.diagnostics ?? []).map((item) => {
    const { file, line, column } = item.location ?? {};
    const where = file
      ? [file, line, column].filter((part) => part !== undefined && part !== null).join(':')
      : '(no location)';
    return `${item.severity ?? 'unknown'} ${item.id ?? 'unknown'} ${where} ${item.message ?? ''}`.trimEnd();
  });
}

const readStamp = (path) => {
  try {
    return readFileSync(path, 'utf8').trim();
  } catch {
    return null;
  }
};

/**
 * HS2-N3XBS3: ESLint pass reuse is local only. CI and `HOTSHEET_LINT_WEB_REUSE=0` always run
 * ESLint, so CI's coverage is the full uncached run.
 */
export function eslintReuseEnabled(env) {
  return !env.CI && env.HOTSHEET_LINT_WEB_REUSE !== '0';
}

export async function loadEslintIgnores(root) {
  const { default: config } = await import(pathToFileURL(join(root, 'eslint.config.mjs')).href);
  return config.flatMap((item) =>
    item && Object.keys(item).length === 1 && Array.isArray(item.ignores) ? item.ignores : [],
  );
}

export async function runLintWeb({
  root = cwd,
  run = execute,
  historyPath = null,
  reportPath = join(root, 'target/lint-web-doctor-report.json'),
  doctorConfigPath = join(root, 'target/lint-web-doctor-config.json'),
  stampPath = join(root, 'target/lint-web-eslint-pass.sha256'),
  node = process.execPath,
  now = Date.now,
  flags = process.env,
  eslintIgnores = () => loadEslintIgnores(root),
  report = (line) => console.error(line),
} = {}) {
  const steps = lintWebCommands(root, reportPath, node, doctorConfigPath);
  const attempt = {
    id: `local:${randomUUID()}`,
    source: 'local',
    cohort: 'lint-web',
    startedAt: now(),
    selected: [...steps.map((step) => step.id), ...DOCTOR_STAGES.map((id) => `web-lint:doctor:${id}`)],
    deferred: [],
    checks: [],
  };
  const remember = () => {
    if (historyPath)
      try {
        recordAttempt(historyPath, attempt);
      } catch (error) {
        console.warn(`Lint observation history unavailable: ${error.message}`);
      }
  };
  remember();
  for (const step of steps) {
    mkdirSync(resolve(root, 'target'), { recursive: true });
    if (step.id === 'web-lint:doctor') {
      rmSync(reportPath, { force: true });
      const packageConfig = JSON.parse(readFileSync(join(root, '.kerf-ui-doctor.json'), 'utf8'));
      writeFileSync(doctorConfigPath, `${JSON.stringify(lintWebDoctorConfig(packageConfig), null, 2)}\n`);
    }
    const startedAt = now();
    const check = { id: step.id, outcome: 'interrupted', startedAt };
    attempt.checks.push(check);
    remember();
    let key = null;
    let keyOf = null;
    if (step.id === 'web-lint:eslint') {
      if (eslintReuseEnabled(flags)) {
        keyOf = async () => eslintInputKey(root, { ignores: await eslintIgnores(), args: step.command.args.slice(2) });
        key = await keyOf();
        if (readStamp(stampPath) === key) {
          check.endedAt = now();
          check.ms = check.endedAt - startedAt;
          check.outcome = 'reused';
          check.category = null;
          console.log('lint:web ESLint inputs match the last passing run; reusing its pass (HS2-N3XBS3).');
          remember();
          continue;
        }
      }
    }
    const result = run(step.command, root);
    check.endedAt = now();
    check.ms = check.endedAt - startedAt;
    check.outcome = result.signal ? 'interrupted' : result.status === 0 ? 'pass' : 'fail';
    check.category =
      check.outcome === 'fail'
        ? `exit:${result.status ?? 'unknown'}`
        : result.signal
          ? `signal:${result.signal}`
          : null;
    if (step.id === 'web-lint:eslint') {
      // An input edited while ESLint ran changes the key, so that pass is not recorded for either version.
      if (key && check.outcome === 'pass' && (await keyOf()) === key) writeFileSync(stampPath, `${key}\n`);
      else rmSync(stampPath, { force: true });
    }
    if (step.id === 'web-lint:doctor') {
      let doctorReport = null;
      try {
        doctorReport = JSON.parse(readFileSync(reportPath, 'utf8'));
        attempt.checks.push(...doctorStageObservations(doctorReport));
      } catch {
        /* Doctor may fail before writing a report; keep its command outcome. */
      }
      // HS2-52W4TK: a failed doctor says why; a passing one stays quiet.
      if (check.outcome === 'fail') {
        const lines = doctorFailureLines(doctorReport);
        if (lines.length > 0) {
          report(`Kerf UI doctor failed with ${lines.length} diagnostic${lines.length === 1 ? '' : 's'}:`);
          for (const line of lines) report(`  ${line}`);
        } else {
          // HS2-52W4TK: the same report path reads the same on every platform.
          const shown = relative(root, reportPath).split(sep).join('/');
          report(
            doctorReport
              ? `Kerf UI doctor failed without a failing diagnostic in ${shown}.`
              : `Kerf UI doctor failed before writing ${shown}.`,
          );
        }
      }
    }
    remember();
    if (result.status !== 0 || result.signal) {
      attempt.endedAt = now();
      remember();
      return result.status ?? 128;
    }
  }
  attempt.endedAt = now();
  remember();
  return 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const git = spawnSync('git', ['rev-parse', '--git-common-dir'], {
    cwd,
    encoding: 'utf8',
    env: withoutGitRepositoryEnv(process.env),
  });
  const historyPath = git.status === 0 ? join(resolve(cwd, git.stdout.trim()), 'hotsheet-gate-timings.json') : null;
  process.exitCode = await runLintWeb({ historyPath });
}
