// HS2-55MWQ9: candidate edges are explanatory until HS2-DAA8AB validates a required-gate change.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

import { edgePreview, edgePreviewForRepo, rustConsumersFromMetadata, rustPrivateBodyOnly } from './gate-edge-plan.mjs';
import { parseCheckScript, plan } from './gate-plan.mjs';

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const rustPath = 'crates/hotsheet-cli/src/broken_pipe.rs';
const webPath = 'clients/web/src/trailing-task-internal.ts';
const fullStages = parseCheckScript(readFileSync(join(root, 'scripts/check-full.sh'), 'utf8'));

test('HS2-55MWQ9 private Rust body changes name direct tests and all Cargo consumers', () => {
  const before = readFileSync(join(root, rustPath), 'utf8');
  const after = before.replace('message.contains("Broken pipe")', 'message.contains("broken pipe")');
  assert.equal(rustPrivateBodyOnly(before, after), true);
  const metadata = JSON.parse(execFileSync('cargo', ['metadata', '--no-deps', '--format-version', '1'], { cwd: root }));
  const consumers = rustConsumersFromMetadata(metadata, 'hotsheet-cli');
  assert.deepEqual(consumers, ['hotsheet-cli', 'hotsheet-mcp', 'hotsheet-server']);
  const plan = edgePreview({ changes: [rustPath], rustBefore: before, rustAfter: after, rustConsumers: consumers });
  assert.equal(plan.mode, 'candidate');
  assert.equal(plan.requiredGate, 'unchanged pending HS2-DAA8AB');
  assert.ok(plan.suggestedStages.some((item) => item.command.includes('--test broken_pipe')));
  assert.ok(plan.suggestedStages.some((item) => consumers.every((name) => item.command.includes(`-p ${name}`))));
});

test('HS2-55MWQ9 public, mixed, missing, and ambiguous Rust changes retain the broad gate', () => {
  const before = readFileSync(join(root, rustPath), 'utf8');
  const consumers = ['hotsheet-cli', 'hotsheet-mcp', 'hotsheet-server'];
  for (const after of [
    before.replace('pub const BROKEN_PIPE_EXIT_CODE: i32 = 0', 'pub const BROKEN_PIPE_EXIT_CODE: i32 = 1'),
    before.replace('pub fn is_broken_pipe_print_panic', 'pub fn recognizes_broken_pipe_print_panic'),
    before.replace('fn mentions_broken_pipe(message: &str)', 'fn mentions_broken_pipe(message: &String)'),
  ]) {
    assert.equal(rustPrivateBodyOnly(before, after), false);
    assert.equal(
      edgePreview({ changes: [rustPath], rustBefore: before, rustAfter: after, rustConsumers: consumers }).mode,
      'broad',
    );
  }
  assert.equal(edgePreview({ changes: [rustPath], rustConsumers: consumers }).mode, 'broad');
  assert.equal(
    edgePreview({ changes: [rustPath, 'docs/contracts/v1/openapi.yaml'], rustConsumers: consumers }).mode,
    'broad',
  );
  assert.equal(edgePreview({ changes: ['crates/hotsheet-server/src/lib.rs'], rustConsumers: consumers }).mode, 'broad');
  assert.equal(edgePreview({ changes: ['migrator/src/export.mjs'] }).mode, 'broad');
  assert.equal(edgePreview({ changes: ['scripts/check.sh'] }).mode, 'broad');
});

test('HS2-55MWQ9 private web module has public-contract and shipped-parent checks', () => {
  const candidate = edgePreview({ changes: [webPath], webBoundaryReady: true });
  assert.equal(candidate.mode, 'candidate');
  assert.ok(candidate.suggestedStages.some((item) => item.command.includes('src/trailing-task.test.ts')));
  assert.ok(candidate.suggestedStages.some((item) => item.command.includes('tests/providers.spec.ts')));
  assert.equal(edgePreview({ changes: [webPath], webBoundaryReady: false }).mode, 'broad');
  assert.equal(edgePreview({ changes: ['clients/web/src/trailing-task.ts'], webBoundaryReady: true }).mode, 'broad');
  assert.equal(
    edgePreview({ changes: [webPath, 'clients/web/src/trailing-task.ts'], webBoundaryReady: true }).mode,
    'broad',
  );
});

test('HS2-55MWQ9 representative private, public-contract and cross-package plans retain full affected checks', () => {
  const required = (path) => plan({ stages: fullStages, changes: [path] });
  const privateWeb = required(webPath);
  assert.ok(privateWeb.stages.some((item) => item.selected && item.command === 'npm run test:unit'));
  const publicWeb = required('clients/web/src/trailing-task.ts');
  assert.ok(publicWeb.stages.some((item) => item.selected && item.command === 'npm run test:unit'));
  const publicContract = required('docs/contracts/v1/openapi.yaml');
  assert.ok(publicContract.stages.some((item) => item.selected && item.command === 'npm run test:unit'));
  const serverRoute = required('crates/hotsheet-server/src/lib.rs');
  for (const group of ['rust', 'web', 'migrator'])
    assert.ok(
      serverRoute.stages.some((item) => item.selected && item.group === group),
      group,
    );
  const exporter = required('migrator/src/export.mjs');
  for (const group of ['rust', 'web', 'migrator'])
    assert.ok(
      exporter.stages.some((item) => item.selected && item.group === group),
      group,
    );
  const unknown = required('scripts/unknown-gate-input.mjs');
  assert.equal(unknown.mode, 'full');
  assert.ok(unknown.stages.every((item) => item.selected));
});

test('HS2-55MWQ9 repository proof requires an existing changed private file and all boundary inputs', (t) => {
  const repo = mkdtempSync(join(tmpdir(), 'hs2-55mwq9-edge-'));
  t.after(() => rmSync(repo, { recursive: true, force: true }));
  const put = (path, body) => {
    mkdirSync(dirname(join(repo, path)), { recursive: true });
    writeFileSync(join(repo, path), body);
  };
  const git = (...args) => execFileSync('git', args, { cwd: repo, encoding: 'utf8' }).trim();
  git('init', '--quiet', '-b', 'main');
  git('config', 'user.name', 'Synthetic');
  git('config', 'user.email', 'synthetic@invalid.example');
  put(webPath, 'export const delay = 250;\n');
  put('clients/web/src/trailing-task.ts', "import { createTrailingTaskInternal } from './trailing-task-internal';\n");
  put('clients/web/eslint.config.mjs', "'**/trailing-task-internal'\n");
  put('clients/web/src/trailing-task.test.ts', 'HS2-K5E2KF preserves the public delay\n');
  put('clients/web/tests/providers.spec.ts', 'defers ticket refresh without hiding an open metadata popup\n');
  put(
    'clients/web/src/app/runtime.tsx',
    "import { createTrailingTask } from '../trailing-task';\ncreateTrailingTask<TicketView>();\n",
  );
  git('add', '.');
  git('commit', '--quiet', '-m', 'baseline');
  const base = git('rev-parse', 'HEAD');
  assert.equal(edgePreviewForRepo(repo, base, [webPath]).mode, 'broad', 'unchanged file is not an edit');
  put(webPath, 'export const delay = 251;\n');
  assert.equal(edgePreviewForRepo(repo, base, [webPath]).mode, 'candidate');
  put('clients/web/eslint.config.mjs', 'missing boundary\n');
  assert.equal(edgePreviewForRepo(repo, base, [webPath]).mode, 'broad');
  assert.equal(edgePreviewForRepo(repo, null, [webPath]).mode, 'broad');
});
