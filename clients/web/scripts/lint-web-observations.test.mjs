import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';

import { afterEach, it } from 'vitest';

import {
  doctorFailureLines,
  doctorStageObservations,
  eslintInputKey,
  eslintReuseEnabled,
  lintWebCommands,
  lintWebDoctorConfig,
  loadEslintIgnores,
  runLintWeb,
} from './lint-web-observations.mjs';

const packageRoot = new URL('..', import.meta.url);
const repoRoot = new URL('../../..', import.meta.url);

it('HS2-9N8ZMD package lint:web uses the observed ordered entrypoint', () => {
  const { scripts } = JSON.parse(readFileSync(new URL('package.json', packageRoot), 'utf8'));
  assert.equal(scripts['lint:web'], 'node scripts/lint-web-observations.mjs');
});

const fixtureRoots = [];
afterEach(() => {
  for (const root of fixtureRoots.splice(0)) rmSync(root, { recursive: true, force: true });
});

const IGNORES = ['dist/**', 'target/**', 'node_modules/**'];
const doctorConfig = {
  schemaVersion: 1,
  mode: 'full',
  stages: { catalog: true, typescript: true, eslint: true, analyzer: true, browser: false },
  failOn: 'warning',
  suppressions: [],
};

function write(root, path, text) {
  mkdirSync(dirname(join(root, path)), { recursive: true });
  writeFileSync(join(root, path), text);
}

function fixture() {
  // The package sits one level down so tests can own its ancestor node_modules.
  const base = mkdtempSync(join(tmpdir(), 'hs2-5rdmev-lint-'));
  fixtureRoots.push(base);
  const root = join(base, 'web');
  write(root, '.kerf-ui-doctor.json', JSON.stringify(doctorConfig));
  write(root, 'src/a.ts', 'export const a = 1;\n');
  write(root, 'src/a.css', '.a { color: red; }\n');
  write(root, 'src/logo.png', 'png');
  write(root, '.claude/skills/kerf-ui/SKILL.md', '# skill\n');
  write(root, 'dist/bundle.js', 'built();\n');
  write(root, '.kerf-cache/ui-doctor-v1.json', '{}');
  write(root, 'node_modules/.package-lock.json', '{"lockfileVersion":3}');
  write(root, 'node_modules/eslint/index.js', 'module.exports = 1;\n');
  return { root, historyPath: join(root, 'target/history.json'), reportPath: join(root, 'target/doctor.json') };
}

/** Runs lint:web against a fixture with a recording fake runner; ESLint exits with `eslintStatus`. */
function lintOnce(paths, { flags = {}, eslintStatus = 0, onEslint = () => {} } = {}) {
  const ran = [];
  let clock = 100;
  const status = runLintWeb({
    ...paths,
    flags,
    eslintIgnores: async () => IGNORES,
    now: () => (clock += 10),
    run(command) {
      const id = command.args.some((arg) => arg.endsWith('eslint.js'))
        ? 'eslint'
        : command.args.some((arg) => arg.endsWith('catalog.js'))
          ? 'catalog'
          : 'doctor';
      ran.push(id);
      if (id === 'eslint') {
        onEslint();
        return { status: eslintStatus };
      }
      return { status: 0 };
    },
  });
  return status.then((code) => {
    const attempts = JSON.parse(readFileSync(paths.historyPath, 'utf8')).attempts;
    return { code, ran, eslint: attempts.at(-1).checks.find(({ id }) => id === 'web-lint:eslint') };
  });
}

