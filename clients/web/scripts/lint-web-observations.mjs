#!/usr/bin/env node
// HS2-5RDMEV: record the three existing lint:web commands without changing their order or stop behavior.
import { spawnSync } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { mkdirSync, readFileSync, rmSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { recordAttempt } from '../../../scripts/gate-timings.mjs';
import { withoutGitRepositoryEnv } from './repository-env.mjs';

const cwd = resolve(fileURLToPath(new URL('..', import.meta.url)));
const execute = (command, dir) =>
  spawnSync(command.file, command.args, { cwd: dir, stdio: 'inherit', env: withoutGitRepositoryEnv(process.env) });

// HS2-5RDMEV: invoke the same package CLIs with the active Node executable on every platform.
export function lintWebCommands(root, reportPath, node = process.execPath) {
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
          '--format',
          'json',
          '--output',
          reportPath,
        ],
      },
    },
  ];
}

export function doctorStageObservations(report) {
  return (report?.stages ?? []).flatMap((stage) => {
    if (!['catalog', 'typescript', 'eslint', 'analyzer', 'browser'].includes(stage.id)) return [];
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

export function runLintWeb({
  root = cwd,
  run = execute,
  historyPath = null,
  reportPath = join(root, 'target/lint-web-doctor-report.json'),
  node = process.execPath,
  now = Date.now,
} = {}) {
  const steps = lintWebCommands(root, reportPath, node);
  const attempt = {
    id: `local:${randomUUID()}`,
    source: 'local',
    cohort: 'lint-web',
    startedAt: now(),
    selected: [
      ...steps.map((step) => step.id),
      ...['catalog', 'typescript', 'eslint', 'analyzer', 'browser'].map((id) => `web-lint:doctor:${id}`),
    ],
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
    if (step.id === 'web-lint:doctor') {
      mkdirSync(resolve(root, 'target'), { recursive: true });
      rmSync(reportPath, { force: true });
    }
    const startedAt = now();
    const check = { id: step.id, outcome: 'interrupted', startedAt };
    attempt.checks.push(check);
    remember();
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
    if (step.id === 'web-lint:doctor') {
      try {
        attempt.checks.push(...doctorStageObservations(JSON.parse(readFileSync(reportPath, 'utf8'))));
      } catch {
        /* Doctor may fail before writing a report; keep its command outcome. */
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
  process.exitCode = runLintWeb({ historyPath });
}
