// HS2-WC74CR: the root-test read recorder. Synthetic repositories only.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { withoutGitRepositoryEnv } from '../clients/web/scripts/repository-env.mjs';

import {
  buildManifest,
  fixtureCandidate,
  fixtureReaders,
  formatManifest,
  LOG_ENV,
  opensForReading,
  recordReads,
  RECORD_ENV,
  repositoryPath,
  ROOT_ENV,
  rootTestName,
  TEST_ENV,
  undeclaredReads,
} from './root-test-reads.mjs';

const preload = pathToFileURL(fileURLToPath(new URL('./root-test-reads-preload.mjs', import.meta.url))).href;

test('HS2-WC74CR maps read targets to repository paths and keeps only fixture candidates', () => {
  const root = resolve('/synthetic/repo');
  assert.equal(repositoryPath(root, '/synthetic/repo/crates/a/src/lib.rs'), 'crates/a/src/lib.rs');
  assert.equal(repositoryPath(root, 'docs/x.md', root), 'docs/x.md');
  assert.equal(repositoryPath(root, new URL('file:///synthetic/repo/docs/y.md')), 'docs/y.md');
  assert.equal(repositoryPath(root, Buffer.from('/synthetic/repo/docs/z.md')), 'docs/z.md');
  for (const outside of ['/synthetic/other/a.rs', '/synthetic/repo', 7, new URL('https://example.invalid/a')])
    assert.equal(repositoryPath(root, outside), null, String(outside));
  assert.equal(fixtureCandidate('crates/a/src/lib.rs'), true);
  for (const path of ['scripts/gate-plan.mjs', '.git/HEAD', 'clients/web/node_modules/x/index.js', 'target/debug/x'])
    assert.equal(fixtureCandidate(path), false, path);
  assert.equal(rootTestName(root, '/synthetic/repo/scripts/gate-edge-plan.test.mjs'), 'gate-edge-plan');
  assert.equal(rootTestName(root, '/synthetic/repo/scripts/gate-edge-plan.mjs'), null);
  assert.equal(rootTestName(root, '/synthetic/repo/clients/web/scripts/a.test.mjs'), null);
  assert.equal(rootTestName(root, undefined), null);
});

test('HS2-WC74CR reports each read entry point and skips write-only opens', async () => {
  const seen = [];
  const calls = [];
  const fake = { promises: {} };
  for (const name of ['readFileSync', 'readFile', 'createReadStream', 'copyFileSync', 'copyFile', 'cpSync', 'cp'])
    fake[name] = (...args) => calls.push([name, ...args]);
  for (const name of ['openSync', 'open']) fake[name] = (...args) => calls.push([name, ...args]);
  for (const name of ['readFile', 'copyFile', 'cp', 'open'])
    fake.promises[name] = async (...args) => calls.push([`promises.${name}`, ...args]);
  recordReads((path) => seen.push(path), fake);
  fake.readFileSync('a', 'utf8');
  fake.readFile('b', () => undefined);
  fake.createReadStream('c');
  fake.copyFileSync('d', 'copy');
  fake.cpSync('e', 'copy');
  fake.openSync('f');
  fake.openSync('g', 'w');
  fake.open('h', 'r+', () => undefined);
  await fake.promises.readFile('i');
  await fake.promises.open('j', 'a');
  assert.deepEqual(seen, ['a', 'b', 'c', 'd', 'e', 'f', 'h', 'i']);
  assert.equal(calls.length, 10, 'every call still reaches the original');
  assert.equal(opensForReading(0), true);
  assert.equal(opensForReading(1), false);
  assert.equal(opensForReading(2), true);
  assert.equal(opensForReading('wx'), false);
});

test('HS2-WC74CR builds a sorted manifest and names only undeclared reads', () => {
  const manifest = buildManifest({
    'gate-edge-plan': ['crates/hotsheet-cli/src/broken_pipe.rs', 'docs/a.md'],
    'local-gate': ['docs/a.md'],
  });
  assert.deepEqual(manifest, {
    version: 1,
    directories: {},
    fixtures: {
      'crates/hotsheet-cli/src/broken_pipe.rs': ['gate-edge-plan'],
      'docs/a.md': ['gate-edge-plan', 'local-gate'],
    },
  });
  assert.equal(
    formatManifest(manifest),
    '{\n  "version": 1,\n  "directories": {},\n  "fixtures": {\n' +
      '    "crates/hotsheet-cli/src/broken_pipe.rs": ["gate-edge-plan"],\n' +
      '    "docs/a.md": ["gate-edge-plan", "local-gate"]\n  }\n}\n',
  );
  assert.deepEqual(JSON.parse(formatManifest(manifest)), manifest);
  assert.equal(formatManifest(buildManifest({})), '{\n  "version": 1,\n  "directories": {},\n  "fixtures": {}\n}\n');
  assert.deepEqual(undeclaredReads(manifest, 'local-gate', ['docs/a.md', 'crates/hotsheet-cli/src/broken_pipe.rs']), [
    'crates/hotsheet-cli/src/broken_pipe.rs',
  ]);
  assert.deepEqual(undeclaredReads(undefined, 'x', ['b', 'a', 'a']), ['a', 'b']);
});