it('HS2-5RDMEV lint subcommands stop at first failure and keep reached outcomes', async () => {
  const paths = fixture();
  const commands = [];
  let clock = 100;
  const status = await runLintWeb({
    ...paths,
    flags: { CI: 'true' },
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
  assert.deepEqual(attempt.selected, [
    'web-lint:eslint',
    'web-lint:catalog',
    'web-lint:doctor',
    'web-lint:doctor:catalog',
    'web-lint:doctor:eslint',
    'web-lint:doctor:analyzer',
    'web-lint:doctor:browser',
  ]);
});

it('HS2-5RDMEV runs the same ordered CLIs through Node on Windows without a shell', () => {
  const root = 'C:\\synthetic\\web';
  const commands = lintWebCommands(
    root,
    'C:\\synthetic\\doctor.json',
    'C:\\node\\node.exe',
    'C:\\synthetic\\doctor-config.json',
  );
  assert.deepEqual(
    commands.map(({ id }) => id),
    ['web-lint:eslint', 'web-lint:catalog', 'web-lint:doctor'],
  );
  assert.ok(commands.every(({ command }) => command.file === 'C:\\node\\node.exe'));
  assert.ok(commands.every(({ command }) => Array.isArray(command.args)));
  assert.equal(commands[0].command.args[0], '--max-old-space-size=4096');
  assert.ok(commands[0].command.args.some((arg) => arg.endsWith('eslint.js')));
  // HS2-N3XBS3: no ESLint --cache; typed rules make per-file cache reuse unsound.
  assert.deepEqual(commands[0].command.args.slice(-3), ['.', '--max-warnings', '0']);
  assert.equal(commands[0].command.args.length, 5);
  assert.ok(commands[1].command.args.some((arg) => arg.endsWith('catalog.js')));
  assert.equal(commands[2].command.args[0], '--max-old-space-size=4096');
  assert.ok(commands[2].command.args.some((arg) => arg.endsWith('cli.mjs')));
  // HS2-N3XBS3: the doctor still runs --full, never --changed, with the lint:web config.
  assert.deepEqual(commands[2].command.args.slice(2), [
    '--full',
    '--config',
    'C:\\synthetic\\doctor-config.json',
    '--format',
    'json',
    '--output',
    'C:\\synthetic\\doctor.json',
  ]);
});

it('HS2-5RDMEV doctor stage statuses are retained without raw diagnostics or invented times', async () => {
  const paths = fixture();
  let clock = 100;
  const status = await runLintWeb({
    ...paths,
    flags: { CI: 'true' },
    now: () => (clock += 10),
    run(command) {
      if (command.args.includes('--full'))
        writeFileSync(
          paths.reportPath,
          JSON.stringify({
            cache: { hit: true },
            stages: [
              { id: 'catalog', status: 'ran' },
              { id: 'typescript', status: 'skipped' },
              { id: 'eslint', status: 'ran' },
              { id: 'analyzer', status: 'cached' },
              { id: 'browser', status: 'cancelled' },
            ],
            diagnostics: [{ stage: 'eslint', message: 'private customer text', severity: 'error' }],
          }),
        );
      return { status: 0 };
    },
  });
  assert.equal(status, 0);
  const stored = readFileSync(paths.historyPath, 'utf8');
  const checks = JSON.parse(stored).attempts[0].checks;
  assert.deepEqual(
    checks.slice(3).map(({ id, outcome }) => [id, outcome]),
    [
      ['web-lint:doctor:catalog', 'pass'],
      ['web-lint:doctor:eslint', 'fail'],
      ['web-lint:doctor:analyzer', 'reused'],
      ['web-lint:doctor:browser', 'interrupted'],
    ],
  );
  assert.ok(checks.slice(3).every(({ ms }) => ms === null));
  assert.equal(stored.includes('private customer text'), false);
  assert.deepEqual(
    doctorStageObservations({ stages: [{ id: 'browser', status: 'skipped' }] }).map((row) => row.outcome),
    ['skip'],
  );
});

it('HS2-52W4TK a failed doctor prints each diagnostic; a passing or report-less run says only what it must', async () => {
  const diagnostic = {
    id: 'KUI-L017',
    severity: 'review',
    stage: 'merged',
    message: '`s` is an exceptional shorthand for `List.gap`.',
    location: { file: 'src/components/synthetic.tsx', line: 38, column: 21 },
  };
  const unlocated = { id: 'KUI-D001', severity: 'error', message: 'Synthetic stage failure.' };
  const runDoctor = async (doctorStatus, doctorReport) => {
    const paths = fixture();
    const printed = [];
    const status = await runLintWeb({
      ...paths,
      flags: { CI: 'true' },
      now: () => 1,
      report: (line) => printed.push(line),
      run(command) {
        if (command.args.includes('--full') && doctorReport)
          writeFileSync(paths.reportPath, JSON.stringify(doctorReport));
        return { status: command.args.includes('--full') ? doctorStatus : 0 };
      },
    });
    return { status, printed };
  };
  const stages = [{ id: 'eslint', status: 'ran' }];

  const failed = await runDoctor(1, { stages, diagnostics: [diagnostic, unlocated] });
  assert.equal(failed.status, 1);
  assert.deepEqual(failed.printed, [
    'Kerf UI doctor failed with 2 diagnostics:',
    '  review KUI-L017 src/components/synthetic.tsx:38:21 `s` is an exceptional shorthand for `List.gap`.',
    '  error KUI-D001 (no location) Synthetic stage failure.',
  ]);

  // A passing doctor stays quiet even when its report carries diagnostics below its threshold.
  const passed = await runDoctor(0, { stages, diagnostics: [diagnostic] });
  assert.deepEqual(passed, { status: 0, printed: [] });

  // A failure with no report, or with nothing listed, still names where to look.
  const crashed = await runDoctor(2, null);
  assert.equal(crashed.status, 2);
  assert.deepEqual(crashed.printed, ['Kerf UI doctor failed before writing target/doctor.json.']);
  const bare = await runDoctor(1, { stages, diagnostics: [] });
  assert.deepEqual(bare.printed, ['Kerf UI doctor failed without a failing diagnostic in target/doctor.json.']);

  // Earlier failing steps never reach the doctor, so they print nothing from it.
  assert.deepEqual(doctorFailureLines(null), []);
  assert.deepEqual(
    doctorFailureLines({ diagnostics: [{ id: 'KUI-X', severity: 'warning', location: { file: 'a.ts' } }] }),
    ['warning KUI-X a.ts'],
  );
});

it('HS2-N3XBS3 the lint:web doctor config differs from the package config only by TypeScript', async () => {
  const packageConfig = JSON.parse(readFileSync(new URL('.kerf-ui-doctor.json', packageRoot), 'utf8'));
  const derived = lintWebDoctorConfig(packageConfig);
  assert.equal(packageConfig.stages.typescript, true, 'ui:doctor keeps its own TypeScript stage');
  assert.deepEqual(derived, { ...packageConfig, stages: { ...packageConfig.stages, typescript: false } });
  for (const stage of ['catalog', 'eslint', 'analyzer']) assert.equal(derived.stages[stage], true, stage);
  assert.equal(derived.failOn, 'warning');

  const paths = fixture();
  await lintOnce(paths, { flags: { CI: 'true' } });
  const written = JSON.parse(readFileSync(join(paths.root, 'target/lint-web-doctor-config.json'), 'utf8'));
  assert.deepEqual(written, lintWebDoctorConfig(doctorConfig));
});

it('HS2-N3XBS3 every runner of lint:web also runs the full web typecheck the doctor no longer repeats', () => {
  const read = (path) => readFileSync(new URL(path, repoRoot), 'utf8');
  const { scripts } = JSON.parse(readFileSync(new URL('package.json', packageRoot), 'utf8'));
  assert.equal(scripts.typecheck, 'tsc --noEmit');
  assert.equal(scripts.lint, 'npm run format:check && npm run lint:web');
  // CI: the web step runs `npm run lint` and then the typecheck.
  const ci = read('.github/workflows/ci.yml');
  const webStep = ci.slice(ci.indexOf('Web client lint, typecheck'), ci.indexOf('Slowest web unit tests'));
  assert.match(webStep, /npm run lint\n\s+npm run typecheck\n/);
  // Full gate and the batch smoke gate.
  assert.match(read('scripts/check-full.sh'), /npm run lint:web && npm run typecheck/);
  const smoke = JSON.parse(read('scripts/gate-smoke.json')).stages.map(({ cwd, command }) => `${cwd}::${command}`);
  assert.ok(smoke.includes('clients/web::npm run lint:web') && smoke.includes('clients/web::npm run typecheck'));
  // Affected gate: lint:web (web_lint) is only ever selected together with typecheck (web), and
  // a selected typecheck is a budget floor.
  const plan = read('scripts/gate-plan.mjs');
  assert.match(plan, /'clients\/web::npm run typecheck': 'web'/);
  for (const line of plan.split('\n').filter((text) => text.includes("'web_lint'") && text.trim().startsWith('[')))
    assert.match(line, /'web', 'web_lint'/, line);
  assert.match(read('scripts/gate-budget.mjs'), /typecheck\/\.test\(stage\.command\)/);
});

it('HS2-N3XBS3 reads the ESLint ignores from the real flat config', async () => {
  const ignores = await loadEslintIgnores(new URL('.', packageRoot).pathname);
  for (const dir of ['dist/**', 'target/**', 'node_modules/**', 'src-tauri/**']) assert.ok(ignores.includes(dir), dir);
});

it('HS2-N3XBS3 reuses an ESLint pass only while every input it reads is unchanged', async () => {
  const paths = fixture();
  const run = () => lintOnce(paths);

  let result = await run();
  assert.deepEqual([result.code, result.ran, result.eslint.outcome], [0, ['eslint', 'catalog', 'doctor'], 'pass']);
  result = await run();
  assert.deepEqual([result.ran, result.eslint.outcome], [['catalog', 'doctor'], 'reused'], 'unchanged inputs');

  // Inputs ESLint cannot read keep the pass: CSS, raster assets, ignored output and the doctor cache.
  for (const [path, text] of [
    ['src/a.css', '.a { color: blue; }\n'],
    ['src/new.css', '.b {}\n'],
    ['src/logo.png', 'png2'],
    ['dist/bundle.js', 'rebuilt();\n'],
    ['target/other.json', '{}'],
    ['.kerf-cache/ui-doctor-v1.json', '{"x":1}'],
  ]) {
    write(paths.root, path, text);
    result = await run();
    assert.deepEqual([result.ran, result.eslint.outcome], [['catalog', 'doctor'], 'reused'], path);
  }

  // Every other input reruns ESLint once, then the new pass is reused again.
  for (const [path, text] of [
    ['src/a.ts', 'export const a = 2;\n'],
    ['src/untracked.ts', 'export {};\n'],
    ['.claude/skills/kerf-ui/SKILL.md', '# skill 2\n'],
    ['tsconfig.json', '{}'],
    ['eslint.config.mjs', 'export default [];\n'],
    ['src/icon.svg', '<svg/>'],
    ['node_modules/.package-lock.json', '{"lockfileVersion":3,"x":1}'],
  ]) {
    write(paths.root, path, text);
    result = await run();
    assert.deepEqual([result.ran[0], result.eslint.outcome], ['eslint', 'pass'], path);
    result = await run();
    assert.equal(result.eslint.outcome, 'reused', `${path} after its pass`);
  }
  rmSync(join(paths.root, 'src/untracked.ts'));
  result = await run();
  assert.equal(result.eslint.outcome, 'pass', 'a deleted input reruns ESLint');
});

it('HS2-N3XBS3 a failed or edited-while-running ESLint pass is never reused', async () => {
  const paths = fixture();
  let result = await lintOnce(paths, { eslintStatus: 1 });
  assert.deepEqual([result.code, result.ran, result.eslint.outcome], [1, ['eslint'], 'fail']);
  result = await lintOnce(paths, { eslintStatus: 1 });
  assert.deepEqual([result.code, result.eslint.outcome], [1, 'fail'], 'a failure is rerun, never reused');

  result = await lintOnce(paths, { onEslint: () => write(paths.root, 'src/a.ts', 'export const a = 3;\n') });
  assert.equal(result.eslint.outcome, 'pass');
  result = await lintOnce(paths);
  assert.equal(result.eslint.outcome, 'pass', 'an input edited during the pass is linted again');
  result = await lintOnce(paths);
  assert.equal(result.eslint.outcome, 'reused');

  // A pass recorded earlier stops counting once a later run fails on the same inputs.
  result = await lintOnce(paths, { flags: { HOTSHEET_LINT_WEB_REUSE: '0' }, eslintStatus: 1 });
  assert.equal(result.eslint.outcome, 'fail');
  result = await lintOnce(paths);
  assert.equal(result.eslint.outcome, 'pass');
});

it('HS2-N3XBS3 CI and the opt-out always run ESLint and record no reusable pass', async () => {
  assert.equal(eslintReuseEnabled({}), true);
  assert.equal(eslintReuseEnabled({ CI: 'true' }), false);
  assert.equal(eslintReuseEnabled({ HOTSHEET_LINT_WEB_REUSE: '0' }), false);
  const paths = fixture();
  for (const flags of [{ CI: 'true' }, { HOTSHEET_LINT_WEB_REUSE: '0' }]) {
    for (let i = 0; i < 2; i += 1) {
      const result = await lintOnce(paths, { flags });
      assert.equal(result.eslint.outcome, 'pass', JSON.stringify(flags));
    }
  }
  assert.equal((await lintOnce(paths)).eslint.outcome, 'pass', 'opted-out runs recorded no pass');
});

it('HS2-N3XBS3 the ESLint input key covers arguments, Node version and file names', () => {
  const { root } = fixture();
  const key = (options = {}) =>
    eslintInputKey(root, { ignores: IGNORES, args: ['.', '--max-warnings', '0'], nodeVersion: 'v22', ...options });
  const base = key();
  assert.equal(key(), base);
  assert.notEqual(key({ args: ['.', '--max-warnings', '5'] }), base);
  assert.notEqual(key({ nodeVersion: 'v24' }), base);
  write(root, 'src/b.ts', readFileSync(join(root, 'src/a.ts')));
  const copied = key();
  assert.notEqual(copied, base);
  rmSync(join(root, 'src/a.ts'));
  assert.notEqual(key(), copied, 'a rename with identical bytes still changes the key');
  const renamed = key();
  write(dirname(root), 'node_modules/.package-lock.json', `{"parent":"${Date.now()}"}`);
  try {
    assert.notEqual(key(), renamed, 'an ancestor installed tree is an input too');
  } finally {
    rmSync(join(dirname(root), 'node_modules'), { recursive: true, force: true });
  }
  assert.equal(key(), renamed);
  rmSync(join(root, 'node_modules/.package-lock.json'));
  assert.equal(typeof key(), 'string', 'a missing installed-tree record still yields a key');
});
