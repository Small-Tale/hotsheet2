import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, it } from 'vitest';

import { doctorStageObservations, lintWebCommands, runLintWeb } from './lint-web-observations.mjs';

it('HS2-9N8ZMD package lint:web uses the observed ordered entrypoint', () => {
  const { scripts } = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'));
  assert.equal(scripts['lint:web'], 'node scripts/lint-web-observations.mjs');
});

const fixtureRoots = [];
afterEach(() => {
  for (const root of fixtureRoots.splice(0)) rmSync(root, { recursive: true, force: true });
});

function fixture() {
  const root = mkdtempSync(join(tmpdir(), 'hs2-5rdmev-lint-'));
  fixtureRoots.push(root);
  return { root, historyPath: join(root, 'history.json'), reportPath: join(root, 'doctor.json') };
}

it('HS2-5RDMEV lint subcommands stop at first failure and keep reached outcomes', () => {
  const paths = fixture();
  const commands = [];
  let clock = 100;
  const status = runLintWeb({
    ...paths,
    now: () => (clock += 10),
    run(command) {
      commands.push(command);
      return { status: commands.length === 2 ? 7 : 0 };
    },
  });
  assert.equal(status, 7);
  assert.equal(commands.length, 2);
  const attempt = JSON.parse(readFileSync(paths.historyPath, 'utf8')).attempts[0];
  assert.deepEqual(
    attempt.checks.map(({ outcome }) => outcome),
    ['pass', 'fail'],
  );
  assert.equal(attempt.deferred.length, 0);
  assert.equal(attempt.selected.length, 8);
});

it('HS2-5RDMEV runs the same ordered CLIs through Node on Windows without a shell', () => {
  const root = 'C:\\synthetic\\web';
  const commands = lintWebCommands(root, 'C:\\synthetic\\doctor.json', 'C:\\node\\node.exe');
  assert.deepEqual(
    commands.map(({ id }) => id),
    ['web-lint:eslint', 'web-lint:catalog', 'web-lint:doctor'],
  );
  assert.ok(commands.every(({ command }) => command.file === 'C:\\node\\node.exe'));
  assert.ok(commands.every(({ command }) => Array.isArray(command.args)));
  assert.equal(commands[0].command.args[0], '--max-old-space-size=4096');
  assert.ok(commands[0].command.args.some((arg) => arg.endsWith('eslint.js')));
  assert.deepEqual(commands[0].command.args.slice(-3), ['.', '--max-warnings', '0']);
  assert.ok(commands[1].command.args.some((arg) => arg.endsWith('catalog.js')));
  assert.equal(commands[2].command.args[0], '--max-old-space-size=4096');
  assert.ok(commands[2].command.args.some((arg) => arg.endsWith('cli.mjs')));
  assert.deepEqual(commands[2].command.args.slice(-5, -2), ['--full', '--format', 'json']);
  assert.deepEqual(commands[2].command.args.slice(-2), ['--output', 'C:\\synthetic\\doctor.json']);
});

it('HS2-5RDMEV doctor stage statuses are retained without raw diagnostics or invented times', () => {
  const paths = fixture();
  let clock = 100;
  const status = runLintWeb({
    ...paths,
    now: () => (clock += 10),
    run(command) {
      if (command.args.includes('--full'))
        writeFileSync(
          paths.reportPath,
          JSON.stringify({
            cache: { hit: true },
            stages: [
              { id: 'catalog', status: 'ran' },
              { id: 'typescript', status: 'ran' },
              { id: 'eslint', status: 'skipped' },
              { id: 'analyzer', status: 'cached' },
              { id: 'browser', status: 'cancelled' },
            ],
            diagnostics: [{ stage: 'typescript', message: 'private customer text', severity: 'error' }],
          }),
        );
      return { status: 0 };
    },
  });
  assert.equal(status, 0);
  const stored = readFileSync(paths.historyPath, 'utf8');
  const checks = JSON.parse(stored).attempts[0].checks;
  assert.deepEqual(
    checks.slice(3).map(({ outcome }) => outcome),
    ['pass', 'fail', 'skip', 'reused', 'interrupted'],
  );
  assert.ok(checks.slice(3).every(({ ms }) => ms === null));
  assert.equal(stored.includes('private customer text'), false);
  assert.deepEqual(
    doctorStageObservations({ stages: [{ id: 'browser', status: 'skipped' }] }).map((row) => row.outcome),
    ['skip'],
  );
});