test('HS2-WC74CR declared directories cover new files for their readers only', () => {
  const directories = { 'clients/web/src/': ['local-gate'] };
  const manifest = buildManifest(
    { 'local-gate': ['clients/web/src/a.ts', 'docs/b.md'], 'browser-engines': ['clients/web/src/a.ts'] },
    directories,
  );
  assert.deepEqual(manifest.directories, directories);
  assert.deepEqual(manifest.fixtures, {
    'clients/web/src/a.ts': ['browser-engines'],
    'docs/b.md': ['local-gate'],
  });
  assert.deepEqual(fixtureReaders(manifest, 'clients/web/src/a.ts'), ['browser-engines', 'local-gate']);
  assert.deepEqual(fixtureReaders(manifest, 'clients/web/src/added-later.ts'), ['local-gate']);
  assert.deepEqual(fixtureReaders(manifest, 'clients/web/srcx/a.ts'), []);
  assert.deepEqual(undeclaredReads(manifest, 'local-gate', ['clients/web/src/added-later.ts']), []);
  assert.deepEqual(undeclaredReads(manifest, 'browser-engines', ['clients/web/src/added-later.ts']), [
    'clients/web/src/added-later.ts',
  ]);
  assert.deepEqual(JSON.parse(formatManifest(manifest)), manifest);
});

/** A synthetic repository whose root test reads a Rust source directly and a doc through a child process. */
function repository(t) {
  const repo = mkdtempSync(join(tmpdir(), 'hs2-wc74cr-reads-'));
  t.after(() => rmSync(repo, { recursive: true, force: true }));
  for (const directory of ['scripts', 'crates/a/src', 'docs', 'lib'])
    mkdirSync(join(repo, directory), { recursive: true });
  writeFileSync(join(repo, 'crates/a/src/lib.rs'), 'pub fn synthetic() {}\n');
  writeFileSync(join(repo, 'docs/guide.md'), '# Synthetic\n');
  writeFileSync(join(repo, 'docs/untracked.md'), '# Untracked\n');
  writeFileSync(join(repo, 'scripts/helper.mjs'), 'export const helper = 1;\n');
  writeFileSync(join(repo, 'lib/imported.mjs'), 'export const imported = 1;\n');
  writeFileSync(
    join(repo, 'scripts/reader.test.mjs'),
    [
      "import { spawnSync } from 'node:child_process';",
      "import { readFileSync } from 'node:fs';",
      "import { test } from 'node:test';",
      "import { imported } from '../lib/imported.mjs';",
      "test('reads fixtures', () => {",
      "  readFileSync(new URL('../crates/a/src/lib.rs', import.meta.url), 'utf8');",
      "  readFileSync(new URL('./helper.mjs', import.meta.url), 'utf8');",
      "  readFileSync(new URL('../docs/untracked.md', import.meta.url), 'utf8');",
      '  const child = spawnSync(process.execPath, [\'-e\', \'require("node:fs").readFileSync("docs/guide.md")\'], {',
      "    cwd: new URL('..', import.meta.url),",
      "    env: Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith('GIT_'))),",
      '  });',
      "  if (child.status !== 0 || imported !== 1) throw new Error('child failed');",
      '});',
      '',
    ].join('\n'),
  );
  const env = { ...withoutGitRepositoryEnv(process.env), [ROOT_ENV]: repo };
  // NODE_TEST_CONTEXT would make the nested runner skip its files as a recursive run.
  for (const key of [TEST_ENV, LOG_ENV, RECORD_ENV, 'NODE_OPTIONS', 'NODE_TEST_CONTEXT']) delete env[key];
  const git = (...args) => spawnSync('git', ['-C', repo, ...args], { env, encoding: 'utf8' });
  git('init', '--quiet');
  git('add', 'scripts', 'crates', 'docs/guide.md', 'lib');
  const run = (extra = {}) =>
    spawnSync(process.execPath, ['--test', '--import', preload, 'scripts/reader.test.mjs'], {
      cwd: repo,
      env: { ...env, ...extra },
      encoding: 'utf8',
    });
  return { repo, run };
}

test('HS2-WC74CR fails a root test that reads an undeclared repository file, directly or in a child', (t) => {
  const { repo, run } = repository(t);
  const failed = run();
  const output = `${failed.stdout}${failed.stderr}`;
  assert.notEqual(failed.status, 0, output);
  assert.match(output, /scripts\/reader\.test\.mjs read repository files that scripts\/root-test-fixtures\.json/);
  assert.match(output, /crates\/a\/src\/lib\.rs/);
  assert.match(output, /docs\/guide\.md/, 'a read in a child Node process counts');
  assert.match(output, /lib\/imported\.mjs/, 'an imported module counts');
  assert.doesNotMatch(output, /helper\.mjs|untracked\.md/, 'scripts/ and untracked files are not fixtures');
  assert.match(output, /node scripts\/root-test-fixtures\.mjs --write/);

  // Declared for another test only: still undeclared for this one.
  const manifest = join(repo, 'scripts/root-test-fixtures.json');
  writeFileSync(
    manifest,
    formatManifest(buildManifest({ other: ['crates/a/src/lib.rs', 'docs/guide.md', 'lib/imported.mjs'] })),
  );
  assert.notEqual(run().status, 0);

  writeFileSync(
    manifest,
    formatManifest(buildManifest({ reader: ['crates/a/src/lib.rs', 'docs/guide.md', 'lib/imported.mjs'] })),
  );
  const passed = run();
  assert.equal(passed.status, 0, `${passed.stdout}${passed.stderr}`);
});

test('HS2-WC74CR record mode writes each test file reads for the generator instead of failing', (t) => {
  const { repo, run } = repository(t);
  const record = join(repo, 'record');
  mkdirSync(record);
  const recorded = run({ [RECORD_ENV]: record });
  assert.equal(recorded.status, 0, `${recorded.stdout}${recorded.stderr}`);
  assert.deepEqual(JSON.parse(readFileSync(join(record, 'reader.json'), 'utf8')), [
    'crates/a/src/lib.rs',
    'docs/guide.md',
    'lib/imported.mjs',
  ]);
});
